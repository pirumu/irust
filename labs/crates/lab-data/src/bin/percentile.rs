//! Lab (g): latency percentiles, merging histograms, tail amplification under fan-out, hedged
//! requests, coordinated omission. All synthetic and deterministic (seeded); nothing sleeps.
//!
//! Run: cargo run -q --release -p lab-data --bin percentile

use lab_data::latency::{self, Machine, OmissionRow, ServiceModel, percentile_sorted, summarize};
use lab_data::rng::Rng;
use lab_data::util::thousands;

const MODEL: ServiceModel = ServiceModel { median_ms: 10.0, sigma: 0.35, p_slow: 0.01, stall_ms: 1000.0 };

fn mean_hides_the_tail() {
    println!("== 1. the mean hides the tail ==");
    let mut a: Vec<f64> = vec![50.0; 950];
    a.extend(vec![1000.0; 50]);
    let s = summarize(&mut a);
    println!("workload A: 95% of requests take 50 ms, 5% take 1000 ms (1000 requests)");
    println!("  mean {:.1} ms   p50 {} ms   p95 {} ms   p99 {} ms", s.mean, s.p50, s.p95, s.p99);

    let mut rng = Rng::new(1);
    let mut b: Vec<f64> = (0..200_000).map(|_| MODEL.sample(&mut rng)).collect();
    let s = summarize(&mut b);
    println!("workload B: lognormal body (median 10 ms, sigma 0.35) plus a 1% chance of a ~1000 ms stall (200,000 requests)");
    println!(
        "  mean {:.1} ms   p50 {:.1}   p95 {:.1}   p99 {:.1}   p99.9 {:.1}   max {:.1}",
        s.mean, s.p50, s.p95, s.p99, s.p999, s.max
    );
}

fn averaging_percentiles() {
    println!("\n== 2. percentiles cannot be averaged ==");
    let mut machines = vec![Machine { requests: 40_000, p_slow: 0.03 }];
    machines.extend((0..9).map(|_| Machine { requests: 60_000 / 9, p_slow: 0.001 }));
    println!("10 machines: #0 serves 40% of the traffic and stalls on 3% of its requests; #1-#9 share 60% and stall on 0.1%");
    let a = latency::aggregate_percentiles(MODEL, &machines, &mut Rng::new(5));
    println!("  mean of the 10 per-machine p99          : {:>8.1} ms", a.mean_of_p99);
    println!("  per-machine p99 weighted by traffic     : {:>8.1} ms", a.weighted_mean_of_p99);
    println!("  p99 of all 100,000 requests pooled      : {:>8.1} ms   <- the number users experience", a.pooled_p99);
    println!("  p99 from the sum of the 10 histograms   : {:>8.1} ms   (1% buckets, so within 1% of the pooled value)", a.merged_histogram_p99);
}

fn fan_out_table() {
    println!("\n== 3. fan-out: a request waits for the slowest of N parallel sub-requests ==");
    let mut rng = Rng::new(9);
    let p = latency::slow_fraction(MODEL, 500.0, 1_000_000, &mut rng);
    println!("one sub-request is \"slow\" (> 500 ms) with probability p = {:.4} (measured on 1,000,000 draws); 20,000 requests per row", p);
    println!("{:>5} {:>18} {:>16} {:>10} {:>10}", "N", "requests slow", "1 - (1-p)^N", "p50 ms", "p99 ms");
    for n in [1, 2, 5, 10, 50, 100, 200, 500] {
        let r = latency::fan_out(MODEL, n, 20_000, 500.0, p, &mut rng);
        println!(
            "{n:>5} {:>17.1}% {:>15.1}% {:>10.1} {:>10.1}",
            r.slow_measured * 100.0,
            r.slow_formula * 100.0,
            r.p50,
            r.p99
        );
    }
}

fn hedging() {
    println!("\n== 4. hedged requests, fan-out 100 ==");
    let mut rng = Rng::new(21);
    let mut calibration: Vec<f64> = (0..200_000).map(|_| MODEL.sample(&mut rng)).collect();
    calibration.sort_by(f64::total_cmp);
    let p95 = percentile_sorted(&calibration, 95.0);
    println!("a sub-request that has not answered after {p95:.1} ms (the p95 of one sub-request) is sent again to another replica; first answer wins");
    let plain = latency::fan_out(MODEL, 100, 20_000, 500.0, 0.01, &mut rng);
    let hedged = latency::hedged_fan_out(MODEL, 100, 20_000, p95, 500.0, &mut rng);
    println!("{:<10} {:>14} {:>9} {:>9} {:>10} {:>12}", "", "requests slow", "p50 ms", "p99 ms", "p99.9 ms", "extra load");
    println!("{:<10} {:>13.1}% {:>9.1} {:>9.1} {:>10} {:>12}", "no hedge", plain.slow_measured * 100.0, plain.p50, plain.p99, "-", "0%");
    println!(
        "{:<10} {:>13.1}% {:>9.1} {:>9.1} {:>10.1} {:>11.1}%",
        "hedged",
        hedged.slow * 100.0,
        hedged.p50,
        hedged.p99,
        hedged.p999,
        hedged.extra_load * 100.0
    );
    println!("(the model assumes the second copy's latency is independent of the first; a correlated cause, such as an overloaded rack, breaks that)");
}

fn print_omission(label: &str, r: &OmissionRow) {
    println!(
        "{label:<34} {:>9} {:>10.2}% {:>9.0} {:>10.0} {:>10.0} {:>10.0}",
        thousands(r.samples as u64),
        r.fast_fraction * 100.0,
        r.p50,
        r.p99,
        r.p9999,
        r.max
    );
}

fn coordinated_omission() {
    println!("\n== 5. coordinated omission (virtual time) ==");
    println!("the server answers in 1 ms for 100 s, then stalls for 100 s; the client intends one request every 10 ms");
    let [raw, corrected, open] = latency::coordinated_omission(1.0, 100.0, 100.0, 10.0);
    println!("{:<34} {:>9} {:>11} {:>9} {:>10} {:>10} {:>10}", "samples (ms)", "count", "<= 1 ms", "p50", "p99", "p99.99", "max");
    print_omission("closed loop (waits for each reply)", &raw);
    print_omission("closed loop + correction", &corrected);
    print_omission("open loop (FIFO queue model)", &open);
}

fn main() {
    mean_hides_the_tail();
    averaging_percentiles();
    fan_out_table();
    hedging();
    coordinated_omission();
}
