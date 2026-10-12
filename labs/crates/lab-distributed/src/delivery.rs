//! Delivery semantics, idempotency and the outbox: four deterministic crash-injection simulations.
//!
//! Every "durable store" here is an in-memory struct whose atomic operations (a single `tick`
//! followed by a state change with no further tick) stand for a database transaction or a
//! Kafka transaction. This is a model of the *protocols*, not of those systems.
//!
//! Crashes are injected by [`Crashes`], which is consulted by a `tick` before every step that
//! touches durable state. A crash drops all volatile state of the actor and restarts it from
//! its durable state. Three crash sources exist: none, an explicit bit mask (used to enumerate
//! *every* crash pattern over a finite horizon) and seeded random.
//!
//! 1. [`run_consumer`]: commit-then-process, process-then-commit, sink-side dedup (non-atomic
//!    and atomic) and "offset stored atomically with the output state".
//! 2. [`run_producer`]: a producer whose acks can be lost, with and without broker-side dedup on
//!    `(producer_id, sequence)`.
//! 3. [`PayServer`]: an `Idempotency-Key` payment endpoint, with a key store that is or is not
//!    atomic with the charge.
//! 4. [`run_dual`]: dual write (both orders) versus transactional outbox.

use crate::rng::Rng;
use std::collections::{BTreeMap, BTreeSet};

// ---------------------------------------------------------------------------------------------
// Crash injection
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum CrashMode {
    Never,
    /// Crash at global tick `t` iff bit `t` of the mask is set (`t < 64`).
    Mask(u64),
    Random {
        rng: Rng,
        p: f64,
    },
}

/// Decides, tick by tick, whether the acting process crashes before its next durable step.
///
/// The tick counter is global to one run and keeps counting across restarts, so a mask
/// describes a complete crash pattern for the whole run, including crashes during recovery.
#[derive(Clone, Debug)]
pub struct Crashes {
    mode: CrashMode,
    tick: u64,
    fired: u32,
}

impl Crashes {
    /// Never crashes.
    pub fn never() -> Self {
        Self {
            mode: CrashMode::Never,
            tick: 0,
            fired: 0,
        }
    }

    /// Crashes exactly at the ticks whose bit is set in `mask`.
    pub fn mask(mask: u64) -> Self {
        Self {
            mode: CrashMode::Mask(mask),
            tick: 0,
            fired: 0,
        }
    }

    /// Crashes at every tick independently with probability `p`.
    pub fn random(rng: Rng, p: f64) -> Self {
        Self {
            mode: CrashMode::Random { rng, p },
            tick: 0,
            fired: 0,
        }
    }

    /// Consumes one tick; `true` means "crash now, before the step".
    pub fn crash_now(&mut self) -> bool {
        let t = self.tick;
        self.tick += 1;
        let hit = match &mut self.mode {
            CrashMode::Never => false,
            CrashMode::Mask(m) => t < 64 && (*m >> t) & 1 == 1,
            CrashMode::Random { rng, p } => rng.chance(*p),
        };
        if hit {
            self.fired += 1;
        }
        hit
    }

    /// Ticks consumed so far (crashed or not).
    pub fn ticks(&self) -> u64 {
        self.tick
    }

    /// Number of crashes fired so far.
    pub fn fired(&self) -> u32 {
        self.fired
    }
}

/// Marker error: the actor crashed and lost its volatile state.
#[derive(Debug)]
struct Crashed;

fn tick(c: &mut Crashes) -> Result<(), Crashed> {
    if c.crash_now() { Err(Crashed) } else { Ok(()) }
}

// ---------------------------------------------------------------------------------------------
// Part 1: consumer semantics
// ---------------------------------------------------------------------------------------------

/// Order of "process" and "commit offset" plus the sink-side defence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumerDesign {
    /// Commit the offset of the batch, then process it: at-most-once.
    AtMostOnce,
    /// Process the batch, then commit the offset: at-least-once.
    AtLeastOnce,
    /// At-least-once plus sink dedup done as separate steps: check, apply effect, record id.
    DedupApplyThenRecord,
    /// At-least-once plus sink dedup done as separate steps: check+record id, then apply effect.
    DedupRecordThenApply,
    /// At-least-once plus dedup where the id is recorded in the same atomic commit as the effect.
    DedupAtomic,
    /// No offset commit in the broker and no id table: the offset lives in the sink and is
    /// written in the same atomic commit as the effect.
    OffsetWithState,
}

impl ConsumerDesign {
    /// All designs in the order the lab prints them.
    pub const ALL: [ConsumerDesign; 6] = [
        ConsumerDesign::AtMostOnce,
        ConsumerDesign::AtLeastOnce,
        ConsumerDesign::DedupApplyThenRecord,
        ConsumerDesign::DedupRecordThenApply,
        ConsumerDesign::DedupAtomic,
        ConsumerDesign::OffsetWithState,
    ];

    /// Short label for tables.
    pub fn label(self) -> &'static str {
        match self {
            ConsumerDesign::AtMostOnce => "commit->process",
            ConsumerDesign::AtLeastOnce => "process->commit",
            ConsumerDesign::DedupApplyThenRecord => "dedup apply,record",
            ConsumerDesign::DedupRecordThenApply => "dedup record,apply",
            ConsumerDesign::DedupAtomic => "dedup atomic",
            ConsumerDesign::OffsetWithState => "offset+state atomic",
        }
    }
}

/// The amount event `offset` adds to the ledger. The ground truth is the sum, once per event.
pub fn amount(offset: usize) -> i64 {
    10 * (offset as i64 + 1)
}

/// Durable state of one consumer deployment. Volatile state (the batch being processed) is
/// dropped on a crash simply by returning from `attempt`.
struct Durable {
    /// Offset committed in the broker (used by every design except `OffsetWithState`).
    committed: usize,
    /// Sink: how many times each event's effect was applied.
    applied: Vec<u32>,
    /// Sink: ids recorded by the dedup designs.
    seen: BTreeSet<usize>,
    /// Sink: offset stored together with the state (`OffsetWithState` only).
    sink_offset: usize,
}

impl Durable {
    fn apply(&mut self, offset: usize) {
        self.applied[offset] += 1;
    }
}

/// Outcome of one consumer run over a log of `n` events.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsumerRun {
    /// How many times the effect of each event reached the sink.
    pub applied: Vec<u32>,
    /// Crashes that happened.
    pub crashes: u32,
    /// Ticks consumed (steps attempted, including those that crashed).
    pub ticks: u64,
}

