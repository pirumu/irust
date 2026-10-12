//! Redis-style cache-aside under concurrency: stampede, stale set, lock release, negative caching.
//!
//! Everything here is an in-memory *model* in virtual milliseconds (no Redis, no sockets, no wall
//! clock). Four independent parts:
//!
//! 1. [`simulate_stampede`] / [`simulate_expiry_burst`]: one hot key (or a batch of keys) expiring,
//!    with naive cache-aside, single-flight (`SET NX PX`), TTL jitter and XFetch (probabilistic early
//!    recompute).
//! 2. [`enumerate_race`]: exhaustive enumeration of every interleaving of one reader
//!    (`GET`, `SELECT`, `SET`) and one writer (two steps, in either order), counting the
//!    interleavings that leave a stale value in the cache, with and without a lease token or a
//!    version compare on `SET`.
//! 3. [`simulate_lock`]: three clients on one lock key, released by plain `DEL` or by
//!    "delete only if the value is still my token".
//! 4. [`simulate_negative_caching`] / [`negative_entry_pitfall`]: caching "this id does not exist".
//!
//! The model leaves out: network latency and loss, Redis eviction, replication/failover, clock
//! skew (time is one exact integer for every actor), DB capacity (a load always takes exactly
//! `compute_ms`), and anything about the real memcached lease implementation beyond "a `delete`
//! invalidates the token".

use crate::rng::Rng;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

// ------------------------------------------------------------------------------------------
// Part 1a: one hot key
// ------------------------------------------------------------------------------------------

/// How a request that finds the key missing (or about to expire) behaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Policy {
    /// `Some(ttl)`: single-flight. A request must win `SET key:load token NX PX ttl` before it
    /// may recompute; losers sleep (20, 40, 80, 160, 160... ms plus up to 50 % random extra) and
    /// re-read. `None`: every request that misses recomputes on its own (naive cache-aside).
    pub lock_ttl_ms: Option<u64>,
    /// `Some(beta)`: XFetch. A request that finds a still-valid entry recomputes anyway when
    /// `now - delta * beta * ln(rand) >= expiry`.
    pub xfetch_beta: Option<f64>,
}

impl Policy {
    /// Plain cache-aside: every miss recomputes.
    pub const fn naive() -> Self {
        Self {
            lock_ttl_ms: None,
            xfetch_beta: None,
        }
    }
    /// Single-flight with the given lock TTL.
    pub const fn single_flight(lock_ttl_ms: u64) -> Self {
        Self {
            lock_ttl_ms: Some(lock_ttl_ms),
            xfetch_beta: None,
        }
    }
    /// XFetch without any lock (the paper's setting: only `get`/`set` available).
    pub const fn xfetch(beta: f64) -> Self {
        Self {
            lock_ttl_ms: None,
            xfetch_beta: Some(beta),
        }
    }
    /// XFetch whose early recompute (and ordinary misses) must also win the lock.
    pub const fn xfetch_with_lock(beta: f64, lock_ttl_ms: u64) -> Self {
        Self {
            lock_ttl_ms: Some(lock_ttl_ms),
            xfetch_beta: Some(beta),
        }
    }
}

/// Parameters of the hot-key scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StampedeCfg {
    /// Logical TTL given to every cache fill.
    pub ttl_ms: u64,
    /// Time one recompute (DB load) takes; also the `delta` stored for XFetch.
    pub compute_ms: u64,
    /// Requests arrive every `interval_ms` (steady stream, random phase from the seed).
    pub interval_ms: u64,
    /// No new request arrives at or after this time; running work is drained afterwards.
    pub horizon_ms: u64,
}

impl StampedeCfg {
    /// The example of the VLDB 2015 paper: 10 req/s (every 100 ms) and a 3 s recompute, TTL 60 s,
    /// 20 simulated minutes.
    pub const PAPER_EXAMPLE: StampedeCfg = StampedeCfg {
        ttl_ms: 60_000,
        compute_ms: 3_000,
        interval_ms: 100,
        horizon_ms: 1_200_000,
    };

    /// The same scenario cut after the first expiry wave: exactly one wave can happen.
    pub const fn one_cycle(self) -> Self {
        Self {
            horizon_ms: self.ttl_ms + self.compute_ms + 2 * self.interval_ms,
            ..self
        }
    }
}

/// Counters of one [`simulate_stampede`] run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StampedeReport {
    /// Requests that arrived.
    pub requests: u64,
    /// Requests that finished (must equal `requests`: every loser eventually reads the value).
    pub completed: u64,
    /// Requests whose first `GET` found no valid entry.
    pub misses: u64,
    /// DB loads started.
    pub recomputes: u64,
    /// Loads started while no other load was running (one wave = one expiry event).
    pub waves: u64,
    /// Largest number of loads running at the same instant.
    pub peak_db: u64,
    /// Requests whose latency was greater than zero (missed, lost the lock, or refreshed early).
    pub waited: u64,
    /// Sum of the latencies of the requests that waited.
    pub wait_sum_ms: u64,
    /// Largest latency of any request.
    pub wait_max_ms: u64,
}

