//! Blocking client for roost's HTTP API.

use std::time::{Duration, Instant};

use flock_proto::{Control, Peer, ReportBatch, StartReq, StartResp, TimeResp, now_ms};

#[derive(Clone)]
pub struct Roost {
    base: String,
    auth: String,
    agent: ureq::Agent,
}

/// What `POST /start` gave.
pub enum Started {
    Run(StartResp),
    /// roost has this owl paused, or no identity is free; the reason it gave.
    Held(String),
}

/// Why an upload did not land.
pub enum UploadError {
    /// roost refused the batch and always will.
    Rejected(String),
    /// Worth retrying.
    Failed(String),
}

impl Roost {
    pub fn new(base: &str, key: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
            auth: format!("Bearer {key}"),
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(15))
                .build(),
        }
    }

    /// The same roost, as the run the token belongs to: every request after
    /// `/start` carries it.
    pub fn with_token(&self, token: &str) -> Self {
        Self {
            auth: format!("Bearer {token}"),
            ..self.clone()
        }
    }

    fn request(&self, method: &str, path: &str) -> ureq::Request {
        self.agent
            .request(method, &format!("{}{path}", self.base))
            .set("Authorization", &self.auth)
    }

    pub fn start(&self, req: &StartReq) -> anyhow::Result<Started> {
        match self.request("POST", "/start").send_json(req) {
            Ok(resp) => Ok(Started::Run(resp.into_json()?)),
            Err(ureq::Error::Status(423, resp)) => {
                Ok(Started::Held(resp.into_string().unwrap_or_default()))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn control(&self) -> anyhow::Result<Control> {
        Ok(self.request("GET", "/control").call()?.into_json()?)
    }
    pub fn owls(&self) -> anyhow::Result<Vec<Peer>> {
        Ok(self.request("GET", "/owls").call()?.into_json()?)
    }

    pub fn post_reports(&self, batch: &ReportBatch) -> Result<(), UploadError> {
        match self.request("POST", "/reports").send_json(batch) {
            Ok(_) => Ok(()),
            Err(ureq::Error::Status(code, resp)) if (400..500).contains(&code) && code != 429 => {
                let body = resp.into_string().unwrap_or_default();
                Err(UploadError::Rejected(format!("{code}: {body}")))
            }
            Err(e) => Err(UploadError::Failed(e.to_string())),
        }
    }

    /// roost's clock minus ours, from the sample with the shortest round trip.
    pub fn clock_offset_ms(&self) -> anyhow::Result<i64> {
        let mut best: Option<(Duration, i64)> = None;
        for _ in 0..5 {
            let sent = now_ms();
            let start = Instant::now();
            let resp: TimeResp = self.request("GET", "/time").call()?.into_json()?;
            let rtt = start.elapsed();
            let offset = resp.now_ms - (sent + rtt.as_millis() as i64 / 2);
            if best.is_none_or(|(b, _)| rtt < b) {
                best = Some((rtt, offset));
            }
        }
        Ok(best.map(|(_, o)| o).unwrap_or(0))
    }
}