impl ConsumerRun {
    /// Events whose effect was applied 0 times.
    pub fn lost(&self) -> usize {
        self.applied.iter().filter(|&&c| c == 0).count()
    }
    /// Events whose effect was applied 2 or more times.
    pub fn dup(&self) -> usize {
        self.applied.iter().filter(|&&c| c >= 2).count()
    }
    /// Events applied exactly once.
    pub fn exact(&self) -> usize {
        self.applied.iter().filter(|&&c| c == 1).count()
    }
    /// Ledger balance the sink actually holds.
    pub fn balance(&self) -> i64 {
        self.applied
            .iter()
            .enumerate()
            .map(|(o, &c)| amount(o) * i64::from(c))
            .sum()
    }
    /// Ledger balance if every event were applied exactly once.
    pub fn expected_balance(&self) -> i64 {
        (0..self.applied.len()).map(amount).sum()
    }
}

/// One attempt of the consumer loop: poll `[pos, end)` from the log and process it.
fn attempt(
    design: ConsumerDesign,
    d: &mut Durable,
    pos: usize,
    end: usize,
    c: &mut Crashes,
) -> Result<(), Crashed> {
    match design {
        ConsumerDesign::AtMostOnce => {
            tick(c)?;
            d.committed = end;
            for o in pos..end {
                tick(c)?;
                d.apply(o);
            }
        }
        ConsumerDesign::AtLeastOnce => {
            for o in pos..end {
                tick(c)?;
                d.apply(o);
            }
            tick(c)?;
            d.committed = end;
        }
        ConsumerDesign::DedupApplyThenRecord => {
            for o in pos..end {
                // The check is a read: crashing right after it equals crashing at the next tick.
                if d.seen.contains(&o) {
                    continue;
                }
                tick(c)?;
                d.apply(o);
                tick(c)?;
                d.seen.insert(o);
            }
            tick(c)?;
            d.committed = end;
        }
        ConsumerDesign::DedupRecordThenApply => {
            for o in pos..end {
                if d.seen.contains(&o) {
                    continue;
                }
                tick(c)?;
                d.seen.insert(o);
                tick(c)?;
                d.apply(o);
            }
            tick(c)?;
            d.committed = end;
        }
        ConsumerDesign::DedupAtomic => {
            for o in pos..end {
                tick(c)?;
                // One atomic commit: "insert id; if it was new, apply the effect".
                if d.seen.insert(o) {
                    d.apply(o);
                }
            }
            tick(c)?;
            d.committed = end;
        }
        ConsumerDesign::OffsetWithState => {
            tick(c)?;
            // One atomic commit: the whole batch's effects and the new offset.
            for o in pos..end {
                d.apply(o);
            }
            d.sink_offset = end;
        }
    }
    Ok(())
}

/// Runs a single-partition consumer over `n` events, polling `batch` events at a time and
/// restarting from the durable offset after every crash.
pub fn run_consumer(
    design: ConsumerDesign,
    n: usize,
    batch: usize,
    crashes: &mut Crashes,
) -> ConsumerRun {
    assert!(batch > 0, "batch must be positive");
    let mut d = Durable {
        committed: 0,
        applied: vec![0; n],
        seen: BTreeSet::new(),
        sink_offset: 0,
    };
    let mut crash_count = 0;
    loop {
        let pos = if design == ConsumerDesign::OffsetWithState {
            d.sink_offset
        } else {
            d.committed
        };
        if pos >= n {
            break;
        }
        let end = (pos + batch).min(n);
        if attempt(design, &mut d, pos, end, crashes).is_err() {
            crash_count += 1;
        }
    }
    ConsumerRun {
        applied: d.applied,
        crashes: crash_count,
        ticks: crashes.ticks(),
    }
}

/// Counts accumulated over many consumer runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Runs accumulated.
    pub runs: u32,
    /// Events over all runs.
    pub events: u64,
    /// Events applied 0 times.
    pub lost: u64,
    /// Events applied 2 or more times.
    pub dup: u64,
    /// Events applied exactly once.
    pub exact: u64,
    /// Runs with at least one lost event.
    pub runs_lost: u32,
    /// Runs with at least one duplicated event.
    pub runs_dup: u32,
    /// Crashes fired.
    pub crashes: u64,
}

impl Tally {
    fn add(&mut self, r: &ConsumerRun) {
        self.runs += 1;
        self.events += r.applied.len() as u64;
        self.lost += r.lost() as u64;
        self.dup += r.dup() as u64;
        self.exact += r.exact() as u64;
        self.runs_lost += u32::from(r.lost() > 0);
        self.runs_dup += u32::from(r.dup() > 0);
        self.crashes += u64::from(r.crashes);
    }
}

/// `runs` independent runs, each with its own seeded random crash source (probability `p` per tick).
pub fn sweep_consumer(
    design: ConsumerDesign,
    n: usize,
    batch: usize,
    p: f64,
    runs: u32,
    seed: u64,
) -> Tally {
    let mut base = Rng::new(seed);
    let mut t = Tally::default();
    for r in 0..runs {
        let mut crashes = Crashes::random(base.fork(u64::from(r)), p);
        t.add(&run_consumer(design, n, batch, &mut crashes));
    }
    t
}

/// Every crash pattern over the first `k` ticks (`2^k` runs, `k <= 20`): each tick either
/// crashes or not, including crashes during recovery. Runs finish crash-free after tick `k`.
pub fn exhaustive_consumer(design: ConsumerDesign, n: usize, batch: usize, k: u32) -> Tally {
    assert!(k <= 20, "2^k runs");
    let mut t = Tally::default();
    for mask in 0..(1u64 << k) {
        let mut crashes = Crashes::mask(mask);
        t.add(&run_consumer(design, n, batch, &mut crashes));
    }
    t
}

// ---------------------------------------------------------------------------------------------
// Part 2: idempotent producer
// ---------------------------------------------------------------------------------------------

/// Parameters of the producer experiment.
#[derive(Clone, Copy, Debug)]
pub struct ProducerParams {
    /// Messages to deliver; message `m` has sequence number `m`.
    pub messages: u64,
    /// Max unacknowledged requests per round (the in-flight window).
    pub window: usize,
    /// Probability a request never reaches the broker.
    pub p_request_lost: f64,
    /// Probability the broker appended but the ack is lost.
    pub p_ack_lost: f64,
    /// Broker-side dedup on `(producer_id, sequence)`.
    pub idempotent: bool,
}

/// Broker answer to one produce request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reply {
    Appended,
    /// Sequence already appended: acknowledged without appending again.
    DuplicateAcked,
    /// Sequence is ahead of the next expected one: rejected.
    OutOfOrder,
}

struct Broker {
    idempotent: bool,
    log: Vec<u64>,
    /// producer id -> next expected sequence number
    next_seq: BTreeMap<u64, u64>,
}

impl Broker {
    fn handle(&mut self, producer: u64, seq: u64, payload: u64) -> Reply {
        if !self.idempotent {
            self.log.push(payload);
            return Reply::Appended;
        }
        let next = self.next_seq.entry(producer).or_insert(0);
        match seq.cmp(next) {
            std::cmp::Ordering::Equal => {
                *next += 1;
                self.log.push(payload);
                Reply::Appended
            }
            std::cmp::Ordering::Less => Reply::DuplicateAcked,
            std::cmp::Ordering::Greater => Reply::OutOfOrder,
        }
    }
}