impl StampedeReport {
    /// Loads per wave (0 when no wave happened).
    pub fn per_wave(&self) -> f64 {
        if self.waves == 0 {
            0.0
        } else {
            self.recomputes as f64 / self.waves as f64
        }
    }
    /// Fraction of requests whose first `GET` missed.
    pub fn miss_frac(&self) -> f64 {
        if self.requests == 0 {
            0.0
        } else {
            self.misses as f64 / self.requests as f64
        }
    }
    /// Fraction of requests that had to wait.
    pub fn waited_frac(&self) -> f64 {
        if self.requests == 0 {
            0.0
        } else {
            self.waited as f64 / self.requests as f64
        }
    }
    /// Mean latency of the requests that waited.
    pub fn wait_mean_ms(&self) -> f64 {
        if self.waited == 0 {
            0.0
        } else {
            self.wait_sum_ms as f64 / self.waited as f64
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    Arrive,
    Wake { start: u64, attempt: u32 },
    LoadDone { start: u64, token: u64 },
}

struct Entry {
    expires_at: u64,
    delta_ms: u64,
}

struct Sim {
    cfg: StampedeCfg,
    policy: Policy,
    heap: BinaryHeap<Reverse<(u64, u64, Ev)>>,
    seq: u64,
    entry: Option<Entry>,
    /// `(token, expires_at)` of the `SET key:load token NX PX` lock.
    lock: Option<(u64, u64)>,
    next_token: u64,
    active: u64,
    xf_rng: Rng,
    back_rng: Rng,
    rep: StampedeReport,
}

impl Sim {
    fn schedule(&mut self, at: u64, ev: Ev) {
        self.seq += 1;
        self.heap.push(Reverse((at, self.seq, ev)));
    }

    fn valid(&self, now: u64) -> bool {
        self.entry.as_ref().is_some_and(|e| e.expires_at > now)
    }

    /// `SET key:load token NX PX lock_ttl`: wins when the key is absent or already expired.
    fn try_lock(&mut self, now: u64, lock_ttl: u64) -> Option<u64> {
        if self.lock.is_some_and(|(_, exp)| exp > now) {
            return None;
        }
        self.next_token += 1;
        self.lock = Some((self.next_token, now + lock_ttl));
        Some(self.next_token)
    }

    fn begin_load(&mut self, now: u64, start: u64, token: u64) {
        if self.active == 0 {
            self.rep.waves += 1;
        }
        self.active += 1;
        self.rep.peak_db = self.rep.peak_db.max(self.active);
        self.rep.recomputes += 1;
        self.schedule(now + self.cfg.compute_ms, Ev::LoadDone { start, token });
    }

    fn complete(&mut self, now: u64, start: u64) {
        self.rep.completed += 1;
        let w = now - start;
        if w > 0 {
            self.rep.waited += 1;
            self.rep.wait_sum_ms += w;
        }
        self.rep.wait_max_ms = self.rep.wait_max_ms.max(w);
    }

    fn backoff(&mut self, attempt: u32) -> u64 {
        let base = (20u64 << attempt.min(3)).min(160);
        base + self.back_rng.below(base / 2 + 1)
    }

    /// Single-flight path shared by a first miss and by a wake-up after sleeping.
    fn lock_or_sleep(&mut self, now: u64, start: u64, attempt: u32, lock_ttl: u64) {
        match self.try_lock(now, lock_ttl) {
            Some(token) => self.begin_load(now, start, token),
            None => {
                let d = self.backoff(attempt);
                self.schedule(
                    now + d,
                    Ev::Wake {
                        start,
                        attempt: attempt + 1,
                    },
                );
            }
        }
    }

    fn on_arrive(&mut self, now: u64) {
        self.rep.requests += 1;
        if self.valid(now) {
            if let (Some(beta), Some(e)) = (self.policy.xfetch_beta, self.entry.as_ref()) {
                // XFetch: `now - delta * beta * ln(rand) >= expiry`; ln(rand) <= 0 so the left
                // side is `now` plus a positive random "head start".
                let u = self.xf_rng.f64();
                let early = now as f64 - e.delta_ms as f64 * beta * u.ln() >= e.expires_at as f64;
                if early {
                    let token = match self.policy.lock_ttl_ms {
                        Some(ttl) => self.try_lock(now, ttl),
                        None => Some(0),
                    };
                    if let Some(token) = token {
                        self.begin_load(now, now, token);
                        return;
                    }
                }
            }
            self.complete(now, now);
            return;
        }
        self.rep.misses += 1;
        match self.policy.lock_ttl_ms {
            None => self.begin_load(now, now, 0),
            Some(ttl) => self.lock_or_sleep(now, now, 0, ttl),
        }
    }

    fn on_wake(&mut self, now: u64, start: u64, attempt: u32) {
        if self.valid(now) {
            self.complete(now, start);
        } else if let Some(ttl) = self.policy.lock_ttl_ms {
            self.lock_or_sleep(now, start, attempt, ttl);
        }
    }

    fn on_load_done(&mut self, now: u64, start: u64, token: u64) {
        self.active -= 1;
        self.entry = Some(Entry {
            expires_at: now + self.cfg.ttl_ms,
            delta_ms: self.cfg.compute_ms,
        });
        // Token-compared unlock: only the holder of the *current* lock value deletes the key.
        if self.policy.lock_ttl_ms.is_some() && self.lock.is_some_and(|(t, _)| t == token) {
            self.lock = None;
        }
        self.complete(now, start);
    }
}

/// Runs one hot key for `cfg.horizon_ms` of virtual time. The cache starts filled and expires
/// at `cfg.ttl_ms`. Deterministic in `seed` (which only picks the arrival phase, the XFetch
/// random numbers and the back-off jitter).
pub fn simulate_stampede(policy: Policy, cfg: StampedeCfg, seed: u64) -> StampedeReport {
    assert!(
        cfg.interval_ms > 0 && cfg.compute_ms > 0,
        "interval and compute time must be > 0"
    );
    let mut root = Rng::new(seed);
    let phase = root.fork(1).below(cfg.interval_ms);
    let mut sim = Sim {
        cfg,
        policy,
        heap: BinaryHeap::new(),
        seq: 0,
        entry: Some(Entry {
            expires_at: cfg.ttl_ms,
            delta_ms: cfg.compute_ms,
        }),
        lock: None,
        next_token: 0,
        active: 0,
        xf_rng: root.fork(2),
        back_rng: root.fork(3),
        rep: StampedeReport::default(),
    };
    sim.schedule(phase, Ev::Arrive);
    while let Some(Reverse((now, _, ev))) = sim.heap.pop() {
        match ev {
            Ev::Arrive => {
                if now + cfg.interval_ms < cfg.horizon_ms {
                    sim.schedule(now + cfg.interval_ms, Ev::Arrive);
                }
                sim.on_arrive(now);
            }
            Ev::Wake { start, attempt } => sim.on_wake(now, start, attempt),
            Ev::LoadDone { start, token } => sim.on_load_done(now, start, token),
        }
    }
    sim.rep
}

// ------------------------------------------------------------------------------------------
// Part 1b: many keys expiring together (TTL jitter)
// ------------------------------------------------------------------------------------------

/// Result of [`simulate_expiry_burst`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BurstReport {
    /// Number of keys.
    pub keys: usize,
    /// Peak number of reloads running at once.
    pub peak_db: usize,
    /// Time from the first reload start to the last reload start.
    pub spread_ms: u64,
}

/// `keys` keys loaded at t = 0 with the same TTL; key `i` actually lives
/// `ttl + below(ttl * jitter_pct / 100 + 1)` ms (no extra when `jitter_pct == 0`). Each key is
/// read steadily, so its reload starts at its expiry plus a random phase below `read_period_ms`;
/// each key is reloaded exactly once (single-flight per key) and the reload takes `compute_ms`.
pub fn simulate_expiry_burst(
    keys: usize,
    ttl_ms: u64,
    jitter_pct: u64,
    compute_ms: u64,
    read_period_ms: u64,
    seed: u64,
) -> BurstReport {
    let mut rng = Rng::new(seed);
    let mut edges: Vec<(u64, i64)> = Vec::with_capacity(keys * 2);
    let (mut lo, mut hi) = (u64::MAX, 0);
    for _ in 0..keys {
        let jitter = if jitter_pct == 0 {
            0
        } else {
            rng.below(ttl_ms * jitter_pct / 100 + 1)
        };
        let start = ttl_ms + jitter + rng.below(read_period_ms.max(1));
        lo = lo.min(start);
        hi = hi.max(start);
        edges.push((start, 1));
        edges.push((start + compute_ms, -1));
    }
    // At equal times the "-1" sorts first: a load ending at t does not overlap one starting at t.
    edges.sort_unstable();
    let (mut cur, mut peak) = (0i64, 0i64);
    for (_, d) in edges {
        cur += d;
        peak = peak.max(cur);
    }
    BurstReport {
        keys,
        peak_db: peak as usize,
        spread_ms: hi.saturating_sub(lo),
    }
}

// ------------------------------------------------------------------------------------------
// Part 2: the stale-set race, exhaustively
// ------------------------------------------------------------------------------------------

/// Who executes a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    /// The cache-aside reader: `r1` GET, `r2` read DB, `r3` SET.
    Reader,
    /// The writer: two steps whose meaning depends on [`WriterOrder`] and [`Fix`].
    Writer,
}

