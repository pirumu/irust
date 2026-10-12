//! Leaderless quorum store: `n` replicas, write quorum `w`, read quorum `r`.
//!
//! The model is deliberately small. Replicas keep one `(version)` per key (the value is implied by
//! the version) and only ever move forward: an incoming version is applied iff it is greater than
//! the local one. Versions come from one logical writer (a monotonic counter). Operations run one
//! after the other (no overlap), except in the scripted counterexample of part 2.
//!
//! * Part 1 ([`Sim`], [`run`]): every message to every replica is lost independently with
//!   probability `loss` ("the replica is unreachable for this operation"). Optional read repair.
//! * Part 2 ([`scripted_trace`], [`scripted_pair`]): the lecture-notes counterexample showing that
//!   `w + r > n` is not linearizability, and the ABD-style write-back that repairs it.
//! * Part 3 ([`run_sloppy`]): strict quorum vs sloppy quorum with hinted handoff.
//!
//! Everything is a pure function of the seed. Nothing here is a Dynamo or Cassandra implementation.

use crate::rng::Rng;

/// Largest supported number of replicas (sets of replicas are `u32` bitmasks, versions live in
/// fixed arrays so a trace event needs no allocation).
pub const MAX_N: usize = 8;

/// Configuration of the random-failure experiment of part 1.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Number of replicas, `1..=MAX_N`.
    pub n: usize,
    /// Replicas that must apply a write for it to be acknowledged, `1..=n`.
    pub w: usize,
    /// Replicas that must answer a read for it to succeed, `1..=n`.
    pub r: usize,
    /// Probability that one message to one replica is lost.
    pub loss: f64,
    /// After a successful read, push the highest version back to answering replicas that were
    /// behind (those messages are lossy too).
    pub read_repair: bool,
    /// Number of operations per run (each is a write or a read with probability 1/2).
    pub ops: usize,
}

impl Config {
    /// Checks the invariants `1 <= w, r <= n <= MAX_N` and returns the config.
    pub fn new(n: usize, w: usize, r: usize, loss: f64, read_repair: bool, ops: usize) -> Self {
        assert!((1..=MAX_N).contains(&n), "n out of range");
        assert!(
            (1..=n).contains(&w) && (1..=n).contains(&r),
            "w and r must be in 1..=n"
        );
        Self {
            n,
            w,
            r,
            loss,
            read_repair,
            ops,
        }
    }

    /// `true` iff the write and read quorums are guaranteed to intersect.
    pub fn overlaps(&self) -> bool {
        self.w + self.r > self.n
    }
}

/// Counters accumulated over one or many runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Writes attempted.
    pub writes: u64,
    /// Writes acknowledged (reached at least `w` replicas).
    pub writes_ok: u64,
    /// Reads attempted.
    pub reads: u64,
    /// Reads that got at least `r` answers.
    pub reads_ok: u64,
    /// Successful reads that returned a version lower than the latest acknowledged write.
    pub stale: u64,
    /// Successful reads that returned a version *higher* than the latest acknowledged write, i.e.
    /// they exposed a write that was reported as failed.
    pub ghost: u64,
}

impl Stats {
    /// Adds another run's counters.
    pub fn add(&mut self, o: &Stats) {
        self.writes += o.writes;
        self.writes_ok += o.writes_ok;
        self.reads += o.reads;
        self.reads_ok += o.reads_ok;
        self.stale += o.stale;
        self.ghost += o.ghost;
    }
}

/// One operation of the random experiment, recorded when a trace is wanted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A write of `version` that was applied by the replicas in the bitmask `reached`.
    Write {
        version: u64,
        reached: u32,
        acked: bool,
    },
    /// A read answered by the replicas in `answered`; `seen[i]` is replica `i`'s version at that
    /// moment. `returned` is `None` when fewer than `r` replicas answered.
    Read {
        answered: u32,
        seen: [u64; MAX_N],
        returned: Option<u64>,
        acked_before: u64,
        repaired: u32,
    },
}

/// Name of replica `i` (`A`, `B`, ...).
pub fn name(i: usize) -> char {
    (b'A' + i as u8) as char
}

fn set_names(mask: u32, n: usize) -> String {
    let s: String = (0..n).filter(|i| mask >> i & 1 == 1).map(name).collect();
    format!("{{{s}}}")
}