/// Outcome of one producer run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProducerRun {
    /// The broker log: payload ids in append order.
    pub log: Vec<u64>,
    /// Produce requests sent (including retries).
    pub requests: u32,
    /// Requests the broker rejected as out-of-order (idempotent mode only).
    pub rejected_out_of_order: u32,
    /// Rounds until every message was acknowledged.
    pub rounds: u32,
}

impl ProducerRun {
    /// Log entries that repeat an earlier entry.
    pub fn duplicates(&self) -> usize {
        let distinct: BTreeSet<u64> = self.log.iter().copied().collect();
        self.log.len() - distinct.len()
    }
    /// Whether the first occurrences of the payloads are in increasing order (send order kept).
    pub fn in_order(&self) -> bool {
        let mut seen = BTreeSet::new();
        let firsts: Vec<u64> = self
            .log
            .iter()
            .copied()
            .filter(|m| seen.insert(*m))
            .collect();
        firsts.windows(2).all(|w| w[0] < w[1])
    }
}

/// Producer loop: every round sends the lowest `window` unacknowledged messages in order and
/// retries whatever is not acknowledged. Deterministic given `rng`.
pub fn run_producer(p: ProducerParams, rng: &mut Rng) -> ProducerRun {
    const PRODUCER_ID: u64 = 1;
    let mut broker = Broker {
        idempotent: p.idempotent,
        log: Vec::new(),
        next_seq: BTreeMap::new(),
    };
    let mut pending: Vec<u64> = (0..p.messages).collect();
    let (mut requests, mut rejected, mut rounds) = (0u32, 0u32, 0u32);
    while !pending.is_empty() {
        rounds += 1;
        assert!(
            rounds < 1_000_000,
            "producer does not converge: loss probabilities too high"
        );
        let sending: Vec<u64> = pending.iter().copied().take(p.window).collect();
        let mut acked = BTreeSet::new();
        for m in sending {
            requests += 1;
            if rng.chance(p.p_request_lost) {
                continue;
            }
            let reply = broker.handle(PRODUCER_ID, m, m);
            let ack_lost = rng.chance(p.p_ack_lost);
            match reply {
                Reply::OutOfOrder => rejected += 1,
                Reply::Appended | Reply::DuplicateAcked if !ack_lost => {
                    acked.insert(m);
                }
                Reply::Appended | Reply::DuplicateAcked => {}
            }
        }
        pending.retain(|m| !acked.contains(m));
    }
    ProducerRun {
        log: broker.log,
        requests,
        rejected_out_of_order: rejected,
        rounds,
    }
}

/// Counts over many producer runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProducerTally {
    /// Runs accumulated.
    pub runs: u32,
    /// Messages the producers wanted to deliver, over all runs.
    pub messages: u64,
    /// Entries in the broker logs, over all runs.
    pub log_entries: u64,
    /// Log entries that repeat an earlier entry.
    pub duplicates: u64,
    /// Messages missing from the log (always 0: the producer retries until acked).
    pub missing: u64,
    /// Runs whose log is not in send order.
    pub misordered_runs: u32,
    /// Requests rejected as out-of-order.
    pub rejected: u64,
    /// Requests sent, retries included.
    pub requests: u64,
}

/// `runs` seeded producer runs.
pub fn sweep_producer(p: ProducerParams, runs: u32, seed: u64) -> ProducerTally {
    let mut base = Rng::new(seed);
    let mut t = ProducerTally::default();
    for r in 0..runs {
        let run = run_producer(p, &mut base.fork(u64::from(r)));
        let distinct: BTreeSet<u64> = run.log.iter().copied().collect();
        t.runs += 1;
        t.messages += p.messages;
        t.log_entries += run.log.len() as u64;
        t.duplicates += run.duplicates() as u64;
        t.missing += p.messages - distinct.len() as u64;
        t.misordered_runs += u32::from(!run.in_order());
        t.rejected += u64::from(run.rejected_out_of_order);
        t.requests += u64::from(run.requests);
    }
    t
}

// ---------------------------------------------------------------------------------------------
// Part 3: Idempotency-Key payment server
// ---------------------------------------------------------------------------------------------

/// How the server uses the idempotency key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayDesign {
    /// No key: every request that reaches the handler charges.
    NoKey,
    /// Key store with InProgress/Done, but "mark Done" is a separate step after the charge.
    KeyNonAtomic,
    /// Key store where "charge + mark Done" is one atomic commit (and re-checks the key).
    KeyAtomic,
}

impl PayDesign {
    /// All designs in the order the lab prints them.
    pub const ALL: [PayDesign; 3] = [
        PayDesign::NoKey,
        PayDesign::KeyNonAtomic,
        PayDesign::KeyAtomic,
    ];

    /// Short label for tables.
    pub fn label(self) -> &'static str {
        match self {
            PayDesign::NoKey => "no key",
            PayDesign::KeyNonAtomic => "key, non-atomic",
            PayDesign::KeyAtomic => "key, atomic",
        }
    }
}

/// `POST /pay` body plus the `Idempotency-Key` header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayRequest {
    /// The idempotency key (one per user intention).
    pub key: u64,
    /// Amount to charge.
    pub amount: u64,
}

/// A server response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PayResponse {
    /// 201 created, 409 in flight, 422 key reused with another payload.
    pub status: u16,
    /// Charge the response refers to (201 only).
    pub charge_id: Option<u64>,
    /// The stored response of an already completed request is replayed.
    pub replay: bool,
}

#[derive(Clone, Copy, Debug)]
enum KeyEntry {
    InProgress { fp: u64, lease_until: u64 },
    Done { fp: u64, charge_id: u64 },
}

#[derive(Clone, Copy, Debug)]
enum Stage {
    Charge,
    Record(u64),
}

/// Volatile state of one request being handled; dropped when the server crashes.
#[derive(Clone, Debug)]
pub struct Handler {
    req: PayRequest,
    stage: Stage,
}

/// Result of [`PayServer::begin`].
#[derive(Clone, Debug)]
pub enum Begin {
    /// Answer immediately (replay, 409, 422).
    Reply(PayResponse),
    /// Run the handler step by step with [`PayServer::advance`].
    Run(Handler),
}

/// Fingerprint of the payload (what the draft calls the request fingerprint). FNV-1a on the amount.
fn fingerprint(req: &PayRequest) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for b in req.amount.to_le_bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A payment endpoint with a durable key store and a durable charge ledger.
pub struct PayServer {
    design: PayDesign,
    lease_ttl: u64,
    keys: BTreeMap<u64, KeyEntry>,
    /// Key of every charge made; charge id = index + 1. This is the ground truth.
    charges: Vec<u64>,
}