/// One step of an interleaving; `idx` is 1-based within its actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// Executing actor.
    pub actor: Actor,
    /// 1-based step number (`r1`, `r2`, `r3`, `w1`, `w2`).
    pub idx: u8,
}

impl Step {
    /// `"r1"`, `"w2"`, ...
    pub fn label(&self) -> String {
        let c = match self.actor {
            Actor::Reader => 'r',
            Actor::Writer => 'w',
        };
        format!("{c}{}", self.idx)
    }
}

/// The writer's two steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterOrder {
    /// A: `w1` update DB, `w2` invalidate the cache (the recommended cache-aside order).
    DbThenInvalidate,
    /// B: `w1` invalidate the cache, `w2` update DB.
    InvalidateThenDb,
}

/// Protection added on top of cache-aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    /// Nothing: `SET` always succeeds, the writer `DEL`s.
    None,
    /// Facebook-style lease: a miss hands out a token, a `DEL` invalidates it, a `SET` with a
    /// token that is no longer valid is rejected. A DB write alone does *not* touch the lease.
    Lease,
    /// The writer *sets* the new version instead of deleting; every `SET` carries the version it
    /// read and is rejected when the cache already holds a newer one.
    VersionedSet,
}

/// Value of the key before the write.
pub const OLD: u8 = 0;
/// Value of the key after the write.
pub const NEW: u8 = 1;

/// All `C(5,2) = 10` interleavings of `r1 r2 r3` with `w1 w2` (each actor's order preserved), in
/// the order of the bit mask that selects the reader's positions.
pub fn interleavings() -> Vec<Vec<Step>> {
    let mut out = Vec::new();
    for mask in 0u8..32 {
        if mask.count_ones() != 3 {
            continue;
        }
        let (mut r, mut w) = (0u8, 0u8);
        let mut steps = Vec::with_capacity(5);
        for pos in 0..5 {
            if mask & (1 << pos) != 0 {
                r += 1;
                steps.push(Step {
                    actor: Actor::Reader,
                    idx: r,
                });
            } else {
                w += 1;
                steps.push(Step {
                    actor: Actor::Writer,
                    idx: w,
                });
            }
        }
        out.push(steps);
    }
    out
}

struct CacheEntry {
    val: u8,
    expires_at: Option<u64>,
}

struct Race {
    order: WriterOrder,
    fix: Fix,
    ttl_ms: Option<u64>,
    db: u8,
    cache: Option<CacheEntry>,
    lease: Option<u64>,
    next_lease: u64,
    // Reader-local state.
    alive: bool,
    read: u8,
    token: Option<u64>,
}

impl Race {
    fn new(order: WriterOrder, fix: Fix, ttl_ms: Option<u64>) -> Self {
        Self {
            order,
            fix,
            ttl_ms,
            db: OLD,
            cache: None,
            lease: None,
            next_lease: 0,
            alive: true,
            read: OLD,
            token: None,
        }
    }

    fn cache_val(&self, now: u64) -> Option<u8> {
        self.cache
            .as_ref()
            .filter(|c| c.expires_at.is_none_or(|e| e > now))
            .map(|c| c.val)
    }

    /// `SET` honouring the fix's acceptance rule (`token` is the lease the reader holds, if any;
    /// for [`Fix::VersionedSet`] the value doubles as its version).
    fn cache_set(&mut self, now: u64, val: u8, token: Option<u64>) {
        let accept = match self.fix {
            Fix::None => true,
            Fix::Lease => self.lease.is_some() && self.lease == token,
            Fix::VersionedSet => self.cache_val(now).is_none_or(|cur| cur <= val),
        };
        if accept {
            if self.fix == Fix::Lease {
                self.lease = None;
            }
            self.cache = Some(CacheEntry {
                val,
                expires_at: self.ttl_ms.map(|t| now + t),
            });
        }
    }

