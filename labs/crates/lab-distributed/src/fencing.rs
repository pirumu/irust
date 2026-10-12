//! Lease, process pause and fencing token (lab d).
//!
//! A tiny discrete-event simulation in virtual milliseconds. A [`LockService`] hands out leases with
//! a TTL and a fencing token; a [`Storage`] holds one versioned counter and, depending on its
//! [`Mode`], checks tokens and/or versions; clients follow
//! `acquire -> read -> compute -> check own lease -> write`, and a pause (GC, `SIGSTOP`, CPU
//! throttling) may be injected before any step.
//!
//! The simulation is a pure function of its configuration and an [`Rng`]. Control flow of the
//! clients never depends on the outcome of a write (a rejected client also releases and moves on),
//! so the same seed produces the *same schedule* in every storage mode and only the storage verdicts
//! differ: sweeps are paired comparisons.
//!
//! What the model leaves out: network delay is a constant per RPC and the effect of a request is
//! applied at the instant it is sent (the reply arrives `latency_ms` later), there are no lost or
//! reordered messages, no lease renewal, one lock, one resource, and clients are never partitioned.
//! The scripted scenarios are constructed counterexamples, not measurements of a real system.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::rng::Rng;

/// Virtual time in milliseconds.
pub type Ms = u64;
/// Client index (0 = "A", 1 = "B", ...).
pub type ClientId = usize;

/// First token handed out by the scripted scenarios (so the chapter's figure reads 33 then 34).
pub const FIRST_TOKEN: u64 = 33;
/// Lease TTL used throughout the lab.
pub const TTL_MS: Ms = 10_000;

/// Display name of a client.
pub fn client_name(c: ClientId) -> char {
    (b'A' + c as u8) as char
}

// ---------------------------------------------------------------------------------------------
// Lock service
// ---------------------------------------------------------------------------------------------

/// One lease that the lock service granted. `expires` is on the lock service's clock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// Holder.
    pub client: ClientId,
    /// Fencing token issued with this grant.
    pub token: u64,
    /// Grant time.
    pub start: Ms,
    /// Expiry time (`start + ttl`).
    pub expires: Ms,
    /// Time of an explicit release, if any.
    pub released: Option<Ms>,
}

/// Lock service: at most one live lease, a counter that produces fencing tokens.
///
/// The replica's copy of the counter (`replica_next`) is frozen at the first token: it models a
/// counter replicated asynchronously whose replica is arbitrarily far behind when the primary
/// fails over (the lab never replicates it forward, which is the worst case).
#[derive(Clone, Debug)]
pub struct LockService {
    ttl: Ms,
    next_token: u64,
    replica_next: u64,
    /// Ground-truth history of every grant; the index is the grant id.
    pub grants: Vec<Grant>,
    current: Option<usize>,
}

impl LockService {
    /// New service with the given TTL; the first token issued is `first_token`.
    pub fn new(ttl: Ms, first_token: u64) -> Self {
        Self {
            ttl,
            next_token: first_token,
            replica_next: first_token,
            grants: Vec::new(),
            current: None,
        }
    }

    /// Tries to grant the lease at `now`. `Err(t)` = still held, until `t` (exclusive: at `t` it is free).
    pub fn acquire(&mut self, client: ClientId, now: Ms) -> Result<usize, Ms> {
        if let Some(g) = self.current {
            let g = &self.grants[g];
            if g.released.is_none() && g.expires > now {
                return Err(g.expires);
            }
        }
        let token = self.next_token;
        self.next_token += 1;
        self.grants.push(Grant {
            client,
            token,
            start: now,
            expires: now + self.ttl,
            released: None,
        });
        self.current = Some(self.grants.len() - 1);
        Ok(self.grants.len() - 1)
    }

    /// Releases grant `id` if it is still the live lease; otherwise a no-op (a late release from an
    /// expired holder must not free somebody else's lease).
    pub fn release(&mut self, id: usize, now: Ms) {
        if self.current == Some(id) && self.grants[id].expires > now {
            self.grants[id].released = Some(now);
            self.current = None;
        }
    }

    /// Ground truth: has a *later* grant started by `now`? (The lease may still have time left on
    /// paper, e.g. after a lock-service failover.)
    pub fn is_superseded(&self, id: usize, now: Ms) -> bool {
        self.grants.iter().skip(id + 1).any(|h| h.start <= now)
    }

    /// Ground truth: is grant `id` expired or superseded at `now`?
    pub fn is_stale(&self, id: usize, now: Ms) -> bool {
        self.grants[id].expires <= now || self.is_superseded(id, now)
    }

    /// Primary fails over to the replica. The lease table is lost (nobody holds the lock any more).
    /// If `counter_survives` is false the counter silently goes back to the replica's older value.
    pub fn failover(&mut self, counter_survives: bool) {
        if !counter_survives {
            self.next_token = self.replica_next;
        }
        self.current = None;
    }

    /// Next token that would be issued.
    pub fn next_token(&self) -> u64 {
        self.next_token
    }
}

// ---------------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------------

/// What the protected resource checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    /// Accepts every write.
    Unfenced,
    /// Rejects a write whose token is lower than the highest token accepted so far.
    Fenced,
    /// Token check plus compare-and-set on the version the writer read (`UPDATE ... WHERE version = $v`).
    FencedCas,
}

impl Mode {
    /// All modes, weakest first.
    pub const ALL: [Mode; 3] = [Mode::Unfenced, Mode::Fenced, Mode::FencedCas];

    /// Short label for tables.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Unfenced => "Unfenced",
            Mode::Fenced => "Fenced",
            Mode::FencedCas => "FencedCas",
        }
    }
}

/// Why the storage refused a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    /// `token < highest`.
    StaleToken { token: u64, highest: u64 },
    /// The version the writer read is no longer the current one.
    VersionMismatch { read: u64, current: u64 },
}

/// A write request as the storage sees it (it knows nothing about leases or time).
#[derive(Clone, Copy, Debug)]
pub struct WriteReq {
    /// Fencing token carried by the request.
    pub token: u64,
    /// Version of the document the writer read.
    pub read_version: u64,
    /// New value.
    pub new_value: i64,
}

