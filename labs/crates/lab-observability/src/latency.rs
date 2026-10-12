//! A service-like workload for chapter 03: per-request latency recorded in an HDR histogram,
//! with and without two deliberate inefficiencies.
//!
//! A "request" is a lookup in a shared table plus a JSON-ish response. One request in
//! `heavy_every` is a "report" that also burns CPU. Two variants serve the same requests:
//!
//! * [`Variant::Naive`] takes one coarse lock for the whole request, burns the report's CPU
//!   *while holding it*, copies the value out with `to_string()` and builds the response with
//!   `format!`: head-of-line blocking plus allocations on the hot path.
//! * [`Variant::Tuned`] holds the lock only for the hash lookup, does the heavy work outside
//!   it, clones an `Arc<str>` instead of a `String`, and writes the response into a reused,
//!   pre-sized buffer.
//!
//! Both return the same checksum for the same request id (the tests pin that down), so the only
//! thing that differs is *how* the answer is produced — and so what the percentiles look like.

use crate::alloc::{self, Stats};
use hdrhistogram::Histogram;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::hint::black_box;
use parking_lot::Mutex;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

/// Number of distinct keys in the shared table.
pub const KEYS: u64 = 1024;

/// SplitMix64: a cheap, well-mixed deterministic hash.
pub fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// Deterministic CPU work: `iters` xorshift rounds. The result depends on both arguments, so the
/// optimizer cannot fold it away, and `black_box` stops it deleting the loop.
#[inline(never)]
pub fn burn(seed: u64, iters: u64) -> u64 {
    let mut x = seed | 1;
    for _ in 0..iters {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
    }
    black_box(x)
}

/// FNV-1a over the response body: cheap, and consumes every byte.
fn checksum(body: &[u8]) -> u64 {
    body.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Naive,
    Tuned,
}