    fn cache_del(&mut self) {
        self.cache = None;
        self.lease = None;
    }

    fn apply(&mut self, step: Step, now: u64) {
        match (step.actor, step.idx) {
            (Actor::Reader, 1) => {
                if self.cache_val(now).is_some() {
                    self.alive = false; // cache hit: nothing to load
                } else if self.fix == Fix::Lease {
                    self.next_lease += 1;
                    self.lease = Some(self.next_lease);
                    self.token = self.lease;
                }
            }
            (Actor::Reader, 2) if self.alive => self.read = self.db,
            (Actor::Reader, 3) if self.alive => self.cache_set(now, self.read, self.token),
            (Actor::Writer, i) => {
                let db_step = matches!(
                    (self.order, i),
                    (WriterOrder::DbThenInvalidate, 1) | (WriterOrder::InvalidateThenDb, 2)
                );
                if db_step {
                    self.db = NEW;
                } else if self.fix == Fix::VersionedSet {
                    self.cache_set(now, NEW, None);
                } else {
                    self.cache_del();
                }
            }
            _ => {}
        }
    }
}

/// Final state of one interleaving.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaceOutcome {
    /// The executed order.
    pub steps: Vec<Step>,
    /// Value left in the cache (`None` = empty).
    pub cache: Option<u8>,
    /// Value in the DB.
    pub db: u8,
    /// `true` when the cache holds [`OLD`] while the DB holds [`NEW`].
    pub stale: bool,
}

impl RaceOutcome {
    /// `"r1 r2 w1 w2 r3"`.
    pub fn trace(&self) -> String {
        self.steps
            .iter()
            .map(Step::label)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Runs every interleaving for one writer order and one fix (no TTL: a stale value stays).
pub fn enumerate_race(order: WriterOrder, fix: Fix) -> Vec<RaceOutcome> {
    interleavings()
        .into_iter()
        .map(|steps| {
            let mut race = Race::new(order, fix, None);
            for (t, s) in steps.iter().enumerate() {
                race.apply(*s, t as u64);
            }
            let cache = race.cache_val(u64::MAX - 1);
            let stale = cache == Some(OLD) && race.db == NEW;
            RaceOutcome {
                steps,
                cache,
                db: race.db,
                stale,
            }
        })
        .collect()
}

/// Number of stale outcomes in [`enumerate_race`].
pub fn stale_count(order: WriterOrder, fix: Fix) -> usize {
    enumerate_race(order, fix)
        .iter()
        .filter(|o| o.stale)
        .count()
}

/// Replays the stale interleaving `r1 r2 w1 w2 r3` of order A on a millisecond timeline: `r1` at 0,
/// `r2` at 10, `w1` at 20, `w2` at 30, and the slow reader's `r3` at `reader_set_at_ms`. The
/// reader's `SET` carries `ttl_ms` (`None` = no expiry). Returns the number of milliseconds in
/// `[0, horizon_ms)` during which the cache served [`OLD`] while the DB held [`NEW`].
pub fn stale_window_ms(
    fix: Fix,
    ttl_ms: Option<u64>,
    reader_set_at_ms: u64,
    horizon_ms: u64,
) -> u64 {
    assert!(
        reader_set_at_ms > 30,
        "r3 must come after w2 for this interleaving"
    );
    let mut race = Race::new(WriterOrder::DbThenInvalidate, fix, ttl_ms);
    let script = [
        (
            0,
            Step {
                actor: Actor::Reader,
                idx: 1,
            },
        ),
        (
            10,
            Step {
                actor: Actor::Reader,
                idx: 2,
            },
        ),
        (
            20,
            Step {
                actor: Actor::Writer,
                idx: 1,
            },
        ),
        (
            30,
            Step {
                actor: Actor::Writer,
                idx: 2,
            },
        ),
        (
            reader_set_at_ms,
            Step {
                actor: Actor::Reader,
                idx: 3,
            },
        ),
    ];
    let mut next = 0;
    let mut stale_ms = 0;
    for t in 0..horizon_ms {
        while next < script.len() && script[next].0 == t {
            race.apply(script[next].1, t);
            next += 1;
        }
        if race.cache_val(t) == Some(OLD) && race.db == NEW {
            stale_ms += 1;
        }
    }
    stale_ms
}

// ------------------------------------------------------------------------------------------
// Part 3: lock release
// ------------------------------------------------------------------------------------------

/// How a client gives the lock back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Release {
    /// `DEL lock`: removes whatever value is there.
    PlainDel,
    /// Script "delete only if the value is still my token".
    TokenCompare,
}

/// One client of the lock scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LockClient {
    /// Display name.
    pub name: char,
    /// First attempt of `SET lock token NX PX ttl`.
    pub first_try_ms: u64,
    /// Delay between attempts while the lock is taken.
    pub retry_ms: u64,
    /// Length of the critical section once the lock is won.
    pub work_ms: u64,
}

/// A client's time inside the critical section, half-open `[acquired_ms, finished_ms)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    /// Client name.
    pub name: char,
    /// Time the lock was won.
    pub acquired_ms: u64,
    /// Time the work ended (the release happens at this instant).
    pub finished_ms: u64,
}

/// Two clients inside the critical section at the same time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlap {
    /// The client that acquired first.
    pub first: char,
    /// The client that acquired later.
    pub second: char,
    /// Start of the overlap.
    pub from_ms: u64,
    /// End of the overlap.
    pub to_ms: u64,
}

