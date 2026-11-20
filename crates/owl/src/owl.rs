//! One owl's run: fetch identity and settings, build the client, create a
//! group, then until stopped: grow its groups by adding owls, now and then
//! remove one, post to them, and report what it sends, receives and sees of
//! them.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Context;
use crypto::Ed25519SigningKey;
use flock_proto::{
    MAX_OPERATOR, Payload, Peer, Report, Settings, Source, StartReq, StartResp, now_ms,
};
use logos_chat::{
    AccountAddr, DbKey, Event, Installation, LogosChatClient, LogosConfig, P2pConfig, client_with,
};
use rand::Rng;
use rand::distr::{Alphanumeric, SampleString};
use rand::seq::IteratorRandom;
use tracing::{error, info, warn};

use crate::group::{CONVO_TYPE, Groups, Reading, fingerprint};
use crate::recorder::Recorder;
use crate::roost::{Roost, Started};
use crate::{Args, anomaly};

/// How often an owl rolls the dice on adding members.
const DECIDE_EVERY: Duration = Duration::from_secs(10);
/// The smallest group an owl removes a member from, so a group never shrinks
/// to one owl.
const MIN_TO_REMOVE: usize = 3;
/// How often it checks each group for a membership change it was not told of.
const CHECK_EVERY: Duration = Duration::from_secs(2);
/// How often it refreshes the list of owls it may add.
const PEERS_EVERY: Duration = Duration::from_secs(30);
const CLOCK_RESYNC: Duration = Duration::from_secs(300);
const CREATE_RETRY: Duration = Duration::from_secs(30);
/// How often a running owl asks roost whether it is paused.
const CONTROL_EVERY: Duration = Duration::from_secs(5);
/// How often an owl that roost holds back (paused, or no identity free) asks
/// to start again.
const HELD_POLL: Duration = Duration::from_secs(10);
/// A group's pull on this owl's posts: recent messages, fading with this
/// time constant, plus a floor so a quiet group still gets a first message.
const ACTIVITY_TAU: Duration = Duration::from_secs(20 * 60);
const ACTIVITY_FLOOR: f64 = 1.0;

/// roost's clock, from ours and the measured offset.
#[derive(Clone)]
pub struct Clock(Arc<AtomicI64>);

impl Clock {
    pub fn now(&self) -> i64 {
        now_ms() + self.0.load(Ordering::Relaxed)
    }
}

/// What the event thread tells the main loop.
enum Notice {
    Joined(String),
    MembersChanged(String),
    Received(String),
}