/// A single versioned register.
#[derive(Clone, Debug)]
pub struct Storage {
    mode: Mode,
    value: i64,
    version: u64,
    highest_token: u64,
}

impl Storage {
    /// Document with `value` at version 1.
    pub fn new(mode: Mode, value: i64) -> Self {
        Self {
            mode,
            value,
            version: 1,
            highest_token: 0,
        }
    }

    /// Current `(value, version)`.
    pub fn read(&self) -> (i64, u64) {
        (self.value, self.version)
    }

    /// Highest token accepted so far (0 = none).
    pub fn highest_token(&self) -> u64 {
        self.highest_token
    }

    /// Applies the mode's checks; on success returns the new version.
    pub fn write(&mut self, req: WriteReq) -> Result<u64, Reject> {
        if self.mode != Mode::Unfenced && req.token < self.highest_token {
            return Err(Reject::StaleToken {
                token: req.token,
                highest: self.highest_token,
            });
        }
        if self.mode == Mode::FencedCas && req.read_version != self.version {
            return Err(Reject::VersionMismatch {
                read: req.read_version,
                current: self.version,
            });
        }
        self.highest_token = self.highest_token.max(req.token);
        self.value = req.new_value;
        self.version += 1;
        Ok(self.version)
    }
}

// ---------------------------------------------------------------------------------------------
// Clients, pauses, configuration
// ---------------------------------------------------------------------------------------------

/// The steps of one client round; a pause can be injected before any of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Ask the lock service for the lease.
    Acquire,
    /// Read the document.
    Read,
    /// Do the work (takes `work` ms).
    Compute,
    /// Local check "is my lease still valid?" (instant).
    Check,
    /// Write `read_value + 1` with the token, then release.
    Write,
}

impl Step {
    fn name(self) -> &'static str {
        match self {
            Step::Acquire => "acquire",
            Step::Read => "read",
            Step::Compute => "compute",
            Step::Check => "check",
            Step::Write => "write",
        }
    }
}

/// A scripted pause: client `client`, in round `round` (0-based), freezes for `dur` ms right before `before`.
#[derive(Clone, Copy, Debug)]
pub struct ScriptedPause {
    pub client: ClientId,
    pub round: u32,
    pub before: Step,
    pub dur: Ms,
}

/// Where pauses come from.
#[derive(Clone, Debug)]
pub enum Pauses {
    /// Never.
    None,
    /// Exactly the listed pauses.
    Scripted(Vec<ScriptedPause>),
    /// Before each of `Read`, `Compute`, `Check`, `Write`, with probability `p`, for a uniform
    /// duration in `1..=max_ms`.
    Random { p: f64, max_ms: Ms },
}

/// One client's workload.
#[derive(Clone, Copy, Debug)]
pub struct ClientPlan {
    /// First acquire attempt.
    pub start: Ms,
    /// Number of read-modify-write rounds.
    pub rounds: u32,
    /// Work duration is uniform in `work_min..=work_max` (fixed, no RNG draw, if equal).
    pub work_min: Ms,
    /// See `work_min`.
    pub work_max: Ms,
}

/// A lock-service failover injected at a given time.
#[derive(Clone, Copy, Debug)]
pub struct Failover {
    /// When.
    pub at: Ms,
    /// Does the token counter survive (consensus-backed) or roll back (async replica)?
    pub counter_survives: bool,
}

/// Everything a run depends on besides the RNG.
#[derive(Clone, Debug)]
pub struct Config {
    /// Storage mode.
    pub mode: Mode,
    /// Lease TTL.
    pub ttl_ms: Ms,
    /// One RPC round trip.
    pub latency_ms: Ms,
    /// Delay before retrying a denied acquire.
    pub retry_ms: Ms,
    /// Pause between two rounds of the same client.
    pub gap_ms: Ms,
    /// First fencing token.
    pub first_token: u64,
    /// Initial document value.
    pub initial_value: i64,
    /// One entry per client.
    pub clients: Vec<ClientPlan>,
    /// Pause source.
    pub pauses: Pauses,
    /// Optional lock-service failover.
    pub failover: Option<Failover>,
    /// Record a human-readable event log.
    pub record_log: bool,
}