/// Result of [`simulate_lock`].
#[derive(Clone, Debug)]
pub struct LockRun {
    /// Human-readable event log (acquisitions and releases only).
    pub trace: Vec<String>,
    /// One entry per client that got the lock, in acquisition order.
    pub holds: Vec<Hold>,
    /// Every pair of holds that overlap, ordered by start time.
    pub overlaps: Vec<Overlap>,
    /// How many releases deleted a lock value that belonged to somebody else.
    pub foreign_deletes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LockEv {
    Try,
    Finish,
}

fn secs(ms: u64) -> String {
    format!("{:>3}.{:03}s", ms / 1000, ms % 1000)
}

/// One lock key with TTL `lock_ttl_ms`; every client retries every `retry_ms` until it wins, works
/// for `work_ms`, then releases with `release`. Virtual time, no failures other than "the TTL ran out".
pub fn simulate_lock(clients: &[LockClient], lock_ttl_ms: u64, release: Release) -> LockRun {
    let mut heap: BinaryHeap<Reverse<(u64, u64, usize, LockEv)>> = BinaryHeap::new();
    let mut seq = 0;
    for (i, c) in clients.iter().enumerate() {
        seq += 1;
        heap.push(Reverse((c.first_try_ms, seq, i, LockEv::Try)));
    }
    let mut lock: Option<(u64, u64)> = None; // (token, expires_at)
    let mut tokens = vec![0u64; clients.len()];
    let mut next_token = 0u64;
    let mut acquired = vec![0u64; clients.len()];
    let mut run = LockRun {
        trace: Vec::new(),
        holds: Vec::new(),
        overlaps: Vec::new(),
        foreign_deletes: 0,
    };
    while let Some(Reverse((now, _, i, ev))) = heap.pop() {
        let c = clients[i];
        match ev {
            LockEv::Try => {
                if lock.is_some_and(|(_, exp)| exp > now) {
                    seq += 1;
                    heap.push(Reverse((now + c.retry_ms, seq, i, LockEv::Try)));
                    continue;
                }
                next_token += 1;
                tokens[i] = next_token;
                lock = Some((next_token, now + lock_ttl_ms));
                acquired[i] = now;
                run.trace.push(format!(
                    "{} {}: SET lock tok{} NX PX {} -> OK (expires {})",
                    secs(now),
                    c.name,
                    c.name,
                    lock_ttl_ms,
                    secs(now + lock_ttl_ms)
                ));
                seq += 1;
                heap.push(Reverse((now + c.work_ms, seq, i, LockEv::Finish)));
            }
            LockEv::Finish => {
                run.holds.push(Hold {
                    name: c.name,
                    acquired_ms: acquired[i],
                    finished_ms: now,
                });
                let holder = lock.filter(|&(_, exp)| exp > now).map(|(t, _)| t);
                let owner_name = |t: u64| {
                    clients
                        .iter()
                        .enumerate()
                        .find(|(j, _)| tokens[*j] == t)
                        .map(|(_, c)| c.name)
                };
                let msg = match (release, holder) {
                    (Release::PlainDel, Some(t)) if t == tokens[i] => {
                        lock = None;
                        "DEL lock -> deleted its own lock".to_string()
                    }
                    (Release::PlainDel, Some(t)) => {
                        lock = None;
                        run.foreign_deletes += 1;
                        format!(
                            "DEL lock -> deleted {}'s lock!",
                            owner_name(t).unwrap_or('?')
                        )
                    }
                    (Release::PlainDel, None) => {
                        lock = None;
                        "DEL lock -> key already gone".to_string()
                    }
                    (Release::TokenCompare, Some(t)) if t == tokens[i] => {
                        lock = None;
                        "unlock script -> token matches, deleted".to_string()
                    }
                    (Release::TokenCompare, Some(t)) => {
                        format!(
                            "unlock script -> token is {}'s, no-op",
                            owner_name(t).unwrap_or('?')
                        )
                    }
                    (Release::TokenCompare, None) => {
                        "unlock script -> key already gone, no-op".to_string()
                    }
                };
                run.trace
                    .push(format!("{} {}: work done; {}", secs(now), c.name, msg));
            }
        }
    }
    run.holds.sort_by_key(|h| (h.acquired_ms, h.name));
    for (a, x) in run.holds.iter().enumerate() {
        for y in &run.holds[a + 1..] {
            let from = x.acquired_ms.max(y.acquired_ms);
            let to = x.finished_ms.min(y.finished_ms);
            if from < to {
                run.overlaps.push(Overlap {
                    first: x.name,
                    second: y.name,
                    from_ms: from,
                    to_ms: to,
                });
            }
        }
    }
    run.overlaps.sort_by_key(|o| (o.from_ms, o.first, o.second));
    run
}

/// The scripted scenario of the lab: A wins at 0 and works 45 s with a 30 s TTL; B polls from
/// 0.5 s every second and works 25 s; C polls from 0.75 s every second and works 20 s.
pub fn scripted_lock_clients() -> [LockClient; 3] {
    [
        LockClient {
            name: 'A',
            first_try_ms: 0,
            retry_ms: 1_000,
            work_ms: 45_000,
        },
        LockClient {
            name: 'B',
            first_try_ms: 500,
            retry_ms: 1_000,
            work_ms: 25_000,
        },
        LockClient {
            name: 'C',
            first_try_ms: 750,
            retry_ms: 1_000,
            work_ms: 20_000,
        },
    ]
}

/// A random 3..=6-client scenario for property sweeps (first attempts in `[0, 20 s)`, work in
/// `[1 s, 50 s)`, retry every second).
pub fn random_lock_clients(seed: u64) -> Vec<LockClient> {
    let mut rng = Rng::new(seed);
    let n = 3 + rng.below(4) as usize;
    (0..n)
        .map(|i| LockClient {
            name: (b'A' + i as u8) as char,
            first_try_ms: rng.below(20_000),
            retry_ms: 1_000,
            work_ms: rng.range(1_000, 50_000),
        })
        .collect()
}

// ------------------------------------------------------------------------------------------
// Part 4: negative caching
// ------------------------------------------------------------------------------------------

/// Result of [`simulate_negative_caching`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NegReport {
    /// Probe requests.
    pub requests: u64,
    /// Lookups that reached the DB.
    pub db_lookups: u64,
    /// Largest number of live negative entries.
    pub peak_entries: usize,
}