impl Event {
    /// One-line human-readable description (`n` is the replica count).
    pub fn describe(&self, n: usize) -> String {
        match *self {
            Event::Write {
                version,
                reached,
                acked,
            } => format!(
                "write v{version}: applied by {} -> {}",
                set_names(reached, n),
                if acked {
                    "ACK"
                } else {
                    "FAILED (but the copies stay)"
                }
            ),
            Event::Read {
                answered,
                seen,
                returned,
                acked_before,
                repaired,
            } => {
                let seen_s: Vec<String> = (0..n)
                    .filter(|i| answered >> i & 1 == 1)
                    .map(|i| format!("{}:v{}", name(i), seen[i]))
                    .collect();
                match returned {
                    None => format!("read: only {} answered -> FAILED", set_names(answered, n)),
                    Some(v) => {
                        let verdict = if v < acked_before {
                            format!("STALE (latest acked is v{acked_before})")
                        } else {
                            "ok".to_string()
                        };
                        let rep = if repaired != 0 {
                            format!(", repair -> {}", set_names(repaired, n))
                        } else {
                            String::new()
                        };
                        format!("read: {} -> returns v{v} {verdict}{rep}", seen_s.join(" "))
                    }
                }
            }
        }
    }
}

/// The step-by-step simulation of part 1.
pub struct Sim {
    cfg: Config,
    rng: Rng,
    versions: Vec<u64>,
    next_version: u64,
    acked: u64,
    /// Counters so far.
    pub stats: Stats,
}

impl Sim {
    /// Fresh simulation: every replica holds version 0, which counts as acknowledged.
    pub fn new(cfg: Config, seed: u64) -> Self {
        Self {
            cfg,
            rng: Rng::new(seed),
            versions: vec![0; cfg.n],
            next_version: 1,
            acked: 0,
            stats: Stats::default(),
        }
    }

    /// Runs one operation (write or read with probability 1/2 each) and returns what happened.
    pub fn step(&mut self) -> Event {
        if self.rng.chance(0.5) {
            self.write()
        } else {
            self.read()
        }
    }

    fn write(&mut self) -> Event {
        let version = self.next_version;
        self.next_version += 1;
        let mut reached = 0u32;
        for i in 0..self.cfg.n {
            if !self.rng.chance(self.cfg.loss) {
                reached |= 1 << i;
                if version > self.versions[i] {
                    self.versions[i] = version;
                }
            }
        }
        let acked = reached.count_ones() as usize >= self.cfg.w;
        self.stats.writes += 1;
        if acked {
            self.stats.writes_ok += 1;
            self.acked = version;
        }
        Event::Write {
            version,
            reached,
            acked,
        }
    }

    fn read(&mut self) -> Event {
        let mut answered = 0u32;
        for i in 0..self.cfg.n {
            if !self.rng.chance(self.cfg.loss) {
                answered |= 1 << i;
            }
        }
        let mut seen = [0u64; MAX_N];
        seen[..self.cfg.n].copy_from_slice(&self.versions);
        self.stats.reads += 1;
        let acked_before = self.acked;
        let mut repaired = 0u32;
        let returned = if answered.count_ones() as usize >= self.cfg.r {
            self.stats.reads_ok += 1;
            let best = (0..self.cfg.n)
                .filter(|i| answered >> i & 1 == 1)
                .map(|i| self.versions[i])
                .max()
                .unwrap_or(0);
            if best < self.acked {
                self.stats.stale += 1;
            } else if best > self.acked {
                self.stats.ghost += 1;
            }
            if self.cfg.read_repair {
                for i in 0..self.cfg.n {
                    if answered >> i & 1 == 1
                        && self.versions[i] < best
                        && !self.rng.chance(self.cfg.loss)
                    {
                        self.versions[i] = best;
                        repaired |= 1 << i;
                    }
                }
            }
            Some(best)
        } else {
            None
        };
        Event::Read {
            answered,
            seen,
            returned,
            acked_before,
            repaired,
        }
    }
}

/// Runs `cfg.ops` operations from `seed` and returns the counters.
pub fn run(cfg: Config, seed: u64) -> Stats {
    let mut sim = Sim::new(cfg, seed);
    for _ in 0..cfg.ops {
        sim.step();
    }
    sim.stats
}

/// Sums [`run`] over the seeds `base..base + seeds`.
pub fn sweep(cfg: Config, base: u64, seeds: u64) -> Stats {
    let mut total = Stats::default();
    for s in base..base + seeds {
        total.add(&run(cfg, s));
    }
    total
}