impl Config {
    /// Lab defaults: ttl 10 s, RPC 5 ms, retry 1.5 s, first token 33, document value 100, no pauses, no log.
    pub fn new(mode: Mode, clients: Vec<ClientPlan>) -> Self {
        Self {
            mode,
            ttl_ms: TTL_MS,
            latency_ms: 5,
            retry_ms: 1_500,
            gap_ms: 50,
            first_token: FIRST_TOKEN,
            initial_value: 100,
            clients,
            pauses: Pauses::None,
            failover: None,
            record_log: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------------------------

/// One line of the event log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Virtual time.
    pub t: Ms,
    /// Acting client (`None` = lock service).
    pub client: Option<ClientId>,
    /// Text.
    pub text: String,
}

/// A write the storage accepted.
#[derive(Clone, Debug)]
pub struct AcceptedWrite {
    pub t: Ms,
    pub client: ClientId,
    pub token: u64,
    pub read_version: u64,
    pub version_after: u64,
    /// Ground truth: the writer's lease was expired or superseded when the write arrived.
    pub stale: bool,
    /// Ground truth: a later grant (another holder) had already started when the write arrived.
    pub superseded: bool,
}

/// A write the storage rejected.
#[derive(Clone, Debug)]
pub struct RejectedWrite {
    pub t: Ms,
    pub client: ClientId,
    pub token: u64,
    pub reason: Reject,
    /// Ground truth, as in [`AcceptedWrite::stale`].
    pub stale: bool,
}

/// Result of one run.
#[derive(Clone, Debug)]
pub struct RunReport {
    /// Storage mode of the run.
    pub mode: Mode,
    /// Event log (empty unless `record_log`).
    pub log: Vec<LogEntry>,
    /// Accepted writes in acceptance order.
    pub accepted: Vec<AcceptedWrite>,
    /// Rejected writes.
    pub rejected: Vec<RejectedWrite>,
    /// Rounds a client abandoned because its own check said "lease expired".
    pub aborted_by_check: u32,
    /// Denied acquire attempts.
    pub denied: u32,
    /// Pairs of different clients whose "I hold the lock" intervals (grant .. write/abort) overlap in time.
    pub overlaps: u32,
    /// Final document value.
    pub final_value: i64,
    /// Initial document value.
    pub initial_value: i64,
    /// Final document version.
    pub final_version: u64,
    /// Time of the last event.
    pub end_time: Ms,
    /// Grants issued by the lock service (ground truth).
    pub grants: Vec<Grant>,
}

impl RunReport {
    /// Accepted increments that are not reflected in the final value (each accepted write is one
    /// increment of the counter; a write based on an older read overwrites newer ones).
    pub fn lost_updates(&self) -> u64 {
        let applied = self.final_value - self.initial_value;
        (self.accepted.len() as i64 - applied).max(0) as u64
    }

    /// Accepted writes whose writer no longer held the lease.
    pub fn stale_accepted(&self) -> usize {
        self.accepted.iter().filter(|w| w.stale).count()
    }

    /// Accepted writes whose writer had already been superseded by another holder.
    pub fn superseded_accepted(&self) -> usize {
        self.accepted.iter().filter(|w| w.superseded).count()
    }

    /// Rejected writes whose writer no longer held the lease.
    pub fn stale_rejected(&self) -> usize {
        self.rejected.iter().filter(|w| w.stale).count()
    }

    /// Rejected writes whose writer still held a valid lease (not expired, not superseded): the
    /// storage refused legitimate work. The model does not retry them; a real client would have
    /// to re-read and redo the round.
    pub fn valid_rejected(&self) -> usize {
        self.rejected.iter().filter(|w| !w.stale).count()
    }

    /// Did the storage ever go back to an older token generation (accepted tokens not non-decreasing)?
    pub fn token_regressed(&self) -> bool {
        self.accepted.windows(2).any(|w| w[1].token < w[0].token)
    }
}

// ---------------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Ev {
    Client(ClientId),
    Failover,
}

struct ClientState {
    round: u32,
    step: Step,
    pause_rolled: bool,
    grant: usize,
    token: u64,
    deadline: Ms,
    read_value: i64,
    read_version: u64,
    belief_start: Ms,
}

struct World<'a> {
    cfg: &'a Config,
    lock: LockService,
    storage: Storage,
    clients: Vec<ClientState>,
    queue: BinaryHeap<Reverse<(Ms, u64, usize)>>,
    evs: Vec<Ev>,
    seq: u64,
    pause_rng: Rng,
    work_rng: Rng,
    report: RunReport,
    beliefs: Vec<(ClientId, Ms, Ms)>,
}

impl<'a> World<'a> {
    fn new(cfg: &'a Config, rng: &mut Rng) -> Self {
        let pause_rng = rng.fork(1);
        let work_rng = rng.fork(2);
        let mut w = World {
            cfg,
            lock: LockService::new(cfg.ttl_ms, cfg.first_token),
            storage: Storage::new(cfg.mode, cfg.initial_value),
            clients: Vec::new(),
            queue: BinaryHeap::new(),
            evs: Vec::new(),
            seq: 0,
            pause_rng,
            work_rng,
            report: RunReport {
                mode: cfg.mode,
                log: Vec::new(),
                accepted: Vec::new(),
                rejected: Vec::new(),
                aborted_by_check: 0,
                denied: 0,
                overlaps: 0,
                final_value: cfg.initial_value,
                initial_value: cfg.initial_value,
                final_version: 1,
                end_time: 0,
                grants: Vec::new(),
            },
            beliefs: Vec::new(),
        };
        if let Some(f) = cfg.failover {
            w.push(f.at, Ev::Failover);
        }
        for (i, p) in cfg.clients.iter().enumerate() {
            w.clients.push(ClientState {
                round: 0,
                step: Step::Acquire,
                pause_rolled: false,
                grant: 0,
                token: 0,
                deadline: 0,
                read_value: 0,
                read_version: 0,
                belief_start: 0,
            });
            if p.rounds > 0 {
                w.push(p.start, Ev::Client(i));
            }
        }
        w
    }

    fn push(&mut self, t: Ms, ev: Ev) {
        self.evs.push(ev);
        self.queue.push(Reverse((t, self.seq, self.evs.len() - 1)));
        self.seq += 1;
    }

    fn log(&mut self, t: Ms, client: Option<ClientId>, text: String) {
        if self.cfg.record_log {
            self.report.log.push(LogEntry { t, client, text });
        }
    }

    fn pause_for(&mut self, c: ClientId, step: Step) -> Option<Ms> {
        match &self.cfg.pauses {
            Pauses::None => None,
            Pauses::Scripted(list) => {
                let round = self.clients[c].round;
                list.iter()
                    .find(|p| p.client == c && p.round == round && p.before == step)
                    .map(|p| p.dur)
            }
            Pauses::Random { p, max_ms } => {
                if step == Step::Acquire {
                    return None;
                }
                if self.pause_rng.chance(*p) {
                    Some(self.pause_rng.range(1, *max_ms + 1))
                } else {
                    None
                }
            }
        }
    }

    fn exec(&mut self) {
        while let Some(Reverse((t, _, idx))) = self.queue.pop() {
            self.report.end_time = t;
            match self.evs[idx] {
                Ev::Failover => {
                    let f = self.cfg.failover.expect("failover event without config");
                    self.lock.failover(f.counter_survives);
                    let text = if f.counter_survives {
                        "lock service FAILOVER: lease table lost, token counter survives"
                            .to_string()
                    } else {
                        format!(
                            "lock service FAILOVER: lease table lost, token counter rolled back to {}",
                            self.lock.next_token()
                        )
                    };
                    self.log(t, None, text);
                }
                Ev::Client(c) => self.client_event(c, t),
            }
        }
        self.report.final_value = self.storage.read().0;
        self.report.final_version = self.storage.read().1;
        self.report.grants = self.lock.grants.clone();
        let b = &self.beliefs;
        let mut overlaps = 0;
        for i in 0..b.len() {
            for j in i + 1..b.len() {
                if b[i].0 != b[j].0 && b[i].1 < b[j].2 && b[j].1 < b[i].2 {
                    overlaps += 1;
                }
            }
        }
        self.report.overlaps = overlaps;
    }