impl Variant {
    pub fn name(self) -> &'static str {
        match self {
            Variant::Naive => "naive",
            Variant::Tuned => "tuned",
        }
    }

    pub fn parse(s: &str) -> Option<Variant> {
        match s {
            "naive" => Some(Variant::Naive),
            "tuned" => Some(Variant::Tuned),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub threads: usize,
    /// Requests each thread issues (and records).
    pub requests_per_thread: u64,
    /// Requests each thread issues first and does not record: warm caches, page-in the table,
    /// let the allocator settle (chapter 03 §1).
    pub warmup_per_thread: u64,
    /// Every `heavy_every`-th request id is a "report" (0 = none).
    pub heavy_every: u64,
    /// CPU burned by a report, in [`burn`] iterations.
    pub heavy_iters: u64,
}

/// Per-thread reusable state of the tuned variant: the response buffer.
pub struct Scratch(String);

impl Scratch {
    pub fn new() -> Scratch {
        // Pre-sized so that even the first request does not allocate (`Vec::with_capacity`, chapter 03 §6).
        Scratch(String::with_capacity(256))
    }
}

impl Default for Scratch {
    fn default() -> Scratch {
        Scratch::new()
    }
}

/// The shared state: a table behind one lock.
pub struct Service {
    variant: Variant,
    heavy_every: u64,
    heavy_iters: u64,
    table: Mutex<HashMap<u64, Arc<str>>>,
}

impl Service {
    pub fn new(variant: Variant, heavy_every: u64, heavy_iters: u64) -> Service {
        let table = (0..KEYS).map(|k| (k, Arc::from(format!("{:016x}", mix(k) ^ k)))).collect();
        Service { variant, heavy_every, heavy_iters, table: Mutex::new(table) }
    }

    fn is_heavy(&self, id: u64) -> bool {
        self.heavy_every != 0 && id.is_multiple_of(self.heavy_every)
    }

    /// Serves request `id` and returns a checksum of the response.
    pub fn handle(&self, id: u64, scratch: &mut Scratch) -> u64 {
        match self.variant {
            Variant::Naive => self.handle_naive(id),
            Variant::Tuned => self.handle_tuned(id, scratch),
        }
    }

    fn handle_naive(&self, id: u64) -> u64 {
        let table = self.table.lock(); // held until the end of the function
        let key = mix(id) % KEYS;
        let value = table[&key].to_string(); // an allocation
        let report = if self.is_heavy(id) { burn(id, self.heavy_iters) } else { 0 }; // CPU under the lock
        let body = format!("{{\"id\":{id},\"key\":{key},\"value\":\"{value}\",\"report\":{report}}}"); // allocations
        checksum(body.as_bytes())
    }

    fn handle_tuned(&self, id: u64, scratch: &mut Scratch) -> u64 {
        let key = mix(id) % KEYS;
        let value = Arc::clone(&self.table.lock()[&key]); // lock held for the lookup only
        let report = if self.is_heavy(id) { burn(id, self.heavy_iters) } else { 0 }; // CPU outside the lock
        scratch.0.clear();
        write!(scratch.0, "{{\"id\":{id},\"key\":{key},\"value\":\"{value}\",\"report\":{report}}}").unwrap();
        checksum(scratch.0.as_bytes())
    }
}

/// What one run measured.
pub struct Report {
    pub variant: Variant,
    pub requests: u64,
    /// Latency of every recorded request, in nanoseconds.
    pub hist: Histogram<u64>,
    /// Wall time of the slowest thread's measured loop.
    pub wall: Duration,
    /// Wrapping sum of the per-request checksums; independent of thread scheduling.
    pub checksum: u64,
    /// Allocator calls made by the worker threads during the measured loops (all zero unless the
    /// binary installed [`alloc::CountingAlloc`]).
    pub allocs: Stats,
}

/// A histogram for latencies from 1 ns to 60 s with 3 significant digits (relative error below 0.1%).
pub fn new_histogram() -> Histogram<u64> {
    Histogram::new_with_bounds(1, 60_000_000_000, 3).expect("valid histogram bounds")
}

/// Runs `cfg.threads` workers against one shared [`Service`]; thread `t` serves the ids
/// `t, t + threads, t + 2*threads, ...`, so the set of ids is the same for any thread count.
pub fn run(cfg: Config, variant: Variant) -> Report {
    let service = Arc::new(Service::new(variant, cfg.heavy_every, cfg.heavy_iters));
    let start = Arc::new(Barrier::new(cfg.threads));
    let workers: Vec<_> = (0..cfg.threads as u64)
        .map(|t| {
            let (service, start) = (Arc::clone(&service), Arc::clone(&start));
            std::thread::Builder::new()
                .name(format!("worker-{t}"))
                .spawn(move || {
                    let stride = cfg.threads as u64;
                    let mut scratch = Scratch::new();
                    let mut hist = new_histogram();
                    // Warm-up ids come from a range the measured ids never use.
                    for i in 0..cfg.warmup_per_thread {
                        black_box(service.handle(black_box(u64::MAX / 2 + i * stride + t), &mut scratch));
                    }
                    start.wait();
                    let began = Instant::now();
                    let (sum, stats) = alloc::measure(|| {
                        let mut sum = 0u64;
                        for i in 0..cfg.requests_per_thread {
                            let id = black_box(i * stride + t);
                            let t0 = Instant::now();
                            let c = service.handle(id, &mut scratch);
                            let ns = t0.elapsed().as_nanos() as u64;
                            hist.saturating_record(ns.max(1));
                            sum = sum.wrapping_add(c);
                        }
                        sum
                    });
                    (hist, sum, stats, began.elapsed())
                })
                .expect("spawn worker")
        })
        .collect();

    let mut hist = new_histogram();
    let (mut checksum, mut allocs, mut wall) = (0u64, Stats::default(), Duration::ZERO);
    for w in workers {
        let (h, sum, stats, took) = w.join().expect("worker panicked");
        hist.add(&h).expect("histograms share bounds");
        checksum = checksum.wrapping_add(sum);
        allocs = Stats {
            allocs: allocs.allocs + stats.allocs,
            reallocs: allocs.reallocs + stats.reallocs,
            frees: allocs.frees + stats.frees,
            bytes: allocs.bytes + stats.bytes,
        };
        wall = wall.max(took);
    }
    Report { variant, requests: cfg.requests_per_thread * cfg.threads as u64, hist, wall, checksum, allocs }
}

/// The distribution in the numbers a service owner reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    pub count: u64,
    pub mean: f64,
    pub p50: u64,
    pub p99: u64,
    pub p999: u64,
    pub max: u64,
}