/// The events of `seed` up to and including its first stale read, or `None` if there is none within
/// `cfg.ops` operations.
pub fn first_stale_trace(cfg: Config, seed: u64) -> Option<Vec<Event>> {
    let mut sim = Sim::new(cfg, seed);
    let mut events = Vec::new();
    for _ in 0..cfg.ops {
        let before = sim.stats.stale;
        events.push(sim.step());
        if sim.stats.stale > before {
            return Some(events);
        }
    }
    None
}

/// The shortest stale-read trace among the seeds `base..base + seeds` (smallest seed on ties).
pub fn shortest_stale_trace(cfg: Config, base: u64, seeds: u64) -> Option<(u64, Vec<Event>)> {
    let mut best: Option<(u64, Vec<Event>)> = None;
    for s in base..base + seeds {
        if let Some(t) = first_stale_trace(cfg, s)
            && best.as_ref().is_none_or(|(_, b)| t.len() < b.len())
        {
            best = Some((s, t));
        }
    }
    best
}

// ---------------------------------------------------------------------------------------------
// Part 2: quorum intersection and the non-linearizable read pair.
// ---------------------------------------------------------------------------------------------

/// `true` iff every set of size at least `w` intersects every set of size at least `r` over `n`
/// replicas, checked by brute force over bitmasks.
pub fn all_quorums_intersect(n: usize, w: usize, r: usize) -> bool {
    let full = 1u32 << n;
    (0..full)
        .filter(|m| m.count_ones() as usize >= w)
        .all(|ws| {
            (0..full)
                .filter(|m| m.count_ones() as usize >= r)
                .all(|rs| ws & rs != 0)
        })
}

/// Two sequential reads over `n` replicas where an *unfinished* write of v1 has reached exactly the
/// replicas in `written` (the others still hold v0). Reader 1 reads the set `q1`, and only after it
/// returned does reader 2 read the set `q2`. With `write_back`, reader 1 first writes the version
/// it is about to return to every replica of `q1` (ABD-style). Returns `(reader1, reader2)`.
pub fn scripted_pair(n: usize, written: u32, q1: u32, q2: u32, write_back: bool) -> (u64, u64) {
    let mut v: Vec<u64> = (0..n).map(|i| u64::from(written >> i & 1 == 1)).collect();
    let max_of = |v: &[u64], q: u32| {
        (0..n)
            .filter(|i| q >> i & 1 == 1)
            .map(|i| v[i])
            .max()
            .unwrap_or(0)
    };
    let r1 = max_of(&v, q1);
    if write_back {
        for (i, x) in v.iter_mut().enumerate() {
            if q1 >> i & 1 == 1 && *x < r1 {
                *x = r1;
            }
        }
    }
    (r1, max_of(&v, q2))
}

/// Number of `(q1, q2)` pairs of read quorums of size `r` (and write sets `written` of size `< w`,
/// i.e. unfinished writes) for which reader 2 returns an older version than reader 1.
pub fn monotonic_violations(n: usize, w: usize, r: usize, write_back: bool) -> u64 {
    let full = 1u32 << n;
    let mut bad = 0;
    for written in (0..full).filter(|m| (m.count_ones() as usize) < w) {
        for q1 in (0..full).filter(|m| m.count_ones() as usize == r) {
            for q2 in (0..full).filter(|m| m.count_ones() as usize == r) {
                let (a, b) = scripted_pair(n, written, q1, q2, write_back);
                if b < a {
                    bad += 1;
                }
            }
        }
    }
    bad
}

/// The narrated lecture-notes trace (n = 3, w = 2, r = 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptTrace {
    /// Narration, one line per step.
    pub log: Vec<String>,
    /// Version returned by reader 1.
    pub reader1: u64,
    /// Version returned by reader 2 (which starts after reader 1 has returned).
    pub reader2: u64,
}

