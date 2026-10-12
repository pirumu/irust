//! Replication lag in a single-leader store: read-your-writes (RYW), monotonic reads (MR),
//! consistent prefix, ack levels and failover loss.
//!
//! The model is a discrete-event simulation in virtual milliseconds. One leader appends writes
//! to a log `(lsn, key)`; `F` followers receive every entry in log order after a network delay
//! and make it visible to reads after a further replay (apply) delay. Delays are drawn per entry
//! but kept **monotone per follower**, so a follower always holds a *prefix* of the leader's log.
//! The last follower is deliberately the slow one.
//!
//! What it is not: LSN is a plain counter, followers never crash, the network is FIFO per
//! follower, a read costs no time, and no number here is a measurement of a real database.
//! All arithmetic is integer; the only randomness comes from [`Rng`].

use crate::rng::Rng;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// How a client's read is routed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Always read the leader.
    Leader,
    /// Naive: a uniformly random follower per read.
    RandomFollower,
    /// The session is pinned to one follower (chosen at random once).
    StickyFollower,
    /// Read the leader for `w` ms after the session's own last write was acked, else a random follower.
    LeaderForWindow(u64),
    /// The session remembers `max(last write LSN, last version read)` and reads a random follower
    /// that has applied at least that LSN; otherwise it falls back to the leader.
    Token,
}

impl Policy {
    /// Short label for tables.
    pub fn label(self) -> String {
        match self {
            Policy::Leader => "Leader".into(),
            Policy::RandomFollower => "RandomFollower".into(),
            Policy::StickyFollower => "StickyFollower".into(),
            Policy::LeaderForWindow(w) => format!("LeaderForWindow({w})"),
            Policy::Token => "Token".into(),
        }
    }
}

/// When the leader acknowledges a write to the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ack {
    /// Immediately (asynchronous replication).
    Async,
    /// When `k` followers have received (flushed) the entry and their reply came back.
    Received(usize),
    /// When `k` followers have applied (replayed) the entry and their reply came back
    /// (PostgreSQL `remote_apply`-like).
    Applied(usize),
}

impl Ack {
    /// Short label for tables; `k == followers` prints as `all`.
    pub fn label(self, followers: usize) -> String {
        let k = |k: usize| {
            if k == followers {
                "all".to_string()
            } else {
                k.to_string()
            }
        };
        match self {
            Ack::Async => "Async".into(),
            Ack::Received(n) => format!("Received({})", k(n)),
            Ack::Applied(n) => format!("Applied({})", k(n)),
        }
    }
}

/// Parameters of one simulated run. [`Config::new`] gives the lab defaults.
#[derive(Clone, Debug)]
pub struct Config {
    /// Number of followers `F` (>= 1). The last one is the slow replica.
    pub followers: usize,
    /// Nominal total lag in ms of the slow follower: receive `lag/4`, then replay `lag - lag/4`;
    /// every other follower is four times quicker in both stages.
    pub lag: u64,
    /// Each stage gets uniform jitter in `[0, stage / jitter_div]` (default 4); 0 switches jitter off.
    pub jitter_div: u64,
    /// Time between the ack of a write and the first read of the same session.
    pub think: u64,
    /// Spacing between the reads of one round.
    pub read_gap: u64,
    /// Reads per round (>= 2 is needed to expose monotonic-reads violations).
    pub reads_per_round: u32,
    /// Number of sessions (users).
    pub sessions: usize,
    /// Write-then-read rounds per session.
    pub rounds: u32,
    /// Session start times are uniform in `[0, horizon)`.
    pub horizon: u64,
    /// Pause between the last read of a round and the next write: uniform in `[rest_lo, rest_hi)`.
    pub rest_lo: u64,
    /// See `rest_lo`.
    pub rest_hi: u64,
    /// Size of the key pool; session `s` writes key `s % keys`.
    pub keys: usize,
    /// Ack level of every write.
    pub ack: Ack,
    /// Read routing policy.
    pub policy: Policy,
    /// Number of random leader-crash instants evaluated per run (0 skips the failover analysis).
    pub crash_probes: u32,
}