pub fn summarize(h: &Histogram<u64>) -> Summary {
    Summary {
        count: h.len(),
        mean: h.mean(),
        p50: h.value_at_quantile(0.50),
        p99: h.value_at_quantile(0.99),
        p999: h.value_at_quantile(0.999),
        max: h.max(),
    }
}

/// Nearest-rank quantile of an ascending slice: the value at rank `ceil(q * n)` (at least 1).
/// This is the rule `hdrhistogram`'s `value_at_quantile` follows, so the two can be compared.
pub fn exact_quantile(sorted: &[u64], q: f64) -> u64 {
    assert!(!sorted.is_empty() && (0.0..=1.0).contains(&q));
    let rank = ((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(variant: Variant, heavy_every: u64) -> Service {
        Service::new(variant, heavy_every, 200)
    }

    /// Differential test: the optimization must not change any answer, heavy or not.
    #[test]
    fn naive_and_tuned_give_identical_responses() {
        let (naive, tuned) = (small(Variant::Naive, 7), small(Variant::Tuned, 7));
        let mut scratch = Scratch::new();
        for id in (0..3000).chain([u64::MAX, u64::MAX / 2, 1 << 40]) {
            assert_eq!(naive.handle(id, &mut Scratch::new()), tuned.handle(id, &mut scratch), "id = {id}");
        }
    }

    /// A report request really differs from a plain one (otherwise the "heavy" work is dead code).
    #[test]
    fn heavy_requests_change_the_response() {
        let with = small(Variant::Tuned, 5);
        let without = small(Variant::Tuned, 0);
        let mut s = Scratch::new();
        assert_ne!(with.handle(10, &mut s), without.handle(10, &mut s), "id 10 is a report when heavy_every = 5");
        assert_eq!(with.handle(11, &mut s), without.handle(11, &mut s), "id 11 is a plain lookup");
    }

    /// Chapter 03 §6, as a counter-example pair: after warm-up the tuned handler asks the
    /// allocator for nothing; the naive handler asks at least once per request.
    #[test]
    fn tuned_handler_is_allocation_free_and_naive_is_not() {
        let (naive, tuned) = (small(Variant::Naive, 10), small(Variant::Tuned, 10));
        let mut scratch = Scratch::new();
        std::hint::black_box(tuned.handle(u64::MAX, &mut scratch));
        let (_, t) = alloc::measure(|| (0..200).fold(0u64, |a, id| a ^ tuned.handle(id, &mut scratch)));
        assert_eq!(t.events(), 0, "tuned allocated: {t:?}");
        let (_, n) = alloc::measure(|| (0..200).fold(0u64, |a, id| a ^ naive.handle(id, &mut Scratch::new())));
        // `Scratch::new()` allocates once per call here, but even without it the naive path
        // makes two or more calls (to_string + format!), so >= 200 is a floor, not a tight bound.
        assert!(n.events() >= 200, "naive only made {} allocator calls for 200 requests", n.events());
    }

    /// The whole run is deterministic in everything except time: the checksum equals the sum over
    /// the ids computed on one thread, whatever the thread count and the variant.
    #[test]
    fn run_checksum_is_independent_of_threads_and_variant() {
        let (requests_per_thread_x_threads, heavy_every, heavy_iters) = (4000u64, 100u64, 50u64);
        let reference = {
            let svc = Service::new(Variant::Tuned, heavy_every, heavy_iters);
            let mut s = Scratch::new();
            (0..requests_per_thread_x_threads).fold(0u64, |a, id| a.wrapping_add(svc.handle(id, &mut s)))
        };
        for (threads, variant) in [(1, Variant::Naive), (1, Variant::Tuned), (2, Variant::Tuned), (4, Variant::Naive), (4, Variant::Tuned)] {
            let cfg = Config {
                threads,
                requests_per_thread: requests_per_thread_x_threads / threads as u64,
                warmup_per_thread: 50,
                heavy_every,
                heavy_iters,
            };
            let report = run(cfg, variant);
            assert_eq!(report.checksum, reference, "{} threads, {}", threads, variant.name());
            assert_eq!(report.hist.len(), requests_per_thread_x_threads);
            assert_eq!(report.requests, requests_per_thread_x_threads);
            if variant == Variant::Tuned && threads == 1 {
                // One thread never contends, so nothing but the request path runs in the measured loop.
                assert_eq!(report.allocs.events(), 0, "tuned worker allocated: {:?}", report.allocs);
            } else if variant == Variant::Tuned {
                // With contention the lock library may allocate a small, request-independent amount of
                // bookkeeping the first time threads have to wait (observed with parking_lot, not per request).
                assert!(report.allocs.events() < 64, "tuned workers allocated per request? {:?}", report.allocs);
            } else {
                assert!(report.allocs.events() >= requests_per_thread_x_threads);
            }
        }
    }

    #[test]
    fn nearest_rank_quantiles_on_small_inputs() {
        assert_eq!(exact_quantile(&[5], 0.0), 5);
        assert_eq!(exact_quantile(&[5], 1.0), 5);
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(exact_quantile(&v, 0.0), 1, "rank is clamped to at least 1");
        assert_eq!(exact_quantile(&v, 0.5), 50);
        assert_eq!(exact_quantile(&v, 0.99), 99);
        assert_eq!(exact_quantile(&v, 0.991), 100, "ceil(99.1) = 100");
        assert_eq!(exact_quantile(&v, 1.0), 100);
    }

    /// The histogram's answer stays within its configured precision of the exact sorted answer,
    /// on a heavy-tailed sample (chapter 03 §2).
    #[test]
    fn histogram_quantiles_match_the_sorted_vector_within_precision() {
        let mut x = 0x1234_5678_9abc_def1u64;
        let mut samples: Vec<u64> = (0..100_000)
            .map(|_| {
                x = mix(x);
                // A log-uniform spread over 1 us .. 50 ms: a long right tail.
                let u = (x >> 11) as f64 / (1u64 << 53) as f64;
                (1_000.0 * (50_000.0f64).powf(u)) as u64
            })
            .collect();
        let mut h = new_histogram();
        for &s in &samples {
            h.record(s).unwrap();
        }
        samples.sort_unstable();
        for q in [0.5, 0.9, 0.99, 0.999, 1.0] {
            let (got, exact) = (h.value_at_quantile(q), exact_quantile(&samples, q));
            let rel = (got as f64 - exact as f64).abs() / exact as f64;
            assert!(rel <= 0.002, "q = {q}: histogram {got} vs exact {exact} (relative error {rel:.5})");
        }
        assert_eq!(h.len(), samples.len() as u64);
        assert_eq!(h.min(), samples[0].max(1));
        assert!(h.max() >= *samples.last().unwrap(), "max is reported as the highest equivalent value");
    }

    /// The SRE-book point of chapter 03 §2 as a counter-example: a mean that looks fine next to a
    /// p99 that does not. 98 requests take 1 ms, 2 take 100 ms.
    #[test]
    fn mean_hides_a_tail_that_p99_shows() {
        let mut h = new_histogram();
        for _ in 0..98 {
            h.record(1_000_000).unwrap();
        }
        for _ in 0..2 {
            h.record(100_000_000).unwrap();
        }
        let s = summarize(&h);
        assert_eq!(s.count, 100);
        assert!((s.mean - 2_980_000.0).abs() / 2_980_000.0 < 0.01, "mean = {}", s.mean);
        assert!(s.p50 < 1_100_000, "p50 = {}", s.p50);
        assert!(s.p99 >= 99_000_000, "p99 = {}", s.p99);
        assert!(s.p99 as f64 > 30.0 * s.mean, "p99 should dwarf the mean: p99 {} mean {}", s.p99, s.mean);
        assert!(s.p50 as f64 <= s.mean && s.mean < s.p99 as f64);
    }

    #[test]
    fn variant_names_round_trip() {
        for v in [Variant::Naive, Variant::Tuned] {
            assert_eq!(Variant::parse(v.name()), Some(v));
        }
        assert_eq!(Variant::parse("fast"), None);
    }
}