/// Builds the narrated trace with or without ABD-style write-back.
pub fn scripted_trace(write_back: bool) -> ScriptTrace {
    let (a, b, c) = (0b001, 0b010, 0b100);
    let (r1, r2) = scripted_pair(3, a, a | b, b | c, write_back);
    let mut log = vec![
        "writer: write v1 sent to A,B,C; only A has applied it so far (B, C in flight)".to_string(),
        "reader 1: reads {A,B}: A:v1 B:v0 -> highest is v1".to_string(),
    ];
    if write_back {
        log.push("reader 1: write-back v1 to {A,B}, waits for both acks -> B:v1".to_string());
    }
    log.push(format!("reader 1: returns v{r1}"));
    log.push("reader 2: starts AFTER reader 1 returned; reads {B,C}".to_string());
    log.push(format!(
        "reader 2: B:v{} C:v0 -> returns v{r2}{}",
        u64::from(write_back),
        if r2 < r1 {
            "   <- older than what reader 1 returned!"
        } else {
            ""
        }
    ));
    ScriptTrace {
        log,
        reader1: r1,
        reader2: r2,
    }
}

// ---------------------------------------------------------------------------------------------
// Part 3: sloppy quorum + hinted handoff.
// ---------------------------------------------------------------------------------------------

/// Strict quorum (write only to the `n` home replicas) or sloppy quorum (write to the first `n`
/// *reachable* nodes of the preference list `home ++ standbys`, standbys keep hints).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Home replicas only; the write is rejected when fewer than `w` of them are reachable.
    Strict,
    /// First `n` reachable nodes of the preference list; hinted handoff after the outage.
    Sloppy,
}

/// Parameters of the sloppy-quorum experiment (`w + r > n` is expected).
#[derive(Clone, Copy, Debug)]
pub struct SloppyConfig {
    /// Home replicas of the key.
    pub n: usize,
    /// Write quorum.
    pub w: usize,
    /// Read quorum (reads contact home replicas only).
    pub r: usize,
    /// Extra nodes at the end of the preference list.
    pub standbys: usize,
    /// Probability that a node is unreachable for one operation during the outage.
    pub loss: f64,
    /// Number of write+read rounds per run.
    pub rounds: usize,
    /// Deliver the hints (standby -> intended home replica) before the second read. `false` is the
    /// counterfactual "the standbys never hand the data back".
    pub deliver_hints: bool,
}

/// Counters of the sloppy-quorum experiment. In every round: write, read during the outage
/// ("before"), deliver hints (if `deliver_hints`), then read again ("after"). Both reads follow the
/// same rule: contact the home replicas, each reachable with probability `1 - loss`, succeed iff at
/// least `r` answered, return the highest version.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SloppyStats {
    /// Writes acknowledged.
    pub writes_ok: u64,
    /// Writes rejected (unavailable).
    pub writes_rejected: u64,
    /// Successful reads during the outage.
    pub reads_before: u64,
    /// Of those, stale.
    pub stale_before: u64,
    /// Successful reads after the hint-delivery step.
    pub reads_after: u64,
    /// Of those, stale.
    pub stale_after: u64,
    /// Hints delivered to a home replica that was behind.
    pub hints_delivered: u64,
}

impl SloppyStats {
    /// Adds another run's counters.
    pub fn add(&mut self, o: &SloppyStats) {
        self.writes_ok += o.writes_ok;
        self.writes_rejected += o.writes_rejected;
        self.reads_before += o.reads_before;
        self.stale_before += o.stale_before;
        self.reads_after += o.reads_after;
        self.stale_after += o.stale_after;
        self.hints_delivered += o.hints_delivered;
    }
}

