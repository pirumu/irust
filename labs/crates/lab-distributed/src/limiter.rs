//! Redis-style rate limiters on a tiny in-memory model of Redis (virtual millisecond clock).
//!
//! This is a MODEL of the patterns, not Redis: [`MiniRedis`] has integers with TTL, sorted sets,
//! hashes and `TIME`; every single command is atomic; [`MiniRedis::exec`] runs a queued list of
//! commands with nothing interleaved (`MULTI`/`EXEC`, no branching on results); a script is any
//! closure that borrows the store for one scheduler step. Races between *non-atomic* multi-command
//! routines are produced by a seeded scheduler that picks which client runs its next command.
//!
//! Not modelled: network, replication/failover, persistence, eviction, memory sizes in bytes,
//! cluster slots, command latency (everything takes zero virtual time).

use crate::rng::Rng;
use std::collections::{BTreeMap, BTreeSet};

/// Value stored under a key.
#[derive(Clone, Debug)]
enum Value {
    Int(i64),
    ZSet(ZSet),
    Hash(BTreeMap<String, i64>),
}

/// Sorted set: unique members, each with a score; ordered by (score, member).
#[derive(Clone, Debug, Default)]
struct ZSet {
    score_of: BTreeMap<String, u64>,
    ordered: BTreeSet<(u64, String)>,
}

#[derive(Clone, Debug)]
struct Entry {
    value: Value,
    /// Absolute expiry in virtual ms; the key is gone once `now >= expire_at`.
    expire_at: Option<u64>,
}

/// `EXPIRE` condition: unconditional, or `NX` (only when the key has no expiry).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpireMode {
    /// Plain `EXPIRE key seconds`: always (re)sets the TTL.
    Always,
    /// `EXPIRE key seconds NX`: only sets the TTL when the key has none.
    Nx,
}

/// A command that can be queued in a `MULTI`/`EXEC` block (no command here can branch on a result).
#[derive(Clone, Debug)]
pub enum Cmd {
    /// `INCR key`
    Incr(String),
    /// `EXPIRE key seconds [NX]`
    Expire(String, u64, ExpireMode),
}

/// Reply of one command in an `EXEC` batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Integer reply.
    Int(i64),
    /// 1/0 reply of `EXPIRE`.
    Bool(bool),
}

/// The in-memory store. Time is a virtual millisecond clock set by the driver.
#[derive(Clone, Debug, Default)]
pub struct MiniRedis {
    now_ms: u64,
    keys: BTreeMap<String, Entry>,
}

impl MiniRedis {
    /// Empty store at virtual time 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the server clock. May move backwards on purpose (clock-step experiments).
    pub fn set_now(&mut self, ms: u64) {
        self.now_ms = ms;
    }

    /// Server clock in ms.
    pub fn now(&self) -> u64 {
        self.now_ms
    }

    /// `TIME` as milliseconds (scripts use the server clock, not the pod's).
    pub fn time_ms(&self) -> u64 {
        self.now_ms
    }

    fn purge(&mut self, key: &str) {
        if self
            .keys
            .get(key)
            .is_some_and(|e| e.expire_at.is_some_and(|t| t <= self.now_ms))
        {
            self.keys.remove(key);
        }
    }

    /// `INCR key`: creates the key at 0 without a TTL; leaves an existing TTL untouched.
    pub fn incr(&mut self, key: &str) -> i64 {
        self.purge(key);
        let e = self.keys.entry(key.to_string()).or_insert(Entry {
            value: Value::Int(0),
            expire_at: None,
        });
        match &mut e.value {
            Value::Int(n) => {
                *n += 1;
                *n
            }
            _ => panic!("WRONGTYPE"),
        }
    }

    /// `EXPIRE key seconds [NX]`: returns whether a TTL was set.
    pub fn expire(&mut self, key: &str, secs: u64, mode: ExpireMode) -> bool {
        self.expire_ms(key, secs * 1000, mode)
    }

    /// `PEXPIRE key ms [NX]`.
    pub fn expire_ms(&mut self, key: &str, ms: u64, mode: ExpireMode) -> bool {
        self.purge(key);
        let now = self.now_ms;
        match self.keys.get_mut(key) {
            Some(e) if mode == ExpireMode::Nx && e.expire_at.is_some() => false,
            Some(e) => {
                e.expire_at = Some(now + ms);
                true
            }
            None => false,
        }
    }

    /// Remaining TTL in ms, `None` if the key is missing or has no TTL.
    pub fn pttl(&mut self, key: &str) -> Option<u64> {
        self.purge(key);
        self.keys.get(key)?.expire_at.map(|t| t - self.now_ms)
    }

    /// Does the (unexpired) key exist?
    pub fn exists(&mut self, key: &str) -> bool {
        self.purge(key);
        self.keys.contains_key(key)
    }

    /// Runs a `MULTI`/`EXEC` batch: all commands, in order, with no other client in between.
    pub fn exec(&mut self, cmds: &[Cmd]) -> Vec<Reply> {
        cmds.iter()
            .map(|c| match c {
                Cmd::Incr(k) => Reply::Int(self.incr(k)),
                Cmd::Expire(k, s, m) => Reply::Bool(self.expire(k, *s, *m)),
            })
            .collect()
    }

