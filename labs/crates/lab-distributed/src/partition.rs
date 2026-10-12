//! Lab (c): partitioning. Deterministic models of the strategies in chapter 06 of the book:
//! `hash mod N`, fixed partitions, a consistent-hash ring with virtual nodes, Jump Consistent Hash,
//! Redis Cluster hash slots (CRC16 + hash tags), Zipf skew with hot-key mitigation, and range vs
//! hash partitioning for monotonically increasing keys.
//!
//! Everything is a pure function of its arguments (and a seed where randomness is involved).
//! Nothing here touches the standard `Hash`/`DefaultHasher`: [`hash64`] is this lab's own, fully
//! specified hash, so a key maps to the same partition on every platform and Rust version.

use crate::rng::Rng;
use std::collections::BTreeMap;
use std::fmt::Write as _;

// ---------------------------------------------------------------------------------------------
// The lab's own specified hash
// ---------------------------------------------------------------------------------------------

/// Specified 64-bit hash of this lab: a byte-wise multiply/xor pass (offset basis
/// `0xcbf29ce484222325`, prime `0x100000001b3`) followed by the 64-bit finalizer
/// (`x ^= x >> 33; x *= 0xff51afd7ed558ccd; x ^= x >> 33; x *= 0xc4ceb9fe1a85ec53; x ^= x >> 33`).
/// The finalizer matters: without it the low bits of the first pass are poorly mixed and
/// `hash % P` would be biased. The input is the raw key bytes, the output is stable forever
/// (pinned by tests): changing it would be a data migration.
pub fn hash64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^ (h >> 33)
}

/// Hash of the key text `"key:{seed}:{i}"`: the key population used by the experiments.
fn key_hashes(seed: u64, count: usize) -> Vec<u64> {
    (0..count)
        .map(|i| hash64(format!("key:{seed}:{i}").as_bytes()))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Placement strategies
// ---------------------------------------------------------------------------------------------

/// Maps a key (through its [`hash64`]) to a node number in `0..nodes()`.
pub trait Placement {
    /// Number of nodes this placement was built for.
    fn nodes(&self) -> u32;
    /// Node owning the key whose hash is `hash`.
    fn node_of(&self, hash: u64) -> u32;
    /// Node owning `key` (hashes the bytes with [`hash64`] first).
    fn locate(&self, key: &[u8]) -> u32 {
        self.node_of(hash64(key))
    }
}

/// The naive design: `hash mod N`.
#[derive(Clone, Debug)]
pub struct ModN {
    n: u32,
}

impl ModN {
    /// `n` must be at least 1.
    pub fn new(n: u32) -> Self {
        assert!(n >= 1);
        Self { n }
    }
}

impl Placement for ModN {
    fn nodes(&self) -> u32 {
        self.n
    }
    fn node_of(&self, hash: u64) -> u32 {
        (hash % u64::from(self.n)) as u32
    }
}

/// Moves `len / (n_old + 1)` entries of `owner` (values `0..n_old`) to the new node `n_old`,
/// always taking from the currently largest owner (ties: lowest id) and, within an owner, from
/// its highest-numbered entries. This is the minimum movement that keeps counts within 1 of each
/// other. Returns the number of entries moved.
fn steal_for_new_node(owner: &mut [u32], n_old: u32) -> usize {
    let target = owner.len() / (n_old as usize + 1);
    let mut counts = vec![0usize; n_old as usize];
    for &o in owner.iter() {
        counts[o as usize] += 1;
    }
    let mut quota = vec![0usize; n_old as usize];
    for _ in 0..target {
        let donor = (0..n_old as usize)
            .max_by_key(|&i| (counts[i], std::cmp::Reverse(i)))
            .expect("n_old >= 1");
        counts[donor] -= 1;
        quota[donor] += 1;
    }
    for slot in (0..owner.len()).rev() {
        let o = owner[slot] as usize;
        if o < n_old as usize && quota[o] > 0 {
            quota[o] -= 1;
            owner[slot] = n_old;
        }
    }
    target
}

/// A fixed number `P` of partitions: `partition = hash % P` never changes; only the
/// partition -> node table is edited when nodes join (Dynamo strategy 3 / Redis slots / Kafka).
#[derive(Clone, Debug)]
pub struct FixedPartitions {
    owner: Vec<u32>,
    n: u32,
}

impl FixedPartitions {
    /// `p` partitions spread round-robin over `n` nodes (`p >= n >= 1`).
    pub fn new(p: u32, n: u32) -> Self {
        assert!(n >= 1 && p >= n);
        Self {
            owner: (0..p).map(|i| i % n).collect(),
            n,
        }
    }

    /// Number of partitions (constant for the life of the data).
    pub fn partitions(&self) -> u32 {
        self.owner.len() as u32
    }

    /// The partition of a key hash; independent of the number of nodes.
    pub fn partition_of(&self, hash: u64) -> u32 {
        (hash % self.owner.len() as u64) as u32
    }

    /// Partition -> node table.
    pub fn owners(&self) -> &[u32] {
        &self.owner
    }

    /// Adds one node, moving the minimum number of whole partitions to it; returns how many.
    pub fn add_node(&mut self) -> usize {
        let moved = steal_for_new_node(&mut self.owner, self.n);
        self.n += 1;
        moved
    }

    /// Grows to `m >= nodes()` nodes one node at a time.
    pub fn grown_to(&self, m: u32) -> Self {
        let mut next = self.clone();
        while next.n < m {
            next.add_node();
        }
        next
    }
}

impl Placement for FixedPartitions {
    fn nodes(&self) -> u32 {
        self.n
    }
    fn node_of(&self, hash: u64) -> u32 {
        self.owner[self.partition_of(hash) as usize]
    }
}

/// Consistent hashing with virtual nodes: node `i` puts `vnodes` points on a 64-bit ring at
/// `hash64(salt, node, j)`; a key goes clockwise to the first point at or after its hash.
/// Because a node's points do not depend on how many other nodes exist, growing the cluster only
/// adds points (the classic monotonicity property).
#[derive(Clone, Debug)]
pub struct Ring {
    points: Vec<(u64, u32)>,
    n: u32,
}

impl Ring {
    /// `n` nodes with `vnodes` points each. `salt` varies the point set (the lab sweeps it to
    /// average over rings).
    pub fn new(n: u32, vnodes: u32, salt: u64) -> Self {
        assert!(n >= 1 && vnodes >= 1);
        let mut points = Vec::with_capacity(n as usize * vnodes as usize);
        for node in 0..n {
            for j in 0..vnodes {
                let mut buf = [0u8; 20];
                buf[..8].copy_from_slice(&salt.to_be_bytes());
                buf[8..12].copy_from_slice(&node.to_be_bytes());
                buf[12..16].copy_from_slice(&j.to_be_bytes());
                buf[16..].copy_from_slice(b"ring");
                points.push((hash64(&buf), node));
            }
        }
        points.sort_unstable();
        Self { points, n }
    }

    /// Fraction of the ring (0..1) owned by each node, from the arc lengths.
    pub fn ownership(&self) -> Vec<f64> {
        let mut arc = vec![0u128; self.n as usize];
        let len = self.points.len();
        for i in 0..len {
            let prev = self.points[(i + len - 1) % len].0;
            let this = self.points[i].0;
            arc[self.points[i].1 as usize] += u128::from(this.wrapping_sub(prev));
        }
        // With one point the arc is the whole ring (wrapping_sub gives 0), fix that case.
        if len == 1 {
            arc[self.points[0].1 as usize] = 1u128 << 64;
        }
        arc.iter()
            .map(|&a| a as f64 / (1u128 << 64) as f64)
            .collect()
    }
}

impl Placement for Ring {
    fn nodes(&self) -> u32 {
        self.n
    }
    fn node_of(&self, hash: u64) -> u32 {
        let i = self.points.partition_point(|&(p, _)| p < hash);
        self.points[i % self.points.len()].1
    }
}

/// Jump Consistent Hash (Lamping & Veach, Figure 1 of arXiv 1406.2294): no storage, buckets are
/// numbered `0..n`, and growing `n` only moves keys to the new last bucket.
pub fn jump_hash(mut key: u64, buckets: u32) -> u32 {
    assert!(buckets >= 1);
    let mut b: i64 = -1;
    let mut j: i64 = 0;
    while j < i64::from(buckets) {
        b = j;
        key = key.wrapping_mul(2_862_933_555_777_941_757).wrapping_add(1);
        // The paper's C++ uses doubles here; IEEE-754 division/multiplication is deterministic.
        j = ((b + 1) as f64 * ((1u64 << 31) as f64 / ((key >> 33) + 1) as f64)) as i64;
    }
    b as u32
}

/// [`jump_hash`] as a [`Placement`].
#[derive(Clone, Debug)]
pub struct Jump {
    n: u32,
}

impl Jump {
    /// `n >= 1` buckets.
    pub fn new(n: u32) -> Self {
        assert!(n >= 1);
        Self { n }
    }
}

impl Placement for Jump {
    fn nodes(&self) -> u32 {
        self.n
    }
    fn node_of(&self, hash: u64) -> u32 {
        jump_hash(hash, self.n)
    }
}

/// How many keys changed owner between two placements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveStats {
    /// Keys examined.
    pub keys: usize,
    /// Keys whose owner changed.
    pub moved: usize,
    /// Moved keys whose new owner is a node that did not exist before (`>= before.nodes()`).
    pub moved_to_new: usize,
}