    fn finish_round(&mut self, c: ClientId, t: Ms, delay: Ms) {
        let st = &mut self.clients[c];
        st.round += 1;
        st.step = Step::Acquire;
        if st.round < self.cfg.clients[c].rounds {
            self.push(t + delay, Ev::Client(c));
        }
    }

    fn client_event(&mut self, c: ClientId, t: Ms) {
        let step = self.clients[c].step;
        if !self.clients[c].pause_rolled {
            self.clients[c].pause_rolled = true;
            if let Some(d) = self.pause_for(c, step) {
                self.log(t, Some(c), format!("PAUSE {d} ms before {}", step.name()));
                self.push(t + d, Ev::Client(c));
                return;
            }
        }
        self.clients[c].pause_rolled = false;
        let lat = self.cfg.latency_ms;
        match step {
            Step::Acquire => match self.lock.acquire(c, t) {
                Ok(g) => {
                    let grant = self.lock.grants[g].clone();
                    let st = &mut self.clients[c];
                    st.grant = g;
                    st.token = grant.token;
                    // Conservative local deadline: measured from the moment the request was sent.
                    st.deadline = t + self.cfg.ttl_ms;
                    st.belief_start = t;
                    st.step = Step::Read;
                    self.log(
                        t,
                        Some(c),
                        format!(
                            "acquire -> token {}, lease until {}",
                            grant.token, grant.expires
                        ),
                    );
                    self.push(t + lat, Ev::Client(c));
                }
                Err(until) => {
                    self.report.denied += 1;
                    self.log(
                        t,
                        Some(c),
                        format!("acquire denied (lease held until {until})"),
                    );
                    self.push(t + self.cfg.retry_ms, Ev::Client(c));
                }
            },
            Step::Read => {
                let (v, ver) = self.storage.read();
                let st = &mut self.clients[c];
                st.read_value = v;
                st.read_version = ver;
                st.step = Step::Compute;
                self.log(t, Some(c), format!("read doc -> value {v} (v{ver})"));
                self.push(t + lat, Ev::Client(c));
            }
            Step::Compute => {
                let plan = self.cfg.clients[c];
                let work = if plan.work_min == plan.work_max {
                    plan.work_min
                } else {
                    self.work_rng.range(plan.work_min, plan.work_max + 1)
                };
                self.clients[c].step = Step::Check;
                self.log(t, Some(c), format!("compute {work} ms"));
                self.push(t + work, Ev::Client(c));
            }
            Step::Check => {
                let st = &mut self.clients[c];
                if t < st.deadline {
                    st.step = Step::Write;
                    let d = st.deadline;
                    self.log(t, Some(c), format!("check lease: ok (local deadline {d})"));
                    self.push(t, Ev::Client(c));
                } else {
                    let d = st.deadline;
                    let start = st.belief_start;
                    self.report.aborted_by_check += 1;
                    self.beliefs.push((c, start, t));
                    self.log(
                        t,
                        Some(c),
                        format!("check lease: EXPIRED (deadline {d}) -> abort round"),
                    );
                    self.finish_round(c, t, self.cfg.gap_ms);
                }
            }
            Step::Write => {
                let (token, grant, rv, rval, start) = {
                    let s = &self.clients[c];
                    (
                        s.token,
                        s.grant,
                        s.read_version,
                        s.read_value,
                        s.belief_start,
                    )
                };
                let stale = self.lock.is_stale(grant, t);
                let superseded = self.lock.is_superseded(grant, t);
                let tag = if stale {
                    "  [writer's lease already expired/superseded]"
                } else {
                    ""
                };
                match self.storage.write(WriteReq {
                    token,
                    read_version: rv,
                    new_value: rval + 1,
                }) {
                    Ok(ver) => {
                        self.report.accepted.push(AcceptedWrite {
                            t,
                            client: c,
                            token,
                            read_version: rv,
                            version_after: ver,
                            stale,
                            superseded,
                        });
                        self.log(
                            t,
                            Some(c),
                            format!("write(token {token}, read v{rv}) -> ACCEPTED, value {} (v{ver}){tag}", rval + 1),
                        );
                    }
                    Err(reason) => {
                        self.report.rejected.push(RejectedWrite {
                            t,
                            client: c,
                            token,
                            reason,
                            stale,
                        });
                        let why = match reason {
                            Reject::StaleToken { token, highest } => {
                                format!("token {token} < highest seen {highest}")
                            }
                            Reject::VersionMismatch { read, current } => {
                                format!("version mismatch (read v{read}, now v{current})")
                            }
                        };
                        self.log(
                            t,
                            Some(c),
                            format!("write(token {token}, read v{rv}) -> REJECTED: {why}{tag}"),
                        );
                    }
                }
                self.beliefs.push((c, start, t));
                self.lock.release(grant, t);
                self.finish_round(c, t, lat + self.cfg.gap_ms);
            }
        }
    }
}

/// Runs one simulation. All randomness comes from `rng` (two forked streams: pauses and work).
pub fn run(cfg: &Config, rng: &mut Rng) -> RunReport {
    let mut w = World::new(cfg, rng);
    w.exec();
    w.report
}

// ---------------------------------------------------------------------------------------------
// Scripted scenarios
// ---------------------------------------------------------------------------------------------

const A: ClientId = 0;
const B: ClientId = 1;

fn script(
    mode: Mode,
    a: ClientPlan,
    b: ClientPlan,
    pauses: Vec<ScriptedPause>,
    failover: Option<Failover>,
) -> RunReport {
    let mut cfg = Config::new(mode, vec![a, b]);
    cfg.pauses = Pauses::Scripted(pauses);
    cfg.failover = failover;
    cfg.record_log = true;
    run(&cfg, &mut Rng::new(0))
}

