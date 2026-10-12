//! Lab (c): per-request latency percentiles of a service-like workload, with and without two
//! deliberate inefficiencies (chapter 03 §1, §2, §6, §8).
//!
//!   cargo run -q --release -p lab-observability --bin latency -- [variant=both] [threads=4] \
//!       [requests_per_thread=100000] [heavy_every=200] [heavy_iters=100000]
//!
//! `variant` is `naive`, `tuned` or `both`. Each worker thread issues requests back to back and
//! records the latency of every one into a `hdrhistogram::Histogram` (3 significant digits).
//! One request in `heavy_every` is a "report" that burns `heavy_iters` rounds of CPU. The
//! `naive` variant burns them while holding the lock that every request needs; `tuned` does not.
//! See `lab_observability::latency` for the two handlers.
//!
//! The load is closed-loop (a thread sends its next request only when the previous one is done),
//! so a stall also suppresses the requests that would have queued behind it. Read the absolute
//! percentiles as a lower bound of what an open-loop client would see.

use lab_observability::alloc::{CountingAlloc, Stats};
use lab_observability::latency::{Config, Report, Variant, burn, run, summarize};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn arg<T: std::str::FromStr>(n: usize, default: T) -> T {
    std::env::args().nth(n).and_then(|a| a.parse().ok()).unwrap_or(default)
}

fn us(ns: f64) -> f64 {
    ns / 1e3
}

fn print_row(r: &Report) {
    let s = summarize(&r.hist);
    let per_request = |stats: Stats| stats.events() as f64 / r.requests as f64;
    println!(
        "{:<8} {:>9} {:>9.1} {:>10.0} {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>10.2} {:>11.3}  {:#018x}",
        r.variant.name(),
        r.requests,
        r.wall.as_secs_f64() * 1e3,
        r.requests as f64 / r.wall.as_secs_f64(),
        us(s.mean),
        us(s.p50 as f64),
        us(s.p99 as f64),
        us(s.p999 as f64),
        us(s.max as f64),
        per_request(r.allocs),
        r.checksum
    );
}

fn main() {
    let variant_arg: String = arg(1, "both".to_string());
    let variants: Vec<Variant> = match variant_arg.as_str() {
        "both" => vec![Variant::Naive, Variant::Tuned],
        other => match Variant::parse(other) {
            Some(v) => vec![v],
            None => {
                eprintln!("usage: latency [naive|tuned|both] [threads] [requests_per_thread] [heavy_every] [heavy_iters]");
                std::process::exit(2);
            }
        },
    };
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let cfg = Config {
        threads: arg(2, 4usize.min(cores)),
        requests_per_thread: arg(3, 100_000),
        warmup_per_thread: 5_000,
        heavy_every: arg(4, 200),
        heavy_iters: arg(5, 100_000),
    };

    // How much CPU does one report need on its own? (Thread CPU time: it does not count time spent
    // waiting for a core, which matters on a busy machine.)
    let cpu0 = lab_observability::thread_cpu_time();
    for i in 0..20 {
        std::hint::black_box(burn(i, cfg.heavy_iters));
    }
    let report_us = (lab_observability::thread_cpu_time() - cpu0).as_secs_f64() * 1e6 / 20.0;
    let load = lab_observability::load_average().map_or_else(|| "n/a".to_string(), |l| format!("{l:.2}"));

    println!(
        "latency: threads={} requests/thread={} warmup/thread={} heavy_every={} heavy_iters={} (one report needs ~{report_us:.0} us of CPU) build={}",
        cfg.threads,
        cfg.requests_per_thread,
        cfg.warmup_per_thread,
        cfg.heavy_every,
        cfg.heavy_iters,
        if cfg!(debug_assertions) { "dev (unoptimized)" } else { "release" }
    );
    println!("machine: cores={cores} load1={load}");
    println!(
        "{:<8} {:>9} {:>9} {:>10} {:>9} {:>9} {:>9} {:>9} {:>10} {:>11}  checksum",
        "variant", "requests", "wall_ms", "req/s", "mean_us", "p50_us", "p99_us", "p99.9_us", "max_us", "allocs/req"
    );
    for v in variants {
        print_row(&run(cfg, v));
    }
}