impl MoveStats {
    /// Fraction of keys that moved.
    pub fn fraction(&self) -> f64 {
        self.moved as f64 / self.keys as f64
    }
    /// Keys that moved from one OLD node to another OLD node (zero for consistent schemes).
    pub fn moved_between_old(&self) -> usize {
        self.moved - self.moved_to_new
    }
}

/// Compares the owner of every key hash before and after a membership change.
pub fn move_stats(before: &dyn Placement, after: &dyn Placement, hashes: &[u64]) -> MoveStats {
    let mut moved = 0;
    let mut moved_to_new = 0;
    for &h in hashes {
        let (a, b) = (before.node_of(h), after.node_of(h));
        if a != b {
            moved += 1;
            if b >= before.nodes() {
                moved_to_new += 1;
            }
        }
    }
    MoveStats {
        keys: hashes.len(),
        moved,
        moved_to_new,
    }
}

/// Per-node load and the balance metrics used in the lab.
#[derive(Clone, Debug)]
pub struct LoadStats {
    /// Requests (or keys) landing on each node.
    pub per_node: Vec<u64>,
}

impl LoadStats {
    /// Counts how many of `hashes` each of `p`'s nodes owns.
    pub fn of_keys(p: &dyn Placement, hashes: &[u64]) -> Self {
        let mut per_node = vec![0u64; p.nodes() as usize];
        for &h in hashes {
            per_node[p.node_of(h) as usize] += 1;
        }
        Self { per_node }
    }
    /// Sum over nodes.
    pub fn total(&self) -> u64 {
        self.per_node.iter().sum()
    }
    /// Load of the hottest node.
    pub fn max(&self) -> u64 {
        self.per_node.iter().copied().max().unwrap_or(0)
    }
    /// `max / mean`; 1.0 is perfect balance. Its inverse is Dynamo's load-balancing efficiency.
    pub fn max_over_mean(&self) -> f64 {
        self.max() as f64 * self.per_node.len() as f64 / self.total() as f64
    }
    /// Hottest node's share of the total, in 0..1.
    pub fn max_share(&self) -> f64 {
        self.max() as f64 / self.total() as f64
    }
    /// Standard deviation of node load divided by mean load.
    pub fn rel_std(&self) -> f64 {
        let n = self.per_node.len() as f64;
        let mean = self.total() as f64 / n;
        let var = self
            .per_node
            .iter()
            .map(|&x| (x as f64 - mean).powi(2))
            .sum::<f64>()
            / n;
        var.sqrt() / mean
    }
}

// ---------------------------------------------------------------------------------------------
// Redis Cluster: CRC16, hash tags, slot map
// ---------------------------------------------------------------------------------------------

/// Number of hash slots in Redis Cluster.
pub const REDIS_SLOTS: usize = 16384;

/// CRC-16/XMODEM as in the Redis Cluster specification: polynomial 0x1021, init 0, no input or
/// output reflection, final xor 0. `crc16_xmodem(b"123456789") == 0x31C3`.
pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The part of `key` that is hashed: if the key has a `{`, a `}` somewhere to its right and at
/// least one byte between the first `{` and the first `}` after it, exactly that substring;
/// otherwise the whole key.
pub fn hash_tag(key: &[u8]) -> &[u8] {
    let Some(s) = key.iter().position(|&b| b == b'{') else {
        return key;
    };
    let Some(e) = key[s + 1..].iter().position(|&b| b == b'}') else {
        return key;
    };
    if e == 0 { key } else { &key[s + 1..s + 1 + e] }
}

/// `HASH_SLOT(key) = CRC16(hash_tag(key)) mod 16384`.
pub fn hash_slot(key: &[u8]) -> u16 {
    (crc16_xmodem(hash_tag(key)) as usize % REDIS_SLOTS) as u16
}

/// Slot -> master table of a Redis-Cluster-like deployment.
#[derive(Clone, Debug)]
pub struct SlotMap {
    owner: Vec<u32>,
    masters: u32,
}

