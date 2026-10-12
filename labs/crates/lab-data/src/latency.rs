//! Lab (g): latency percentiles and why they are not averages.
//!
//! * [`percentile_sorted`]: the nearest-rank percentile used throughout the lab.
//! * [`Histogram`]: a mergeable log-scale histogram (1% relative error). Percentiles of several
//!   machines cannot be averaged, but their histograms can be added.
//! * [`fan_out`] and [`hedged_fan_out`]: a request that waits for `n` parallel sub-requests, with and
//!   without a hedged duplicate after a delay.
//! * [`coordinated_omission`]: what a load generator that waits for each response does to the numbers
//!   when the server stalls, in virtual time.

use crate::rng::Rng;

/// Rank (1-based) of the nearest-rank percentile `p` among `n` samples: `ceil(p/100 * n)`, computed
/// with a tolerance of 1e-9 so that a product such as `0.9999 * 10001` that is mathematically an
/// integer is not pushed one rank up by rounding error.
fn nearest_rank(p: f64, n: usize) -> usize {
    (((p / 100.0) * n as f64 - 1e-9).ceil() as usize).clamp(1, n)
}

/// Nearest-rank percentile: the smallest sample such that at least `p`% of all samples are `<=` it.
/// `sorted` must be ascending and non-empty.
pub fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    assert!(!sorted.is_empty());
    sorted[nearest_rank(p, sorted.len()) - 1]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub n: usize,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub p999: f64,
    pub max: f64,
}

/// Sorts `samples` in place and summarizes them.
pub fn summarize(samples: &mut [f64]) -> Summary {
    samples.sort_by(f64::total_cmp);
    Summary {
        n: samples.len(),
        mean: samples.iter().sum::<f64>() / samples.len() as f64,
        p50: percentile_sorted(samples, 50.0),
        p95: percentile_sorted(samples, 95.0),
        p99: percentile_sorted(samples, 99.0),
        p999: percentile_sorted(samples, 99.9),
        max: samples[samples.len() - 1],
    }
}

/// Latency of one sub-request in milliseconds: a lognormal body around `median_ms`, plus with
/// probability `p_slow` a stall of about `stall_ms` (a GC pause, a compaction, a retransmit).
#[derive(Debug, Clone, Copy)]
pub struct ServiceModel {
    pub median_ms: f64,
    pub sigma: f64,
    pub p_slow: f64,
    pub stall_ms: f64,
}

impl ServiceModel {
    pub fn sample(&self, rng: &mut Rng) -> f64 {
        let body = self.median_ms * (self.sigma * rng.normal()).exp();
        if rng.chance(self.p_slow) { body + self.stall_ms * (0.8 + 0.4 * rng.f64()) } else { body }
    }
}

const LOW: f64 = 0.01;
const RATIO: f64 = 1.01;
const BUCKETS: usize = 1900; // 0.01 ms * 1.01^1900 is about 1.6e6 ms

/// Histogram with buckets that are 1% wide at every magnitude, from 0.01 ms to over 1000 s.
/// A reported percentile is the upper edge of the bucket that holds the sample, so it is at most 1%
/// above the exact value. Histograms add bucket by bucket, which is what makes them mergeable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Histogram {
    counts: Vec<u64>,
    total: u64,
}

impl Default for Histogram {
    fn default() -> Histogram {
        Histogram { counts: vec![0; BUCKETS], total: 0 }
    }
}

impl Histogram {
    fn bucket(v: f64) -> usize {
        if v <= LOW { 0 } else { (((v / LOW).ln() / RATIO.ln()) as usize).min(BUCKETS - 1) }
    }

    pub fn record(&mut self, v: f64) {
        self.counts[Histogram::bucket(v)] += 1;
        self.total += 1;
    }

    pub fn count(&self) -> u64 {
        self.total
    }

    /// Adds every sample of `other`.
    pub fn add(&mut self, other: &Histogram) {
        for (a, b) in self.counts.iter_mut().zip(&other.counts) {
            *a += b;
        }
        self.total += other.total;
    }

    /// Nearest-rank percentile, reported as the upper edge of its bucket.
    pub fn value_at_percentile(&self, p: f64) -> f64 {
        let rank = nearest_rank(p, self.total as usize) as u64;
        let mut seen = 0;
        for (i, &c) in self.counts.iter().enumerate() {
            seen += c;
            if seen >= rank {
                return LOW * RATIO.powi(i as i32 + 1);
            }
        }
        unreachable!("rank <= total")
    }
}

