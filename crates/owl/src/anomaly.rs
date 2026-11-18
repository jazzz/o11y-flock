//! Makes anomalies visible to roost: libchat's warning and error logs, and
//! panics. Failed operations are reported where they happen.

use std::fmt::Write as _;
use std::sync::Mutex;

use flock_proto::{Report, ReportBatch, Source};
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::owl::Clock;
use crate::recorder::Recorder;
use crate::roost::Roost;

/// Where forwarded logs go. Unset until the run has a recorder, and taken
/// away again before the final flush, which waits for every recorder to go.
static SINK: Mutex<Option<(Recorder, Clock)>> = Mutex::new(None);

pub fn connect(rec: Recorder, clock: Clock) {
    *SINK.lock().unwrap() = Some((rec, clock));
}

pub fn disconnect() {
    SINK.lock().unwrap().take();
}

/// Forwards warnings and errors logged by anything but the owl itself: the
/// owl reports its own failures directly.
pub struct LogLayer;

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let meta = event.metadata();
        let source = match *meta.level() {
            Level::ERROR => Source::LibchatError,
            Level::WARN => Source::LibchatWarn,
            _ => return,
        };
        if meta.target().starts_with("owl") {
            return;
        }
        let sink = SINK.lock().unwrap();
        let Some((rec, clock)) = sink.as_ref() else {
            return;
        };
        let mut v = Fields::default();
        event.record(&mut v);
        rec.record(Report::Anomaly {
            ts_ms: clock.now(),
            source,
            group: None,
            message: v.message + &v.rest,
            location: Some(meta.target().to_string()),
        });
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    rest: String,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            let _ = write!(self.message, "{value:?}");
        } else {
            let _ = write!(self.rest, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            let _ = write!(self.rest, " {}={value}", field.name());
        }
    }
}

/// Reports a panic straight to roost before the owl dies: release builds
/// abort on panic, so the recorder would never flush it.
pub fn report_panics(roost: Roost, run: i64, clock: Clock) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic with a non-string payload".into());
        let thread = std::thread::current();
        let batch = ReportBatch {
            run,
            reports: vec![Report::Anomaly {
                ts_ms: clock.now(),
                source: Source::Panic,
                group: None,
                message: format!(
                    "thread '{}' panicked: {message}",
                    thread.name().unwrap_or("unnamed")
                ),
                location: info
                    .location()
                    .map(|l| format!("{}:{}", l.file(), l.line())),
            }],
        };
        let _ = roost.post_reports(&batch);
        previous(info);
    }));
}