fn plan(start: Ms, work: Ms) -> ClientPlan {
    ClientPlan {
        start,
        rounds: 1,
        work_min: work,
        work_max: work,
    }
}

/// Scenario 1 (the chapter's figure): A gets token 33, reads, checks its lease OK, then pauses
/// 15 s (> ttl 10 s); B (token 34) writes after the lease expired; A wakes up and writes.
pub fn scenario_pause(mode: Mode) -> RunReport {
    script(
        mode,
        plan(0, 2_000),
        plan(9_000, 3_000),
        vec![ScriptedPause {
            client: A,
            round: 0,
            before: Step::Write,
            dur: 15_000,
        }],
        None,
    )
}

/// Check-then-act: the same 15 s pause as [`scenario_pause`] but placed *before the check*
/// (`before_check = true`) or *between the check and the write* (`false`). Unfenced storage.
pub fn scenario_check_then_act(before_check: bool) -> RunReport {
    let before = if before_check {
        Step::Check
    } else {
        Step::Write
    };
    script(
        Mode::Unfenced,
        plan(0, 2_000),
        plan(9_000, 3_000),
        vec![ScriptedPause {
            client: A,
            round: 0,
            before,
            dur: 15_000,
        }],
        None,
    )
}

/// Scenario 2: residual window of token-only fencing. A (33) read and checked, pauses 11.5 s; B
/// acquires (34) and reads; A's late write (33) lands *before* B's write. Token-only storage
/// accepts A (it has not seen 34 yet) and then B overwrites it from an older read.
pub fn scenario_residual(mode: Mode) -> RunReport {
    script(
        mode,
        plan(0, 1_000),
        plan(10_500, 3_000),
        vec![ScriptedPause {
            client: A,
            round: 0,
            before: Step::Write,
            dur: 11_500,
        }],
        None,
    )
}

/// Scenario 3: the lock service fails over at t=5 s and loses its lease table; B acquires at
/// t=6 s while A (token 33) is still computing and then pauses 15 s before writing. With
/// `counter_survives = false` the token counter rolled back to the replica's value and B gets
/// the same token 33.
pub fn scenario_token_source(mode: Mode, counter_survives: bool) -> RunReport {
    script(
        mode,
        plan(0, 2_000),
        plan(6_000, 1_000),
        vec![ScriptedPause {
            client: A,
            round: 0,
            before: Step::Write,
            dur: 15_000,
        }],
        Some(Failover {
            at: 5_000,
            counter_survives,
        }),
    )
}

/// Outcome of the clock-skew scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkewReport {
    /// Client wall clock minus lock-service wall clock (negative = client behind).
    pub skew_ms: i64,
    /// Real time elapsed since the client sent its acquire request.
    pub elapsed_ms: Ms,
    /// Ground truth on the lock service's clock.
    pub truly_valid: bool,
    /// Client check: `client_wall_now < expiry_timestamp_from_server`.
    pub wall_clock_says_valid: bool,
    /// Client check: `monotonic_now < monotonic_at_send + ttl`.
    pub monotonic_says_valid: bool,
    /// Token of the other holder that already wrote (34).
    pub other_token: u64,
    /// A writes with its (stale) token 33 after B's write: accepted by `Unfenced`?
    pub unfenced_accepts: bool,
    /// ... accepted by `Fenced`?
    pub fenced_accepts: bool,
}

/// Scenario 4: no pause at all, only a wrong clock. The lock service replies with an absolute
/// expiry timestamp on *its* clock; the client's wall clock is `skew_ms` off. `elapsed_ms` after
/// sending the request the client tests its lease two ways. B has acquired (34) and written
/// once the lease really expired, and A then writes with token 33 to each storage mode.
pub fn scenario_clock_skew(skew_ms: i64, elapsed_ms: Ms) -> SkewReport {
    let server_t0: Ms = 1_000_000; // lock service wall clock when it granted
    let mono_t0: Ms = 5_000_000; // client monotonic clock when it sent the request (arbitrary origin)
    let mut lock = LockService::new(TTL_MS, FIRST_TOKEN);
    let g = lock.acquire(A, server_t0).expect("fresh lock");
    let expires_at = lock.grants[g].expires; // timestamp the server sends back
    let server_now = server_t0 + elapsed_ms;
    let client_wall_now = (server_now as i64 + skew_ms) as Ms;
    let wall_clock_says_valid = client_wall_now < expires_at;
    let monotonic_says_valid = mono_t0 + elapsed_ms < mono_t0 + TTL_MS;
    let truly_valid = !lock.is_stale(g, server_now);
    // B takes over as soon as the lease is really over (here: 1 ms after expiry) and writes.
    let b_time = expires_at + 1;
    let gb = lock.acquire(B, b_time).expect("lease expired");
    let other_token = lock.grants[gb].token;
    let mut verdicts = [false; 2];
    for (i, mode) in [Mode::Unfenced, Mode::Fenced].into_iter().enumerate() {
        let mut s = Storage::new(mode, 100);
        let (v, ver) = s.read();
        s.write(WriteReq {
            token: other_token,
            read_version: ver,
            new_value: v + 1,
        })
        .expect("B writes first");
        verdicts[i] = s
            .write(WriteReq {
                token: FIRST_TOKEN,
                read_version: ver,
                new_value: v + 1,
            })
            .is_ok();
    }
    SkewReport {
        skew_ms,
        elapsed_ms,
        truly_valid,
        wall_clock_says_valid,
        monotonic_says_valid,
        other_token,
        unfenced_accepts: verdicts[0],
        fenced_accepts: verdicts[1],
    }
}

// ---------------------------------------------------------------------------------------------
// Seeded sweep
// ---------------------------------------------------------------------------------------------

/// Parameters of the random sweep: two clients, each doing `rounds` read-modify-write increments.
#[derive(Clone, Copy, Debug)]
pub struct SweepParams {
    /// Rounds per client.
    pub rounds: u32,
    /// Probability of a pause before each of read/compute/check/write.
    pub p_pause: f64,
    /// Work duration range.
    pub work: (Ms, Ms),
}