impl SlotMap {
    /// Splits the 16384 slots into `n` contiguous ranges of near-equal size (the first
    /// `16384 % n` masters get one extra slot). This is the lab's choice of initial layout.
    pub fn even(n: u32) -> Self {
        assert!(n >= 1);
        let (base, extra) = (REDIS_SLOTS / n as usize, REDIS_SLOTS % n as usize);
        let mut owner = Vec::with_capacity(REDIS_SLOTS);
        for m in 0..n as usize {
            let len = base + usize::from(m < extra);
            owner.extend(std::iter::repeat_n(m as u32, len));
        }
        Self { owner, masters: n }
    }

    /// Number of masters.
    pub fn masters(&self) -> u32 {
        self.masters
    }

    /// Master owning `slot`.
    pub fn owner_of(&self, slot: u16) -> u32 {
        self.owner[slot as usize]
    }

    /// Number of slots held by each master.
    pub fn counts(&self) -> Vec<usize> {
        let mut c = vec![0; self.masters as usize];
        for &o in &self.owner {
            c[o as usize] += 1;
        }
        c
    }

    /// Contiguous `(first_slot, last_slot, master)` runs.
    pub fn ranges(&self) -> Vec<(u16, u16, u32)> {
        let mut out: Vec<(u16, u16, u32)> = Vec::new();
        for (slot, &o) in self.owner.iter().enumerate() {
            match out.last_mut() {
                Some(last) if last.2 == o => last.1 = slot as u16,
                _ => out.push((slot as u16, slot as u16, o)),
            }
        }
        out
    }

    /// A new master joins and takes the minimum number of slots (`16384 / (n+1)`), from the
    /// largest masters first. Returns the new map and the slots that moved.
    pub fn add_master(&self) -> (SlotMap, Vec<u16>) {
        let mut owner = self.owner.clone();
        steal_for_new_node(&mut owner, self.masters);
        let moved = (0..REDIS_SLOTS as u16)
            .filter(|&s| owner[s as usize] != self.owner[s as usize])
            .collect();
        (
            SlotMap {
                owner,
                masters: self.masters + 1,
            },
            moved,
        )
    }
}

impl Placement for SlotMap {
    fn nodes(&self) -> u32 {
        self.masters
    }
    fn node_of(&self, hash: u64) -> u32 {
        // Placement is defined on hashes; for Redis the "hash" is the slot number itself.
        self.owner[(hash % REDIS_SLOTS as u64) as usize]
    }
}

// ---------------------------------------------------------------------------------------------
// Skew: Zipf sampler and hot-key mitigation
// ---------------------------------------------------------------------------------------------

/// Zipf distribution over ranks `0..k`: `P(rank r) ∝ 1 / (r+1)^s`. Sampling is an integer
/// inverse-CDF lookup (binary search in a table scaled to 2^53), so the stream of samples is a
/// pure function of the RNG stream.
#[derive(Clone, Debug)]
pub struct Zipf {
    cdf: Vec<u64>,
    probs: Vec<f64>,
}

impl Zipf {
    /// `k >= 1` keys, exponent `s >= 0`.
    pub fn new(k: usize, s: f64) -> Self {
        assert!(k >= 1 && s >= 0.0);
        let weights: Vec<f64> = (1..=k).map(|r| (r as f64).powf(-s)).collect();
        let total: f64 = weights.iter().sum();
        let scale = (1u64 << 53) as f64;
        let mut acc = 0.0;
        let mut cdf = Vec::with_capacity(k);
        for w in &weights {
            acc += w / total;
            cdf.push((acc * scale) as u64);
        }
        *cdf.last_mut().expect("k >= 1") = 1u64 << 53;
        Self {
            cdf,
            probs: weights.iter().map(|w| w / total).collect(),
        }
    }

    /// Theoretical probability of rank `r` (0 = hottest).
    pub fn prob(&self, r: usize) -> f64 {
        self.probs[r]
    }

    /// Draws a rank.
    pub fn sample(&self, rng: &mut Rng) -> usize {
        let u = rng.next_u64() >> 11;
        self.cdf.partition_point(|&c| c <= u)
    }
}

/// How a hot key is protected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotMitigation {
    /// Nothing: one key is one node.
    None,
    /// Write sharding: the `hot` hottest keys become `s` keys `key#0..key#s-1`. A write picks one
    /// random suffix (1 node request); a read must query all `s` (s node requests).
    Suffix { hot: usize, s: u32 },
    /// Copies for read-mostly keys: a read picks one random copy (1 request); a write updates all
    /// `s` copies (s requests).
    Copies { hot: usize, s: u32 },
}

/// Outcome of [`hot_key_run`].
#[derive(Clone, Debug)]
pub struct HotReport {
    /// Node-level requests landing on each node.
    pub load: LoadStats,
    /// Logical (client) requests issued.
    pub logical: u64,
    /// Fraction of logical requests that touched the hottest key, as drawn.
    pub top_key_share: f64,
}

impl HotReport {
    /// Node requests per logical request (the cost of the mitigation).
    pub fn amplification(&self) -> f64 {
        self.load.total() as f64 / self.logical as f64
    }
}

/// Runs `requests` logical requests against `nodes` nodes (fixed partitions, P = 1024). Keys are
/// drawn from `Zipf(keys, zipf_s)`; each request is a write with probability `write_pct`%.
pub fn hot_key_run(
    seed: u64,
    nodes: u32,
    keys: usize,
    zipf_s: f64,
    requests: u64,
    write_pct: u64,
    mitigation: HotMitigation,
) -> HotReport {
    let placement = FixedPartitions::new(1024, nodes);
    let zipf = Zipf::new(keys, zipf_s);
    let mut rng = Rng::new(seed);
    let base: Vec<u64> = (0..keys)
        .map(|r| hash64(format!("key:{r}").as_bytes()))
        .collect();
    let (hot, s) = match mitigation {
        HotMitigation::None => (0, 1),
        HotMitigation::Suffix { hot, s } | HotMitigation::Copies { hot, s } => (hot, s as usize),
    };
    let variants: Vec<Vec<u64>> = (0..hot)
        .map(|r| {
            (0..s)
                .map(|j| hash64(format!("key:{r}#{j}").as_bytes()))
                .collect()
        })
        .collect();
    let mut per_node = vec![0u64; nodes as usize];
    let mut top = 0u64;
    for _ in 0..requests {
        let r = zipf.sample(&mut rng);
        let is_write = rng.below(100) < write_pct;
        if r == 0 {
            top += 1;
        }
        if r >= hot || mitigation == HotMitigation::None {
            per_node[placement.node_of(base[r]) as usize] += 1;
            continue;
        }
        let one_random = is_write == matches!(mitigation, HotMitigation::Suffix { .. });
        if one_random {
            let j = rng.below(s as u64) as usize;
            per_node[placement.node_of(variants[r][j]) as usize] += 1;
        } else {
            for &h in &variants[r] {
                per_node[placement.node_of(h) as usize] += 1;
            }
        }
    }
    HotReport {
        load: LoadStats { per_node },
        logical: requests,
        top_key_share: top as f64 / requests as f64,
    }
}