impl Config {
    /// Lab defaults: 2 followers, 400 sessions x 3 rounds, think 20 ms, 3 reads 5 ms apart.
    pub fn new(lag: u64, ack: Ack, policy: Policy) -> Self {
        Config {
            followers: 2,
            lag,
            jitter_div: 4,
            think: 20,
            read_gap: 5,
            reads_per_round: 3,
            sessions: 400,
            rounds: 3,
            horizon: 1000,
            rest_lo: 10,
            rest_hi: 40,
            keys: 100,
            ack,
            policy,
            crash_probes: 0,
        }
    }

    fn delays(&self) -> Vec<Delay> {
        // Slow follower: `lag` split into a short network stage (lag/4) and a longer replay stage
        // (the rest); fast followers are four times quicker in both stages.
        let u = self.lag / 4;
        (0..self.followers)
            .map(|f| {
                let (recv, apply) = if f + 1 == self.followers {
                    (u, self.lag - u)
                } else {
                    (u / 4, (self.lag - u) / 4)
                };
                let jit = |stage: u64| stage.checked_div(self.jitter_div).unwrap_or(0);
                Delay {
                    recv,
                    apply,
                    jit_recv: jit(recv),
                    jit_apply: jit(apply),
                }
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug)]
struct Delay {
    recv: u64,
    apply: u64,
    jit_recv: u64,
    jit_apply: u64,
}

/// The leader's log plus, for each follower, the time every entry was received and applied.
/// Entry `lsn` lives at index `lsn - 1`.
#[derive(Clone, Debug)]
pub struct Cluster {
    /// Time each entry was appended on the leader.
    pub log_time: Vec<u64>,
    /// Receive time of each entry on each follower.
    pub recv: Vec<Vec<u64>>,
    /// Apply (visible-to-reads) time of each entry on each follower.
    pub apply: Vec<Vec<u64>>,
    key_lsns: Vec<Vec<u64>>,
    delays: Vec<Delay>,
}

impl Cluster {
    fn new(cfg: &Config) -> Self {
        Cluster {
            log_time: Vec::new(),
            recv: vec![Vec::new(); cfg.followers],
            apply: vec![Vec::new(); cfg.followers],
            key_lsns: vec![Vec::new(); cfg.keys],
            delays: cfg.delays(),
        }
    }

    /// Appends a write to `key` at leader time `t` (non-decreasing across calls); returns its LSN.
    fn append(&mut self, key: usize, t: u64, net: &mut Rng) -> u64 {
        self.log_time.push(t);
        let lsn = self.log_time.len() as u64;
        self.key_lsns[key].push(lsn);
        for (f, d) in self.delays.iter().enumerate() {
            // Monotone per follower: an entry is never received/applied before its predecessor.
            let r = (t + d.recv + net.below(d.jit_recv + 1))
                .max(self.recv[f].last().copied().unwrap_or(0));
            let a = (r + d.apply + net.below(d.jit_apply + 1))
                .max(self.apply[f].last().copied().unwrap_or(0));
            self.recv[f].push(r);
            self.apply[f].push(a);
        }
        lsn
    }

    /// Time at which the leader acks entry `lsn` (appended at `t`) under `ack`.
    fn ack_time(&self, ack: Ack, lsn: u64, t: u64) -> u64 {
        let i = lsn as usize - 1;
        let (k, times): (usize, Vec<u64>) = match ack {
            Ack::Async => return t,
            // The follower's reply travels back with the same base delay as the way out.
            Ack::Received(k) => (
                k,
                (0..self.recv.len())
                    .map(|f| self.recv[f][i] + self.delays[f].recv)
                    .collect(),
            ),
            Ack::Applied(k) => (
                k,
                (0..self.apply.len())
                    .map(|f| self.apply[f][i] + self.delays[f].recv)
                    .collect(),
            ),
        };
        let mut times = times;
        times.sort_unstable();
        times[k.clamp(1, times.len()) - 1]
    }

    /// Highest LSN follower `f` has applied (readable) at time `t`.
    pub fn applied_lsn(&self, f: usize, t: u64) -> u64 {
        self.apply[f].partition_point(|&a| a <= t) as u64
    }

    /// Highest LSN follower `f` has received at time `t`.
    pub fn received_lsn(&self, f: usize, t: u64) -> u64 {
        self.recv[f].partition_point(|&r| r <= t) as u64
    }

    /// Version (LSN of the latest write) of `key` on the leader; 0 if never written.
    fn leader_version(&self, key: usize) -> u64 {
        self.key_lsns[key].last().copied().unwrap_or(0)
    }

    /// Version of `key` visible on follower `f` at time `t`.
    fn follower_version(&self, f: usize, key: usize, t: u64) -> u64 {
        let applied = self.applied_lsn(f, t);
        let lsns = &self.key_lsns[key];
        match lsns.partition_point(|&l| l <= applied) {
            0 => 0,
            n => lsns[n - 1],
        }
    }

    /// Brute-force prefix check: per follower, receive and apply times never decrease along the
    /// log and an entry is never applied before it is received, so the set of entries visible at
    /// any instant is `1..=applied_lsn`, never a set with holes.
    pub fn check_prefix(&self) -> bool {
        (0..self.recv.len()).all(|f| {
            self.recv[f].windows(2).all(|w| w[0] <= w[1])
                && self.apply[f].windows(2).all(|w| w[0] <= w[1])
                && self.recv[f].iter().zip(&self.apply[f]).all(|(r, a)| r <= a)
        })
    }
}

/// Counters of one run (or the sum of several: [`Report::add`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Writes issued.
    pub writes: u64,
    /// Reads issued.
    pub reads: u64,
    /// Reads whose version was below the session's own last write.
    pub ryw: u64,
    /// Reads whose version was below a version the session had already read.
    pub mr: u64,
    /// Reads served by the leader (for `Token`: the fallbacks).
    pub leader_reads: u64,
    /// Sum of write latencies (ack - issue), ms.
    pub lat_sum: u64,
    /// Max write latency, ms.
    pub lat_max: u64,
    /// Min write latency, ms (`u64::MAX` before any write).
    pub lat_min: u64,
    /// Leader-crash instants evaluated.
    pub crashes: u64,
    /// Crash instants after which at least one acked write was missing on the promoted follower.
    pub crashes_with_loss: u64,
    /// Sum over crash instants of acked writes missing on the promoted follower.
    pub lost_acked: u64,
}

impl Report {
    /// Adds another report (counters sum, extrema combine).
    pub fn add(&mut self, o: &Report) {
        self.writes += o.writes;
        self.reads += o.reads;
        self.ryw += o.ryw;
        self.mr += o.mr;
        self.leader_reads += o.leader_reads;
        self.lat_sum += o.lat_sum;
        self.lat_max = self.lat_max.max(o.lat_max);
        self.lat_min = self.lat_min.min(o.lat_min);
        self.crashes += o.crashes;
        self.crashes_with_loss += o.crashes_with_loss;
        self.lost_acked += o.lost_acked;
    }