    /// `ZREMRANGEBYSCORE key -inf max` (inclusive); returns the number removed.
    pub fn zremrangebyscore_upto(&mut self, key: &str, max: u64) -> usize {
        self.purge(key);
        let Some(Entry {
            value: Value::ZSet(z),
            ..
        }) = self.keys.get_mut(key)
        else {
            return 0;
        };
        let doomed: Vec<(u64, String)> = z
            .ordered
            .range(..=(max, "\u{10FFFF}".to_string()))
            .cloned()
            .collect();
        for (s, m) in &doomed {
            z.ordered.remove(&(*s, m.clone()));
            z.score_of.remove(m);
        }
        let n = doomed.len();
        if z.score_of.is_empty() {
            self.keys.remove(key);
        }
        n
    }

    /// `ZCARD key`.
    pub fn zcard(&mut self, key: &str) -> usize {
        self.purge(key);
        match self.keys.get(key) {
            Some(Entry {
                value: Value::ZSet(z),
                ..
            }) => z.score_of.len(),
            _ => 0,
        }
    }

    /// `ZADD key score member`: returns `true` if the member is new. An existing member only has
    /// its score updated (so two requests sharing a member count once).
    pub fn zadd(&mut self, key: &str, score: u64, member: &str) -> bool {
        self.purge(key);
        let e = self.keys.entry(key.to_string()).or_insert(Entry {
            value: Value::ZSet(ZSet::default()),
            expire_at: None,
        });
        let Value::ZSet(z) = &mut e.value else {
            panic!("WRONGTYPE")
        };
        let old = z.score_of.insert(member.to_string(), score);
        if let Some(o) = old {
            z.ordered.remove(&(o, member.to_string()));
        }
        z.ordered.insert((score, member.to_string()));
        old.is_none()
    }

    /// `HGET key field`.
    pub fn hget(&mut self, key: &str, field: &str) -> Option<i64> {
        self.purge(key);
        match self.keys.get(key) {
            Some(Entry {
                value: Value::Hash(h),
                ..
            }) => h.get(field).copied(),
            _ => None,
        }
    }

    /// `HSET key field value`. Leaves any TTL untouched.
    pub fn hset(&mut self, key: &str, field: &str, v: i64) {
        self.purge(key);
        let e = self.keys.entry(key.to_string()).or_insert(Entry {
            value: Value::Hash(BTreeMap::new()),
            expire_at: None,
        });
        let Value::Hash(h) = &mut e.value else {
            panic!("WRONGTYPE")
        };
        h.insert(field.to_string(), v);
    }

    /// Number of stored elements under `key`: 1 for an integer, the member count of a zset, the
    /// field count of a hash, 0 if missing. (Elements, not bytes.)
    pub fn elements(&mut self, key: &str) -> usize {
        self.purge(key);
        match self.keys.get(key) {
            None => 0,
            Some(Entry {
                value: Value::Int(_),
                ..
            }) => 1,
            Some(Entry {
                value: Value::ZSet(z),
                ..
            }) => z.score_of.len(),
            Some(Entry {
                value: Value::Hash(h),
                ..
            }) => h.len(),
        }
    }

    /// Integer value of a counter key, if it exists.
    pub fn get_int(&mut self, key: &str) -> Option<i64> {
        self.purge(key);
        match self.keys.get(key) {
            Some(Entry {
                value: Value::Int(n),
                ..
            }) => Some(*n),
            _ => None,
        }
    }

    /// Live keys that have no TTL at all (the "leaked" keys).
    pub fn keys_without_ttl(&self) -> Vec<&str> {
        self.keys
            .iter()
            .filter(|(_, e)| e.expire_at.is_none())
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

/// Limit parameters shared by all algorithms: at most `limit` requests per `window_ms`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Params {
    /// Requests allowed per window (also the sustained token-bucket rate per window).
    pub limit: u64,
    /// Window length in ms; must be a multiple of 1000 (the fixed window uses `EXPIRE` in seconds).
    pub window_ms: u64,
}

/// A rate-limiting algorithm (atomic versions only; non-atomic ones are separate functions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    /// `INCR rl:{k}:{floor(app_now / W)}` + `EXPIRE ... W NX` in `MULTI`/`EXEC`: window id from the
    /// pod's clock, windows aligned to multiples of `W`.
    FixedAligned,
    /// `INCR rl:{k}` + `EXPIRE rl:{k} W NX` in `MULTI`/`EXEC`: the window starts at the first request.
    FixedTtl,
    /// ZSET log in one script: remove old, count, add.
    SlidingLog,
    /// Hash `tokens`/`ts` with lazy refill in one script; `cap` is the bucket size, the refill rate
    /// is `limit` tokens per `window_ms`.
    TokenBucket {
        /// Bucket capacity in tokens (maximum burst).
        cap: u64,
    },
}

impl Algo {
    /// Short label for tables.
    pub fn name(&self) -> String {
        match self {
            Algo::FixedAligned => "fixed (aligned id)".into(),
            Algo::FixedTtl => "fixed (TTL, NX)".into(),
            Algo::SlidingLog => "sliding log".into(),
            Algo::TokenBucket { cap } => format!("token bucket cap={cap}"),
        }
    }