/// One run. Availability is drawn first and identically for both modes, so for the same seed
/// `Strict` and `Sloppy` face exactly the same failures.
pub fn run_sloppy(cfg: SloppyConfig, mode: Mode, seed: u64) -> SloppyStats {
    assert!((1..=cfg.n).contains(&cfg.w) && (1..=cfg.n).contains(&cfg.r));
    let total = cfg.n + cfg.standbys;
    let mut rng = Rng::new(seed);
    let mut home = vec![0u64; cfg.n];
    let (mut next_version, mut acked) = (1u64, 0u64);
    let mut st = SloppyStats::default();
    for _ in 0..cfg.rounds {
        let write_up: Vec<bool> = (0..total).map(|_| !rng.chance(cfg.loss)).collect();
        let read_up: Vec<bool> = (0..cfg.n).map(|_| !rng.chance(cfg.loss)).collect();
        // Drawn unconditionally so that `deliver_hints` does not change the random stream.
        let read_up_after: Vec<bool> = (0..cfg.n).map(|_| !rng.chance(cfg.loss)).collect();
        let version = next_version;
        next_version += 1;

        // Write.
        let mut hints: Vec<(usize, u64)> = Vec::new(); // (intended home replica, version)
        let applied = match mode {
            Mode::Strict => {
                let mut k = 0;
                for i in (0..cfg.n).filter(|&i| write_up[i]) {
                    home[i] = home[i].max(version);
                    k += 1;
                }
                k
            }
            Mode::Sloppy => {
                let chosen: Vec<usize> = (0..total).filter(|&i| write_up[i]).take(cfg.n).collect();
                for &i in chosen.iter().filter(|&&i| i < cfg.n) {
                    home[i] = home[i].max(version);
                }
                let down_homes = (0..cfg.n).filter(|&i| !write_up[i]);
                let standbys = chosen.iter().filter(|&&i| i >= cfg.n);
                for (h, _) in down_homes.zip(standbys) {
                    hints.push((h, version));
                }
                chosen.len()
            }
        };
        if applied >= cfg.w {
            st.writes_ok += 1;
            acked = version;
        } else {
            st.writes_rejected += 1;
        }

        // Read during the outage: home replicas only.
        let answers: Vec<u64> = (0..cfg.n)
            .filter(|&i| read_up[i])
            .map(|i| home[i])
            .collect();
        if answers.len() >= cfg.r {
            st.reads_before += 1;
            if answers.iter().copied().max().unwrap_or(0) < acked {
                st.stale_before += 1;
            }
        }

        // Hint delivery (home replicas are back), then a second read under the same rule as the
        // first. After delivery at least `w` home replicas hold the acked version, so with
        // `w + r > n` every successful r-quorum read is fresh.
        if cfg.deliver_hints {
            for (h, v) in hints {
                if v > home[h] {
                    home[h] = v;
                    st.hints_delivered += 1;
                }
            }
        }
        let answers: Vec<u64> = (0..cfg.n)
            .filter(|&i| read_up_after[i])
            .map(|i| home[i])
            .collect();
        if answers.len() >= cfg.r {
            st.reads_after += 1;
            if answers.iter().copied().max().unwrap_or(0) < acked {
                st.stale_after += 1;
            }
        }
    }
    st
}