impl PayServer {
    /// `lease_ttl`: how long an InProgress mark blocks retries (virtual time units).
    pub fn new(design: PayDesign, lease_ttl: u64) -> Self {
        Self {
            design,
            lease_ttl,
            keys: BTreeMap::new(),
            charges: Vec::new(),
        }
    }

    /// Number of charges recorded for `key`.
    pub fn charges_for(&self, key: u64) -> usize {
        self.charges.iter().filter(|&&k| k == key).count()
    }

    /// Total number of charges.
    pub fn total_charges(&self) -> usize {
        self.charges.len()
    }

    fn charge(&mut self, key: u64) -> u64 {
        self.charges.push(key);
        self.charges.len() as u64
    }

    fn created(charge_id: u64, replay: bool) -> PayResponse {
        PayResponse {
            status: 201,
            charge_id: Some(charge_id),
            replay,
        }
    }

    /// First stage of handling a request at virtual time `now`: the key lookup. Its effect
    /// (taking the lease) is a single atomic "set if absent".
    pub fn begin(&mut self, now: u64, req: PayRequest) -> Begin {
        let run = Begin::Run(Handler {
            req,
            stage: Stage::Charge,
        });
        if self.design == PayDesign::NoKey {
            return run;
        }
        let fp = fingerprint(&req);
        match self.keys.get(&req.key).copied() {
            None => {}
            Some(KeyEntry::InProgress { fp: old, .. } | KeyEntry::Done { fp: old, .. })
                if old != fp =>
            {
                return Begin::Reply(PayResponse {
                    status: 422,
                    charge_id: None,
                    replay: false,
                });
            }
            Some(KeyEntry::Done { charge_id, .. }) => {
                return Begin::Reply(Self::created(charge_id, true));
            }
            Some(KeyEntry::InProgress { lease_until, .. }) if now < lease_until => {
                return Begin::Reply(PayResponse {
                    status: 409,
                    charge_id: None,
                    replay: false,
                });
            }
            // A stale lease: the original handler is presumed dead, take over.
            Some(KeyEntry::InProgress { .. }) => {}
        }
        self.keys.insert(
            req.key,
            KeyEntry::InProgress {
                fp,
                lease_until: now + self.lease_ttl,
            },
        );
        run
    }

    /// Executes the next durable step of `h`; returns the response once the handler is done.
    pub fn advance(&mut self, h: &mut Handler) -> Option<PayResponse> {
        let key = h.req.key;
        let fp = fingerprint(&h.req);
        match (self.design, h.stage) {
            (PayDesign::NoKey, Stage::Charge) => Some(Self::created(self.charge(key), false)),
            (PayDesign::KeyNonAtomic, Stage::Charge) => {
                let id = self.charge(key);
                h.stage = Stage::Record(id);
                None
            }
            (PayDesign::KeyNonAtomic, Stage::Record(id)) => {
                self.keys.insert(key, KeyEntry::Done { fp, charge_id: id });
                Some(Self::created(id, false))
            }
            (PayDesign::KeyAtomic, Stage::Charge) => {
                // Atomic commit with a unique-key check: a stale handler that lost its lease
                // finds the key already Done and must not charge again.
                if let Some(KeyEntry::Done { charge_id, .. }) = self.keys.get(&key).copied() {
                    return Some(Self::created(charge_id, true));
                }
                let id = self.charge(key);
                self.keys.insert(key, KeyEntry::Done { fp, charge_id: id });
                Some(Self::created(id, false))
            }
            (PayDesign::NoKey | PayDesign::KeyAtomic, Stage::Record(_)) => {
                unreachable!("only the non-atomic design has a separate record stage")
            }
        }
    }
}

/// What the client observed for one attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Served {
    /// The server crashed before answering: the client sees a timeout.
    Crashed,
    /// The server answered.
    Reply(PayResponse),
}

/// Handles `req` end to end, with a crash tick before every durable handler step.
pub fn serve(server: &mut PayServer, now: u64, req: PayRequest, crashes: &mut Crashes) -> Served {
    match server.begin(now, req) {
        Begin::Reply(r) => Served::Reply(r),
        Begin::Run(mut h) => loop {
            if crashes.crash_now() {
                return Served::Crashed;
            }
            if let Some(r) = server.advance(&mut h) {
                return Served::Reply(r);
            }
        },
    }
}

/// Client retry policy: at most this many attempts per intention.
pub const MAX_ATTEMPTS: u32 = 8;
/// Lease length of an InProgress mark in the random runs.
pub const LEASE_TTL: u64 = 4;

/// What happened to one user intention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntentRun {
    /// The client saw a 201.
    pub confirmed: bool,
    /// Attempts made.
    pub attempts: u32,
    /// 409 responses the client saw.
    pub conflicts: u32,
}

/// One user intention: the same request is retried, with linearly growing delay, until the
/// client sees a 201 (or a 422), or gives up. Every reply, 409 included, can also be lost
/// (one more crash tick).
pub fn run_intent(
    server: &mut PayServer,
    key: u64,
    now: &mut u64,
    crashes: &mut Crashes,
) -> IntentRun {
    let req = PayRequest {
        key,
        amount: 100 + key,
    };
    let mut out = IntentRun {
        confirmed: false,
        attempts: 0,
        conflicts: 0,
    };
    for attempt in 0..MAX_ATTEMPTS {
        *now += u64::from(attempt);
        out.attempts += 1;
        let Served::Reply(r) = serve(server, *now, req, crashes) else {
            continue;
        };
        if crashes.crash_now() {
            continue; // the reply was lost on the way back
        }
        match r.status {
            201 => {
                out.confirmed = true;
                break;
            }
            409 => out.conflicts += 1,
            _ => break,
        }
    }
    out
}

/// Counts over many intentions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PayTally {
    /// Runs (exhaustive mode) or intentions (random mode) accumulated.
    pub intents: u32,
    /// Intentions with a charge count of 0.
    pub never_charged: u32,
    /// Intentions with exactly one charge.
    pub charged_once: u32,
    /// Intentions charged two or more times.
    pub double_charged: u32,
    /// Intentions the client saw confirmed.
    pub confirmed: u32,
    /// 409 responses seen by clients.
    pub conflicts: u32,
    /// Requests sent.
    pub attempts: u32,
}

impl PayTally {
    fn add(&mut self, charges: usize, r: IntentRun) {
        self.intents += 1;
        match charges {
            0 => self.never_charged += 1,
            1 => self.charged_once += 1,
            _ => self.double_charged += 1,
        }
        self.confirmed += u32::from(r.confirmed);
        self.conflicts += r.conflicts;
        self.attempts += r.attempts;
    }
}