    /// Decides one request. `app_now` is the pod's own clock (used only by `FixedAligned`);
    /// scripts read the server clock via `TIME`. `req_id` makes ZSET members unique.
    pub fn allow(
        &self,
        p: &Params,
        r: &mut MiniRedis,
        key: &str,
        app_now: u64,
        req_id: u64,
    ) -> bool {
        assert!(p.window_ms.is_multiple_of(1000) && p.window_ms > 0 && p.limit > 0);
        match self {
            Algo::FixedAligned => {
                let k = format!("rl:{{{key}}}:{}", app_now / p.window_ms);
                fixed_window(r, &k, p, ExpireMode::Nx)
            }
            Algo::FixedTtl => fixed_window(r, &format!("rl:{{{key}}}"), p, ExpireMode::Nx),
            Algo::SlidingLog => sliding_log_script(r, &format!("rl:{{{key}}}"), p, req_id),
            Algo::TokenBucket { cap } => {
                token_bucket_script(r, &format!("rl:{{{key}}}"), p, *cap, 1)
            }
        }
    }
}

/// Fixed window as one `MULTI`/`EXEC`: `INCR key; EXPIRE key W [NX]`; allow iff count <= limit.
/// With [`ExpireMode::Always`] every request pushes the expiry away (the trap `NX` avoids).
pub fn fixed_window(r: &mut MiniRedis, key: &str, p: &Params, mode: ExpireMode) -> bool {
    let replies = r.exec(&[
        Cmd::Incr(key.to_string()),
        Cmd::Expire(key.to_string(), p.window_ms / 1000, mode),
    ]);
    matches!(replies[0], Reply::Int(n) if n as u64 <= p.limit)
}

/// The NON-atomic counter from the Redis docs: `INCR`, then (a separate round trip) `EXPIRE` only if
/// the count is 1. `crash_between` models the client dying after `INCR` and before `EXPIRE`.
/// Returns `None` when the client crashed (no decision was made).
pub fn fixed_window_naive(
    r: &mut MiniRedis,
    key: &str,
    p: &Params,
    crash_between: bool,
) -> Option<bool> {
    let n = r.incr(key);
    if crash_between {
        return None;
    }
    if n == 1 {
        r.expire(key, p.window_ms / 1000, ExpireMode::Always);
    }
    Some(n as u64 <= p.limit)
}

/// Sliding-window log as ONE atomic script. Window is `(now - W, now]`.
pub fn sliding_log_script(r: &mut MiniRedis, key: &str, p: &Params, req_id: u64) -> bool {
    let now = r.time_ms();
    // Before `now >= W` nothing is old enough to leave the window (an entry at score 0 leaves at exactly W).
    if let Some(max) = now.checked_sub(p.window_ms) {
        r.zremrangebyscore_upto(key, max);
    }
    if (r.zcard(key) as u64) < p.limit {
        r.zadd(key, now, &format!("{now}:{req_id}"));
        r.expire_ms(key, p.window_ms, ExpireMode::Always);
        true
    } else {
        false
    }
}

/// Scale used by the token bucket: tokens are stored multiplied by `window_ms`, so that a refill of
/// `limit` tokens per `window_ms` is exactly `elapsed_ms * limit` units and no rounding is needed.
pub fn refill_scaled(tokens: i64, elapsed_ms: u64, limit: u64, cap_scaled: i64) -> i64 {
    cap_scaled.min(tokens + (elapsed_ms * limit) as i64)
}

/// Token bucket as ONE atomic script (hash `tokens`, `ts`; lazy refill; `max(0, now - ts)`;
/// TTL = time to refill from empty + 1 s). Costs `cost` tokens.
pub fn token_bucket_script(r: &mut MiniRedis, key: &str, p: &Params, cap: u64, cost: u64) -> bool {
    let now = r.time_ms();
    let cap_scaled = (cap * p.window_ms) as i64;
    let (tokens, ts) = match (r.hget(key, "tokens"), r.hget(key, "ts")) {
        (Some(t), Some(s)) => (t, s as u64),
        _ => (cap_scaled, now),
    };
    // Clock going backwards: elapsed saturates at 0, no tokens are added.
    let mut tokens = refill_scaled(tokens, now.saturating_sub(ts), p.limit, cap_scaled);
    let need = (cost * p.window_ms) as i64;
    let allowed = tokens >= need;
    if allowed {
        tokens -= need;
    }
    r.hset(key, "tokens", tokens);
    r.hset(key, "ts", now as i64);
    r.expire_ms(key, token_bucket_ttl_ms(p, cap), ExpireMode::Always);
    allowed
}

/// TTL of a token-bucket key: time to refill from empty to full, rounded up, plus 1000 ms.
pub fn token_bucket_ttl_ms(p: &Params, cap: u64) -> u64 {
    (cap * p.window_ms).div_ceil(p.limit) + 1000
}

/// Largest number of requests (from `ts`, sorted ascending) inside any half-open window
/// `[t, t + w)`; the maximum is attained with `t` equal to one of the timestamps.
pub fn max_in_window(ts: &[u64], w: u64) -> usize {
    let mut best = 0;
    let mut lo = 0;
    for hi in 0..ts.len() {
        while ts[hi] - ts[lo] >= w {
            lo += 1;
        }
        best = best.max(hi - lo + 1);
    }
    best
}