// ---------------------------------------------------------------------------------------------
// Range vs hash for monotonically increasing keys
// ---------------------------------------------------------------------------------------------

/// Range partitioning with fixed, pre-chosen boundaries: `parts` equal ranges of `0..space`.
pub fn static_range_partition(key: u64, parts: u64, space: u64) -> u64 {
    key / space.div_ceil(parts)
}

/// Number of partitions a scan of `lo..hi` (consecutive integer keys) touches under static range
/// partitioning.
pub fn range_scan_partitions(lo: u64, hi: u64, parts: u64, space: u64) -> u64 {
    static_range_partition(hi - 1, parts, space) - static_range_partition(lo, parts, space) + 1
}

/// Number of partitions a scan of `lo..hi` touches when each key lives in `hash64(key) % parts`.
pub fn hash_scan_partitions(lo: u64, hi: u64, parts: u64) -> u64 {
    let mut seen = vec![false; parts as usize];
    for k in lo..hi {
        seen[(hash64(&k.to_be_bytes()) % parts) as usize] = true;
    }
    seen.iter().filter(|&&s| s).count() as u64
}

/// Range-partitioned store with automatic splitting (the Bigtable/MongoDB style): a partition
/// holding more than `cap` keys is split at its median key.
#[derive(Clone, Debug)]
pub struct SplitRangeTable {
    cap: usize,
    /// lower bound -> sorted keys of that partition
    parts: BTreeMap<u64, Vec<u64>>,
}

impl SplitRangeTable {
    /// Starts with one partition covering everything. `cap >= 2`.
    pub fn new(cap: usize) -> Self {
        assert!(cap >= 2);
        let mut parts = BTreeMap::new();
        parts.insert(0, Vec::new());
        Self { cap, parts }
    }

    /// Inserts `key` and returns the lower bound (identity) of the partition that took the write
    /// (before any split it triggers).
    pub fn insert(&mut self, key: u64) -> u64 {
        let (&lower, _) = self
            .parts
            .range(..=key)
            .next_back()
            .expect("lower bound 0 always exists");
        let keys = self.parts.get_mut(&lower).expect("just found");
        let at = keys.partition_point(|&k| k < key);
        keys.insert(at, key);
        if keys.len() > self.cap {
            let right = keys.split_off(keys.len() / 2);
            self.parts.insert(right[0], right);
        }
        lower
    }

    /// Current number of partitions.
    pub fn partitions(&self) -> usize {
        self.parts.len()
    }
}

/// Mean over consecutive windows of `window` writes of "share of the window's writes that hit its
/// busiest partition" (partition ids as given).
pub fn mean_busiest_share(ids: &[u64], window: usize) -> f64 {
    let mut total = 0.0;
    let mut n = 0;
    for chunk in ids.chunks_exact(window) {
        let mut counts: BTreeMap<u64, usize> = BTreeMap::new();
        for &id in chunk {
            *counts.entry(id).or_default() += 1;
        }
        let top = counts.values().copied().max().unwrap_or(0);
        total += top as f64 / window as f64;
        n += 1;
    }
    total / n as f64
}

// ---------------------------------------------------------------------------------------------
// Report printed by the `partitioning` binary
// ---------------------------------------------------------------------------------------------

const KEYS: usize = 100_000;
const PARTITIONS: u32 = 1024;
const RING_VNODES: u32 = 100;

fn pct(x: f64) -> f64 {
    x * 100.0
}

fn section_moves(seed: u64, out: &mut String) {
    let hashes = key_hashes(seed, KEYS);
    let _ = writeln!(
        out,
        "-- (1) keys moved when growing N -> M nodes ({KEYS} keys; ring: {RING_VNODES} vnodes; fixed: P={PARTITIONS}) --"
    );
    let _ = writeln!(
        out,
        "{:>6} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "N->M", "min%", "modN%", "fixed%", "ring%", "jump%"
    );
    let cases: [(u32, u32); 7] = [(2, 3), (3, 4), (5, 6), (8, 9), (9, 10), (10, 11), (10, 12)];
    let mut old_moves = (0, 0, 0, 0);
    for (n, m) in cases {
        let minimum = f64::from(m - n) / f64::from(m);
        let fixed_before = FixedPartitions::new(PARTITIONS, n);
        let fixed_after = fixed_before.grown_to(m);
        let ring_before = Ring::new(n, RING_VNODES, seed);
        let ring_after = Ring::new(m, RING_VNODES, seed);
        let a = move_stats(&ModN::new(n), &ModN::new(m), &hashes);
        let b = move_stats(&fixed_before, &fixed_after, &hashes);
        let c = move_stats(&ring_before, &ring_after, &hashes);
        let d = move_stats(&Jump::new(n), &Jump::new(m), &hashes);
        let _ = writeln!(
            out,
            "{:>3}->{:<2} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1}",
            n,
            m,
            pct(minimum),
            pct(a.fraction()),
            pct(b.fraction()),
            pct(c.fraction()),
            pct(d.fraction())
        );
        if (n, m) == (10, 12) {
            old_moves = (
                a.moved_between_old(),
                b.moved_between_old(),
                c.moved_between_old(),
                d.moved_between_old(),
            );
        }
    }
    let _ = writeln!(
        out,
        "10->12 keys moved between two OLD nodes: modN={} fixed={} ring={} jump={}",
        old_moves.0, old_moves.1, old_moves.2, old_moves.3
    );
}