/// A bot probes ids that do not exist, one request every `interval_ms`, each id drawn uniformly
/// from `[0, id_space)`. With `neg_ttl_ms = Some(t)` a DB "not found" is cached for `t` ms.
pub fn simulate_negative_caching(
    seed: u64,
    requests: u64,
    interval_ms: u64,
    id_space: u64,
    neg_ttl_ms: Option<u64>,
) -> NegReport {
    let mut rng = Rng::new(seed);
    let mut live: BTreeMap<u64, u64> = BTreeMap::new(); // id -> expires_at
    let mut order: VecDeque<(u64, u64)> = VecDeque::new(); // (expires_at, id), expiry-sorted
    let mut rep = NegReport {
        requests,
        db_lookups: 0,
        peak_entries: 0,
    };
    for k in 0..requests {
        let now = k * interval_ms;
        while let Some(&(exp, id)) = order.front() {
            if exp > now {
                break;
            }
            order.pop_front();
            if live.get(&id) == Some(&exp) {
                live.remove(&id);
            }
        }
        let id = rng.below(id_space);
        if live.contains_key(&id) {
            continue; // negative hit: answered from cache
        }
        rep.db_lookups += 1;
        if let Some(ttl) = neg_ttl_ms {
            live.insert(id, now + ttl);
            order.push_back((now + ttl, id));
            rep.peak_entries = rep.peak_entries.max(live.len());
        }
    }
    rep
}