impl SweepParams {
    /// Lab defaults: 5 rounds, work 1-4 s, pause probability 0.03, pause up to 3 x ttl.
    pub const LAB: SweepParams = SweepParams {
        rounds: 5,
        p_pause: 0.03,
        work: (1_000, 4_000),
    };

    /// Same but no pauses at all.
    pub const NO_PAUSE: SweepParams = SweepParams {
        p_pause: 0.0,
        ..SweepParams::LAB
    };
}

/// Aggregate over many runs of one storage mode.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SweepRow {
    pub runs: u32,
    /// Runs in which the "I hold the lock" intervals of two clients overlapped.
    pub runs_overlap: u32,
    /// Runs with at least one write attempt by a client whose lease was already gone.
    pub runs_stale_attempt: u32,
    /// Stale writes the storage accepted.
    pub stale_accepted: u64,
    /// Of those, writes whose writer had been superseded by another holder (somebody else was
    /// already the legitimate holder); the rest merely wrote after their own lease ran out.
    pub superseded_accepted: u64,
    /// Stale writes the storage rejected.
    pub stale_rejected: u64,
    /// Rejected writes whose writer still held a valid lease (legitimate work refused; the model
    /// does not retry it).
    pub valid_rejected: u64,
    /// Runs with at least one lost update.
    pub runs_lost: u32,
    /// Total lost updates.
    pub lost_updates: u64,
    /// Runs in which accepted tokens went backwards.
    pub runs_token_regress: u32,
    /// Total accepted writes.
    pub accepted: u64,
    /// Rounds abandoned by the client's own lease check.
    pub aborted_by_check: u64,
}

/// Config for sweep run: clients A and B both start at 0 (A first in the tie-break).
pub fn sweep_config(mode: Mode, params: SweepParams) -> Config {
    let c = ClientPlan {
        start: 0,
        rounds: params.rounds,
        work_min: params.work.0,
        work_max: params.work.1,
    };
    let mut cfg = Config::new(mode, vec![c, c]);
    cfg.initial_value = 0;
    cfg.pauses = if params.p_pause > 0.0 {
        Pauses::Random {
            p: params.p_pause,
            max_ms: 3 * TTL_MS,
        }
    } else {
        Pauses::None
    };
    cfg
}

/// The run with index `i` of a sweep (same seed and index give the same schedule in every mode).
pub fn sweep_run(seed: u64, i: u32, mode: Mode, params: SweepParams) -> RunReport {
    let mut rng = Rng::new(seed).fork(i as u64);
    run(&sweep_config(mode, params), &mut rng)
}

