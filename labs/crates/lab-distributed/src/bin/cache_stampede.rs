//! Lab (k): cache-aside stampede, stale-set race, lock release, negative caching.
//! Usage: `cache_stampede [seed=42] [all]`.

use lab_distributed::cache::{
    Fix, Policy, Release, StampedeCfg, WriterOrder, enumerate_race, negative_entry_pitfall,
    scripted_lock_clients, simulate_expiry_burst, simulate_lock, simulate_negative_caching,
    simulate_stampede, stale_count, stale_window_ms,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(42);
    // Optional second argument `all`: also list every interleaving of part [2].
    let full = args.next().as_deref() == Some("all");
    println!("Lab (k) cache_stampede  seed={seed}  (model in virtual ms, not Redis)");

    let cfg = StampedeCfg::PAPER_EXAMPLE;
    println!(
        "\n[1] hot key: {} req/s, ttl {} s, recompute {} s, {} min of virtual time",
        1000 / cfg.interval_ms,
        cfg.ttl_ms / 1000,
        cfg.compute_ms / 1000,
        cfg.horizon_ms / 60_000
    );
    println!(
        "{:<27}{:>7}{:>6}{:>9}{:>8}{:>7}{:>8}{:>9}",
        "policy", "recomp", "waves", "per_wave", "peak_db", "miss%", "waited%", "wait_max"
    );
    let rows: [(&str, Policy); 7] = [
        ("naive", Policy::naive()),
        ("single-flight ttl=10s", Policy::single_flight(10_000)),
        ("single-flight ttl=1s<comp", Policy::single_flight(1_000)),
        ("xfetch beta=0.5", Policy::xfetch(0.5)),
        ("xfetch beta=1", Policy::xfetch(1.0)),
        ("xfetch beta=2", Policy::xfetch(2.0)),
        (
            "xfetch beta=1 + lock 10s",
            Policy::xfetch_with_lock(1.0, 10_000),
        ),
    ];
    for (name, p) in rows {
        let r = simulate_stampede(p, cfg, seed);
        println!(
            "{:<27}{:>7}{:>6}{:>9.1}{:>8}{:>7.2}{:>8.2}{:>8}ms",
            name,
            r.recomputes,
            r.waves,
            r.per_wave(),
            r.peak_db,
            100.0 * r.miss_frac(),
            100.0 * r.waited_frac(),
            r.wait_max_ms
        );
    }
    println!(
        "(naive, one cycle: 10 req/s x 3 s = {} recomputes)",
        simulate_stampede(Policy::naive(), cfg.one_cycle(), seed).recomputes
    );

    println!(
        "\n[1b] 500 keys loaded together, ttl 60 s, reload 500 ms, each key read every 100 ms"
    );
    for jitter in [0, 10] {
        let b = simulate_expiry_burst(500, 60_000, jitter, 500, 100, seed);
        println!(
            "jitter 0..{jitter:>2}% of ttl: peak concurrent reloads {:>3}, reload starts spread over {} ms",
            b.peak_db, b.spread_ms
        );
    }

    println!(
        "\n[2] stale set: reader r1=GET(miss) r2=SELECT r3=SET; every interleaving with the writer's w1 w2"
    );
    println!(
        "A: w1=UPDATE db, w2=DEL cache     B: w1=DEL cache, w2=UPDATE db   (stale = cache old, db new)"
    );
    println!(
        "{:<34}{:>8}{:>9}{:>16}",
        "stale interleavings of 10", "plain", "lease", "versioned SET"
    );
    for (name, order) in [
        ("A: db then DEL", WriterOrder::DbThenInvalidate),
        ("B: DEL then db", WriterOrder::InvalidateThenDb),
    ] {
        println!(
            "{:<34}{:>8}{:>9}{:>16}",
            name,
            stale_count(order, Fix::None),
            stale_count(order, Fix::Lease),
            stale_count(order, Fix::VersionedSet)
        );
    }
    for (name, order, fix) in [
        ("A plain", WriterOrder::DbThenInvalidate, Fix::None),
        ("B lease", WriterOrder::InvalidateThenDb, Fix::Lease),
    ] {
        for o in enumerate_race(order, fix).iter().filter(|o| o.stale) {
            println!("  stale in {name}: {}", o.trace());
        }
    }
    if full {
        for (name, order) in [
            ("A", WriterOrder::DbThenInvalidate),
            ("B", WriterOrder::InvalidateThenDb),
        ] {
            println!("order {name}, plain, all 10 (S = stale):");
            for o in enumerate_race(order, Fix::None) {
                println!(
                    "  {}  cache={:?} db={} {}",
                    o.trace(),
                    o.cache,
                    o.db,
                    if o.stale { "S" } else { "" }
                );
            }
        }
    }
    println!("stale lifetime of r1 r2 w1 w2 r3 (reader's SET at 2 s, horizon 300 s):");
    for ttl in [None, Some(60_000), Some(5_000)] {
        let label = ttl.map_or("no ttl".to_string(), |t| format!("ttl {} s", t / 1000));
        println!(
            "  {label:<8} plain: {:>6} ms   lease: {} ms",
            stale_window_ms(Fix::None, ttl, 2_000, 300_000),
            stale_window_ms(Fix::Lease, ttl, 2_000, 300_000)
        );
    }

    println!("\n[3] lock ttl 30 s; A works 45 s, B 25 s, C 20 s; B and C poll every 1 s");
    for (name, rel) in [
        ("plain DEL", Release::PlainDel),
        ("token compare", Release::TokenCompare),
    ] {
        let run = simulate_lock(&scripted_lock_clients(), 30_000, rel);
        println!("-- release by {name}");
        for line in &run.trace {
            println!("   {line}");
        }
        let ov: Vec<String> = run
            .overlaps
            .iter()
            .map(|o| {
                format!(
                    "{}&{} {:.3}s..{:.3}s",
                    o.first,
                    o.second,
                    o.from_ms as f64 / 1000.0,
                    o.to_ms as f64 / 1000.0
                )
            })
            .collect();
        println!(
            "   mutual-exclusion violations: {} [{}]; foreign deletes: {}",
            run.overlaps.len(),
            ov.join(", "),
            run.foreign_deletes
        );
    }

    println!(
        "\n[4] negative caching: bot probes 12000 non-existent ids at 200 req/s (60 s), neg ttl 30 s"
    );
    for space in [1_000u64, 100_000_000] {
        let off = simulate_negative_caching(seed, 12_000, 5, space, None);
        let on = simulate_negative_caching(seed, 12_000, 5, space, Some(30_000));
        println!(
            "id space {space:>9}: db lookups {} -> {}  (peak negative entries {})",
            off.db_lookups, on.db_lookups, on.peak_entries
        );
    }
    println!(
        "row created at 5 s while a negative entry lives to 30 s; reads every 100 ms still returning 404: no DEL on create = {}, DEL on create = {}",
        negative_entry_pitfall(false, 30_000, 5_000, 100, 60_000),
        negative_entry_pitfall(true, 30_000, 5_000, 100, 60_000)
    );
}