/// Id 7 is probed at t = 0 (DB says "not found", negative entry cached for `neg_ttl_ms`); the row
/// is created at `create_at_ms`; a client then reads it every `poll_ms` until `end_ms`. Returns
/// how many reads still got "404" although the row exists. With `delete_on_create` the creation
/// also deletes the negative entry.
pub fn negative_entry_pitfall(
    delete_on_create: bool,
    neg_ttl_ms: u64,
    create_at_ms: u64,
    poll_ms: u64,
    end_ms: u64,
) -> u64 {
    let mut neg_expires: Option<u64> = Some(neg_ttl_ms);
    let mut wrong = 0;
    let mut t = create_at_ms;
    if delete_on_create {
        neg_expires = None;
    }
    t += poll_ms;
    while t < end_ms {
        match neg_expires {
            Some(exp) if exp > t => wrong += 1, // cached "not found" shadows the new row
            _ => neg_expires = None,            // reads the DB, finds the row
        }
        t += poll_ms;
    }
    wrong
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: StampedeCfg = StampedeCfg::PAPER_EXAMPLE;

    // ---- part 1 ----

    #[test]
    fn naive_recomputes_equal_rate_times_compute_time() {
        // 10 req/s x 3 s = 30 (the VLDB 2015 example), for every arrival phase.
        for seed in 0..100 {
            let r = simulate_stampede(Policy::naive(), CFG.one_cycle(), seed);
            assert_eq!(r.recomputes, 30, "seed {seed}");
            assert_eq!(r.waves, 1);
            assert_eq!(r.peak_db, 30);
            assert_eq!(r.misses, 30);
            assert_eq!(r.completed, r.requests);
            assert_eq!(r.wait_max_ms, CFG.compute_ms);
        }
        // Other rates/compute times: rate x time stays exact on the 100 ms grid.
        for (interval, compute) in [(50, 2_000), (250, 3_000), (100, 700)] {
            let cfg = StampedeCfg {
                interval_ms: interval,
                compute_ms: compute,
                ..CFG
            }
            .one_cycle();
            let r = simulate_stampede(Policy::naive(), cfg, 5);
            assert_eq!(
                r.recomputes,
                compute / interval,
                "interval {interval} compute {compute}"
            );
        }
    }

    #[test]
    fn naive_over_many_cycles_is_thirty_per_wave() {
        for seed in 0..10 {
            let r = simulate_stampede(Policy::naive(), CFG, seed);
            assert!(r.waves >= 15);
            assert_eq!(r.recomputes, 30 * r.waves, "seed {seed}");
        }
    }

    #[test]
    fn single_flight_recomputes_once_and_every_loser_reads_the_value() {
        for seed in 0..100 {
            let r = simulate_stampede(Policy::single_flight(10_000), CFG, seed);
            assert_eq!(r.recomputes, r.waves, "seed {seed}: one recompute per wave");
            assert_eq!(r.peak_db, 1);
            assert_eq!(
                r.completed, r.requests,
                "seed {seed}: every loser eventually finishes"
            );
            assert!(r.misses >= 30, "the 30 arrivals during a load all missed");
            // Loser waits for the load, then up to one capped back-off sleep (160 + 80 jitter).
            assert!(
                r.wait_max_ms <= CFG.compute_ms + 240,
                "seed {seed}: {}",
                r.wait_max_ms
            );
            assert!(
                r.wait_max_ms >= CFG.compute_ms,
                "the winner pays the whole compute time"
            );
        }
    }

    #[test]
    fn lock_ttl_shorter_than_compute_lets_a_second_recompute_in() {
        for seed in 0..50 {
            let short = simulate_stampede(Policy::single_flight(1_000), CFG.one_cycle(), seed);
            assert!(short.recomputes >= 2, "seed {seed}: {}", short.recomputes);
            assert_eq!(short.completed, short.requests);
            // The cure is lock_ttl > compute time; the extra slack covers the back-off sleep.
            let ok = simulate_stampede(
                Policy::single_flight(CFG.compute_ms + 500),
                CFG.one_cycle(),
                seed,
            );
            assert_eq!(ok.recomputes, 1, "seed {seed}");
        }
    }

    #[test]
    fn xfetch_needs_a_few_recomputes_per_wave_instead_of_thirty_and_almost_never_misses() {
        let mut recomputes = 0;
        let mut waves = 0;
        let mut misses = 0;
        let mut requests = 0;
        for seed in 0..40 {
            let r = simulate_stampede(Policy::xfetch(1.0), CFG, seed);
            recomputes += r.recomputes;
            waves += r.waves;
            misses += r.misses;
            requests += r.requests;
            assert_eq!(r.completed, r.requests);
        }
        let per_wave = recomputes as f64 / waves as f64;
        // Not exactly 1: requests that arrive while the early recompute runs may also trigger.
        assert!((1.0..5.0).contains(&per_wave), "per wave {per_wave}");
        assert!(
            per_wave < 30.0 / 5.0,
            "far fewer than the 30 of the naive policy"
        );
        let miss_frac = misses as f64 / requests as f64;
        assert!(miss_frac < 0.001, "miss fraction {miss_frac}");
        // Naive misses 30 of every ~660 requests.
        let naive = simulate_stampede(Policy::naive(), CFG, 1);
        assert!(naive.miss_frac() > 0.04);
    }

    #[test]
    fn xfetch_with_lock_is_exactly_one_per_wave_and_larger_beta_refreshes_earlier() {
        for seed in 0..40 {
            let r = simulate_stampede(Policy::xfetch_with_lock(1.0, 10_000), CFG, seed);
            assert_eq!(r.recomputes, r.waves, "seed {seed}");
            assert_eq!(r.peak_db, 1);
        }
        let waves = |beta| simulate_stampede(Policy::xfetch_with_lock(beta, 10_000), CFG, 3).waves;
        assert!(
            waves(0.5) < waves(1.0) && waves(1.0) < waves(2.0),
            "{} {} {}",
            waves(0.5),
            waves(1.0),
            waves(2.0)
        );
    }

    #[test]
    fn jitter_strictly_lowers_the_peak_db_concurrency() {
        for seed in 0..50 {
            let same = simulate_expiry_burst(500, 60_000, 0, 500, 100, seed);
            let jit = simulate_expiry_burst(500, 60_000, 10, 500, 100, seed);
            assert_eq!(
                same.peak_db, 500,
                "identical TTL: everything reloads inside one read period"
            );
            assert!(
                jit.peak_db < same.peak_db / 5,
                "seed {seed}: {} vs {}",
                jit.peak_db,
                same.peak_db
            );
            assert!(jit.spread_ms > same.spread_ms);
        }
    }

    #[test]
    fn simulation_is_a_pure_function_of_the_seed() {
        let a = simulate_stampede(Policy::xfetch(1.0), CFG, 9);
        let b = simulate_stampede(Policy::xfetch(1.0), CFG, 9);
        assert_eq!(a, b);
        let c = simulate_stampede(Policy::xfetch(1.0), CFG, 10);
        assert_ne!(a, c);
    }

    // ---- part 2 ----

    #[test]
    fn there_are_ten_interleavings_each_keeping_program_order() {
        let all = interleavings();
        assert_eq!(all.len(), 10);
        let mut seen: Vec<String> = Vec::new();
        for steps in &all {
            let t = steps.iter().map(Step::label).collect::<Vec<_>>().join(" ");
            assert!(!seen.contains(&t), "duplicate {t}");
            seen.push(t);
            for a in [Actor::Reader, Actor::Writer] {
                let idx: Vec<u8> = steps
                    .iter()
                    .filter(|s| s.actor == a)
                    .map(|s| s.idx)
                    .collect();
                let want: Vec<u8> = if a == Actor::Reader {
                    vec![1, 2, 3]
                } else {
                    vec![1, 2]
                };
                assert_eq!(idx, want);
            }
        }
    }

    #[test]
    fn order_a_has_exactly_one_stale_interleaving_and_it_is_r1_r2_w1_w2_r3() {
        let a = enumerate_race(WriterOrder::DbThenInvalidate, Fix::None);
        let stale: Vec<String> = a
            .iter()
            .filter(|o| o.stale)
            .map(RaceOutcome::trace)
            .collect();
        assert_eq!(stale, vec!["r1 r2 w1 w2 r3".to_string()]);
    }

    /// Independent oracle: position arithmetic instead of a state machine.
    #[test]
    fn counts_match_a_hand_derived_predicate_and_are_pinned() {
        let pos = |steps: &[Step], actor, idx| {
            steps
                .iter()
                .position(|s| s.actor == actor && s.idx == idx)
                .unwrap()
        };
        for o in enumerate_race(WriterOrder::DbThenInvalidate, Fix::None) {
            let (r2, r3) = (
                pos(&o.steps, Actor::Reader, 2),
                pos(&o.steps, Actor::Reader, 3),
            );
            let (w1, w2) = (
                pos(&o.steps, Actor::Writer, 1),
                pos(&o.steps, Actor::Writer, 2),
            );
            // Stale iff the reader read the old row (r2 < w1: DB write) and SET after the DEL (w2 < r3).
            assert_eq!(o.stale, r2 < w1 && w2 < r3, "{}", o.trace());
        }
        for o in enumerate_race(WriterOrder::InvalidateThenDb, Fix::None) {
            let (r2, r3) = (
                pos(&o.steps, Actor::Reader, 2),
                pos(&o.steps, Actor::Reader, 3),
            );
            let (w1, w2) = (
                pos(&o.steps, Actor::Writer, 1),
                pos(&o.steps, Actor::Writer, 2),
            );
            // Stale iff the reader read before the DB write (r2 < w2) and its SET survives the DEL (w1 < r3).
            assert_eq!(o.stale, r2 < w2 && w1 < r3, "{}", o.trace());
        }
        assert_eq!(stale_count(WriterOrder::DbThenInvalidate, Fix::None), 1);
        assert_eq!(stale_count(WriterOrder::InvalidateThenDb, Fix::None), 6);
    }

    #[test]
    fn lease_removes_every_stale_set_of_order_a_but_not_the_wrong_order_b() {
        assert_eq!(stale_count(WriterOrder::DbThenInvalidate, Fix::Lease), 0);
        // In order B the DEL comes before the DB write: a reader that gets its lease after the
        // DEL and reads before the DB write keeps a valid token, so exactly these two survive.
        let b = enumerate_race(WriterOrder::InvalidateThenDb, Fix::Lease);
        let mut stale: Vec<String> = b
            .iter()
            .filter(|o| o.stale)
            .map(RaceOutcome::trace)
            .collect();
        stale.sort();
        assert_eq!(
            stale,
            vec!["w1 r1 r2 r3 w2".to_string(), "w1 r1 r2 w2 r3".to_string()]
        );
    }

    #[test]
    fn versioned_set_leaves_no_stale_interleaving_in_either_order() {
        assert_eq!(
            stale_count(WriterOrder::DbThenInvalidate, Fix::VersionedSet),
            0
        );
        assert_eq!(
            stale_count(WriterOrder::InvalidateThenDb, Fix::VersionedSet),
            0
        );
    }

    #[test]
    fn a_fix_never_adds_a_stale_outcome_that_the_naive_design_did_not_have() {
        for order in [WriterOrder::DbThenInvalidate, WriterOrder::InvalidateThenDb] {
            let naive = enumerate_race(order, Fix::None);
            for fix in [Fix::Lease, Fix::VersionedSet] {
                for (n, f) in naive.iter().zip(enumerate_race(order, fix)) {
                    assert_eq!(n.steps, f.steps);
                    assert!(!f.stale || n.stale, "{order:?} {fix:?} {}", f.trace());
                }
            }
        }
    }

    #[test]
    fn stale_lifetime_is_bounded_by_ttl_and_unbounded_without_one() {
        let horizon = 300_000;
        let pause = 2_000;
        for ttl in [1_000, 5_000, 60_000] {
            let w = stale_window_ms(Fix::None, Some(ttl), pause, horizon);
            assert_eq!(w, ttl, "stale for exactly the TTL of the stale SET");
        }
        assert_eq!(
            stale_window_ms(Fix::None, None, pause, horizon),
            horizon - pause
        );
        assert_eq!(stale_window_ms(Fix::Lease, Some(60_000), pause, horizon), 0);
        assert_eq!(stale_window_ms(Fix::VersionedSet, None, pause, horizon), 0);
    }

    // ---- part 3 ----

    #[test]
    fn plain_del_release_creates_a_second_violation_that_token_compare_avoids() {
        let clients = scripted_lock_clients();
        let plain = simulate_lock(&clients, 30_000, Release::PlainDel);
        let safe = simulate_lock(&clients, 30_000, Release::TokenCompare);
        let pairs = |r: &LockRun| {
            r.overlaps
                .iter()
                .map(|o| (o.first, o.second))
                .collect::<Vec<_>>()
        };
        assert_eq!(pairs(&plain), vec![('A', 'B'), ('B', 'C')]);
        assert_eq!(pairs(&safe), vec![('A', 'B')]);
        // A's DEL removes B's lock, and then B's own DEL removes C's: the damage cascades.
        assert_eq!(plain.foreign_deletes, 2);
        assert_eq!(safe.foreign_deletes, 0);
        // A's own overlap with B is the TTL's doing (A worked 45 s on a 30 s lock): both variants have it.
        assert_eq!(
            (safe.overlaps[0].from_ms, safe.overlaps[0].to_ms),
            (30_500, 45_000)
        );
        // Plain DEL: C walks in at its first poll after A's DEL, while B (done at 55.5 s) still works.
        let c = plain.holds.iter().find(|h| h.name == 'C').unwrap();
        assert_eq!(c.acquired_ms, 45_750);
        let b = plain.holds.iter().find(|h| h.name == 'B').unwrap();
        assert!(c.acquired_ms < b.finished_ms);
        // Token compare: C only enters after B has released.
        let c = safe.holds.iter().find(|h| h.name == 'C').unwrap();
        let b = safe.holds.iter().find(|h| h.name == 'B').unwrap();
        assert!(c.acquired_ms >= b.finished_ms);
    }

    #[test]
    fn token_compare_only_overlaps_when_the_earlier_holder_outlived_the_ttl() {
        let ttl = 30_000;
        let work = |r: &LockRun, n: char| {
            r.holds
                .iter()
                .find(|h| h.name == n)
                .map(|h| h.finished_ms - h.acquired_ms)
                .unwrap()
        };
        let mut plain_trampled_a_fast_holder = 0;
        let mut safe_overlaps = 0;
        for seed in 0..500 {
            let clients = random_lock_clients(seed);
            let safe = simulate_lock(&clients, ttl, Release::TokenCompare);
            let plain = simulate_lock(&clients, ttl, Release::PlainDel);
            assert_eq!(
                safe.holds.len(),
                clients.len(),
                "everybody eventually gets the lock"
            );
            for o in &safe.overlaps {
                safe_overlaps += 1;
                assert!(
                    work(&safe, o.first) > ttl,
                    "seed {seed}: overlap although {} finished within the TTL",
                    o.first
                );
            }
            for o in &plain.overlaps {
                if work(&plain, o.first) <= ttl {
                    plain_trampled_a_fast_holder += 1;
                }
            }
        }
        assert!(
            safe_overlaps > 0,
            "slow holders do overlap even with token compare: that needs fencing"
        );
        assert!(
            plain_trampled_a_fast_holder > 0,
            "plain DEL overlaps a holder that stayed within its TTL"
        );
    }

    // ---- part 4 ----

    #[test]
    fn negative_caching_cuts_db_lookups_when_ids_repeat_and_not_when_they_do_not() {
        let (n, dt) = (12_000, 5); // 200 req/s for 60 s
        let without = simulate_negative_caching(1, n, dt, 1_000, None);
        let with = simulate_negative_caching(1, n, dt, 1_000, Some(30_000));
        assert_eq!(without.db_lookups, n);
        assert!(
            with.db_lookups * 4 <= without.db_lookups,
            "{} vs {}",
            with.db_lookups,
            without.db_lookups
        );
        assert!(with.peak_entries <= 1_000);
        // A scan over a huge id space never repeats an id: the cache cannot help (and fills up).
        let scan = simulate_negative_caching(1, n, dt, 100_000_000, Some(30_000));
        assert!(scan.db_lookups as f64 >= 0.99 * n as f64);
        assert!(scan.peak_entries > 5_000);
    }

    #[test]
    fn creating_the_row_must_delete_the_negative_entry() {
        let wrong = |del| negative_entry_pitfall(del, 30_000, 5_000, 100, 60_000);
        assert_eq!(
            wrong(false),
            249,
            "reads at 5.1 s ..= 29.9 s still see the cached 404"
        );
        assert_eq!(wrong(true), 0);
    }
}