/// Upper bound on admissions in any window of `w` ms for a token bucket: `cap + rate * w`, floored
/// to whole tokens (an admission costs one whole token).
pub fn token_bucket_bound(p: &Params, cap: u64, w: u64) -> u64 {
    cap + p.limit * w / p.window_ms
}

/// Runs `algo` over arrival times `arrivals` (sorted, ms) for one key; returns the admitted times.
pub fn admitted_times(algo: Algo, p: &Params, arrivals: &[u64]) -> Vec<u64> {
    let mut r = MiniRedis::new();
    let mut out = Vec::new();
    for (i, &t) in arrivals.iter().enumerate() {
        r.set_now(t);
        if algo.allow(p, &mut r, "k", t, i as u64) {
            out.push(t);
        }
    }
    out
}

/// Boundary-burst arrivals: optionally one request at `t = 0` that opens the first window, then
/// `limit` requests at 1 ms spacing ending at `window - 1` ms, then `limit` more starting at `window`.
pub fn boundary_arrivals(p: &Params, opener: bool) -> Vec<u64> {
    let mut v = Vec::new();
    if opener {
        v.push(0);
    }
    v.extend((0..p.limit).map(|i| p.window_ms - p.limit + i));
    v.extend((0..p.limit).map(|i| p.window_ms + i));
    v
}

/// Admissions inside `[from, to)` of the boundary scenario for `algo`.
pub fn boundary_burst(algo: Algo, p: &Params, opener: bool) -> u64 {
    let arrivals = boundary_arrivals(p, opener);
    let from = p.window_ms - p.limit;
    let to = p.window_ms + p.limit;
    admitted_times(algo, p, &arrivals)
        .iter()
        .filter(|&&t| t >= from && t < to)
        .count() as u64
}

/// Seeded bursty overload trace over `duration_ms`: bursts of `limit/2..=2*limit` requests spaced
/// 1–3 ms apart, separated by random gaps of up to 1.5 windows, starting at a random phase.
pub fn bursty_trace(rng: &mut Rng, p: &Params, duration_ms: u64) -> Vec<u64> {
    let mut out = Vec::new();
    let mut t = 1 + rng.below(p.window_ms);
    while t < duration_ms {
        let n = rng.range(p.limit / 2, 2 * p.limit + 1);
        let spacing = rng.range(1, 4);
        out.extend((0..n).map(|i| t + i * spacing));
        t += n * spacing + rng.below(p.window_ms * 3 / 2);
    }
    out
}

/// Per-algorithm statistics of the "max admitted in any window of W" experiment.
#[derive(Clone, Debug, PartialEq)]
pub struct WindowStats {
    /// Algorithm.
    pub algo: Algo,
    /// Largest admitted count in any window over all traces.
    pub max: usize,
    /// Mean over traces of the per-trace maximum.
    pub mean: f64,
    /// Number of traces whose per-trace maximum exceeds `limit`.
    pub over_limit: usize,
}

/// Runs `traces` seeded bursty traces (seed derives each trace) through `algo`.
pub fn window_stats(
    algo: Algo,
    p: &Params,
    seed: u64,
    traces: u64,
    duration_ms: u64,
) -> WindowStats {
    let mut max = 0;
    let mut sum = 0usize;
    let mut over = 0;
    for i in 0..traces {
        let arrivals = bursty_trace(&mut Rng::new(seed).fork(i), p, duration_ms);
        let m = max_in_window(&admitted_times(algo, p, &arrivals), p.window_ms);
        max = max.max(m);
        sum += m;
        over += usize::from(m as u64 > p.limit);
    }
    WindowStats {
        algo,
        max,
        mean: sum as f64 / traces as f64,
        over_limit: over,
    }
}

/// Steady overload: one request every `gap_ms` for `duration_ms`; returns the admitted count.
pub fn steady_overload(algo: Algo, p: &Params, gap_ms: u64, duration_ms: u64) -> usize {
    let arrivals: Vec<u64> = (1..=duration_ms / gap_ms).map(|i| i * gap_ms).collect();
    admitted_times(algo, p, &arrivals).len()
}

