//! Buffers reports and uploads them to roost in batches.
//!
//! An upload that fails is retried with the batch kept, so a roost outage
//! delays reports rather than losing them — it must never look like protocol
//! loss. Only if the buffer outgrows `MAX_BUFFERED` are the oldest dropped.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use flock_proto::{Report, ReportBatch};
use tracing::warn;

use crate::roost::{Roost, UploadError};

const BATCH_MAX: usize = 500;
const BATCH_INTERVAL: Duration = Duration::from_secs(1);
const MAX_BUFFERED: usize = 200_000;
/// Shutdown has to fit in the container's stop grace period.
const FINAL_FLUSH_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct Recorder {
    tx: mpsc::Sender<Report>,
}

pub struct Uploader {
    handle: JoinHandle<()>,
}

impl Recorder {
    pub fn start(roost: Roost, run: i64) -> (Self, Uploader) {
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::Builder::new()
            .name("uploader".into())
            .spawn(move || upload_loop(roost, run, rx))
            .expect("spawn uploader");
        (Self { tx }, Uploader { handle })
    }

    pub fn record(&self, report: Report) {
        // The uploader only stops once every Recorder is dropped.
        let _ = self.tx.send(report);
    }
}

impl Uploader {
    /// Waits for the final flush. Drop every [`Recorder`] first.
    pub fn finish(self) {
        let _ = self.handle.join();
    }
}

fn upload_loop(roost: Roost, run: i64, rx: mpsc::Receiver<Report>) {
    let mut pending: Vec<Report> = Vec::new();
    let mut last_flush = Instant::now();
    let mut closed = false;

    while !closed {
        match rx.recv_timeout(BATCH_INTERVAL) {
            Ok(report) => pending.push(report),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => closed = true,
        }
        if pending.len() < BATCH_MAX && last_flush.elapsed() < BATCH_INTERVAL && !closed {
            continue;
        }
        if pending.len() > MAX_BUFFERED {
            let excess = pending.len() - MAX_BUFFERED;
            pending.drain(..excess);
            warn!(excess, "report buffer full; dropped oldest reports");
        }
        flush(&roost, run, &mut pending);
        last_flush = Instant::now();
    }

    let deadline = Instant::now() + FINAL_FLUSH_DEADLINE;
    while !pending.is_empty() && Instant::now() < deadline {
        if !flush(&roost, run, &mut pending) {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    if !pending.is_empty() {
        warn!(lost = pending.len(), "gave up flushing reports to roost");
    }
}

/// Uploads `pending` in batches, removing what landed or was refused for
/// good. `false` if roost could not be reached.
fn flush(roost: &Roost, run: i64, pending: &mut Vec<Report>) -> bool {
    while !pending.is_empty() {
        let n = pending.len().min(BATCH_MAX);
        let batch = ReportBatch {
            run,
            reports: pending[..n].to_vec(),
        };
        match roost.post_reports(&batch) {
            Ok(()) => {}
            Err(UploadError::Rejected(e)) => warn!(reports = n, "roost refused reports: {e}"),
            Err(UploadError::Failed(e)) => {
                warn!("upload failed, will retry: {e}");
                return false;
            }
        }
        pending.drain(..n);
    }
    true
}