/// One machine's share of the traffic.
#[derive(Debug, Clone, Copy)]
pub struct Machine {
    pub requests: usize,
    pub p_slow: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Aggregation {
    /// Mean of the machines' own p99.
    pub mean_of_p99: f64,
    /// Mean of the machines' own p99 weighted by their share of the requests.
    pub weighted_mean_of_p99: f64,
    /// p99 of all requests pooled.
    pub pooled_p99: f64,
    /// p99 read from the sum of the machines' histograms.
    pub merged_histogram_p99: f64,
}

/// Why "average the p99s" is wrong, and what is right.
pub fn aggregate_percentiles(base: ServiceModel, machines: &[Machine], rng: &mut Rng) -> Aggregation {
    let total: usize = machines.iter().map(|m| m.requests).sum();
    let mut pooled = Vec::with_capacity(total);
    let mut merged = Histogram::default();
    let (mut sum_p99, mut weighted) = (0.0, 0.0);
    for m in machines {
        let model = ServiceModel { p_slow: m.p_slow, ..base };
        let mut samples: Vec<f64> = (0..m.requests).map(|_| model.sample(rng)).collect();
        let mut h = Histogram::default();
        samples.iter().for_each(|&v| h.record(v));
        merged.add(&h);
        samples.sort_by(f64::total_cmp);
        let p99 = percentile_sorted(&samples, 99.0);
        sum_p99 += p99;
        weighted += p99 * m.requests as f64 / total as f64;
        pooled.extend(samples);
    }
    pooled.sort_by(f64::total_cmp);
    Aggregation {
        mean_of_p99: sum_p99 / machines.len() as f64,
        weighted_mean_of_p99: weighted,
        pooled_p99: percentile_sorted(&pooled, 99.0),
        merged_histogram_p99: merged.value_at_percentile(99.0),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FanOutRow {
    pub n: usize,
    /// Fraction of requests whose latency exceeds the threshold, measured.
    pub slow_measured: f64,
    /// `1 - (1 - p)^n` with `p` the measured fraction of slow sub-requests.
    pub slow_formula: f64,
    pub p50: f64,
    pub p99: f64,
}

/// Fraction of sub-requests above `threshold_ms`, measured on `samples` draws.
pub fn slow_fraction(model: ServiceModel, threshold_ms: f64, samples: usize, rng: &mut Rng) -> f64 {
    (0..samples).filter(|_| model.sample(rng) > threshold_ms).count() as f64 / samples as f64
}

/// A request that finishes when the slowest of `n` parallel sub-requests does.
pub fn fan_out(model: ServiceModel, n: usize, requests: usize, threshold_ms: f64, p_sub: f64, rng: &mut Rng) -> FanOutRow {
    let mut latencies: Vec<f64> = (0..requests)
        .map(|_| (0..n).map(|_| model.sample(rng)).fold(0.0, f64::max))
        .collect();
    let slow = latencies.iter().filter(|&&l| l > threshold_ms).count() as f64 / requests as f64;
    let s = summarize(&mut latencies);
    FanOutRow { n, slow_measured: slow, slow_formula: 1.0 - (1.0 - p_sub).powi(n as i32), p50: s.p50, p99: s.p99 }
}

#[derive(Debug, Clone, Copy)]
pub struct HedgeRow {
    /// Share of sub-requests for which a second copy was sent.
    pub extra_load: f64,
    pub slow: f64,
    pub p50: f64,
    pub p99: f64,
    pub p999: f64,
}

/// Like [`fan_out`], but a sub-request that has not answered after `hedge_after_ms` is sent again to
/// another replica (independent latency); the first answer wins.
pub fn hedged_fan_out(model: ServiceModel, n: usize, requests: usize, hedge_after_ms: f64, threshold_ms: f64, rng: &mut Rng) -> HedgeRow {
    let mut hedged = 0u64;
    let mut latencies: Vec<f64> = (0..requests)
        .map(|_| {
            (0..n)
                .map(|_| {
                    let first = model.sample(rng);
                    if first > hedge_after_ms {
                        hedged += 1;
                        first.min(hedge_after_ms + model.sample(rng))
                    } else {
                        first
                    }
                })
                .fold(0.0, f64::max)
        })
        .collect();
    let slow = latencies.iter().filter(|&&l| l > threshold_ms).count() as f64 / requests as f64;
    let s = summarize(&mut latencies);
    HedgeRow { extra_load: hedged as f64 / (requests * n) as f64, slow, p50: s.p50, p99: s.p99, p999: s.p999 }
}

/// A sample series with the numbers the coordinated-omission discussion looks at.
#[derive(Debug, Clone, Copy)]
pub struct OmissionRow {
    pub samples: usize,
    /// Fraction of samples at or below `fast_ms`.
    pub fast_fraction: f64,
    pub p50: f64,
    pub p99: f64,
    pub p9999: f64,
    pub max: f64,
}

fn omission_row(mut v: Vec<f64>, fast_ms: f64) -> OmissionRow {
    v.sort_by(f64::total_cmp);
    OmissionRow {
        samples: v.len(),
        fast_fraction: v.iter().filter(|&&x| x <= fast_ms).count() as f64 / v.len() as f64,
        p50: percentile_sorted(&v, 50.0),
        p99: percentile_sorted(&v, 99.0),
        p9999: percentile_sorted(&v, 99.99),
        max: v[v.len() - 1],
    }
}

/// What a recorder that notices a stall afterwards adds: for a sample `v` larger than the expected
/// interval between samples, the samples `v - interval`, `v - 2 interval`, ... down to `interval`
/// that a non-waiting sender would have taken during the stall (the idea of HdrHistogram's
/// `record_correct`).
pub fn correct_for_omission(samples: &[f64], interval_ms: f64) -> Vec<f64> {
    let mut out = Vec::with_capacity(samples.len());
    for &v in samples {
        out.push(v);
        let mut missing = v - interval_ms;
        while missing >= interval_ms {
            out.push(missing);
            missing -= interval_ms;
        }
    }
    out
}

/// Three views of one scenario, all in virtual time (nothing sleeps): a server that answers in
/// `service_ms` for `calm_s` seconds, then stalls for `stall_s` seconds; the client intends to send
/// one request every `interval_ms`.
///
/// * `raw`: the client waits for each response before sending the next (closed loop). During the
///   stall it sends nothing, so the stall is a *single* slow sample.
/// * `corrected`: `raw` plus the samples a non-waiting sender would have taken ([`correct_for_omission`]).
/// * `open_loop`: requests are sent on schedule regardless; those sent during the stall queue up and
///   are served in order after it, each paying for the wait since its *intended* send time.
pub fn coordinated_omission(service_ms: f64, calm_s: f64, stall_s: f64, interval_ms: f64) -> [OmissionRow; 3] {
    let calm = (calm_s * 1000.0 / interval_ms) as usize;
    let mut raw = vec![service_ms; calm];
    raw.push(stall_s * 1000.0);
    let corrected = correct_for_omission(&raw, interval_ms);

    let stalled = (stall_s * 1000.0 / interval_ms) as usize;
    let stall_end = (calm_s + stall_s) * 1000.0;
    let mut open_loop = vec![service_ms; calm];
    let mut server_free = stall_end;
    for k in 0..stalled {
        let intended = calm_s * 1000.0 + k as f64 * interval_ms;
        let done = server_free.max(intended) + service_ms;
        open_loop.push(done - intended);
        server_free = done;
    }
    [omission_row(raw, service_ms), omission_row(corrected, service_ms), omission_row(open_loop, service_ms)]
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: ServiceModel = ServiceModel { median_ms: 10.0, sigma: 0.35, p_slow: 0.01, stall_ms: 1000.0 };

    #[test]
    fn nearest_rank_percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile_sorted(&v, 50.0), 50.0);
        assert_eq!(percentile_sorted(&v, 95.0), 95.0);
        assert_eq!(percentile_sorted(&v, 99.0), 99.0);
        assert_eq!(percentile_sorted(&v, 99.9), 100.0);
        assert_eq!(percentile_sorted(&v, 0.0), 1.0);
        assert_eq!(percentile_sorted(&v, 100.0), 100.0);
        assert_eq!(percentile_sorted(&[7.0], 99.0), 7.0);
    }

    #[test]
    fn the_mean_hides_a_tail_that_percentiles_show() {
        // 95% of requests take 50 ms, 5% take 1000 ms.
        let mut v: Vec<f64> = vec![50.0; 950];
        v.extend(vec![1000.0; 50]);
        let s = summarize(&mut v);
        assert_eq!((s.p50, s.p95, s.p99, s.max), (50.0, 50.0, 1000.0, 1000.0));
        assert!((s.mean - 97.5).abs() < 1e-9, "{s:?}");
        // The mean is under 100 ms although one request in twenty takes a second.
        assert!(s.mean < 100.0 && s.p99 > 10.0 * s.mean);
    }

    #[test]
    fn histogram_percentiles_are_within_one_percent_and_merge_exactly() {
        let mut rng = Rng::new(3);
        let mut all = Vec::new();
        let mut parts = [Histogram::default(), Histogram::default(), Histogram::default()];
        for i in 0..90_000 {
            let v = MODEL.sample(&mut rng);
            all.push(v);
            parts[i % 3].record(v);
        }
        let mut merged = Histogram::default();
        parts.iter().for_each(|p| merged.add(p));
        let mut direct = Histogram::default();
        all.iter().for_each(|&v| direct.record(v));
        assert_eq!(merged, direct, "adding histograms equals recording everything in one");
        assert_eq!(merged.count(), 90_000);

        all.sort_by(f64::total_cmp);
        for p in [50.0, 90.0, 99.0, 99.9] {
            let exact = percentile_sorted(&all, p);
            let approx = merged.value_at_percentile(p);
            assert!(approx >= exact * 0.9999 && approx <= exact * 1.0101, "p{p}: exact {exact} approx {approx}");
        }
    }

    #[test]
    fn averaging_percentiles_gives_the_wrong_answer_and_merging_histograms_the_right_one() {
        // One busy, unhealthy machine (3% slow) and nine quiet healthy ones (0.1% slow).
        let mut machines = vec![Machine { requests: 40_000, p_slow: 0.03 }];
        machines.extend((0..9).map(|_| Machine { requests: 60_000 / 9, p_slow: 0.001 }));
        let a = aggregate_percentiles(MODEL, &machines, &mut Rng::new(5));
        assert!(a.pooled_p99 > 700.0, "{a:?}");
        assert!(a.mean_of_p99 < a.pooled_p99 / 4.0, "the mean of p99s misses the tail: {a:?}");
        assert!(a.weighted_mean_of_p99 < a.pooled_p99 * 0.7, "weighting by traffic does not fix it: {a:?}");
        assert!((a.merged_histogram_p99 / a.pooled_p99 - 1.0).abs() < 0.011, "merged histograms agree with the pooled samples: {a:?}");
    }

    #[test]
    fn fan_out_turns_a_one_percent_tail_into_most_requests() {
        let mut rng = Rng::new(9);
        let p = slow_fraction(MODEL, 500.0, 1_000_000, &mut rng);
        assert!((p - 0.01).abs() < 0.0006, "{p}");
        let mut prev = 0.0;
        for n in [1, 10, 100] {
            let row = fan_out(MODEL, n, 20_000, 500.0, p, &mut rng);
            // Two independent estimates of the same quantity (sd about 0.004 for n = 100): 0.02 is 5 sd.
            assert!((row.slow_measured - row.slow_formula).abs() < 0.02, "{row:?}");
            assert!(row.slow_measured > prev);
            prev = row.slow_measured;
            if n == 100 {
                assert!(row.slow_formula > 0.62 && row.slow_formula < 0.65, "1 - 0.99^100 is about 63.4%: {row:?}");
                assert!(row.p50 > 500.0, "the median request is now slow: {row:?}");
            }
        }
    }

    #[test]
    fn hedging_after_the_p95_costs_about_five_percent_extra_load_and_cuts_the_tail() {
        let mut rng = Rng::new(21);
        let mut calibration: Vec<f64> = (0..200_000).map(|_| MODEL.sample(&mut rng)).collect();
        calibration.sort_by(f64::total_cmp);
        let p95 = percentile_sorted(&calibration, 95.0);
        let plain = fan_out(MODEL, 100, 10_000, 500.0, 0.01, &mut rng);
        let hedged = hedged_fan_out(MODEL, 100, 10_000, p95, 500.0, &mut rng);
        assert!(hedged.extra_load > 0.04 && hedged.extra_load < 0.06, "{hedged:?}");
        assert!(plain.slow_measured > 0.55, "{plain:?}");
        assert!(hedged.slow < 0.05, "two independent stalls are needed to be slow: {hedged:?}");
        assert!(hedged.p50 * 10.0 < plain.p50, "{hedged:?} vs {plain:?}");
    }

    #[test]
    fn a_closed_loop_sender_records_a_stall_as_a_single_slow_sample() {
        // 100 s of 1 ms answers sampled every 10 ms, then a 100 s stall (the HdrHistogram README scenario).
        let [raw, corrected, open] = coordinated_omission(1.0, 100.0, 100.0, 10.0);
        assert_eq!(raw.samples, 10_001);
        assert!(raw.fast_fraction > 0.9998, "about 99.99% of raw samples are 1 ms: {raw:?}");
        assert_eq!((raw.p50, raw.p99, raw.p9999, raw.max), (1.0, 1.0, 1.0, 100_000.0));
        assert_eq!(corrected.samples, 10_000 + 10_000, "one real sample of 100 s stands for 9999 missed ones + itself");
        assert!((corrected.fast_fraction - 0.5).abs() < 1e-3, "about half of the corrected samples are 1 ms: {corrected:?}");
        assert!(corrected.p99 > 90_000.0 && corrected.p50 <= 50_000.0, "{corrected:?}");
        // The open-loop run sees the same stall, and the queue that follows it.
        assert_eq!(open.samples, 20_000);
        assert!(open.p99 > 90_000.0 && open.max > 100_000.0, "{open:?}");
    }

    #[test]
    fn the_correction_adds_nothing_when_nothing_is_late() {
        let v = [1.0, 2.0, 9.0, 10.0, 19.0];
        assert_eq!(correct_for_omission(&v, 10.0), v.to_vec());
        assert_eq!(correct_for_omission(&[35.0], 10.0), vec![35.0, 25.0, 15.0]);
    }
}