/// `intents` intentions against one server; every server step and every reply fails with probability `p`.
pub fn run_payments(design: PayDesign, intents: u32, p: f64, seed: u64) -> PayTally {
    let mut server = PayServer::new(design, LEASE_TTL);
    let mut crashes = Crashes::random(Rng::new(seed), p);
    let mut tally = PayTally::default();
    let mut now = 0;
    for key in 0..u64::from(intents) {
        let r = run_intent(&mut server, key, &mut now, &mut crashes);
        tally.add(server.charges_for(key), r);
        now += 10;
    }
    tally
}

/// One intention, every crash/lost-reply pattern over the first `k` ticks (`2^k` runs).
pub fn exhaustive_payments(design: PayDesign, k: u32) -> PayTally {
    assert!(k <= 20, "2^k runs");
    let mut tally = PayTally::default();
    for mask in 0..(1u64 << k) {
        let mut server = PayServer::new(design, LEASE_TTL);
        let mut crashes = Crashes::mask(mask);
        let mut now = 0;
        let r = run_intent(&mut server, 7, &mut now, &mut crashes);
        tally.add(server.charges_for(7), r);
    }
    tally
}

/// One line of the scripted run.
#[derive(Clone, Debug)]
pub struct ScriptLine {
    /// Virtual time of the request.
    pub time: u64,
    /// What the request is.
    pub what: &'static str,
    /// Key and amount sent.
    pub req: PayRequest,
    /// The response.
    pub resp: PayResponse,
}

/// The four situations of chapter 12 section 4, driven in order on a keyed server:
/// first request (still in flight), retry while in flight, completion, retry after done,
/// same key with a different payload. Returns the lines and the number of charges.
pub fn script_four_situations(design: PayDesign) -> (Vec<ScriptLine>, usize) {
    assert!(design != PayDesign::NoKey, "the script needs a key store");
    let mut s = PayServer::new(design, LEASE_TTL);
    let a = PayRequest {
        key: 7,
        amount: 500,
    };
    let other = PayRequest {
        key: 7,
        amount: 900,
    };
    let mut lines = Vec::new();
    let mut push = |time, what, req, resp| {
        lines.push(ScriptLine {
            time,
            what,
            req,
            resp,
        })
    };

    let Begin::Run(mut h) = s.begin(0, a) else {
        unreachable!("fresh key")
    };
    // t=1: the client timed out and retried while the original handler is still running.
    let Begin::Reply(r) = s.begin(1, a) else {
        unreachable!("lease is live")
    };
    push(1, "retry while original in flight", a, r);
    // t=2: the original finishes (its response is lost on the way back).
    let r = s
        .advance(&mut h)
        .or_else(|| s.advance(&mut h))
        .expect("handler finishes");
    push(2, "first request completes", a, r);
    let Begin::Reply(r) = s.begin(3, a) else {
        unreachable!("key is Done")
    };
    push(3, "retry after done", a, r);
    let Begin::Reply(r) = s.begin(4, other) else {
        unreachable!("fingerprint differs")
    };
    push(4, "same key, different amount", other, r);
    let charges = s.total_charges();
    (lines, charges)
}

/// A handler that is merely slow, not dead: its lease expires, a second handler takes over and
/// finishes, then the first one resumes. Returns the number of charges for the key.
pub fn script_stale_handler(design: PayDesign) -> usize {
    assert!(design != PayDesign::NoKey, "the script needs a key store");
    let mut s = PayServer::new(design, LEASE_TTL);
    let req = PayRequest {
        key: 7,
        amount: 500,
    };
    let Begin::Run(mut slow) = s.begin(0, req) else {
        unreachable!("fresh key")
    };
    let Begin::Run(mut fast) = s.begin(LEASE_TTL + 1, req) else {
        unreachable!("stale lease is taken over")
    };
    while s.advance(&mut fast).is_none() {}
    while s.advance(&mut slow).is_none() {}
    s.charges_for(7)
}

// ---------------------------------------------------------------------------------------------
// Part 4: dual write vs outbox
// ---------------------------------------------------------------------------------------------

/// How a service updates its database and publishes the event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualDesign {
    /// Commit the DB, then publish. A crash in between loses the event.
    DbThenBroker,
    /// Publish, then commit the DB. A crash in between leaves a phantom event.
    BrokerThenDb,
    /// One atomic commit of the DB change and an outbox row; a relay publishes and marks rows.
    Outbox,
}

impl DualDesign {
    /// All designs in the order the lab prints them.
    pub const ALL: [DualDesign; 3] = [
        DualDesign::DbThenBroker,
        DualDesign::BrokerThenDb,
        DualDesign::Outbox,
    ];

    /// Short label for tables.
    pub fn label(self) -> &'static str {
        match self {
            DualDesign::DbThenBroker => "dual write DB->broker",
            DualDesign::BrokerThenDb => "dual write broker->DB",
            DualDesign::Outbox => "outbox + relay",
        }
    }
}

/// Final state after a dual-write / outbox run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualRun {
    /// `db[i]`: the business change of operation `i` is durable.
    pub db: Vec<bool>,
    /// Event ids in the order the broker received them.
    pub broker: Vec<usize>,
    /// Crashes that happened (service and relay).
    pub crashes: u32,
}

impl DualRun {
    fn messages(&self, i: usize) -> usize {
        self.broker.iter().filter(|&&e| e == i).count()
    }
    /// Operations committed in the DB whose event never reached the broker.
    pub fn lost(&self) -> usize {
        (0..self.db.len())
            .filter(|&i| self.db[i] && self.messages(i) == 0)
            .count()
    }
    /// Events in the broker whose DB change does not exist.
    pub fn phantom(&self) -> usize {
        (0..self.db.len())
            .filter(|&i| !self.db[i] && self.messages(i) > 0)
            .count()
    }
    /// Operations whose event is in the broker two or more times.
    pub fn dup_events(&self) -> usize {
        (0..self.db.len())
            .filter(|&i| self.messages(i) >= 2)
            .count()
    }
    /// How many times a consumer applies each event's effect: with `dedup` it records the event
    /// id and applies each id once; without, it applies every message.
    pub fn effects(&self, dedup: bool) -> Vec<u32> {
        let mut effects = vec![0; self.db.len()];
        let mut seen = BTreeSet::new();
        for &e in &self.broker {
            if !dedup || seen.insert(e) {
                effects[e] += 1;
            }
        }
        effects
    }
    /// Operations whose effect was applied 2 or more times by the consumer.
    pub fn effect_dups(&self, dedup: bool) -> usize {
        self.effects(dedup).iter().filter(|&&c| c >= 2).count()
    }
}