    fn empty() -> Self {
        Report {
            lat_min: u64::MAX,
            ..Report::default()
        }
    }
}

/// `n / d` as a percentage with one decimal ("12.3"), rounded half up, using integers only.
pub fn pct1(n: u64, d: u64) -> String {
    ratio1(n * 100, d)
}

/// `n / d` with one decimal, rounded half up, using integers only; "-" if `d == 0`.
pub fn ratio1(n: u64, d: u64) -> String {
    if d == 0 {
        return "-".into();
    }
    let v = (n * 20 + d) / (2 * d);
    format!("{}.{}", v / 10, v % 10)
}

struct Session {
    key: usize,
    /// Schedule randomness (start time, pauses): never touched by the read policy.
    rng: Rng,
    /// Routing randomness (which follower): the only stream the read policy consumes.
    route: Rng,
    pinned: usize,
    round: u32,
    last_ack: u64,
    last_write: u64,
    max_seen: u64,
    token: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ev {
    Write,
    Read(u32),
}

/// Runs one simulation. Pure function of `(cfg, seed)`: workload and network randomness use
/// forks that do not depend on the read policy, so two policies see the *same* trace
/// (the schedule only depends on `cfg.ack`).
pub fn run(cfg: &Config, seed: u64) -> Report {
    assert!(cfg.followers >= 1 && cfg.keys >= 1 && cfg.reads_per_round >= 1);
    let mut master = Rng::new(seed);
    let mut net = master.fork(1);
    let mut crash_rng = master.fork(2);
    let mut sessions: Vec<Session> = (0..cfg.sessions)
        .map(|s| {
            let mut rng = master.fork(100 + s as u64);
            let mut route = rng.fork(1);
            let pinned = route.below(cfg.followers as u64) as usize;
            Session {
                key: s % cfg.keys,
                rng,
                route,
                pinned,
                round: 0,
                last_ack: 0,
                last_write: 0,
                max_seen: 0,
                token: 0,
            }
        })
        .collect();
    let mut cl = Cluster::new(cfg);
    let mut heap: BinaryHeap<Reverse<(u64, u64, usize, Ev)>> = BinaryHeap::new();
    let mut seq = 0u64;
    let mut push =
        |heap: &mut BinaryHeap<Reverse<(u64, u64, usize, Ev)>>, t: u64, s: usize, ev: Ev| {
            seq += 1;
            heap.push(Reverse((t, seq, s, ev)));
        };
    for (s, sess) in sessions.iter_mut().enumerate() {
        let t0 = sess.rng.below(cfg.horizon);
        push(&mut heap, t0, s, Ev::Write);
    }
    let mut rep = Report::empty();
    let mut ack_by_lsn: Vec<u64> = Vec::new();
    while let Some(Reverse((t, _, s, ev))) = heap.pop() {
        let sess = &mut sessions[s];
        match ev {
            Ev::Write => {
                let lsn = cl.append(sess.key, t, &mut net);
                let ack = cl.ack_time(cfg.ack, lsn, t);
                ack_by_lsn.push(ack);
                sess.last_write = lsn;
                sess.last_ack = ack;
                sess.token = sess.token.max(lsn);
                let lat = ack - t;
                rep.writes += 1;
                rep.lat_sum += lat;
                rep.lat_max = rep.lat_max.max(lat);
                rep.lat_min = rep.lat_min.min(lat);
                push(&mut heap, ack + cfg.think, s, Ev::Read(0));
            }
            Ev::Read(j) => {
                let source = match cfg.policy {
                    Policy::Leader => None,
                    Policy::RandomFollower => Some(sess.route.below(cfg.followers as u64) as usize),
                    Policy::StickyFollower => Some(sess.pinned),
                    Policy::LeaderForWindow(w) => (t >= sess.last_ack + w)
                        .then(|| sess.route.below(cfg.followers as u64) as usize),
                    Policy::Token => {
                        let ok: Vec<usize> = (0..cfg.followers)
                            .filter(|&f| cl.applied_lsn(f, t) >= sess.token)
                            .collect();
                        sess.route.choose(&ok).copied()
                    }
                };
                let version = match source {
                    None => {
                        rep.leader_reads += 1;
                        cl.leader_version(sess.key)
                    }
                    Some(f) => cl.follower_version(f, sess.key, t),
                };
                rep.reads += 1;
                rep.ryw += u64::from(version < sess.last_write);
                rep.mr += u64::from(version < sess.max_seen);
                sess.max_seen = sess.max_seen.max(version);
                sess.token = sess.token.max(version);
                if j + 1 < cfg.reads_per_round {
                    push(&mut heap, t + cfg.read_gap, s, Ev::Read(j + 1));
                } else {
                    sess.round += 1;
                    if sess.round < cfg.rounds {
                        let rest = sess.rng.range(cfg.rest_lo, cfg.rest_hi);
                        push(&mut heap, t + rest, s, Ev::Write);
                    }
                }
            }
        }
    }
    assert!(
        cl.check_prefix(),
        "followers must hold a prefix of the leader log"
    );
    if cfg.crash_probes > 0 {
        let t_end = cl.log_time.last().copied().unwrap_or(0);
        for _ in 0..cfg.crash_probes {
            let t = crash_rng.range(0, t_end + 1);
            // Promote the follower with the highest *received* LSN; entries still in flight are lost.
            let promoted = (0..cfg.followers)
                .map(|f| cl.received_lsn(f, t))
                .max()
                .unwrap_or(0) as usize;
            let lost = ack_by_lsn[promoted..].iter().filter(|&&a| a <= t).count() as u64;
            rep.crashes += 1;
            rep.crashes_with_loss += u64::from(lost > 0);
            rep.lost_acked += lost;
        }
    }
    rep
}

/// Sum of [`run`] over seeds `seed .. seed + runs`.
pub fn run_many(cfg: &Config, seed: u64, runs: u64) -> Report {
    let mut total = Report::empty();
    for s in seed..seed + runs {
        total.add(&run(cfg, s));
    }
    total
}

/// Result of [`consistent_prefix_demo`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefixReport {
    /// Number of random read instants.
    pub probes: u64,
    /// Probes at which a reader of one replica per shard sees a payment whose order is missing
    /// (two independent logs).
    pub independent: u64,
    /// The same count when orders and payments share ONE log and one replica.
    pub single_log: u64,
}

/// Apply times of a log whose entries were committed at `commit` (non-decreasing): each entry
/// takes `lag/2 + U[0, lag]` ms, kept monotone so the replica holds a prefix.
fn replica_times(commit: &[u64], lag: u64, rng: &mut Rng) -> Vec<u64> {
    let mut prev = 0;
    commit
        .iter()
        .map(|&c| {
            prev = (c + lag / 2 + rng.below(lag + 1)).max(prev);
            prev
        })
        .collect()
}

/// "Payment visible but its order not" demo. `pairs` (order, payment) pairs are committed with
/// the payment 2 ms after its order. With two shards (own logs, own lag) a reader that reads one
/// replica per shard can see a state that never existed on any leader; with one log it cannot.
pub fn consistent_prefix_demo(seed: u64, pairs: usize, lag: u64) -> PrefixReport {
    let mut rng = Rng::new(seed);
    let mut commit: Vec<u64> = (0..pairs).map(|_| rng.below(1000)).collect();
    commit.sort_unstable();
    let pay_commit: Vec<u64> = commit.iter().map(|c| c + 2).collect();
    // Two independent shards, each with its own replica lag draw.
    let orders = replica_times(&commit, lag, &mut rng.fork(1));
    let payments = replica_times(&pay_commit, lag, &mut rng.fork(2));
    // One log: order i then payment i, merged by commit time (an order always precedes its payment).
    let mut merged: Vec<(u64, u8, usize)> = (0..pairs)
        .flat_map(|i| [(commit[i], 0, i), (pay_commit[i], 1, i)])
        .collect();
    merged.sort_unstable();
    let times: Vec<u64> = merged.iter().map(|m| m.0).collect();
    let applied = replica_times(&times, lag, &mut rng.fork(3));
    let (mut ord1, mut pay1) = (vec![0; pairs], vec![0; pairs]);
    for (&(_, kind, i), &a) in merged.iter().zip(&applied) {
        if kind == 0 { ord1[i] = a } else { pay1[i] = a }
    }
    let mut rep = PrefixReport {
        probes: 500,
        independent: 0,
        single_log: 0,
    };
    for _ in 0..rep.probes {
        let t = rng.below(1000 + 2 * lag + 3);
        rep.independent += u64::from((0..pairs).any(|i| payments[i] <= t && orders[i] > t));
        rep.single_log += u64::from((0..pairs).any(|i| pay1[i] <= t && ord1[i] > t));
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAGS: [u64; 3] = [0, 20, 80];

    /// A smaller workload so a 50-seed sweep stays fast.
    fn small(lag: u64, ack: Ack, policy: Policy) -> Config {
        let mut c = Config::new(lag, ack, policy);
        c.sessions = 80;
        c.keys = 20;
        c
    }

    fn sweep(lag: u64, ack: Ack, policy: Policy, seeds: u64) -> Report {
        run_many(&small(lag, ack, policy), 1, seeds)
    }

    /// Replays a run's cluster for brute-force probing (same construction as `run`, writes only).
    fn build_cluster(cfg: &Config, seed: u64) -> Cluster {
        let mut rng = Rng::new(seed);
        let mut cl = Cluster::new(cfg);
        let mut t = 0;
        for i in 0..300 {
            t += rng.below(4);
            cl.append(i % cfg.keys, t, &mut rng);
        }
        cl
    }

    #[test]
    fn every_follower_holds_a_prefix_at_every_probe() {
        for seed in 0..30 {
            for lag in LAGS {
                for followers in [1, 2, 3] {
                    let mut cfg = Config::new(lag, Ack::Async, Policy::Leader);
                    cfg.followers = followers;
                    let cl = build_cluster(&cfg, seed);
                    assert!(cl.check_prefix());
                    let n = cl.log_time.len() as u64;
                    for t in (0..cl.log_time[n as usize - 1] + 4 * lag + 10).step_by(7) {
                        for f in 0..followers {
                            // Brute force: the set of visible LSNs must be exactly 1..=applied.
                            let visible: Vec<u64> = (1..=n)
                                .filter(|&l| cl.apply[f][l as usize - 1] <= t)
                                .collect();
                            let applied = cl.applied_lsn(f, t);
                            assert_eq!(
                                visible,
                                (1..=applied).collect::<Vec<_>>(),
                                "hole in follower {f} at t={t}"
                            );
                            assert!(cl.received_lsn(f, t) >= applied, "applied beyond received");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn leader_and_token_never_violate_ryw_or_mr() {
        for lag in LAGS {
            for policy in [Policy::Leader, Policy::Token] {
                let r = sweep(lag, Ack::Async, policy, 50);
                assert!(r.reads > 10_000);
                assert_eq!((r.ryw, r.mr), (0, 0), "{policy:?} lag={lag}");
            }
        }
    }

    #[test]
    fn token_uses_followers_when_they_are_fresh_and_falls_back_when_they_are_not() {
        let fresh = sweep(0, Ack::Async, Policy::Token, 20);
        assert_eq!(fresh.leader_reads, 0, "lag 0: every follower is caught up");
        let lagging = sweep(80, Ack::Async, Policy::Token, 20);
        assert!(lagging.leader_reads > 0 && lagging.leader_reads < lagging.reads);
    }

    #[test]
    fn random_follower_breaks_ryw_and_mr_when_lag_exceeds_think_and_not_at_zero_lag() {
        let lagged = sweep(80, Ack::Async, Policy::RandomFollower, 50);
        assert!(lagged.ryw > 0 && lagged.mr > 0);
        // With lag much larger than think, a large share of reads are stale.
        assert!(
            lagged.ryw * 10 > lagged.reads,
            "ryw {} of {}",
            lagged.ryw,
            lagged.reads
        );
        let none = sweep(0, Ack::Async, Policy::RandomFollower, 50);
        assert_eq!((none.ryw, none.mr), (0, 0));
    }

    #[test]
    fn sticky_follower_gives_monotonic_reads_but_not_read_your_writes() {
        for lag in LAGS {
            assert_eq!(
                sweep(lag, Ack::Async, Policy::StickyFollower, 50).mr,
                0,
                "lag={lag}"
            );
        }
        assert!(sweep(80, Ack::Async, Policy::StickyFollower, 50).ryw > 0);
        assert_eq!(sweep(0, Ack::Async, Policy::StickyFollower, 50).ryw, 0);
    }

    #[test]
    fn leader_window_must_outlast_the_lag_to_protect_ryw() {
        // Slowest follower: recv + apply = 80 ms (+ jitter) at lag 80; reads start `think` after the ack.
        let wide = sweep(80, Ack::Async, Policy::LeaderForWindow(200), 30);
        assert_eq!(wide.ryw, 0);
        assert!(wide.leader_reads > 0);
        let narrow = sweep(80, Ack::Async, Policy::LeaderForWindow(25), 30);
        assert!(
            narrow.ryw > 0,
            "a window shorter than the lag lets stale follower reads through"
        );
    }

    #[test]
    fn async_ack_is_instant_and_sync_acks_cost_round_trips_in_order() {
        // Without jitter the latencies are exact. lag 80: slow = recv 20 + apply 60, fast = recv 5 + apply 15.
        let lat = |ack| {
            let mut c = small(80, ack, Policy::Leader);
            c.jitter_div = 0;
            let r = run(&c, 7);
            assert_eq!(
                r.lat_min, r.lat_max,
                "no jitter: every write has the same latency"
            );
            r.lat_max
        };
        // Replies travel back with the receive delay: latency = (recv [+ apply]) + recv.
        assert_eq!(lat(Ack::Async), 0);
        assert_eq!(lat(Ack::Received(1)), 5 + 5);
        assert_eq!(lat(Ack::Received(2)), 20 + 20);
        assert_eq!(lat(Ack::Applied(1)), 5 + 15 + 5);
        assert_eq!(lat(Ack::Applied(2)), 20 + 60 + 20);
        let lag = 80;
        // With jitter on, mean latency still increases along the chain.
        let mean = |ack| {
            let r = sweep(lag, ack, Policy::Leader, 10);
            r.lat_sum * 1000 / r.writes
        };
        assert!(mean(Ack::Async) < mean(Ack::Received(1)));
        assert!(mean(Ack::Received(1)) < mean(Ack::Received(2)));
        assert!(mean(Ack::Received(2)) < mean(Ack::Applied(2)));
    }

    #[test]
    fn failover_loses_acked_writes_only_with_async() {
        for lag in LAGS {
            for ack in [
                Ack::Received(1),
                Ack::Received(2),
                Ack::Applied(1),
                Ack::Applied(2),
            ] {
                let mut c = small(lag, ack, Policy::Leader);
                c.crash_probes = 100;
                let r = run_many(&c, 1, 50);
                assert_eq!(r.lost_acked, 0, "{ack:?} lag={lag}");
                assert_eq!(r.crashes, 50 * 100);
            }
        }
        let loss = |lag| {
            let mut c = small(lag, Ack::Async, Policy::Leader);
            c.crash_probes = 100;
            run_many(&c, 1, 50)
        };
        let (l0, l20, l80) = (loss(0), loss(20), loss(80));
        assert_eq!(l0.lost_acked, 0, "no lag, nothing in flight");
        assert!(l20.lost_acked > 0 && l20.crashes_with_loss > 0);
        // PostgreSQL docs: loss is proportional to the replication delay; the model agrees in direction.
        assert!(l80.lost_acked > l20.lost_acked);
    }

    #[test]
    fn applied_all_gives_ryw_on_random_follower_reads_but_weaker_acks_do_not() {
        for lag in LAGS {
            let r = sweep(lag, Ack::Applied(2), Policy::RandomFollower, 50);
            assert_eq!(r.ryw, 0, "Applied(all) lag={lag}");
        }
        // Counterexamples: waiting for one follower, or for receipt only, is not enough.
        assert!(sweep(80, Ack::Applied(1), Policy::RandomFollower, 50).ryw > 0);
        assert!(sweep(80, Ack::Received(2), Policy::RandomFollower, 50).ryw > 0);
    }

    #[test]
    fn independent_logs_show_payment_without_order_and_one_log_never_does() {
        let mut total_indep = 0;
        for seed in 0..50 {
            let r = consistent_prefix_demo(seed, 40, 30);
            assert_eq!(r.single_log, 0, "seed {seed}");
            total_indep += r.independent;
            assert_eq!(
                consistent_prefix_demo(seed, 40, 0).independent,
                0,
                "lag 0 seed {seed}"
            );
        }
        assert!(total_indep > 0);
    }

    #[test]
    fn same_seed_same_report_different_seed_differs() {
        let mut c = small(80, Ack::Applied(1), Policy::Token);
        c.crash_probes = 50;
        assert_eq!(run(&c, 5), run(&c, 5));
        assert_ne!(run(&c, 5), run(&c, 6));
        assert_eq!(
            consistent_prefix_demo(3, 40, 30),
            consistent_prefix_demo(3, 40, 30)
        );
    }

    #[test]
    fn policies_see_the_same_trace_under_the_same_ack() {
        // Reads never change the write schedule: writes and reads counts and write latency agree.
        let a = run(&small(80, Ack::Applied(1), Policy::Leader), 9);
        let b = run(&small(80, Ack::Applied(1), Policy::RandomFollower), 9);
        assert_eq!(
            (a.writes, a.reads, a.lat_sum, a.lat_max),
            (b.writes, b.reads, b.lat_sum, b.lat_max)
        );
    }

    #[test]
    fn percent_helpers_round_half_up() {
        assert_eq!(pct1(1, 3), "33.3");
        assert_eq!(pct1(2, 3), "66.7");
        assert_eq!(pct1(0, 5), "0.0");
        assert_eq!(pct1(5, 5), "100.0");
        assert_eq!(ratio1(7, 2), "3.5");
        assert_eq!(ratio1(1, 0), "-");
    }
}