/// Race between `clients` clients, each sending `per_client` requests at the same virtual instant to
/// one shared sliding log. `atomic = true`: each request is one script step. `atomic = false`: three
/// separate commands (`ZREMRANGEBYSCORE`, `ZCARD`, then `ZADD` if the *earlier* count was below the
/// limit); a seeded scheduler picks which client runs its next command. Returns admitted requests.
pub fn race_sliding(seed: u64, atomic: bool, clients: usize, per_client: u32, p: &Params) -> u64 {
    struct Client {
        left: u32,
        stage: u8,
        seen: u64,
    }
    let mut rng = Rng::new(seed);
    let mut r = MiniRedis::new();
    r.set_now(1_000_000);
    let now = r.now();
    let key = "rl:{k}";
    let mut cs: Vec<Client> = (0..clients)
        .map(|_| Client {
            left: per_client,
            stage: 0,
            seen: 0,
        })
        .collect();
    let mut admitted = 0;
    let mut seq = 0u64;
    loop {
        let active: Vec<usize> = (0..clients).filter(|&i| cs[i].left > 0).collect();
        let Some(&i) = rng.choose(&active) else { break };
        let c = &mut cs[i];
        if atomic {
            seq += 1;
            admitted += u64::from(sliding_log_script(&mut r, key, p, seq));
            c.left -= 1;
            continue;
        }
        match c.stage {
            0 => {
                if let Some(max) = now.checked_sub(p.window_ms) {
                    r.zremrangebyscore_upto(key, max);
                }
                c.stage = 1;
            }
            1 => {
                c.seen = r.zcard(key) as u64;
                c.stage = 2;
            }
            _ => {
                if c.seen < p.limit {
                    seq += 1;
                    r.zadd(key, now, &format!("{now}:{seq}"));
                    admitted += 1;
                }
                c.left -= 1;
                c.stage = 0;
            }
        }
    }
    admitted
}

/// Result of the TTL-leak experiment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeakStats {
    /// Keys left without any TTL at the end.
    pub leaked: usize,
    /// Keys whose last request was denied although the client stays under the limit when healthy.
    pub blocked: usize,
    /// Requests lost to a client crash.
    pub crashes: u32,
}

/// `keys` API keys, each sending `requests` requests spaced `gap_ms` apart. Before each request a
/// client crashes with probability `crash_p`. `atomic = false`: the doc's `INCR` + `EXPIRE if 1`,
/// the crash lands between the two commands. `atomic = true`: `MULTI`/`EXEC` with `EXPIRE NX`, the
/// crash lands before `EXEC` so nothing runs. The healthy client stays below the limit.
pub fn ttl_leak(
    seed: u64,
    atomic: bool,
    keys: usize,
    requests: u64,
    gap_ms: u64,
    crash_p: f64,
    p: &Params,
) -> LeakStats {
    let mut rng = Rng::new(seed);
    let mut r = MiniRedis::new();
    let mut last_denied = vec![false; keys];
    let mut crashes = 0;
    for step in 0..requests {
        r.set_now(step * gap_ms);
        for (k, denied) in last_denied.iter_mut().enumerate() {
            let key = format!("rl:{{{k}}}");
            let crash = rng.chance(crash_p);
            crashes += u32::from(crash);
            let verdict = if atomic {
                if crash {
                    None
                } else {
                    Some(fixed_window(&mut r, &key, p, ExpireMode::Nx))
                }
            } else {
                fixed_window_naive(&mut r, &key, p, crash)
            };
            if let Some(ok) = verdict {
                *denied = !ok;
            }
        }
    }
    LeakStats {
        leaked: r.keys_without_ttl().len(),
        blocked: last_denied.iter().filter(|&&d| d).count(),
        crashes,
    }
}

/// Steady client that sends one request every `gap_ms` for `requests` requests against a single
/// fixed-window key; returns how many were admitted (`mode` selects `EXPIRE NX` vs plain `EXPIRE`).
pub fn fixed_window_admitted(mode: ExpireMode, p: &Params, gap_ms: u64, requests: u64) -> u64 {
    let mut r = MiniRedis::new();
    let mut admitted = 0;
    for i in 0..requests {
        r.set_now(i * gap_ms);
        admitted += u64::from(fixed_window(&mut r, "rl:{k}", p, mode));
    }
    admitted
}

