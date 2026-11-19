//! The contract between owls and roost.
//!
//! Owls report facts; roost computes values and judges correctness. Every
//! owl holds the same flock key. On start an owl names itself (its
//! *instance*) and the person running it (its *operator*), roost assigns it a
//! free identity and the settings, and the owl creates one group and runs
//! until it stops, reporting what it sends, receives and sees of its groups.
//! Each start is a new *run* of that owl.
//!
//! An identity is one of the named owls (`barn-01`), which an instance keeps
//! for good, or one of the *ephemeral owls* (`owlet-01`), whose names roost
//! passes from one ephemeral owl to the next.

use serde::{Deserialize, Serialize};

/// The named owls: slot `n` (1-based) is `OWL_NAMES[n - 1]`. An instance
/// that has one keeps it, through restarts and outages.
#[rustfmt::skip]
pub const OWL_NAMES: [&str; 50] = [
    "barn-01", "snowy-02", "great-horned-03", "barred-04", "burrowing-05",
    "screech-06", "tawny-07", "little-08", "eagle-09", "great-grey-10",
    "long-eared-11", "short-eared-12", "spotted-13", "elf-14", "pygmy-15",
    "saw-whet-16", "boreal-17", "scops-18", "hawk-owl-19", "flammulated-20",
    "ural-21", "barking-22", "powerful-23", "rufous-24", "morepork-25",
    "boobook-26", "spectacled-27", "masked-28", "sooty-29", "grass-30",
    "bay-31", "crested-32", "striped-33", "stygian-34", "fish-35",
    "jungle-36", "collared-37", "buff-fronted-38", "mottled-39", "band-bellied-40",
    "black-banded-41", "fulvous-42", "rusty-barred-43", "pharaoh-44", "vermiculated-45",
    "pels-46", "maned-47", "laughing-48", "pearl-spotted-49", "humes-50",
];

/// Owlet `n` (1-based). roost registers owlets as they are wanted and hands
/// each to any owl started as ephemeral, so an owlet's name says nothing
/// about which owl process had it before.
pub fn owlet_name(n: usize) -> String {
    format!("owlet-{n:02}")
}

/// The longest operator roost keeps, in characters.
pub const MAX_OPERATOR: usize = 64;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_millis() as i64
}

/// The group name every flock group carries.
pub const GROUP_NAME: &str = "flock";

/// What an owl sends as chat message content. Content is assumed valid; the
/// body only gives messages a realistic size.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    /// `{owl}/{run}/{n}`: unique for good.
    pub msg_id: String,
    pub body: String,
}

impl Payload {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("payload serializes")
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

// ── Reports ─────────────────────────────────────────────────────────────────

/// Where an [`Report::Anomaly`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An operation failed.
    Send,
    AddMember,
    CreateGroup,
    /// libchat could not process an inbound frame.
    Inbound,
    /// libchat logged a warning or an error.
    LibchatWarn,
    LibchatError,
    /// The owl panicked; it dies right after reporting this.
    Panic,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Send => "send",
            Self::AddMember => "add_member",
            Self::CreateGroup => "create_group",
            Self::Inbound => "inbound",
            Self::LibchatWarn => "libchat_warn",
            Self::LibchatError => "libchat_error",
            Self::Panic => "panic",
        }
    }
}

/// One fact an owl reports. Times are on roost's clock: the owl corrects
/// them by its measured offset.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Report {
    /// The owl sent a message while at `fingerprint`, which ties it to an
    /// epoch and so to who was owed it.
    Sent {
        msg_id: String,
        group: String,
        ts_ms: i64,
        fingerprint: String,
    },
    Received {
        msg_id: String,
        group: String,
        ts_ms: i64,
    },
    /// The group's members as this owl now sees them, by owl name.
    Members {
        group: String,
        ts_ms: i64,
        members: Vec<String>,
    },
    /// The owl applied a commit. `prev` is `None` when it first joins.
    Commit {
        group: String,
        ts_ms: i64,
        prev: Option<String>,
        next: String,
    },
    /// Something that should not happen, made visible for debugging.
    Anomaly {
        ts_ms: i64,
        source: Source,
        group: Option<String>,
        message: String,
        /// The log target, or the file and line a panic happened at.
        #[serde(default)]
        location: Option<String>,
    },
}