/// Runs `ops` operations through `design`. `crashes` is consulted before every durable step of
/// the service and of the relay; `sched` decides who runs next when both have work (outbox).
/// A crashed service operation is abandoned (the client gave up); a crashed relay restarts
/// and polls the outbox again.
pub fn run_dual(design: DualDesign, ops: usize, crashes: &mut Crashes, sched: &mut Rng) -> DualRun {
    // Outbox rows: 0 absent, 1 unpublished, 2 published.
    let mut db = vec![false; ops];
    let mut outbox = vec![0u8; ops];
    let mut broker = Vec::new();
    let (mut next_op, mut sub) = (0usize, 0u8);
    let mut relay_pending: Option<usize> = None;
    let mut crash_count = 0;
    loop {
        let service_done = next_op >= ops;
        let relay_work =
            design == DualDesign::Outbox && (relay_pending.is_some() || outbox.contains(&1));
        if service_done && !relay_work {
            break;
        }
        let run_service = !service_done && (!relay_work || sched.chance(0.5));
        if run_service {
            if crashes.crash_now() {
                crash_count += 1;
                next_op += 1;
                sub = 0;
                continue;
            }
            match (design, sub) {
                (DualDesign::DbThenBroker, 0) => {
                    db[next_op] = true;
                    sub = 1;
                }
                (DualDesign::DbThenBroker, _) => {
                    broker.push(next_op);
                    sub = 0;
                    next_op += 1;
                }
                (DualDesign::BrokerThenDb, 0) => {
                    broker.push(next_op);
                    sub = 1;
                }
                (DualDesign::BrokerThenDb, _) => {
                    db[next_op] = true;
                    sub = 0;
                    next_op += 1;
                }
                (DualDesign::Outbox, _) => {
                    // One atomic commit: the business row and the outbox row.
                    db[next_op] = true;
                    outbox[next_op] = 1;
                    next_op += 1;
                }
            }
        } else {
            if crashes.crash_now() {
                crash_count += 1;
                relay_pending = None;
                continue;
            }
            if let Some(id) = relay_pending.take() {
                outbox[id] = 2; // mark published
            } else if let Some(id) = outbox.iter().position(|&s| s == 1) {
                broker.push(id); // publish; the mark is a separate step
                relay_pending = Some(id);
            }
        }
    }
    DualRun {
        db,
        broker,
        crashes: crash_count,
    }
}

/// Counts over many dual-write / outbox runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DualTally {
    /// Runs accumulated.
    pub runs: u32,
    /// Operations over all runs.
    pub ops: u64,
    /// Operations durable in the DB.
    pub committed: u64,
    /// See [`DualRun::lost`].
    pub lost: u64,
    /// See [`DualRun::phantom`].
    pub phantom: u64,
    /// See [`DualRun::dup_events`].
    pub dup_events: u64,
    /// Effect duplicates when the consumer does not dedup.
    pub effect_dup_naive: u64,
    /// Effect duplicates when the consumer dedups by event id.
    pub effect_dup_dedup: u64,
    /// Crashes fired.
    pub crashes: u64,
}

impl DualTally {
    fn add(&mut self, r: &DualRun) {
        self.runs += 1;
        self.ops += r.db.len() as u64;
        self.committed += r.db.iter().filter(|&&b| b).count() as u64;
        self.lost += r.lost() as u64;
        self.phantom += r.phantom() as u64;
        self.dup_events += r.dup_events() as u64;
        self.effect_dup_naive += r.effect_dups(false) as u64;
        self.effect_dup_dedup += r.effect_dups(true) as u64;
        self.crashes += u64::from(r.crashes);
    }
}

/// `runs` seeded runs of `ops` operations, crash probability `p` per durable step.
pub fn sweep_dual(design: DualDesign, ops: usize, p: f64, runs: u32, seed: u64) -> DualTally {
    let mut base = Rng::new(seed);
    let mut t = DualTally::default();
    for r in 0..runs {
        let mut crashes = Crashes::random(base.fork(2 * u64::from(r)), p);
        let mut sched = base.fork(2 * u64::from(r) + 1);
        t.add(&run_dual(design, ops, &mut crashes, &mut sched));
    }
    t
}