/// Elements stored under the limiter's key(s) after `requests` back-to-back requests at one instant.
pub fn memory_elements(algo: Algo, p: &Params, requests: u64) -> usize {
    let mut r = MiniRedis::new();
    r.set_now(5_000);
    for i in 0..requests {
        algo.allow(p, &mut r, "k", 5_000, i);
    }
    ["rl:{k}", &format!("rl:{{k}}:{}", 5_000 / p.window_ms)]
        .iter()
        .map(|k| r.elements(k))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Params = Params {
        limit: 100,
        window_ms: 60_000,
    };
    const ALGOS: [Algo; 5] = [
        Algo::FixedAligned,
        Algo::FixedTtl,
        Algo::SlidingLog,
        Algo::TokenBucket { cap: 20 },
        Algo::TokenBucket { cap: 100 },
    ];

    #[test]
    fn mini_redis_expiry_incr_ttl_and_zset_semantics() {
        let mut r = MiniRedis::new();
        r.set_now(1_000);
        assert_eq!(r.incr("a"), 1);
        assert!(r.expire("a", 2, ExpireMode::Nx));
        assert_eq!(r.pttl("a"), Some(2_000));
        // INCR leaves the TTL alone.
        r.set_now(1_500);
        assert_eq!(r.incr("a"), 2);
        assert_eq!(r.pttl("a"), Some(1_500));
        // NX refuses when a TTL exists, plain EXPIRE overrides it.
        assert!(!r.expire("a", 10, ExpireMode::Nx));
        assert!(r.expire("a", 10, ExpireMode::Always));
        // Gone exactly at expire_at.
        r.set_now(1_500 + 10_000 - 1);
        assert!(r.exists("a"));
        r.set_now(1_500 + 10_000);
        assert!(!r.exists("a"));
        assert_eq!(r.incr("a"), 1, "expired key restarts from 0 and has no TTL");
        assert_eq!(r.pttl("a"), None);
        // EXPIRE on a missing key does nothing.
        assert!(!r.expire("nope", 5, ExpireMode::Always));
        // ZSET members are unique: re-adding updates the score.
        assert!(r.zadd("z", 5, "m"));
        assert!(!r.zadd("z", 9, "m"));
        assert_eq!(r.zcard("z"), 1);
        assert_eq!(r.zremrangebyscore_upto("z", 8), 0);
        assert_eq!(r.zremrangebyscore_upto("z", 9), 1);
        assert!(!r.exists("z"), "an empty zset disappears");
    }

    #[test]
    fn exec_runs_batch_in_order() {
        let mut r = MiniRedis::new();
        let out = r.exec(&[
            Cmd::Incr("k".into()),
            Cmd::Expire("k".into(), 60, ExpireMode::Nx),
            Cmd::Incr("k".into()),
            Cmd::Expire("k".into(), 60, ExpireMode::Nx),
        ]);
        assert_eq!(
            out,
            vec![
                Reply::Int(1),
                Reply::Bool(true),
                Reply::Int(2),
                Reply::Bool(false)
            ]
        );
        assert!(r.keys_without_ttl().is_empty());
    }

    #[test]
    fn boundary_burst_pins_per_algorithm() {
        // No opener: aligned windows [0,60s) and [60s,120s) each take L -> 2L in 0.2 s.
        assert_eq!(boundary_burst(Algo::FixedAligned, &P, false), 2 * P.limit);
        // TTL-anchored window starts at the first burst request, so all 2L fall into ONE window.
        assert_eq!(boundary_burst(Algo::FixedTtl, &P, false), P.limit);
        // An opener at t=0 starts the window early; the opener itself uses one unit of the budget,
        // so the straddle gets (L-1) + L = 2L-1 for both fixed variants.
        assert_eq!(
            boundary_burst(Algo::FixedAligned, &P, true),
            2 * P.limit - 1
        );
        assert_eq!(boundary_burst(Algo::FixedTtl, &P, true), 2 * P.limit - 1);
        for opener in [false, true] {
            assert_eq!(boundary_burst(Algo::SlidingLog, &P, opener), P.limit);
            // Burst length is 0.2 s; refill in 0.2 s is 100*200/60000 = 0.33 token -> floor 0.
            for cap in [20, 100] {
                let got = boundary_burst(Algo::TokenBucket { cap }, &P, opener);
                assert!(got <= token_bucket_bound(&P, cap, 200), "cap {cap}: {got}");
                assert_eq!(got, cap, "cap {cap}, opener {opener}");
            }
        }
    }

    #[test]
    fn window_invariants_over_200_traces() {
        let (mut fixed_over, mut fixed_max) = (0, 0);
        for seed in 0..200u64 {
            let arrivals = bursty_trace(&mut Rng::new(seed), &P, 600_000);
            for algo in ALGOS {
                let m = max_in_window(&admitted_times(algo, &P, &arrivals), P.window_ms) as u64;
                match algo {
                    Algo::FixedAligned | Algo::FixedTtl => {
                        assert!(m <= 2 * P.limit, "seed {seed} {algo:?}: {m}");
                        fixed_over += u64::from(m > P.limit);
                        fixed_max = fixed_max.max(m);
                    }
                    Algo::SlidingLog => assert!(m <= P.limit, "seed {seed}: sliding admitted {m}"),
                    Algo::TokenBucket { cap } => {
                        assert!(
                            m <= token_bucket_bound(&P, cap, P.window_ms),
                            "seed {seed} cap {cap}: {m}"
                        )
                    }
                }
            }
        }
        assert!(fixed_over > 0, "fixed window never exceeded L in 400 runs");
        assert!(fixed_max > P.limit + P.limit / 2, "max fixed {fixed_max}");
    }

    #[test]
    fn max_in_window_is_half_open() {
        assert_eq!(max_in_window(&[], 10), 0);
        assert_eq!(max_in_window(&[0, 9, 10, 19], 10), 2);
        assert_eq!(max_in_window(&[5, 5, 5], 1), 3);
    }

    #[test]
    fn steady_overload_admits_about_the_sustained_rate() {
        // 3 req/s against 100/min for 10 min: 1000 for the counter-based and log limiters.
        let (gap, dur) = (333, 600_000);
        let fixed = steady_overload(Algo::FixedTtl, &P, gap, dur);
        let sliding = steady_overload(Algo::SlidingLog, &P, gap, dur);
        let tb = steady_overload(Algo::TokenBucket { cap: 20 }, &P, gap, dur);
        assert_eq!(fixed, 1000);
        assert!((990..=1000).contains(&sliding), "{sliding}");
        // cap + rate*600s = 20 + 1000.
        assert!((1010..=1020).contains(&tb), "{tb}");
    }

    #[test]
    fn non_atomic_sliding_over_admits_atomic_never() {
        let p = Params {
            limit: 20,
            window_ms: 60_000,
        };
        let (mut over, mut worst) = (0, 0);
        for seed in 0..200u64 {
            let atomic = race_sliding(seed, true, 8, 5, &p);
            assert_eq!(atomic, p.limit, "seed {seed}");
            let naive = race_sliding(seed, false, 8, 5, &p);
            assert!(naive >= p.limit, "seed {seed}: {naive}");
            assert!(naive <= 8 * 5);
            over += usize::from(naive > p.limit);
            worst = worst.max(naive);
        }
        assert!(over > 150, "only {over}/200 seeds over-admitted");
        assert!(worst > p.limit + 5, "worst {worst}");
        // One client alone cannot race with itself.
        assert_eq!(race_sliding(1, false, 1, 50, &p), p.limit);
    }

    #[test]
    fn non_atomic_incr_expire_leaks_multi_never() {
        let p = Params {
            limit: 5,
            window_ms: 60_000,
        };
        let mut leaked_total = 0;
        for seed in 0..20u64 {
            let naive = ttl_leak(seed, false, 100, 80, 15_000, 0.02, &p);
            let multi = ttl_leak(seed, true, 100, 80, 15_000, 0.02, &p);
            assert_eq!(multi.leaked, 0, "seed {seed}");
            assert_eq!(
                multi.blocked, 0,
                "seed {seed}: a healthy client under the limit is never denied"
            );
            assert!(multi.crashes > 0);
            assert!(
                naive.blocked <= naive.leaked,
                "only leaked keys can block a healthy client"
            );
            leaked_total += naive.leaked;
            // A leaked key counts up forever: after 80 requests it is far above the limit.
            assert!(naive.blocked > 0 || naive.leaked == 0, "seed {seed}");
        }
        assert!(leaked_total > 0);
    }

    #[test]
    fn leaked_key_blocks_forever_even_after_many_windows() {
        let p = Params {
            limit: 5,
            window_ms: 60_000,
        };
        let mut r = MiniRedis::new();
        assert_eq!(fixed_window_naive(&mut r, "k", &p, true), None);
        assert_eq!(r.keys_without_ttl(), vec!["k"]);
        let verdicts: Vec<bool> = (1..=10u64)
            .map(|i| {
                r.set_now(i * 600_000);
                fixed_window_naive(&mut r, "k", &p, false).unwrap()
            })
            .collect();
        // Count was 1 after the crash, so requests 2..5 pass, then forever denied (10 minutes apart!).
        assert_eq!(
            verdicts,
            [
                true, true, true, true, false, false, false, false, false, false
            ]
        );
        assert_eq!(r.pttl("k"), None);
    }

    #[test]
    fn expire_nx_keeps_first_ttl_plain_expire_extends_forever() {
        let p = Params {
            limit: 5,
            window_ms: 60_000,
        };
        // One request per 10 s for 10 minutes.
        let nx = fixed_window_admitted(ExpireMode::Nx, &p, 10_000, 60);
        let plain = fixed_window_admitted(ExpireMode::Always, &p, 10_000, 60);
        // NX: windows of 6 requests, 5 admitted each, 10 windows.
        assert_eq!(nx, 50);
        // Plain EXPIRE: the key never expires, the counter just grows: 5 admitted in total.
        assert_eq!(plain, 5);
        // And directly on the TTL: after the first request the TTL under NX is not pushed.
        let mut r = MiniRedis::new();
        fixed_window(&mut r, "k", &p, ExpireMode::Nx);
        r.set_now(30_000);
        fixed_window(&mut r, "k", &p, ExpireMode::Nx);
        assert_eq!(r.pttl("k"), Some(30_000));
        let mut r2 = MiniRedis::new();
        fixed_window(&mut r2, "k", &p, ExpireMode::Always);
        r2.set_now(30_000);
        fixed_window(&mut r2, "k", &p, ExpireMode::Always);
        assert_eq!(r2.pttl("k"), Some(60_000));
    }

    #[test]
    fn token_bucket_refill_math_is_pinned() {
        let cap_scaled = (20 * P.window_ms) as i64;
        // 100 tokens / 60 s: 600 ms refill exactly one token (= window_ms units).
        assert_eq!(
            refill_scaled(0, 600, P.limit, cap_scaled),
            P.window_ms as i64
        );
        // 6 s -> 10 tokens; 60 s from empty -> 100 tokens but cap 20 wins.
        assert_eq!(
            refill_scaled(0, 6_000, P.limit, cap_scaled),
            10 * P.window_ms as i64
        );
        assert_eq!(refill_scaled(0, 60_000, P.limit, cap_scaled), cap_scaled);
        // Never above cap.
        assert_eq!(
            refill_scaled(cap_scaled, 1_000, P.limit, cap_scaled),
            cap_scaled
        );
        // Time to refill from empty: 20 * 60000 / 100 = 12 s, +1 s.
        assert_eq!(token_bucket_ttl_ms(&P, 20), 13_000);
        assert_eq!(token_bucket_ttl_ms(&P, 100), 61_000);
        assert_eq!(token_bucket_bound(&P, 20, 60_000), 120);
        assert_eq!(token_bucket_bound(&P, 100, 200), 100);
    }

    #[test]
    fn token_bucket_script_drains_then_refills_one_token_per_600ms() {
        let mut r = MiniRedis::new();
        r.set_now(1_000);
        let allowed = (0..30)
            .filter(|_| token_bucket_script(&mut r, "b", &P, 20, 1))
            .count();
        assert_eq!(allowed, 20);
        // Rate is 1 token / 600 ms: at +599 ms still denied, at +600 ms exactly one allowed.
        r.set_now(1_599);
        assert!(!token_bucket_script(&mut r, "b", &P, 20, 1));
        r.set_now(1_600);
        assert!(token_bucket_script(&mut r, "b", &P, 20, 1));
        assert!(!token_bucket_script(&mut r, "b", &P, 20, 1));
    }

    #[test]
    fn token_bucket_clock_going_backwards_adds_no_tokens() {
        let mut r = MiniRedis::new();
        r.set_now(100_000);
        for _ in 0..20 {
            assert!(token_bucket_script(&mut r, "b", &P, 20, 1));
        }
        // Master with a slower clock: 50 s in the past. No tokens are created.
        r.set_now(50_000);
        assert!(!token_bucket_script(&mut r, "b", &P, 20, 1));
        assert_eq!(r.hget("b", "tokens"), Some(0));
    }

    #[test]
    fn token_bucket_backward_step_is_recredited_when_clock_recovers() {
        // Limitation of the chapter's script (it stores ts = now even when now < ts): the time
        // between the stepped-back clock and the old ts is credited a second time.
        let mut r = MiniRedis::new();
        r.set_now(100_000);
        for _ in 0..20 {
            assert!(token_bucket_script(&mut r, "b", &P, 20, 1));
        }
        r.set_now(94_000); // 6 s back; denied, ts is now 94_000
        assert!(!token_bucket_script(&mut r, "b", &P, 20, 1));
        r.set_now(100_000); // clock recovers: 6 s of refill = 10 tokens that were already "used up" once
        let extra = (0..20)
            .filter(|_| token_bucket_script(&mut r, "b", &P, 20, 1))
            .count();
        assert_eq!(extra, 10);
    }

    #[test]
    fn token_bucket_ttl_expiry_does_not_change_behaviour() {
        // Reference: the same bucket without TTL. Admission sequences must be identical, including
        // after idle gaps longer than the TTL.
        for seed in 0..50u64 {
            let mut rng = Rng::new(seed);
            let mut t = 1_000u64;
            let mut with_ttl = MiniRedis::new();
            let (mut tokens, mut ts) = (20 * P.window_ms as i64, t);
            for _ in 0..400 {
                t += if rng.chance(0.05) {
                    rng.range(13_000, 90_000)
                } else {
                    rng.below(2_000)
                };
                with_ttl.set_now(t);
                let real = token_bucket_script(&mut with_ttl, "b", &P, 20, 1);
                tokens = refill_scaled(tokens, t - ts, P.limit, 20 * P.window_ms as i64);
                ts = t;
                let reference = tokens >= P.window_ms as i64;
                if reference {
                    tokens -= P.window_ms as i64;
                }
                assert_eq!(real, reference, "seed {seed} t {t}");
            }
        }
    }

    #[test]
    fn memory_is_o_limit_for_log_and_constant_for_others() {
        for limit in [100u64, 1_000, 10_000] {
            let p = Params {
                limit,
                window_ms: 60_000,
            };
            assert_eq!(
                memory_elements(Algo::SlidingLog, &p, 2 * limit),
                limit as usize
            );
            assert_eq!(memory_elements(Algo::FixedTtl, &p, 2 * limit), 1);
            assert_eq!(memory_elements(Algo::FixedAligned, &p, 2 * limit), 1);
            assert_eq!(
                memory_elements(Algo::TokenBucket { cap: 20 }, &p, 2 * limit),
                2
            );
        }
    }

    #[test]
    fn results_are_pure_functions_of_the_seed() {
        let a = window_stats(Algo::FixedAligned, &P, 7, 20, 600_000);
        let b = window_stats(Algo::FixedAligned, &P, 7, 20, 600_000);
        assert_eq!(a, b);
        assert_eq!(
            race_sliding(3, false, 8, 5, &P),
            race_sliding(3, false, 8, 5, &P)
        );
        let p5 = Params {
            limit: 5,
            window_ms: 60_000,
        };
        assert_eq!(
            ttl_leak(9, false, 50, 40, 15_000, 0.02, &p5),
            ttl_leak(9, false, 50, 40, 15_000, 0.02, &p5)
        );
    }

    #[test]
    fn app_clock_skew_splits_the_aligned_window_key() {
        // F3: two pods whose clocks differ by 1 s use different window ids around the boundary.
        let mut r = MiniRedis::new();
        r.set_now(59_500);
        let p = Params {
            limit: 2,
            window_ms: 60_000,
        };
        assert!(Algo::FixedAligned.allow(&p, &mut r, "k", 59_500, 0)); // pod A, window 0
        assert!(Algo::FixedAligned.allow(&p, &mut r, "k", 59_500, 1));
        assert!(!Algo::FixedAligned.allow(&p, &mut r, "k", 59_500, 2));
        // pod B is 1 s ahead, so it counts into window 1 and is allowed again.
        assert!(Algo::FixedAligned.allow(&p, &mut r, "k", 60_500, 3));
    }
}