pub fn run(args: Args) -> anyhow::Result<()> {
    // `stop` ends the run, for any reason; `terminated` says a signal asked
    // for it, so the owl exits rather than asking roost again.
    let stop = Arc::new(AtomicBool::new(false));
    let terminated = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(signal, stop.clone())?;
        signal_hook::flag::register(signal, terminated.clone())?;
    }

    let operator = args.operator.trim().to_string();
    anyhow::ensure!(
        !operator.is_empty(),
        "OWL_OPERATOR is empty: say who runs this owl"
    );
    anyhow::ensure!(
        operator.chars().count() <= MAX_OPERATOR,
        "OWL_OPERATOR is longer than {MAX_OPERATOR} characters"
    );

    let roost = Roost::new(&args.roost, &args.key);
    let Some(offset) = retry("clock offset", &stop, || roost.clock_offset_ms()) else {
        return Ok(());
    };
    let clock = Clock(Arc::new(AtomicI64::new(offset)));
    let req = StartReq {
        instance: instance(&args)?,
        process: process(),
        libchat_rev: env!("LIBCHAT_REV").into(),
        owl_rev: env!("OWL_REV").into(),
        convo_type: CONVO_TYPE.into(),
        operator,
        ephemeral: args.ephemeral,
    };
    let mut held: Option<String> = None;
    let start = loop {
        match retry("start", &stop, || roost.start(&req)) {
            None => return Ok(()),
            Some(Started::Run(start)) => break start,
            Some(Started::Held(why)) => {
                if held.as_ref() != Some(&why) {
                    info!(instance = %req.instance, "{why}; waiting");
                    held = Some(why);
                }
                sleep_unless(&stop, HELD_POLL);
            }
        }
    };
    info!(
        owl = %start.owl, run = start.run, instance = %req.instance, operator = %req.operator,
        offset_ms = offset, adds_per_hour = start.settings.adds_per_hour,
        removes_per_hour = start.settings.removes_per_hour, "started"
    );
    // Everything after /start goes as this run.
    let roost = roost.with_token(&start.token);
    let restart = Arc::new(AtomicBool::new(false));
    watch_control(roost.clone(), stop.clone(), restart.clone())?;
    anomaly::report_panics(roost.clone(), start.run, clock.clone());

    let settings = &start.settings;
    // The DB path and key are unused: client_with keeps everything in memory.
    let mut cfg = LogosConfig::new(String::new(), DbKey::from_encryption_key([0; 32]));
    cfg.set_registry_url(&settings.registry_url);
    cfg.set_p2p_config(P2pConfig {
        preset: settings.preset.clone(),
        log_level: args.delivery_log_level.clone(),
        ..P2pConfig::default()
    });
    let (mut client, chat_events) = client_with(installation(&start)?, cfg)
        .map_err(|e| anyhow::anyhow!("build client: {e}"))?;
    info!(owl = %start.owl, account = %client.addr(), "client ready");

    let (rec, uploader) = Recorder::start(roost.clone(), start.run);
    anomaly::connect(rec.clone(), clock.clone());
    let (notices, notice_rx) = mpsc::channel();
    let event_thread = std::thread::Builder::new()
        .name("chat-events".into())
        .spawn({
            let (rec, clock) = (rec.clone(), clock.clone());
            let own = format!("{}/{}/", start.owl, start.run);
            move || {
                for event in chat_events.iter() {
                    inbound(event, &own, &rec, &clock, &notices);
                }
            }
        })?;

    let mut fuzzer = Fuzzer {
        client: &mut client,
        roost: &roost,
        start: &start,
        rec: &rec,
        clock: &clock,
        groups: HashMap::new(),
        peers: Vec::new(),
        names: HashMap::new(),
        sent: 0,
        rng: rand::rng(),
    };
    fuzzer.run(&notice_rx, &stop);

    // Dropping the client stops its worker, which ends the event stream.
    drop(client);
    let _ = event_thread.join();
    anomaly::disconnect();
    drop(rec);
    info!(owl = %start.owl, "flushing reports");
    uploader.finish();
    info!(owl = %start.owl, run = start.run, "stopped");
    // The claim on the identity stays: a restart of this instance, including
    // its container being recreated, gets the same owl back.
    if restart.load(Ordering::Relaxed) && !terminated.load(Ordering::Relaxed) {
        return Err(restart_self(&req.process));
    }
    Ok(())
}

/// Where the instance ID is kept, in the working directory.
const INSTANCE_FILE: &str = "instance";
/// Passes the process ID on when an owl restarts itself.
const PROCESS_ENV: &str = "FLOCK_PROCESS";

/// Replaces this process with a fresh one, the same instance and process ID,
/// which asks roost for its identity again: while it is paused it waits there
/// without a client, and if another instance took it, it gets a free one. A
/// fresh process rather than a second client in this one: whether
/// liblogosdelivery can start a node again after destroying one is untested.
/// Returns only if that fails.
fn restart_self(process: &str) -> anyhow::Error {
    use std::os::unix::process::CommandExt;
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return anyhow::Error::from(e).context("find own executable"),
    };
    info!("restarting to ask roost again");
    let err = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .env(PROCESS_ENV, process)
        .exec();
    anyhow::Error::from(err).context("restart")
}

/// Names this owl across restarts: `OWL_INSTANCE`, or else the ID in
/// `./instance`, made on first start from the hostname and a random suffix.
fn instance(args: &Args) -> anyhow::Result<String> {
    if let Some(instance) = args.instance.as_ref().filter(|i| !i.trim().is_empty()) {
        return Ok(instance.clone());
    }
    match std::fs::read_to_string(INSTANCE_FILE) {
        Ok(id) if !id.trim().is_empty() => return Ok(id.trim().to_string()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("read ./instance"),
    }
    let host = hostname();
    let host = host.split('.').next().unwrap_or("owl");
    let id = format!("{host}-{}", hex::encode(rand::random::<[u8; 3]>()));
    std::fs::write(INSTANCE_FILE, format!("{id}\n")).context("write ./instance")?;
    info!(instance = %id, "made a new instance ID in ./instance");
    Ok(id)
}

/// This process's ID: the one it was restarted with, or a new random one.
fn process() -> String {
    std::env::var(PROCESS_ENV)
        .ok()
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| hex::encode(rand::random::<[u8; 8]>()))
}