/// Aggregates `runs` runs of `mode`.
pub fn sweep(seed: u64, runs: u32, mode: Mode, params: SweepParams) -> SweepRow {
    let mut row = SweepRow {
        runs,
        ..SweepRow::default()
    };
    for i in 0..runs {
        let r = sweep_run(seed, i, mode, params);
        row.runs_overlap += (r.overlaps > 0) as u32;
        let stale_attempts = r.stale_accepted() + r.stale_rejected();
        row.runs_stale_attempt += (stale_attempts > 0) as u32;
        row.stale_accepted += r.stale_accepted() as u64;
        row.superseded_accepted += r.superseded_accepted() as u64;
        row.stale_rejected += r.stale_rejected() as u64;
        row.valid_rejected += r.valid_rejected() as u64;
        row.runs_lost += (r.lost_updates() > 0) as u32;
        row.lost_updates += r.lost_updates();
        row.runs_token_regress += r.token_regressed() as u32;
        row.accepted += r.accepted.len() as u64;
        row.aborted_by_check += r.aborted_by_check as u64;
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(r: &RunReport) -> Vec<u64> {
        r.accepted.iter().map(|w| w.token).collect()
    }

    // ---- building blocks -------------------------------------------------------------------

    #[test]
    fn lock_service_tokens_strictly_increase_and_expiry_boundary_is_exclusive() {
        let mut l = LockService::new(10, 33);
        let g0 = l.acquire(0, 0).unwrap();
        assert_eq!(l.grants[g0].token, 33);
        assert_eq!(l.acquire(1, 9), Err(10), "still held at t=9");
        let g1 = l.acquire(1, 10).expect("free exactly at expiry");
        assert_eq!(l.grants[g1].token, 34);
        assert!(l.is_stale(g0, 10) && !l.is_stale(g1, 10));
        // A late release from the expired holder must not free the new holder's lease.
        l.release(g0, 11);
        assert_eq!(l.acquire(0, 12), Err(20));
        l.release(g1, 12);
        let g2 = l.acquire(0, 12).unwrap();
        assert_eq!(l.grants[g2].token, 35);
    }

    #[test]
    fn storage_token_rule_boundaries() {
        let w = |token, read_version| WriteReq {
            token,
            read_version,
            new_value: 1,
        };
        let mut s = Storage::new(Mode::Fenced, 0);
        assert!(s.write(w(34, 1)).is_ok());
        assert_eq!(
            s.write(w(33, 2)),
            Err(Reject::StaleToken {
                token: 33,
                highest: 34
            })
        );
        assert!(
            s.write(w(34, 2)).is_ok(),
            "an equal token is accepted: the holder writes many times"
        );
        assert!(
            s.write(w(35, 1)).is_ok(),
            "plain Fenced ignores the version"
        );
        let mut c = Storage::new(Mode::FencedCas, 0);
        assert!(c.write(w(34, 1)).is_ok());
        assert_eq!(
            c.write(w(34, 1)),
            Err(Reject::VersionMismatch {
                read: 1,
                current: 2
            })
        );
        assert_eq!(
            c.write(w(33, 2)),
            Err(Reject::StaleToken {
                token: 33,
                highest: 34
            })
        );
        let mut u = Storage::new(Mode::Unfenced, 0);
        assert!(u.write(w(34, 1)).is_ok() && u.write(w(33, 1)).is_ok());
    }

    // ---- scenario 1 -------------------------------------------------------------------------

    #[test]
    fn scenario1_unfenced_clobbers_and_fenced_rejects_with_the_right_reason() {
        let u = scenario_pause(Mode::Unfenced);
        assert_eq!(
            tokens(&u),
            vec![34, 33],
            "B (34) writes first, A's stale 33 lands after"
        );
        assert_eq!(u.accepted[1].client, A);
        assert!(u.accepted[1].stale);
        assert!(u.overlaps >= 1, "A still 'holds' the lock while B holds it");
        assert_eq!((u.final_value, u.final_version), (101, 3));
        assert_eq!(
            u.lost_updates(),
            1,
            "two accepted increments, counter only went up by one"
        );
        assert_eq!(
            u.grants.iter().map(|g| g.token).collect::<Vec<_>>(),
            vec![33, 34]
        );
        assert!(u.denied >= 1, "B was refused while A's lease was alive");

        let f = scenario_pause(Mode::Fenced);
        assert_eq!(tokens(&f), vec![34]);
        assert_eq!(f.rejected.len(), 1);
        assert_eq!(f.rejected[0].client, A);
        assert_eq!(
            f.rejected[0].reason,
            Reject::StaleToken {
                token: 33,
                highest: 34
            }
        );
        assert!(f.rejected[0].stale);
        assert_eq!(f.lost_updates(), 0);
        assert_eq!((f.final_value, f.final_version), (101, 2));
        // Fencing does not remove the overlap, it makes it harmless.
        assert_eq!(f.overlaps, u.overlaps);
    }

    #[test]
    fn check_then_act_pause_before_check_is_caught_pause_before_write_is_not() {
        let caught = scenario_check_then_act(true);
        assert_eq!(caught.aborted_by_check, 1);
        assert_eq!(caught.stale_accepted(), 0);
        assert_eq!(caught.lost_updates(), 0);
        let missed = scenario_check_then_act(false);
        assert_eq!(missed.aborted_by_check, 0);
        assert_eq!(
            missed.stale_accepted(),
            1,
            "the check passed at t=2010, the write happens at t=17010"
        );
        assert_eq!(missed.lost_updates(), 1);
    }

    // ---- scenario 2 -------------------------------------------------------------------------

    #[test]
    fn scenario2_token_only_fencing_has_a_residual_window_cas_closes_it() {
        let f = scenario_residual(Mode::Fenced);
        assert_eq!(
            tokens(&f),
            vec![33, 34],
            "A's late write is accepted first, B's 34 second"
        );
        assert!(f.accepted[0].stale && !f.accepted[1].stale);
        assert_eq!(
            f.accepted[1].read_version, 1,
            "B's write is based on the read before A's write"
        );
        assert!(f.rejected.is_empty());
        assert_eq!(f.lost_updates(), 1, "B overwrote A's increment");
        assert_eq!(f.final_value, 101);

        let c = scenario_residual(Mode::FencedCas);
        assert_eq!(
            tokens(&c),
            vec![33],
            "A's late write is still accepted (it is a valid CAS) ..."
        );
        assert_eq!(c.rejected.len(), 1);
        assert_eq!(c.rejected[0].client, B);
        assert_eq!(
            c.rejected[0].reason,
            Reject::VersionMismatch {
                read: 1,
                current: 2
            }
        );
        assert_eq!(
            c.lost_updates(),
            0,
            "... but nothing is lost: B is told to re-read"
        );
        assert_eq!(
            c.valid_rejected(),
            1,
            "B's write was legitimate, CAS refused it"
        );
        assert_eq!(f.valid_rejected(), 0);

        let u = scenario_residual(Mode::Unfenced);
        assert_eq!(u.lost_updates(), 1);
    }

    // ---- scenario 3 -------------------------------------------------------------------------

    #[test]
    fn scenario3_rolled_back_counter_issues_the_same_token_twice_and_fencing_is_defeated() {
        let bad = scenario_token_source(Mode::Fenced, false);
        let toks: Vec<u64> = bad.grants.iter().map(|g| g.token).collect();
        assert_eq!(
            toks,
            vec![33, 33],
            "same token issued to two different holders"
        );
        assert_eq!(tokens(&bad), vec![33, 33], "storage accepts both");
        assert!(bad.rejected.is_empty());
        assert!(
            bad.accepted[1].stale,
            "A's write is stale: B's grant superseded it"
        );
        assert_eq!(bad.lost_updates(), 1);
        assert!(bad.overlaps >= 1);

        let good = scenario_token_source(Mode::Fenced, true);
        let toks: Vec<u64> = good.grants.iter().map(|g| g.token).collect();
        assert_eq!(toks, vec![33, 34]);
        assert_eq!(tokens(&good), vec![34]);
        assert_eq!(good.rejected.len(), 1);
        assert_eq!(
            good.rejected[0].reason,
            Reject::StaleToken {
                token: 33,
                highest: 34
            }
        );
        assert_eq!(good.lost_updates(), 0);
        // The overlap (two holders after a failover that lost the lease table) exists in both runs.
        assert!(good.overlaps >= 1);
    }

    // ---- scenario 4 -------------------------------------------------------------------------

    #[test]
    fn scenario4_wall_clock_check_is_fooled_by_skew_monotonic_since_send_is_not() {
        let r = scenario_clock_skew(-8_000, 12_000);
        assert!(!r.truly_valid);
        assert!(
            r.wall_clock_says_valid,
            "client clock 8 s behind: it thinks it still has 6 s"
        );
        assert!(!r.monotonic_says_valid);
        assert_eq!(r.other_token, 34);
        assert!(r.unfenced_accepts);
        assert!(
            !r.fenced_accepts,
            "fencing does not depend on anybody's clock"
        );
        // No skew: both checks agree with the truth, before and after expiry.
        for elapsed in [0, 5_000, 9_999, 10_000, 12_000] {
            let r = scenario_clock_skew(0, elapsed);
            assert_eq!(r.wall_clock_says_valid, r.truly_valid, "elapsed {elapsed}");
            assert_eq!(r.monotonic_says_valid, r.truly_valid, "elapsed {elapsed}");
        }
        // A client clock that is AHEAD errs on the safe side (liveness cost only).
        let ahead = scenario_clock_skew(8_000, 4_000);
        assert!(ahead.truly_valid && !ahead.wall_clock_says_valid && ahead.monotonic_says_valid);
    }

    // ---- sweeps -----------------------------------------------------------------------------

    #[test]
    fn same_seed_gives_same_schedule_in_every_mode_and_is_deterministic() {
        for i in 0..200 {
            let u = sweep_run(7, i, Mode::Unfenced, SweepParams::LAB);
            let f = sweep_run(7, i, Mode::Fenced, SweepParams::LAB);
            let c = sweep_run(7, i, Mode::FencedCas, SweepParams::LAB);
            for other in [&f, &c] {
                assert_eq!(u.overlaps, other.overlaps);
                assert_eq!(u.end_time, other.end_time);
                assert_eq!(u.grants, other.grants);
                assert_eq!(u.aborted_by_check, other.aborted_by_check);
                assert_eq!(u.denied, other.denied);
                // Same schedule => same number of write attempts; the mode only splits them
                // into accepted and rejected.
                assert_eq!(
                    u.accepted.len() + u.rejected.len(),
                    other.accepted.len() + other.rejected.len(),
                    "run {i}"
                );
            }
            let again = sweep_run(7, i, Mode::Unfenced, SweepParams::LAB);
            assert_eq!(again.final_value, u.final_value);
            assert_eq!(again.end_time, u.end_time);
        }
    }

    #[test]
    fn sweep_invariants_over_2000_seeds() {
        let n = 2000;
        let mut regress_unfenced = 0;
        let mut valid_rejected_cas = 0;
        let mut residual_runs = 0;
        for i in 0..n {
            let u = sweep_run(1, i, Mode::Unfenced, SweepParams::LAB);
            let f = sweep_run(1, i, Mode::Fenced, SweepParams::LAB);
            let c = sweep_run(1, i, Mode::FencedCas, SweepParams::LAB);
            // Fenced storage never goes back to an older generation.
            assert!(!f.token_regressed(), "run {i}: {:?}", tokens(&f));
            assert!(!c.token_regressed(), "run {i}: {:?}", tokens(&c));
            // CAS never loses an update, even though stale writers still appear.
            assert_eq!(c.lost_updates(), 0, "run {i}");
            // Same schedule in all three modes: only the verdicts differ.
            assert_eq!(u.rejected.len(), 0, "unfenced storage rejects nothing");
            assert!(
                f.rejected.len() <= c.rejected.len(),
                "run {i}: CAS refuses a superset"
            );
            assert_eq!(
                f.accepted.len() + f.rejected.len(),
                u.accepted.len(),
                "run {i}: same attempts, only verdicts differ"
            );
            regress_unfenced += u.token_regressed() as u32;
            residual_runs += (f.lost_updates() > 0) as u32;
            // With a token check, a lost update needs a stale writer that slipped in before the
            // successor's first write; without any such write nothing can be lost.
            if f.lost_updates() > 0 {
                assert!(f.superseded_accepted() > 0, "run {i}");
            }
            // CAS reaches zero lost updates by refusing: whenever the token-only run lost an
            // update, the CAS run turned some write into a rejection.
            if f.lost_updates() > 0 {
                assert!(c.rejected.len() > f.rejected.len(), "run {i}");
            }
            valid_rejected_cas += c.valid_rejected() as u32;
        }
        assert!(
            regress_unfenced > 0,
            "naive storage does go back to older tokens"
        );
        assert!(
            residual_runs > 0,
            "token-only fencing keeps a residual window the sweep can hit"
        );
        assert!(
            valid_rejected_cas > 0,
            "CAS refuses some writes of holders whose lease was still valid"
        );
    }

    #[test]
    fn unfenced_loses_updates_fenced_loses_fewer_cas_none() {
        let n = 2000;
        let u = sweep(42, n, Mode::Unfenced, SweepParams::LAB);
        let f = sweep(42, n, Mode::Fenced, SweepParams::LAB);
        let c = sweep(42, n, Mode::FencedCas, SweepParams::LAB);
        assert!(u.runs_lost > 0);
        assert!(
            f.lost_updates < u.lost_updates,
            "token check removes most of the damage"
        );
        assert!(f.runs_lost > 0, "but not all of it");
        assert_eq!(c.lost_updates, 0);
        assert_eq!(c.runs_lost, 0);
        // Stale writers exist in every mode (same schedule): fencing changes verdicts, not behaviour.
        assert_eq!(u.runs_stale_attempt, f.runs_stale_attempt);
        assert_eq!(u.runs_overlap, c.runs_overlap);
        assert!(u.runs_overlap > 0 && u.stale_accepted > 0);
        assert!(f.stale_rejected > 0 && c.stale_rejected > 0);
        assert_eq!(u.stale_rejected, 0);
        assert!(c.valid_rejected > 0);
        assert_eq!(
            f.valid_rejected, 0,
            "token check alone never refuses a valid holder here"
        );
        assert_eq!(u.valid_rejected, 0);
    }

    #[test]
    fn pause_free_runs_lose_nothing_and_never_overlap_in_any_mode() {
        for mode in Mode::ALL {
            let row = sweep(5, 500, mode, SweepParams::NO_PAUSE);
            assert_eq!(row.runs_overlap, 0, "{mode:?}");
            assert_eq!(row.lost_updates, 0, "{mode:?}");
            assert_eq!(
                row.stale_accepted + row.stale_rejected + row.valid_rejected,
                0,
                "{mode:?}"
            );
            assert_eq!(row.aborted_by_check, 0, "{mode:?}");
            assert_eq!(
                row.accepted,
                500 * 2 * 5,
                "every round of every client commits"
            );
        }
    }

    #[test]
    fn pause_free_run_counts_exactly_ten_increments() {
        let r = sweep_run(3, 0, Mode::FencedCas, SweepParams::NO_PAUSE);
        assert_eq!(r.final_value, 10);
        assert_eq!(r.final_version, 11);
        assert_eq!(
            tokens(&r),
            (33..43).collect::<Vec<_>>(),
            "ten sequential grants, ten tokens"
        );
    }
}