fn section_redis(seed: u64, out: &mut String) {
    let _ = writeln!(
        out,
        "-- (2) Redis Cluster: CRC16/XMODEM, hash tags, slots --"
    );
    let _ = writeln!(
        out,
        "CRC16(\"123456789\") = 0x{:04X}   (Redis spec: 31C3)",
        crc16_xmodem(b"123456789")
    );
    let _ = writeln!(out, "{:<22} {:<12} {:>5}", "key", "hashed", "slot");
    for key in [
        "somekey",
        "foo{hash_tag}",
        "bar{hash_tag}",
        "{user1000}.following",
        "{user1000}.followers",
        "foo{}{bar}",
        "foo{{bar}}zap",
        "foo{bar}{zap}",
    ] {
        let tag = String::from_utf8_lossy(hash_tag(key.as_bytes())).into_owned();
        let _ = writeln!(
            out,
            "{:<22} {:<12} {:>5}",
            key,
            tag,
            hash_slot(key.as_bytes())
        );
    }
    let three = SlotMap::even(3);
    let (four, moved) = three.add_master();
    let show = |m: &SlotMap| -> String {
        m.ranges()
            .iter()
            .map(|&(a, b, o)| format!("M{o}:{a}-{b}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let _ = writeln!(out, "3 masters: {}", show(&three));
    let _ = writeln!(out, "4 masters: {}", show(&four));
    let sample = key_hashes(seed, KEYS);
    let keys_moved = sample
        .iter()
        .filter(|&&h| three.node_of(h) != four.node_of(h))
        .count();
    let to_new = moved.iter().filter(|&&s| four.owner_of(s) == 3).count();
    let _ = writeln!(
        out,
        "slots moved: {} ({:.2}% of 16384; all to M3: {}); slot counts after: {:?}; keys moved: {:.1}% (min 25.0%)",
        moved.len(),
        pct(moved.len() as f64 / REDIS_SLOTS as f64),
        to_new == moved.len(),
        four.counts(),
        pct(keys_moved as f64 / KEYS as f64)
    );
}

fn section_vnodes(seed: u64, out: &mut String) {
    let n = 10;
    let rings = 20;
    let hashes = key_hashes(seed, KEYS);
    let _ = writeln!(
        out,
        "-- (3) load balance, N={n} nodes, {KEYS} keys, mean over {rings} rings (max/mean: 1.00 is perfect) --"
    );
    let _ = writeln!(
        out,
        "{:<14} {:>9} {:>9} {:>10}",
        "scheme", "max/mean", "worst", "std/mean%"
    );
    for vnodes in [1u32, 10, 100, 1000] {
        let (mut sum, mut worst, mut std) = (0.0, 0.0f64, 0.0);
        for r in 0..rings {
            let ring = Ring::new(n, vnodes, seed.wrapping_add(r));
            let l = LoadStats::of_keys(&ring, &hashes);
            sum += l.max_over_mean();
            worst = worst.max(l.max_over_mean());
            std += l.rel_std();
        }
        let _ = writeln!(
            out,
            "{:<14} {:>9.3} {:>9.3} {:>10.2}",
            format!("ring v={vnodes}"),
            sum / rings as f64,
            worst,
            pct(std / rings as f64)
        );
    }
    for (name, p) in [
        (
            "fixed P=1024",
            &FixedPartitions::new(PARTITIONS, n) as &dyn Placement,
        ),
        ("jump", &Jump::new(n)),
    ] {
        let l = LoadStats::of_keys(p, &hashes);
        let _ = writeln!(
            out,
            "{:<14} {:>9.3} {:>9} {:>10.2}",
            name,
            l.max_over_mean(),
            "-",
            pct(l.rel_std())
        );
    }
}

fn section_hot(seed: u64, out: &mut String) {
    let (keys, s, requests) = (10_000usize, 1.0, 200_000u64);
    let z = Zipf::new(keys, s);
    let _ = writeln!(
        out,
        "-- (4) Zipf(s={s}) over {keys} keys, {requests} requests; fixed partitions; hot key has P={:.1}% --",
        pct(z.prob(0))
    );
    let _ = writeln!(
        out,
        "{:>6} {:>12} {:>9}   (no mitigation, 50% writes)",
        "nodes", "hottest node", "max/mean"
    );
    for nodes in [5u32, 10, 20, 40] {
        let r = hot_key_run(seed, nodes, keys, s, requests, 50, HotMitigation::None);
        let _ = writeln!(
            out,
            "{:>6} {:>11.1}% {:>9.2}",
            nodes,
            pct(r.load.max_share()),
            r.load.max_over_mean()
        );
    }
    let _ = writeln!(
        out,
        "N=40, protect the top H keys with S=16 variants/copies; cell = max/mean | node requests per client request"
    );
    let _ = writeln!(
        out,
        "{:>3} {:>16} {:>16}   {:>16} {:>16}",
        "H", "90%w suffix", "10%w copies", "90%w copies(!)", "10%w suffix(!)"
    );
    for hot in [0usize, 1, 4, 10] {
        let cell = |wp: u64, copies: bool| {
            let m = if copies {
                HotMitigation::Copies { hot, s: 16 }
            } else {
                HotMitigation::Suffix { hot, s: 16 }
            };
            let r = hot_key_run(seed, 40, keys, s, requests, wp, m);
            format!("{:.2} | {:.2}x", r.load.max_over_mean(), r.amplification())
        };
        let _ = writeln!(
            out,
            "{:>3} {:>16} {:>16}   {:>16} {:>16}",
            hot,
            cell(90, false),
            cell(10, true),
            cell(90, true),
            cell(10, false)
        );
    }
    let sweep: Vec<String> = [16u32, 64, 256]
        .iter()
        .map(|&sv| {
            let r = hot_key_run(
                seed,
                40,
                keys,
                s,
                requests,
                90,
                HotMitigation::Suffix { hot: 10, s: sv },
            );
            format!(
                "S={sv}: {:.2} | {:.2}x",
                r.load.max_over_mean(),
                r.amplification()
            )
        })
        .collect();
    let _ = writeln!(
        out,
        "H=10, 90%w, suffix, more variants -> {}",
        sweep.join("   ")
    );
}

fn section_range(_seed: u64, out: &mut String) {
    let (parts, space, writes, base) = (16u64, 1u64 << 32, 100_000u64, 1_000_000u64);
    let _ = writeln!(
        out,
        "-- (5) {writes} writes with increasing keys, {parts} partitions --"
    );
    let mut range_counts = vec![0u64; parts as usize];
    let mut hash_counts = vec![0u64; parts as usize];
    for k in base..base + writes {
        range_counts[static_range_partition(k, parts, space) as usize] += 1;
        hash_counts[(hash64(&k.to_be_bytes()) % parts) as usize] += 1;
    }
    let share = |c: &[u64]| *c.iter().max().unwrap_or(&0) as f64 / writes as f64;
    let _ = writeln!(
        out,
        "static ranges: busiest partition takes {:.1}% of writes; hash: {:.1}% (even would be {:.1}%)",
        pct(share(&range_counts)),
        pct(share(&hash_counts)),
        pct(1.0 / parts as f64)
    );
    let mut table = SplitRangeTable::new(1000);
    let ids: Vec<u64> = (base..base + writes).map(|k| table.insert(k)).collect();
    let hash_ids: Vec<u64> = (base..base + writes)
        .map(|k| hash64(&k.to_be_bytes()) % parts)
        .collect();
    let _ = writeln!(
        out,
        "auto-split ranges (cap 1000): {} partitions at the end; in each window of 100 consecutive writes the busiest partition takes {:.1}% (hash: {:.1}%)",
        table.partitions(),
        pct(mean_busiest_share(&ids, 100)),
        pct(mean_busiest_share(&hash_ids, 100))
    );
    let (lo, hi) = (base + 500, base + 600);
    let _ = writeln!(
        out,
        "scan of {} consecutive keys: range touches {} partition(s), hash touches {} of {}",
        hi - lo,
        range_scan_partitions(lo, hi, parts, space),
        hash_scan_partitions(lo, hi, parts),
        parts
    );
}

/// Full text printed by the `partitioning` binary. `only` selects one section (1..=5).
pub fn report(seed: u64, only: Option<u32>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "== Lab (c) partitioning, seed {seed} ==");
    type Section = fn(u64, &mut String);
    let sections: [(u32, Section); 5] = [
        (1, section_moves),
        (2, section_redis),
        (3, section_vnodes),
        (4, section_hot),
        (5, section_range),
    ];
    for (id, f) in sections {
        if only.is_none_or(|o| o == id) {
            f(seed, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hashes(count: usize, tag: &str) -> Vec<u64> {
        (0..count)
            .map(|i| hash64(format!("{tag}:{i}").as_bytes()))
            .collect()
    }

    // ---- the lab's hash: the "data contract" of chapter 06 section 8.2 ----

    /// Pinned outputs. If any of these changes, every stored key would map to a different partition.
    #[test]
    fn hash64_and_partition_vectors_are_pinned() {
        let vectors: [(&str, u64, u64); 5] = [
            ("", 0xefd0_1f60_ba99_2926, 294),
            ("a", 0x82a2_a958_a9be_ce5b, 603),
            ("user:42", 0x9988_17be_e671_8248, 584),
            ("user:1000", 0x71fc_27d0_40be_722c, 556),
            ("order:2026-10-12", 0x4a58_826f_ea5c_f40c, 12),
        ];
        let fixed = FixedPartitions::new(1024, 4);
        for (key, h, p) in vectors {
            assert_eq!(hash64(key.as_bytes()), h, "hash of {key:?}");
            assert_eq!(u64::from(fixed.partition_of(h)), p, "partition of {key:?}");
        }
        assert_eq!(hash64(&42u64.to_be_bytes()), 0x641d_ede4_f097_3e8c);
    }

    #[test]
    fn hash64_low_bits_are_uniform_for_sequential_keys() {
        // Sequential keys are the worst case for a weak hash; check hash % 16 over 160k keys.
        let mut counts = [0u32; 16];
        for h in hashes(160_000, "seq") {
            counts[(h % 16) as usize] += 1;
        }
        // Expected 10_000 per bucket; std of a binomial is about 97, so +-500 is > 5 sigma.
        for c in counts {
            assert!((9_500..=10_500).contains(&c), "bucket {c}");
        }
    }

    // ---- Redis Cluster ----

    #[test]
    fn crc16_matches_the_redis_spec_vector() {
        assert_eq!(crc16_xmodem(b"123456789"), 0x31C3);
        assert_eq!(crc16_xmodem(b""), 0);
    }

    #[test]
    fn hash_slot_matches_the_examples_in_the_redis_docs() {
        // CLUSTER KEYSLOT examples from the Redis command documentation.
        assert_eq!(hash_slot(b"somekey"), 11058);
        assert_eq!(hash_slot(b"foo{hash_tag}"), 2515);
        assert_eq!(hash_slot(b"bar{hash_tag}"), 2515);
        // Every hash-tag example of the Redis Cluster specification.
        assert_eq!(
            hash_slot(b"{user1000}.following"),
            hash_slot(b"{user1000}.followers")
        );
        assert_eq!(hash_tag(b"{user1000}.following"), b"user1000");
        assert_eq!(hash_tag(b"foo{}{bar}"), b"foo{}{bar}"); // empty tag: whole key
        assert_eq!(hash_tag(b"foo{{bar}}zap"), b"{bar");
        assert_eq!(hash_tag(b"foo{bar}{zap}"), b"bar");
        assert_eq!(hash_tag(b"{}anything{tag}"), b"{}anything{tag}"); // starts with {}: whole key
        // More edge cases of the rules.
        assert_eq!(hash_tag(b"no-braces"), b"no-braces");
        assert_eq!(hash_tag(b"open{only"), b"open{only");
        assert_eq!(hash_tag(b"}close{only"), b"}close{only");
        assert_eq!(hash_tag(b""), b"");
        assert_eq!(hash_slot(b"foo{{bar}}zap"), hash_slot(b"{bar"));
        assert_eq!(hash_slot(b"foo{bar}{zap}"), hash_slot(b"bar"));
    }

    #[test]
    fn slots_are_in_range_and_cover_the_space() {
        let mut seen = vec![false; REDIS_SLOTS];
        for i in 0..200_000u32 {
            let s = hash_slot(format!("key:{i}").as_bytes()) as usize;
            assert!(s < REDIS_SLOTS);
            seen[s] = true;
        }
        let covered = seen.iter().filter(|&&x| x).count();
        assert!(
            covered > 16_000,
            "only {covered} of 16384 slots used by 200k keys"
        );
    }

    #[test]
    fn slot_map_even_split_and_minimum_move_join() {
        let three = SlotMap::even(3);
        assert_eq!(three.counts(), vec![5462, 5461, 5461]);
        assert_eq!(
            three.ranges(),
            vec![(0, 5461, 0), (5462, 10922, 1), (10923, 16383, 2)]
        );
        let (four, moved) = three.add_master();
        assert_eq!(moved.len(), REDIS_SLOTS / 4); // the minimum: the new master's fair share
        assert!(moved.iter().all(|&s| four.owner_of(s) == 3));
        assert_eq!(four.counts(), vec![4096; 4]);
        // Slots that did not move kept their owner.
        for s in 0..REDIS_SLOTS as u16 {
            if !moved.contains(&s) {
                assert_eq!(four.owner_of(s), three.owner_of(s));
            }
        }
        // 4 -> 5 masters: 16384/5 = 3276 slots move, counts stay within 1.
        let (five, moved5) = four.add_master();
        assert_eq!(moved5.len(), 3276);
        let c = five.counts();
        assert!(
            c.iter().max().unwrap() - c.iter().min().unwrap() <= 1,
            "{c:?}"
        );
    }

    // ---- rebalancing: keys moved when the node count changes ----

    #[test]
    fn mod_n_moves_n_over_n_plus_one_of_the_keys() {
        let hs = hashes(100_000, "modn");
        for n in 2..=10u32 {
            let s = move_stats(&ModN::new(n), &ModN::new(n + 1), &hs);
            let expect = f64::from(n) / f64::from(n + 1);
            assert!(
                (s.fraction() - expect).abs() < 0.02,
                "N={n}: moved {:.4}, expected {expect:.4}",
                s.fraction()
            );
        }
        // The example of the Jump Consistent Hash paper: 10 -> 12 shards moves ~5/6, not 1/6.
        let s = move_stats(&ModN::new(10), &ModN::new(12), &hs);
        assert!(s.fraction() >= 0.8, "{}", s.fraction());
        // ... and many of those keys move between two nodes that both existed before (wasted).
        assert!(s.moved_between_old() as f64 > 0.5 * hs.len() as f64);
    }

    #[test]
    fn consistent_schemes_move_about_one_over_n_plus_one_and_only_to_the_new_node() {
        let hs = hashes(100_000, "cons");
        for n in 2..=10u32 {
            let minimum = 1.0 / f64::from(n + 1);
            let fixed = FixedPartitions::new(1024, n);
            let fixed_grown = fixed.grown_to(n + 1);
            let ring_a = Ring::new(n, 1000, 7);
            let ring_b = Ring::new(n + 1, 1000, 7);
            let cases: [(&str, MoveStats, f64); 3] = [
                // Fixed: exactly floor(1024/(N+1)) partitions move; keys per partition vary a bit.
                ("fixed", move_stats(&fixed, &fixed_grown, &hs), 0.012),
                // Jump: the expected fraction is exactly 1/(N+1); sampling noise is ~0.001.
                (
                    "jump",
                    move_stats(&Jump::new(n), &Jump::new(n + 1), &hs),
                    0.01,
                ),
                // Ring: the new node's arc share is random; with v vnodes its relative std is about
                // 1/sqrt(v), so allow 5 sigma = 5 * minimum / sqrt(1000).
                (
                    "ring",
                    move_stats(&ring_a, &ring_b, &hs),
                    5.0 * minimum / 1000f64.sqrt(),
                ),
            ];
            for (name, s, tol) in cases {
                assert!(
                    (s.fraction() - minimum).abs() < tol,
                    "{name} N={n}: moved {:.4}, minimum {minimum:.4}",
                    s.fraction()
                );
                // Monotonicity (Karger): no key moves between two OLD nodes.
                assert_eq!(s.moved_between_old(), 0, "{name} N={n}");
                assert!(s.moved > 0);
            }
        }
    }

    #[test]
    fn ring_monotonicity_holds_for_many_salts_and_sizes() {
        let hs = hashes(20_000, "salts");
        for salt in 0..30u64 {
            for n in [1u32, 2, 5, 9] {
                let s = move_stats(&Ring::new(n, 50, salt), &Ring::new(n + 1, 50, salt), &hs);
                assert_eq!(s.moved_between_old(), 0, "salt {salt} N={n}");
                assert_eq!(s.moved, s.moved_to_new);
            }
        }
    }

    #[test]
    fn growing_by_two_nodes_keeps_monotonicity() {
        let hs = hashes(50_000, "grow2");
        let fixed = FixedPartitions::new(1024, 10);
        type Pair = (&'static str, Box<dyn Placement>, Box<dyn Placement>);
        let after: [Pair; 3] = [
            (
                "fixed",
                Box::new(fixed.clone()),
                Box::new(fixed.grown_to(12)),
            ),
            (
                "ring",
                Box::new(Ring::new(10, 200, 3)),
                Box::new(Ring::new(12, 200, 3)),
            ),
            ("jump", Box::new(Jump::new(10)), Box::new(Jump::new(12))),
        ];
        for (name, a, b) in after {
            let s = move_stats(a.as_ref(), b.as_ref(), &hs);
            assert_eq!(s.moved_between_old(), 0, "{name}");
            assert!(
                (s.fraction() - 1.0 / 6.0).abs() < 0.04,
                "{name}: {}",
                s.fraction()
            );
        }
    }

    #[test]
    fn fixed_partitions_never_change_a_keys_partition_and_stay_balanced() {
        let hs = hashes(20_000, "fp");
        let mut fp = FixedPartitions::new(1024, 3);
        let before: Vec<u32> = hs.iter().map(|&h| fp.partition_of(h)).collect();
        for n in 3..12u32 {
            let moved = fp.add_node();
            assert_eq!(
                moved,
                1024 / (n as usize + 1),
                "partitions moved joining node {n}"
            );
            let mut counts = vec![0usize; fp.nodes() as usize];
            for &o in fp.owners() {
                counts[o as usize] += 1;
            }
            let (mn, mx) = (counts.iter().min().unwrap(), counts.iter().max().unwrap());
            assert!(mx - mn <= 1, "unbalanced: {counts:?}");
        }
        let after: Vec<u32> = hs.iter().map(|&h| fp.partition_of(h)).collect();
        assert_eq!(before, after);
        assert_eq!(fp.partitions(), 1024);
    }

    #[test]
    fn jump_hash_is_in_range_and_only_jumps_to_the_new_bucket() {
        for key in hashes(5_000, "jh") {
            assert_eq!(jump_hash(key, 1), 0);
            let mut prev = jump_hash(key, 1);
            for n in 2..=64u32 {
                let b = jump_hash(key, n);
                assert!(b < n);
                assert!(
                    b == prev || b == n - 1,
                    "key {key:#x}: {prev} -> {b} at n={n}"
                );
                prev = b;
            }
        }
    }

    #[test]
    fn ring_ownership_sums_to_one_and_matches_key_shares() {
        let ring = Ring::new(5, 200, 1);
        let own = ring.ownership();
        assert!((own.iter().sum::<f64>() - 1.0).abs() < 1e-9);
        let l = LoadStats::of_keys(&ring, &hashes(100_000, "own"));
        for (i, &o) in own.iter().enumerate() {
            let share = l.per_node[i] as f64 / 100_000.0;
            assert!(
                (share - o).abs() < 0.01,
                "node {i}: arc {o:.4} vs keys {share:.4}"
            );
        }
    }

    // ---- load balance ----

    #[test]
    fn more_vnodes_means_better_balance() {
        let hs = hashes(100_000, "vn");
        let avg = |v: u32| -> f64 {
            (0..10u64)
                .map(|salt| LoadStats::of_keys(&Ring::new(10, v, salt), &hs).max_over_mean())
                .sum::<f64>()
                / 10.0
        };
        let (a, b, c, d) = (avg(1), avg(10), avg(100), avg(1000));
        assert!(a > b && b > c && c > d, "{a} {b} {c} {d}");
        assert!(
            a > 1.8,
            "a single point per node should be badly unbalanced: {a}"
        );
        assert!(d < 1.12, "1000 vnodes should be within ~10%: {d}");
        // Chapter 06: relative std of node load is about 1/sqrt(vnodes).
        let std1000 = (0..10u64)
            .map(|salt| LoadStats::of_keys(&Ring::new(10, 1000, salt), &hs).rel_std())
            .sum::<f64>()
            / 10.0;
        assert!((0.015..0.045).contains(&std1000), "{std1000}");
    }

    #[test]
    fn fixed_partitions_and_jump_balance_uniform_keys() {
        let hs = hashes(100_000, "bal");
        let n = 10;
        let f = LoadStats::of_keys(&FixedPartitions::new(1024, n), &hs);
        let j = LoadStats::of_keys(&Jump::new(n), &hs);
        assert!(f.max_over_mean() < 1.05, "{}", f.max_over_mean());
        assert!(j.max_over_mean() < 1.05, "{}", j.max_over_mean());
        assert_eq!(f.total(), 100_000);
    }

    // ---- skew ----

    #[test]
    fn zipf_sampler_matches_its_distribution() {
        let z = Zipf::new(10_000, 1.0);
        assert!((z.prob(0) - 1.0 / 9.787_606).abs() < 1e-4); // 1 / H_10000
        let mut rng = Rng::new(5);
        let mut counts = [0u32; 4];
        let n = 200_000;
        for _ in 0..n {
            let r = z.sample(&mut rng);
            assert!(r < 10_000);
            if r < 4 {
                counts[r] += 1;
            }
        }
        for (r, &c) in counts.iter().enumerate() {
            let got = f64::from(c) / f64::from(n);
            assert!(
                (got - z.prob(r)).abs() < 0.004,
                "rank {r}: {got} vs {}",
                z.prob(r)
            );
        }
        assert!(counts[0] > counts[1] && counts[1] > counts[2] && counts[2] > counts[3]);
    }

    #[test]
    fn a_single_hot_key_pins_its_node_whatever_the_cluster_size() {
        let (keys, reqs) = (10_000, 100_000);
        for seed in 1..=5u64 {
            let mut prev = 0.0;
            for nodes in [5u32, 10, 20, 40] {
                let r = hot_key_run(seed, nodes, keys, 1.0, reqs, 50, HotMitigation::None);
                // The hottest node holds at least the hot key: its share cannot go below the hot key's.
                assert!(
                    r.load.max_share() >= r.top_key_share,
                    "N={nodes}: node share {} < key share {}",
                    r.load.max_share(),
                    r.top_key_share
                );
                assert!(r.top_key_share > 0.09);
                // So max/mean = N * share keeps growing: adding nodes does not cure it.
                assert!(r.load.max_over_mean() > prev, "N={nodes}");
                prev = r.load.max_over_mean();
            }
            assert!(prev > 4.0, "{prev}");
        }
    }

    #[test]
    fn suffixing_or_copies_spread_the_hot_key_for_the_right_mix() {
        let none = hot_key_run(1, 40, 10_000, 1.0, 100_000, 90, HotMitigation::None);
        let suffix = hot_key_run(
            1,
            40,
            10_000,
            1.0,
            100_000,
            90,
            HotMitigation::Suffix { hot: 4, s: 16 },
        );
        assert!(suffix.load.max_over_mean() < 0.6 * none.load.max_over_mean());
        assert!(suffix.load.max_share() < none.load.max_share() * 0.6);
        // Write-heavy mix: cheap. Same mitigation on a read-heavy mix: reads fan out to S nodes.
        let wrong = hot_key_run(
            1,
            40,
            10_000,
            1.0,
            100_000,
            10,
            HotMitigation::Suffix { hot: 4, s: 16 },
        );
        assert!(suffix.amplification() < 1.6);
        assert!(wrong.amplification() > 3.0);
        // Copies are the mirror image.
        let copies_read = hot_key_run(
            1,
            40,
            10_000,
            1.0,
            100_000,
            10,
            HotMitigation::Copies { hot: 4, s: 16 },
        );
        assert!(copies_read.load.max_over_mean() < 0.6 * none.load.max_over_mean());
        assert!(copies_read.amplification() < 1.6);
    }

    #[test]
    fn read_fan_out_cost_is_exactly_s_per_hot_read() {
        // 100% reads, one hot key with S=8 suffixes: every read of rank 0 costs 8 node requests.
        let r = hot_key_run(
            9,
            10,
            1_000,
            1.0,
            50_000,
            0,
            HotMitigation::Suffix { hot: 1, s: 8 },
        );
        let hot_reads = (r.top_key_share * 50_000.0).round() as u64;
        assert_eq!(r.load.total(), 50_000 + hot_reads * 7);
        // 100% writes: no amplification at all.
        let w = hot_key_run(
            9,
            10,
            1_000,
            1.0,
            50_000,
            100,
            HotMitigation::Suffix { hot: 1, s: 8 },
        );
        assert_eq!(w.load.total(), 50_000);
    }

    // ---- range vs hash for monotonic keys ----

    #[test]
    fn range_partitioning_concentrates_monotonic_writes() {
        let (parts, space, base) = (16u64, 1u64 << 32, 1_000_000u64);
        let mut range = [0u64; 16];
        let mut hash = [0u64; 16];
        for k in base..base + 100_000 {
            range[static_range_partition(k, parts, space) as usize] += 1;
            hash[(hash64(&k.to_be_bytes()) % parts) as usize] += 1;
        }
        assert!(*range.iter().max().unwrap() >= 90_000);
        assert!(*hash.iter().max().unwrap() < 7_000); // mean is 6250
        // A short scan: 1 partition (2 if it straddles a boundary) vs all 16 for hash.
        assert!(range_scan_partitions(base + 500, base + 600, parts, space) <= 2);
        assert_eq!(hash_scan_partitions(base + 500, base + 600, parts), 16);
        // A scan straddling a boundary touches exactly 2.
        let boundary = space.div_ceil(parts);
        assert_eq!(
            range_scan_partitions(boundary - 5, boundary + 5, parts, space),
            2
        );
    }

    #[test]
    fn auto_splitting_ranges_do_not_cure_monotonic_hot_spots_but_do_help_random_keys() {
        let mut mono = SplitRangeTable::new(1000);
        let mono_ids: Vec<u64> = (0..100_000u64)
            .map(|k| mono.insert(1_000_000 + k))
            .collect();
        assert!(mono.partitions() > 100);
        let share = mean_busiest_share(&mono_ids, 100);
        assert!(share >= 0.9, "monotonic keys: busiest share {share}");

        let mut rnd = SplitRangeTable::new(1000);
        let mut rng = Rng::new(3);
        let rnd_ids: Vec<u64> = (0..100_000)
            .map(|_| rnd.insert(rng.below(1 << 32)))
            .collect();
        let share = mean_busiest_share(&rnd_ids, 100);
        assert!(share < 0.2, "random keys: busiest share {share}");
        // Bookkeeping: all keys are stored, partitions stay <= cap.
        let stored: usize = rnd.parts.values().map(Vec::len).sum();
        assert_eq!(stored, 100_000);
        assert!(rnd.parts.values().all(|v| v.len() <= 1000));
    }

    // ---- the report ----

    #[test]
    fn report_is_a_pure_function_of_the_seed() {
        assert_eq!(report(11, Some(2)), report(11, Some(2)));
        assert_eq!(report(11, Some(5)), report(11, Some(5)));
        assert_ne!(report(11, Some(3)), report(12, Some(3)));
        assert!(report(1, Some(2)).contains("0x31C3"));
    }
}