/// The hostname: a container's ID in Docker.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            let out = std::process::Command::new("hostname").output().ok()?;
            let host = String::from_utf8(out.stdout).ok()?.trim().to_string();
            (!host.is_empty()).then_some(host)
        })
        .unwrap_or_else(|| "owl".into())
}

fn installation(start: &StartResp) -> anyhow::Result<Installation> {
    let seed: [u8; 32] = hex::decode(&start.seed)
        .context("seed is not hex")?
        .try_into()
        .map_err(|_| anyhow::anyhow!("seed is not 32 bytes"))?;
    let account: AccountAddr = start
        .account
        .parse()
        .map_err(|e| anyhow::anyhow!("account {}: {e}", start.account))?;
    Ok(Installation::make(
        Ed25519SigningKey::from_seed(&seed),
        account,
    ))
}

/// A group this owl is in.
struct Group {
    /// Where this owl last reported it is; `None` until the first check.
    fingerprint: Option<String>,
    /// What it last reported of the group.
    reading: Option<Reading>,
    next_check: Instant,
    activity: f64,
    activity_at: Instant,
}

impl Group {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            fingerprint: None,
            reading: None,
            next_check: now,
            activity: 0.0,
            activity_at: now,
        }
    }

    fn decayed(&self, now: Instant) -> f64 {
        let dt = now.saturating_duration_since(self.activity_at);
        self.activity * (-dt.as_secs_f64() / ACTIVITY_TAU.as_secs_f64()).exp()
    }

    fn weight(&self, now: Instant) -> f64 {
        ACTIVITY_FLOOR + self.decayed(now)
    }

    fn bump(&mut self) {
        let now = Instant::now();
        self.activity = self.decayed(now) + 1.0;
        self.activity_at = now;
    }
}

struct Fuzzer<'a> {
    client: &'a mut LogosChatClient,
    roost: &'a Roost,
    start: &'a StartResp,
    rec: &'a Recorder,
    clock: &'a Clock,
    groups: HashMap<String, Group>,
    peers: Vec<Peer>,
    /// Account → owl name, for every owl.
    names: HashMap<String, String>,
    /// Messages this run has sent.
    sent: u64,
    rng: rand::rngs::ThreadRng,
}