impl Report {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Sent { .. } => "sent",
            Self::Received { .. } => "received",
            Self::Members { .. } => "members",
            Self::Commit { .. } => "commit",
            Self::Anomaly { .. } => "anomaly",
        }
    }
}

// ── roost HTTP API ──────────────────────────────────────────────────────────
//
// `POST /start` carries `Authorization: Bearer <flock key>`; every later
// request of the run carries `Authorization: Bearer <run token>`, which says
// which owl and run it is. `GET /time` is open.

/// What every owl does, set once on roost for the whole flock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub preset: String,
    pub registry_url: String,
    /// Mean messages an owl posts per minute, across all its groups.
    pub posts_per_min: f64,
    /// Members a group adds per hour, shared among its members.
    pub adds_per_hour: f64,
    pub min_body: usize,
    pub max_body: usize,
}

/// `GET /time`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeResp {
    pub now_ms: i64,
}

/// `POST /start`: the owl's first request. roost assigns the instance a free
/// identity, the same one it had if it had one. It answers `423 Locked` while
/// that owl is paused, no identity is free, or another process is using the
/// instance; the owl waits, without a client, and asks again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartReq {
    /// Names this owl across restarts: an ID kept in its working directory,
    /// which in Docker is a volume that outlives the container.
    pub instance: String,
    /// Random for each owl process, and kept when an owl restarts itself to
    /// wait out a pause. Tells two processes with one instance apart.
    pub process: String,
    pub libchat_rev: String,
    /// flock's own git commit, with `-dirty` if built from local changes.
    pub owl_rev: String,
    /// The conversation type it creates, e.g. `GroupV3`.
    pub convo_type: String,
    /// Who runs this owl, as they put it: a name or handle, for following up
    /// on what it reports. Not checked; at most [`MAX_OPERATOR`] characters.
    #[serde(default)]
    pub operator: String,
    /// Asks for an owlet rather than a named owl.
    #[serde(default)]
    pub ephemeral: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartResp {
    pub owl: String,
    pub run: i64,
    /// Authenticates the run's later requests.
    pub token: String,
    /// Hex of the installation's 32-byte Ed25519 signing seed.
    pub seed: String,
    /// The account that endorses this installation.
    pub account: String,
    pub settings: Settings,
}

/// `GET /owls`: every owl, and whether it is connected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Peer {
    pub owl: String,
    pub account: String,
    pub connected: bool,
}

/// `GET /control`: what roost wants of a running owl. Once `paused`, the owl
/// flushes its reports and shuts its client down, so it puts nothing on the
/// network and asks nothing of the registry, then waits to be resumed.
/// Resuming starts a new run: libchat's state was in memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Control {
    pub paused: bool,
    /// roost gave this owl's identity to another instance, after not
    /// hearing from this one for a while. The owl shuts down the same way and
    /// asks for an identity again.
    #[serde(default)]
    pub superseded: bool,
}

/// `POST /reports`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportBatch {
    pub run: i64,
    pub reports: Vec<Report>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_tagged_by_kind() {
        let r = Report::Commit {
            group: "g".into(),
            ts_ms: 1,
            prev: None,
            next: "ab".into(),
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["kind"], "commit");
        assert_eq!(r.kind(), "commit");
        let e: Report = serde_json::from_str(
            r#"{"kind":"anomaly","ts_ms":2,"source":"add_member","group":null,"message":"x"}"#,
        )
        .unwrap();
        assert!(matches!(
            e,
            Report::Anomaly {
                source: Source::AddMember,
                location: None,
                ..
            }
        ));
    }

    #[test]
    fn start_from_an_older_owl_has_no_operator() {
        let req: StartReq = serde_json::from_str(
            r#"{"instance":"i","process":"p","libchat_rev":"l","owl_rev":"o","convo_type":"GroupV3"}"#,
        )
        .unwrap();
        assert_eq!(req.operator, "");
        assert!(!req.ephemeral);
    }

    #[test]
    fn owlets_are_numbered() {
        assert_eq!(owlet_name(1), "owlet-01");
        assert_eq!(owlet_name(123), "owlet-123");
    }
}