/// Every crash pattern over the first `k` ticks, for each of `sched_seeds` scheduler seeds.
pub fn exhaustive_dual(design: DualDesign, ops: usize, k: u32, sched_seeds: u64) -> DualTally {
    assert!(k <= 20, "2^k runs");
    let mut t = DualTally::default();
    for seed in 0..sched_seeds {
        for mask in 0..(1u64 << k) {
            let mut crashes = Crashes::mask(mask);
            let mut sched = Rng::new(seed);
            t.add(&run_dual(design, ops, &mut crashes, &mut sched));
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;
    use ConsumerDesign::*;

    /// Horizon for exhaustive enumeration: 2^14 crash patterns per configuration.
    const K: u32 = 14;
    /// (events, batch) configurations enumerated exhaustively.
    const CONFIGS: [(usize, usize); 4] = [(3, 1), (4, 2), (3, 3), (5, 2)];

    // ----- Part 1 -----

    #[test]
    fn crash_free_tick_counts_are_pinned() {
        // Ticks consumed by a run without crashes: n events, batch 1.
        let ticks = |d, n, b| run_consumer(d, n, b, &mut Crashes::never()).ticks;
        assert_eq!(ticks(AtMostOnce, 3, 1), 6); // commit + apply per event
        assert_eq!(ticks(AtLeastOnce, 3, 1), 6);
        assert_eq!(ticks(DedupApplyThenRecord, 3, 1), 9); // apply + record + commit
        assert_eq!(ticks(DedupAtomic, 3, 1), 6);
        assert_eq!(ticks(OffsetWithState, 3, 1), 3); // one atomic commit per event
        assert_eq!(ticks(AtLeastOnce, 3, 3), 4); // 3 applies + 1 commit
        assert_eq!(ticks(OffsetWithState, 3, 3), 1);
        for d in ConsumerDesign::ALL {
            let r = run_consumer(d, 6, 2, &mut Crashes::never());
            assert_eq!((r.lost(), r.dup(), r.exact()), (0, 0, 6), "{d:?}");
            assert_eq!(r.balance(), r.expected_balance());
        }
    }

    #[test]
    fn single_crash_pins_one_anomaly_per_event_boundary() {
        // n = 3, batch 1: crashing exactly once, at each of the 6 ticks.
        let (mut amo_loss, mut alo_dup) = (0, 0);
        for t in 0..6 {
            let amo = run_consumer(AtMostOnce, 3, 1, &mut Crashes::mask(1 << t));
            let alo = run_consumer(AtLeastOnce, 3, 1, &mut Crashes::mask(1 << t));
            assert_eq!(amo.dup(), 0);
            assert_eq!(alo.lost(), 0);
            assert!(amo.lost() <= 1 && alo.dup() <= 1);
            amo_loss += amo.lost();
            alo_dup += alo.dup();
        }
        // The window of vulnerability is one tick per event: exactly 3 of the 6 crash points hurt.
        assert_eq!(amo_loss, 3);
        assert_eq!(alo_dup, 3);
    }

    #[test]
    fn at_most_once_loses_and_never_duplicates_exhaustively() {
        for (n, b) in CONFIGS {
            let t = exhaustive_consumer(AtMostOnce, n, b, K);
            assert_eq!(t.runs, 1 << K);
            assert!(t.lost > 0, "n={n} b={b}");
            assert_eq!(t.dup, 0, "n={n} b={b}");
        }
    }

    #[test]
    fn at_least_once_duplicates_and_never_loses_exhaustively() {
        for (n, b) in CONFIGS {
            let t = exhaustive_consumer(AtLeastOnce, n, b, K);
            assert!(t.dup > 0, "n={n} b={b}");
            assert_eq!(t.lost, 0, "n={n} b={b}");
        }
    }

    #[test]
    fn non_atomic_dedup_is_not_exactly_once_exhaustively() {
        for (n, b) in CONFIGS {
            // check, apply, record: a crash between apply and record re-applies later.
            let a = exhaustive_consumer(DedupApplyThenRecord, n, b, K);
            assert!(a.dup > 0 && a.lost == 0, "n={n} b={b}: {a:?}");
            // check+record, apply: a crash between record and apply makes the redelivery skip it.
            let r = exhaustive_consumer(DedupRecordThenApply, n, b, K);
            assert!(r.lost > 0 && r.dup == 0, "n={n} b={b}: {r:?}");
        }
    }

    #[test]
    fn atomic_dedup_and_atomic_offset_are_exactly_once_for_every_crash_pattern() {
        for design in [DedupAtomic, OffsetWithState] {
            for (n, b) in CONFIGS {
                let t = exhaustive_consumer(design, n, b, K);
                assert_eq!(t.runs, 1 << K);
                assert_eq!((t.lost, t.dup), (0, 0), "{design:?} n={n} b={b}");
                assert_eq!(t.exact, t.events);
                // The patterns really did crash a lot (the check is not vacuous).
                assert!(t.crashes >= u64::from(t.runs) / 2, "{design:?}");
            }
        }
    }

    #[test]
    fn random_crashes_confirm_the_same_picture_over_many_seeds() {
        for seed in 0..600 {
            let n = 1 + (seed % 9) as usize;
            let b = 1 + (seed % 4) as usize;
            let p = 0.05 + 0.1 * (seed % 4) as f64;
            for design in [DedupAtomic, OffsetWithState] {
                let t = sweep_consumer(design, n, b, p, 1, seed);
                assert_eq!((t.lost, t.dup), (0, 0), "{design:?} seed={seed}");
            }
        }
        let amo = sweep_consumer(AtMostOnce, 8, 2, 0.15, 500, 1);
        assert!(amo.lost > 0 && amo.dup == 0);
        let alo = sweep_consumer(AtLeastOnce, 8, 2, 0.15, 500, 1);
        assert!(alo.dup > 0 && alo.lost == 0);
        let a = sweep_consumer(DedupApplyThenRecord, 8, 2, 0.15, 500, 1);
        assert!(
            a.dup > 0,
            "non-atomic dedup must leak duplicates for some seed"
        );
        let r = sweep_consumer(DedupRecordThenApply, 8, 2, 0.15, 500, 1);
        assert!(
            r.lost > 0,
            "non-atomic dedup must lose events for some seed"
        );
        // Only 8 events: at least one run of the 500 is fully clean for the naive designs too,
        // so "some seeds are fine" does not prove anything: the anomalies are what matters.
        assert!(alo.runs_dup < 500);
    }

    #[test]
    fn sweeps_are_deterministic_and_seed_sensitive() {
        let a = sweep_consumer(AtLeastOnce, 8, 2, 0.15, 200, 7);
        assert_eq!(a, sweep_consumer(AtLeastOnce, 8, 2, 0.15, 200, 7));
        assert_ne!(a, sweep_consumer(AtLeastOnce, 8, 2, 0.15, 200, 8));
    }

    // ----- Part 2 -----

    fn producer_sweep(idempotent: bool, window: usize, runs: u32) -> ProducerTally {
        let p = ProducerParams {
            messages: 20,
            window,
            p_request_lost: 0.2,
            p_ack_lost: 0.2,
            idempotent,
        };
        sweep_producer(p, runs, 11)
    }

    #[test]
    fn retrying_producer_without_idempotence_duplicates_and_with_it_does_not() {
        for window in [1, 3, 5] {
            let naive = producer_sweep(false, window, 300);
            assert!(naive.duplicates > 0, "window={window}");
            assert_eq!(naive.missing, 0, "retry means nothing is lost");
            let idem = producer_sweep(true, window, 300);
            assert_eq!(idem.duplicates, 0, "window={window}");
            assert_eq!(idem.misordered_runs, 0, "window={window}");
            assert_eq!(idem.missing, 0);
            assert_eq!(
                idem.log_entries, idem.messages,
                "exactly one log entry per message"
            );
        }
    }

    #[test]
    fn sequence_numbers_preserve_order_when_requests_are_pipelined() {
        // Without dedup a retried request lands behind later ones when the window is > 1 ...
        assert!(producer_sweep(false, 3, 300).misordered_runs > 0);
        // ... with window 1 there is nothing to overtake ...
        assert_eq!(producer_sweep(false, 1, 300).misordered_runs, 0);
        // ... and the idempotent broker keeps the order by rejecting the overtaker.
        let idem = producer_sweep(true, 3, 300);
        assert_eq!(idem.misordered_runs, 0);
        assert!(idem.rejected > 0);
        assert_eq!(producer_sweep(true, 1, 300).rejected, 0);
    }

    #[test]
    fn broker_dedup_state_is_exact_on_hand_written_sequences() {
        let mut b = Broker {
            idempotent: true,
            log: vec![],
            next_seq: BTreeMap::new(),
        };
        assert_eq!(b.handle(1, 0, 100), Reply::Appended);
        assert_eq!(b.handle(1, 0, 100), Reply::DuplicateAcked);
        assert_eq!(b.handle(1, 2, 102), Reply::OutOfOrder);
        assert_eq!(b.handle(1, 1, 101), Reply::Appended);
        assert_eq!(b.handle(1, 2, 102), Reply::Appended);
        // A different producer id has its own sequence space.
        assert_eq!(b.handle(2, 0, 200), Reply::Appended);
        assert_eq!(b.log, vec![100, 101, 102, 200]);
    }

    // ----- Part 3 -----

    #[test]
    fn four_situations_give_201_409_replay_422_and_one_charge() {
        for design in [PayDesign::KeyNonAtomic, PayDesign::KeyAtomic] {
            let (lines, charges) = script_four_situations(design);
            let got: Vec<(u16, bool)> = lines
                .iter()
                .map(|l| (l.resp.status, l.resp.replay))
                .collect();
            assert_eq!(
                got,
                vec![(409, false), (201, false), (201, true), (422, false)],
                "{design:?}"
            );
            assert_eq!(charges, 1, "{design:?}");
            // The replay returns the very same charge, not a new one.
            assert_eq!(lines[1].resp.charge_id, lines[2].resp.charge_id);
            // 422 and 409 never carry a charge.
            assert_eq!(lines[0].resp.charge_id, None);
            assert_eq!(lines[3].resp.charge_id, None);
        }
    }

    #[test]
    fn lease_expiry_lets_a_retry_through_after_a_dead_original() {
        let mut s = PayServer::new(PayDesign::KeyAtomic, 4);
        let req = PayRequest { key: 1, amount: 10 };
        let Begin::Run(_dead) = s.begin(0, req) else {
            panic!()
        };
        assert!(matches!(
            s.begin(3, req),
            Begin::Reply(PayResponse { status: 409, .. })
        ));
        assert!(
            matches!(s.begin(4, req), Begin::Run(_)),
            "lease valid while now < 4"
        );
    }

    #[test]
    fn stale_handler_double_charges_only_without_an_atomic_key_check() {
        assert_eq!(script_stale_handler(PayDesign::KeyNonAtomic), 2);
        assert_eq!(script_stale_handler(PayDesign::KeyAtomic), 1);
    }

    #[test]
    fn exhaustive_crash_and_lost_reply_patterns_one_intention() {
        let k = 12;
        let no_key = exhaustive_payments(PayDesign::NoKey, k);
        assert_eq!(no_key.intents, 1 << k);
        assert!(
            no_key.double_charged > 0,
            "no key: retry after a lost reply charges twice"
        );
        let non_atomic = exhaustive_payments(PayDesign::KeyNonAtomic, k);
        assert!(
            non_atomic.double_charged > 0,
            "crash between charge and 'mark done' + lease expiry charges twice"
        );
        let atomic = exhaustive_payments(PayDesign::KeyAtomic, k);
        assert_eq!(atomic.double_charged, 0);
        // A confirmed intention was charged exactly once, in every pattern.
        assert!(atomic.confirmed > 0 && atomic.charged_once >= atomic.confirmed);
    }

    #[test]
    fn random_payments_atomic_key_store_never_double_charges() {
        for seed in 0..300 {
            let p = 0.05 + 0.05 * (seed % 5) as f64;
            let t = run_payments(PayDesign::KeyAtomic, 40, p, seed);
            assert_eq!(t.double_charged, 0, "seed={seed} p={p}");
        }
        let (mut no_key, mut non_atomic) = (0, 0);
        for seed in 0..300 {
            no_key += run_payments(PayDesign::NoKey, 40, 0.15, seed).double_charged;
            non_atomic += run_payments(PayDesign::KeyNonAtomic, 40, 0.15, seed).double_charged;
        }
        assert!(no_key > 0 && non_atomic > 0);
    }

    #[test]
    fn stale_lease_after_a_server_crash_shows_up_as_409_for_the_client() {
        let t = run_payments(PayDesign::KeyAtomic, 400, 0.2, 5);
        assert!(
            t.conflicts > 0,
            "a crashed handler leaves a lease that blocks retries with 409"
        );
        assert_eq!(run_payments(PayDesign::NoKey, 400, 0.2, 5).conflicts, 0);
    }

    // ----- Part 4 -----

    #[test]
    fn dual_write_loses_or_invents_events_and_outbox_does_neither() {
        let db_first = sweep_dual(DualDesign::DbThenBroker, 20, 0.1, 400, 3);
        assert!(db_first.lost > 0);
        assert_eq!(db_first.phantom, 0);
        assert_eq!(db_first.dup_events, 0);
        let broker_first = sweep_dual(DualDesign::BrokerThenDb, 20, 0.1, 400, 3);
        assert!(broker_first.phantom > 0);
        assert_eq!(broker_first.lost, 0);
        let outbox = sweep_dual(DualDesign::Outbox, 20, 0.1, 400, 3);
        assert_eq!((outbox.lost, outbox.phantom), (0, 0));
        // The relay can crash between publish and mark: duplicates exist ...
        assert!(outbox.dup_events > 0);
        assert!(outbox.effect_dup_naive > 0);
        // ... and the consumer's dedup by event id removes them from the effects.
        assert_eq!(outbox.effect_dup_dedup, 0);
        // Dedup cannot repair what dual write lost or invented.
        assert_eq!(db_first.effect_dup_dedup, 0);
    }

    #[test]
    fn outbox_with_dedup_is_exactly_once_for_every_crash_pattern() {
        // 2^12 crash patterns x 4 scheduler seeds, 3 operations: service and relay both crash.
        let t = exhaustive_dual(DualDesign::Outbox, 3, 12, 4);
        assert_eq!(t.runs, 4 * (1 << 12));
        assert_eq!((t.lost, t.phantom, t.effect_dup_dedup), (0, 0, 0));
        assert!(t.dup_events > 0 && t.effect_dup_naive > 0);
        // And the effect of every committed operation is applied exactly once.
        for seed in 0..4u64 {
            for mask in 0..(1u64 << 12) {
                let r = run_dual(
                    DualDesign::Outbox,
                    3,
                    &mut Crashes::mask(mask),
                    &mut Rng::new(seed),
                );
                let eff = r.effects(true);
                for (i, &e) in eff.iter().enumerate() {
                    assert_eq!(e, u32::from(r.db[i]), "mask={mask} seed={seed} op={i}");
                }
            }
        }
    }

    #[test]
    fn dual_write_anomalies_exist_in_exhaustive_enumeration_with_exact_counts() {
        // 2 ticks per operation; crash at the second tick of op i leaves exactly one anomaly.
        let db_first = exhaustive_dual(DualDesign::DbThenBroker, 3, 6, 1);
        let broker_first = exhaustive_dual(DualDesign::BrokerThenDb, 3, 6, 1);
        assert!(db_first.lost > 0 && db_first.phantom == 0);
        assert!(broker_first.phantom > 0 && broker_first.lost == 0);
        let one_crash = |d, t: u32| run_dual(d, 3, &mut Crashes::mask(1 << t), &mut Rng::new(0));
        let anomalies_db: usize = (0..6)
            .map(|t| one_crash(DualDesign::DbThenBroker, t).lost())
            .sum();
        let anomalies_broker: usize = (0..6)
            .map(|t| one_crash(DualDesign::BrokerThenDb, t).phantom())
            .sum();
        assert_eq!((anomalies_db, anomalies_broker), (3, 3));
    }

    #[test]
    fn dual_runs_are_deterministic() {
        let a = sweep_dual(DualDesign::Outbox, 20, 0.1, 100, 9);
        assert_eq!(a, sweep_dual(DualDesign::Outbox, 20, 0.1, 100, 9));
        assert_ne!(a, sweep_dual(DualDesign::Outbox, 20, 0.1, 100, 10));
    }
}
