//! Lab (j): Redis-style rate limiters (fixed window, sliding log, token bucket) on a model of Redis.
//! Usage: `rate_limiter [seed]` (default 42).

use lab_distributed::limiter::{
    Algo, ExpireMode, Params, boundary_burst, fixed_window_admitted, memory_elements, race_sliding,
    steady_overload, token_bucket_bound, ttl_leak, window_stats,
};

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .map_or(42, |s| s.parse().expect("seed must be a u64"));
    let p = Params {
        limit: 100,
        window_ms: 60_000,
    };
    let algos = [
        Algo::FixedAligned,
        Algo::FixedTtl,
        Algo::SlidingLog,
        Algo::TokenBucket { cap: 20 },
        Algo::TokenBucket { cap: 100 },
    ];
    println!("== Lab (j) rate limiter model, seed {seed} ==");
    println!(
        "limit L={} per W={} s; token bucket refills L tokens per W (cap 20 or 100)",
        p.limit,
        p.window_ms / 1000
    );

    println!(
        "\n(a) boundary burst: admitted in the 0.2 s [59.9 s, 60.1 s) when L requests arrive at 59.900..59.999 s"
    );
    println!(
        "    and L more at 60.000..60.099 s (opener = one extra request at t=0 that starts the first window)"
    );
    println!("{:<22} {:>10} {:>10}", "algorithm", "no opener", "opener@0");
    for a in algos {
        println!(
            "{:<22} {:>10} {:>10}",
            a.name(),
            boundary_burst(a, &p, false),
            boundary_burst(a, &p, true)
        );
    }

    let (gap, dur) = (333, 600_000);
    println!(
        "\n(b) steady overload: 1 request / {gap} ms (about 3/s, 180 per minute) for {} s, admitted:",
        dur / 1000
    );
    for a in algos {
        println!("  {:<22} {:>5}", a.name(), steady_overload(a, &p, gap, dur));
    }

    let traces = 200;
    println!(
        "\n(c) max admitted in ANY {} s window, {traces} seeded bursty traces of {} s",
        p.window_ms / 1000,
        dur / 1000
    );
    println!(
        "{:<22} {:>6} {:>5} {:>7} {:>10}",
        "algorithm", "bound", "max", "mean", "traces>L"
    );
    for a in algos {
        let bound = match a {
            Algo::FixedAligned | Algo::FixedTtl => 2 * p.limit,
            Algo::SlidingLog => p.limit,
            Algo::TokenBucket { cap } => token_bucket_bound(&p, cap, p.window_ms),
        };
        let s = window_stats(a, &p, seed, traces, dur);
        println!(
            "{:<22} {:>6} {:>5} {:>7.1} {:>10}",
            a.name(),
            bound,
            s.max,
            s.mean,
            s.over_limit
        );
    }

    let pr = Params {
        limit: 20,
        window_ms: 60_000,
    };
    let (clients, per, seeds) = (8, 5, 200u64);
    println!(
        "\n(d) race: {clients} clients x {per} requests at the same ms, sliding log L={}, {seeds} seeds",
        pr.limit
    );
    println!(
        "{:<30} {:>5} {:>6} {:>5} {:>9}",
        "variant", "min", "mean", "max", "seeds>L"
    );
    for (name, atomic) in [
        ("3 separate commands", false),
        ("one script (atomic)", true),
    ] {
        let v: Vec<u64> = (0..seeds)
            .map(|i| race_sliding(seed.wrapping_add(i), atomic, clients, per, &pr))
            .collect();
        let (min, max) = (v.iter().min().unwrap(), v.iter().max().unwrap());
        let mean = v.iter().sum::<u64>() as f64 / seeds as f64;
        let over = v.iter().filter(|&&x| x > pr.limit).count();
        println!("{name:<30} {min:>5} {mean:>6.1} {max:>5} {over:>9}");
    }

    let pl = Params {
        limit: 5,
        window_ms: 60_000,
    };
    let (keys, reqs, crash_p) = (1000, 80, 0.02);
    println!(
        "\n(d2) TTL leak: {keys} keys, {reqs} requests each at 1 per 15 s, L=5 per 60 s (a healthy client sends 4 per window),"
    );
    println!("     crash probability {crash_p} per request");
    println!(
        "{:<34} {:>7} {:>8} {:>8}",
        "variant", "leaked", "blocked", "crashes"
    );
    for (name, atomic) in [
        ("INCR; EXPIRE if count==1", false),
        ("MULTI INCR; EXPIRE NX; EXEC", true),
    ] {
        let s = ttl_leak(seed, atomic, keys, reqs, 15_000, crash_p, &pl);
        println!(
            "{name:<34} {:>7} {:>8} {:>8}",
            s.leaked, s.blocked, s.crashes
        );
    }
    println!(
        "     1 request / 10 s for 600 s, L=5: admitted with EXPIRE NX = {}, with plain EXPIRE = {}",
        fixed_window_admitted(ExpireMode::Nx, &pl, 10_000, 60),
        fixed_window_admitted(ExpireMode::Always, &pl, 10_000, 60)
    );

    println!("\n(e) elements stored under the key(s) after 2L back-to-back requests");
    println!("{:<22} {:>8} {:>8}", "algorithm", "L=100", "L=10000");
    for a in [
        Algo::FixedTtl,
        Algo::SlidingLog,
        Algo::TokenBucket { cap: 20 },
    ] {
        let m = |limit| {
            memory_elements(
                a,
                &Params {
                    limit,
                    window_ms: 60_000,
                },
                2 * limit,
            )
        };
        println!("{:<22} {:>8} {:>8}", a.name(), m(100), m(10_000));
    }
}