impl Fuzzer<'_> {
    fn settings(&self) -> &Settings {
        &self.start.settings
    }

    fn run(&mut self, notices: &mpsc::Receiver<Notice>, stop: &AtomicBool) {
        let now = Instant::now();
        let mut next_create = Some(now);
        let mut next_peers = now;
        let mut next_decide = now + DECIDE_EVERY;
        let mut next_resync = now + CLOCK_RESYNC;
        let mut next_post = now + self.post_gap();

        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= next_peers {
                next_peers = now + PEERS_EVERY;
                self.refresh_peers();
            }
            if now >= next_resync {
                next_resync = now + CLOCK_RESYNC;
                match self.roost.clock_offset_ms() {
                    Ok(o) => self.clock.0.store(o, Ordering::Relaxed),
                    Err(e) => warn!("clock resync: {e}"),
                }
            }
            if next_create.is_some_and(|t| now >= t) {
                next_create = (!self.create()).then(|| now + CREATE_RETRY);
            }

            for notice in notices.try_iter() {
                match notice {
                    Notice::Joined(id) => {
                        if self.client.is_flock_group(&id) && !self.groups.contains_key(&id) {
                            info!(group = %short(&id), "joined group");
                            self.groups.insert(id, Group::new());
                        }
                    }
                    Notice::MembersChanged(id) => {
                        if let Some(g) = self.groups.get_mut(&id) {
                            g.next_check = now;
                        }
                    }
                    Notice::Received(id) => {
                        if let Some(g) = self.groups.get_mut(&id) {
                            g.bump();
                        }
                    }
                }
            }
            self.check_membership();

            if now >= next_decide {
                next_decide = now + DECIDE_EVERY;
                self.maybe_grow();
                self.maybe_shrink();
            }
            if now >= next_post {
                next_post = now + self.post_gap();
                self.post();
            }

            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn refresh_peers(&mut self) {
        match self.roost.owls() {
            Ok(peers) => {
                self.names = peers
                    .iter()
                    .map(|p| (p.account.clone(), p.owl.clone()))
                    .collect();
                self.peers = peers;
            }
            Err(e) => warn!("fetch owls: {e}"),
        }
    }

    /// Whether the group was created.
    fn create(&mut self) -> bool {
        match self.client.create_group() {
            Ok(id) => {
                info!(group = %short(&id), "created group");
                self.groups.insert(id, Group::new());
                true
            }
            Err(err) => {
                warn!("create group: {err}");
                self.anomaly(Source::CreateGroup, None, err);
                false
            }
        }
    }

    /// Reports a commit and the member list whenever this owl's view of a
    /// group moves: a new epoch, or more members at the same one (see
    /// [`Reading::is_news`]). A reading it does not trust waits for the next
    /// check. It stops following a group it has been removed from; if it is
    /// added again, it joins it anew.
    fn check_membership(&mut self) {
        let now = Instant::now();
        let due: Vec<String> = self
            .groups
            .iter()
            .filter(|(_, g)| g.next_check <= now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in due {
            self.groups.get_mut(&id).unwrap().next_check = now + CHECK_EVERY;
            // libchat stops a removed member sending. A member list without
            // this owl is not enough: auth can leave it out. Whether the list
            // still has it is logged, to see which libchat does.
            if !Groups::can_send(&*self.client, &id) {
                let listed = self
                    .client
                    .member_accounts(&id)
                    .map(|m| m.contains(&self.start.account));
                info!(group = %short(&id), still_listed = ?listed.ok(), "removed from group");
                self.groups.remove(&id);
                self.rec.record(Report::Removed {
                    group: id,
                    ts_ms: self.clock.now(),
                });
                continue;
            }
            // The epoch on both sides of the member list, so that the two
            // belong together: libchat's state moves underneath.
            let epoch = self.client.epoch(&id);
            let Ok(members) = self.client.member_accounts(&id) else {
                continue;
            };
            if self.client.epoch(&id) != epoch {
                continue;
            }
            let reading = Reading { epoch, members };
            if !reading.is_news(&self.start.account, self.groups[&id].reading.as_ref()) {
                continue;
            }
            let fp = fingerprint(&reading.members);
            let mut members: Vec<String> = reading.members.iter().map(|a| self.name(a)).collect();
            members.sort();
            let (epoch, epoch_fingerprint) = match &reading.epoch {
                Some(e) => (Some(e.number), Some(e.fingerprint.clone())),
                None => (None, None),
            };
            let g = self.groups.get_mut(&id).unwrap();
            let prev = g.fingerprint.replace(fp.clone());
            g.reading = Some(reading);
            let ts_ms = self.clock.now();
            self.rec.record(Report::Commit {
                group: id.clone(),
                ts_ms,
                prev,
                next: fp,
                epoch,
                epoch_fingerprint,
            });
            self.rec.record(Report::Members {
                group: id,
                ts_ms,
                members,
            });
        }
    }

    fn name(&self, account: &str) -> String {
        if account == self.start.account {
            return self.start.owl.clone();
        }
        self.names
            .get(account)
            .cloned()
            .unwrap_or_else(|| format!("?{}", &account[..8.min(account.len())]))
    }

    /// Any member may add another owl. Each member rolls independently, with
    /// odds scaled down by the group's size, so a group grows at about
    /// `adds_per_hour` whatever its size, and members sometimes add at the
    /// same moment. Groups grow until every connected owl is in.
    fn maybe_grow(&mut self) {
        let ids: Vec<String> = self.groups.keys().cloned().collect();
        for id in ids {
            let (Ok(members), Ok(pending)) = (
                self.client.member_accounts(&id),
                self.client.pending_accounts(&id),
            ) else {
                continue;
            };
            let p =
                chance(self.settings().adds_per_hour, DECIDE_EVERY) / members.len().max(1) as f64;
            if !self.rng.random_bool(p.min(1.0)) {
                continue;
            }
            let Some(peer) = self
                .peers
                .iter()
                .filter(|p| p.connected && p.owl != self.start.owl)
                .filter(|p| !members.contains(&p.account) && !pending.contains(&p.account))
                .choose(&mut self.rng)
                .cloned()
            else {
                continue;
            };
            match self.client.add_member(&id, &peer.account) {
                Ok(()) => {
                    info!(group = %short(&id), owl = %peer.owl, size = members.len() + 1, "added member")
                }
                Err(err) => {
                    warn!(group = %short(&id), owl = %peer.owl, "add member: {err}");
                    self.anomaly(Source::AddMember, Some(&id), err);
                }
            }
            if let Some(g) = self.groups.get_mut(&id) {
                g.next_check = Instant::now();
            }
        }
    }

    /// Any member may remove another, never itself, from a group of at least
    /// [`MIN_TO_REMOVE`]. Each rolls as for adds, so a group loses about
    /// `removes_per_hour` whatever its size. An owl does not remove while its
    /// own add to the group is still landing.
    fn maybe_shrink(&mut self) {
        let ids: Vec<String> = self.groups.keys().cloned().collect();
        for id in ids {
            let (Ok(members), Ok(pending)) = (
                self.client.member_accounts(&id),
                self.client.pending_accounts(&id),
            ) else {
                continue;
            };
            if members.len() < MIN_TO_REMOVE
                || !pending.is_empty()
                || !members.contains(&self.start.account)
                || !Groups::can_send(&*self.client, &id)
            {
                continue;
            }
            let p = chance(self.settings().removes_per_hour, DECIDE_EVERY) / members.len() as f64;
            if !self.rng.random_bool(p.min(1.0)) {
                continue;
            }
            let Some(account) = members
                .iter()
                .filter(|a| **a != self.start.account)
                .choose(&mut self.rng)
                .cloned()
            else {
                continue;
            };
            let owl = self.name(&account);
            match self.client.remove_member(&id, &account) {
                Ok(()) => {
                    info!(group = %short(&id), owl = %owl, size = members.len() - 1, "removed member")
                }
                Err(err) => {
                    warn!(group = %short(&id), owl = %owl, "remove member: {err}");
                    self.anomaly(Source::RemoveMember, Some(&id), err);
                }
            }
            if let Some(g) = self.groups.get_mut(&id) {
                g.next_check = Instant::now();
            }
        }
    }

    /// Posts one message, to a group chosen by its recent activity.
    fn post(&mut self) {
        let now = Instant::now();
        let candidates: Vec<(String, String, f64)> = self
            .groups
            .iter()
            .filter_map(|(id, g)| Some((id.clone(), g.fingerprint.clone()?, g.weight(now))))
            .filter(|(id, _, _)| {
                self.client.member_accounts(id).is_ok_and(|m| m.len() > 1)
                    && Groups::can_send(&*self.client, id)
            })
            .collect();
        let total: f64 = candidates.iter().map(|(_, _, w)| w).sum();
        if candidates.is_empty() {
            return;
        }
        let mut x = self.rng.random_range(0.0..total);
        let (id, fingerprint, _) = candidates
            .iter()
            .find(|(_, _, w)| {
                x -= w;
                x < 0.0
            })
            .unwrap_or(candidates.last().unwrap())
            .clone();

        let s = self.settings();
        let (lo, hi) = (s.min_body, s.max_body.max(s.min_body));
        let len = self.rng.random_range(lo..=hi);
        let payload = Payload {
            msg_id: format!("{}/{}/{}", self.start.owl, self.start.run, self.sent),
            body: Alphanumeric.sample_string(&mut self.rng, len),
        };
        self.sent += 1;
        let ts_ms = self.clock.now();
        match self.client.send(&id, &payload.encode()) {
            Ok(()) => {
                self.rec.record(Report::Sent {
                    msg_id: payload.msg_id,
                    group: id.clone(),
                    ts_ms,
                    fingerprint,
                });
                self.groups.get_mut(&id).unwrap().bump();
            }
            Err(err) => self.anomaly(Source::Send, Some(&id), err),
        }
    }

    /// Time to this owl's next post: Poisson around the flock's mean.
    fn post_gap(&mut self) -> Duration {
        let rate = self.settings().posts_per_min;
        if rate <= 0.0 {
            return Duration::from_secs(3600);
        }
        let u: f64 = self.rng.random_range(f64::EPSILON..1.0);
        Duration::from_secs_f64(-u.ln() * 60.0 / rate)
    }

    fn anomaly(&self, source: Source, group: Option<&str>, message: String) {
        self.rec.record(Report::Anomaly {
            ts_ms: self.clock.now(),
            source,
            group: group.map(str::to_string),
            message,
            location: None,
        });
    }
}

/// Chance that something happening `per_hour` on average happens within `dt`.
fn chance(per_hour: f64, dt: Duration) -> f64 {
    if per_hour <= 0.0 {
        return 0.0;
    }
    1.0 - (-per_hour * dt.as_secs_f64() / 3600.0).exp()
}

fn short(id: &str) -> &str {
    &id[..12.min(id.len())]
}

/// Handles one chat event on the event thread. `own` prefixes this run's
/// message ids.
fn inbound(event: Event, own: &str, rec: &Recorder, clock: &Clock, notices: &mpsc::Sender<Notice>) {
    match event {
        Event::ConversationStarted { convo_id, .. } => {
            let _ = notices.send(Notice::Joined(convo_id.to_string()));
        }
        Event::ConversationMembersChanged { convo_id } => {
            let _ = notices.send(Notice::MembersChanged(convo_id.to_string()));
        }
        Event::MessageReceived {
            convo_id, content, ..
        } => {
            let ts_ms = clock.now();
            let Some(p) = Payload::decode(&content) else {
                return;
            };
            if p.msg_id.starts_with(own) {
                return;
            }
            let group = convo_id.to_string();
            rec.record(Report::Received {
                msg_id: p.msg_id,
                group: group.clone(),
                ts_ms,
            });
            let _ = notices.send(Notice::Received(group));
        }
        Event::InboundError { message } => rec.record(Report::Anomaly {
            ts_ms: clock.now(),
            source: Source::Inbound,
            group: None,
            message,
            location: None,
        }),
        _ => {}
    }
}

/// Sets `restart` and `stop` once roost pauses this owl or has given its
/// identity to another instance, which ends the run as SIGTERM does. Asking
/// keeps this instance's claim on the identity alive. Runs until `stop` is
/// set either way.
fn watch_control(
    roost: Roost,
    stop: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            let mut failing = false;
            while !stop.load(Ordering::Relaxed) {
                sleep_unless(&stop, CONTROL_EVERY);
                match roost.control() {
                    Ok(c) => {
                        failing = false;
                        if c.paused || c.superseded {
                            if c.superseded {
                                info!("another owl process has this identity now: shutting the client down");
                            } else {
                                info!("paused from roost: shutting the client down");
                            }
                            restart.store(true, Ordering::Relaxed);
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                    // Once per outage: the uploader already reports roost
                    // being unreachable.
                    Err(e) if !failing => {
                        failing = true;
                        warn!("control: {e}");
                    }
                    Err(_) => {}
                }
            }
        })?;
    Ok(())
}

/// Retries `f` until it succeeds or `stop` is set: an owl that cannot reach
/// roost has nothing useful to do.
fn retry<T>(what: &str, stop: &AtomicBool, mut f: impl FnMut() -> anyhow::Result<T>) -> Option<T> {
    let mut delay = Duration::from_millis(500);
    while !stop.load(Ordering::Relaxed) {
        match f() {
            Ok(v) => return Some(v),
            Err(e) => {
                error!("{what}: {e}; retrying in {delay:?}");
                sleep_unless(stop, delay);
                delay = (delay * 2).min(Duration::from_secs(10));
            }
        }
    }
    None
}

/// Sleeps for `d`, waking early if `stop` is set.
fn sleep_unless(stop: &AtomicBool, d: Duration) {
    let until = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(200)));
    }
}