/// Sums [`run_sloppy`] over the seeds `base..base + seeds`.
pub fn sweep_sloppy(cfg: SloppyConfig, mode: Mode, base: u64, seeds: u64) -> SloppyStats {
    let mut total = SloppyStats::default();
    for s in base..base + seeds {
        total.add(&run_sloppy(cfg, mode, s));
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOSSES: [f64; 4] = [0.0, 0.1, 0.3, 0.5];

    #[test]
    fn quorums_intersect_iff_w_plus_r_exceeds_n_for_all_n_up_to_7() {
        for n in 1..=7usize {
            for w in 1..=n {
                for r in 1..=n {
                    assert_eq!(
                        all_quorums_intersect(n, w, r),
                        w + r > n,
                        "n={n} w={w} r={r}"
                    );
                }
            }
        }
    }

    #[test]
    fn overlapping_quorums_never_read_stale_over_many_seeds_and_losses() {
        for n in 1..=5usize {
            for w in 1..=n {
                for r in (1..=n).filter(|r| w + r > n) {
                    for loss in LOSSES {
                        for repair in [false, true] {
                            let s = sweep(Config::new(n, w, r, loss, repair, 100), 1000, 100);
                            assert_eq!(s.stale, 0, "n={n} w={w} r={r} loss={loss} repair={repair}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn non_overlapping_quorums_do_read_stale_in_aggregate() {
        for n in 2..=5usize {
            for w in 1..n {
                for r in (1..=n - w).filter(|r| w + r <= n) {
                    for loss in [0.2, 0.3, 0.5] {
                        let s = sweep(Config::new(n, w, r, loss, false, 200), 2000, 200);
                        assert!(
                            s.stale > 0,
                            "n={n} w={w} r={r} loss={loss} saw no stale read"
                        );
                    }
                }
            }
        }
        // With no loss every message arrives, so even a non-overlapping config is never stale.
        assert_eq!(
            sweep(Config::new(3, 1, 1, 0.0, false, 200), 0, 100).stale,
            0
        );
    }

    #[test]
    fn minimal_counterexample_for_n3_w1_r1_is_a_two_event_trace() {
        let cfg = Config::new(3, 1, 1, 0.2, false, 50);
        let (seed, trace) =
            shortest_stale_trace(cfg, 42, 1000).expect("w+r<=n must have a stale read");
        assert_eq!(
            seed, 176,
            "pinned: smallest seed with a 2-event trace in 42..1042"
        );
        assert_eq!(trace.len(), 2);
        let Event::Write {
            version: 1,
            reached,
            acked: true,
        } = trace[0]
        else {
            panic!("{:?}", trace[0])
        };
        let Event::Read {
            answered,
            returned: Some(0),
            acked_before: 1,
            ..
        } = trace[1]
        else {
            panic!("{:?}", trace[1])
        };
        assert_eq!(
            reached.count_ones(),
            1,
            "w=1: the write sits on a single replica"
        );
        assert_eq!(
            answered & reached,
            0,
            "the read missed the only replica holding v1"
        );
        // The fixed design (w+r>n) has no such trace at all.
        for (w, r) in [(2, 2), (3, 1), (1, 3)] {
            let ok = Config::new(3, w, r, 0.3, false, 200);
            assert!(
                (0..500).all(|s| first_stale_trace(ok, s).is_none()),
                "w={w} r={r}"
            );
        }
    }

    #[test]
    fn success_rates_decrease_with_w_and_r_and_match_binomial_theory() {
        let loss = 0.2;
        let rate = |w: usize, r: usize| {
            let s = sweep(Config::new(3, w, r, loss, false, 100), 0, 200);
            (
                s.writes_ok as f64 / s.writes as f64,
                s.reads_ok as f64 / s.reads as f64,
            )
        };
        let mut prev = (2.0, 2.0);
        for k in 1..=3 {
            let (w_rate, r_rate) = rate(k, k);
            assert!(w_rate < prev.0 && r_rate < prev.1, "k={k}");
            prev = (w_rate, r_rate);
        }
        // P(at least k of 3 replicas reachable) with p = 0.8: k=1 -> .992, k=2 -> .896, k=3 -> .512.
        for (k, theory) in [(1, 0.992), (2, 0.896), (3, 0.512)] {
            let (w_rate, r_rate) = rate(k, k);
            assert!((w_rate - theory).abs() < 0.015, "write k={k}: {w_rate}");
            assert!((r_rate - theory).abs() < 0.015, "read k={k}: {r_rate}");
        }
        let perfect = sweep(Config::new(3, 3, 3, 0.0, false, 100), 0, 10);
        assert_eq!(
            (perfect.writes_ok, perfect.reads_ok),
            (perfect.writes, perfect.reads)
        );
    }

    #[test]
    fn replicas_only_move_forward_and_runs_are_deterministic() {
        for repair in [false, true] {
            let cfg = Config::new(5, 2, 3, 0.3, repair, 300);
            let mut sim = Sim::new(cfg, 7);
            let mut last = [0u64; MAX_N];
            for _ in 0..cfg.ops {
                if let Event::Read { seen, .. } = sim.step() {
                    for i in 0..5 {
                        assert!(seen[i] >= last[i], "replica {i} went backwards");
                        last[i] = seen[i];
                    }
                }
            }
            assert_eq!(run(cfg, 7), run(cfg, 7));
            assert_ne!(run(cfg, 7), run(cfg, 8));
        }
    }

    #[test]
    fn failed_write_leaves_traces_even_with_overlapping_quorums() {
        // n=3, w=3, r=1 overlaps (w+r>n) yet reads expose writes that were reported as failed.
        let s = sweep(Config::new(3, 3, 1, 0.3, false, 100), 0, 100);
        assert_eq!(s.stale, 0);
        assert!(
            s.ghost > 0,
            "a FAILED write must be visible to some later read"
        );
        // w=1: a write fails only if it reached nobody, so it can never be seen.
        assert_eq!(
            sweep(Config::new(3, 1, 3, 0.3, false, 100), 0, 100).ghost,
            0
        );
    }

    #[test]
    fn read_repair_strictly_lowers_stale_reads_on_the_same_seeds() {
        for (n, w, r) in [(3, 1, 2), (5, 2, 2), (5, 2, 3)] {
            let off = sweep(Config::new(n, w, r, 0.2, false, 200), 42, 200);
            let on = sweep(Config::new(n, w, r, 0.2, true, 200), 42, 200);
            assert!(
                off.stale > 0 && on.stale < off.stale,
                "n={n} w={w} r={r}: {} -> {}",
                off.stale,
                on.stale
            );
        }
    }

    #[test]
    fn scripted_trace_is_not_linearizable_without_write_back_and_is_with_it() {
        let plain = scripted_trace(false);
        assert_eq!((plain.reader1, plain.reader2), (1, 0));
        let abd = scripted_trace(true);
        assert_eq!((abd.reader1, abd.reader2), (1, 1));
    }

    #[test]
    fn write_back_removes_every_read_read_inversion_over_all_quorum_choices() {
        // n=3, w=2, r=2: unfinished writes on 0 or 1 replica; 4 written sets x 3 x 3 quorum pairs.
        // Without write-back: for each single written replica X, reader 1 includes X (2 choices) and
        // reader 2 is exactly the other pair (1 choice) -> 3 * 2 = 6 inversions.
        assert_eq!(monotonic_violations(3, 2, 2, false), 6);
        assert_eq!(monotonic_violations(3, 2, 2, true), 0);
        for n in 3..=6usize {
            let majority = n / 2 + 1;
            assert!(
                monotonic_violations(n, majority, majority, false) > 0,
                "n={n}"
            );
            assert_eq!(
                monotonic_violations(n, majority, majority, true),
                0,
                "n={n}"
            );
        }
    }

    fn sloppy_cfg(standbys: usize) -> SloppyConfig {
        SloppyConfig {
            n: 3,
            w: 2,
            r: 2,
            standbys,
            loss: 0.4,
            rounds: 100,
            deliver_hints: true,
        }
    }

    #[test]
    fn sloppy_quorum_reads_stale_with_w_plus_r_gt_n_until_hints_are_delivered() {
        let c = sloppy_cfg(2);
        assert!(c.w + c.r > c.n);
        let sloppy = sweep_sloppy(c, Mode::Sloppy, 42, 200);
        assert!(
            sloppy.stale_before > 0,
            "sloppy quorum must break read-your-writes before handoff"
        );
        assert!(sloppy.reads_after > 0);
        assert_eq!(
            sloppy.stale_after, 0,
            "after handoff >= w home replicas hold every acked write, so any r-quorum read is fresh"
        );
        assert!(sloppy.hints_delivered > 0);
        // Per seed, not just in aggregate.
        for seed in 0..200 {
            assert_eq!(run_sloppy(c, Mode::Sloppy, seed).stale_after, 0);
        }
        // Counterfactual: same seeds and failures, the standbys never hand the data back. The
        // second read is the same r-quorum read, so the only difference is the hint delivery
        // (earlier rounds' leftovers make later reads differ too, so only writes_ok is identical).
        let never = SloppyConfig {
            deliver_hints: false,
            ..c
        };
        let stuck = sweep_sloppy(never, Mode::Sloppy, 42, 200);
        assert!(stuck.stale_after > 0, "without handoff stale reads persist");
        assert_eq!(stuck.hints_delivered, 0);
        assert_eq!(stuck.writes_ok, sloppy.writes_ok);
    }

    #[test]
    fn strict_quorum_rejects_writes_instead_of_reading_stale() {
        let c = sloppy_cfg(2);
        let strict = sweep_sloppy(c, Mode::Strict, 42, 200);
        let sloppy = sweep_sloppy(c, Mode::Sloppy, 42, 200);
        assert_eq!((strict.stale_before, strict.stale_after), (0, 0));
        assert_eq!(strict.hints_delivered, 0);
        assert!(
            strict.writes_rejected > sloppy.writes_rejected,
            "sloppy buys write availability"
        );
        assert_eq!(
            strict.writes_ok + strict.writes_rejected,
            sloppy.writes_ok + sloppy.writes_rejected
        );
        // Without standbys, a sloppy quorum is a strict quorum.
        let none = sloppy_cfg(0);
        assert_eq!(
            sweep_sloppy(none, Mode::Sloppy, 42, 50),
            sweep_sloppy(none, Mode::Strict, 42, 50)
        );
        // And with a healthy network neither mode is ever stale.
        let calm = SloppyConfig { loss: 0.0, ..c };
        for mode in [Mode::Strict, Mode::Sloppy] {
            let s = sweep_sloppy(calm, mode, 0, 20);
            assert_eq!((s.stale_before, s.writes_rejected), (0, 0));
        }
    }

    #[test]
    #[should_panic(expected = "w and r must be in 1..=n")]
    fn config_rejects_quorum_larger_than_n() {
        let _ = Config::new(3, 4, 1, 0.1, false, 10);
    }
}
