//! Lab (b): leaderless quorum store. Usage: `quorum [seed] [loss]` (defaults: 42, 0.2).

use lab_distributed::quorum::{
    Config, Mode, SloppyConfig, Stats, scripted_trace, shortest_stale_trace, sweep, sweep_sloppy,
};

const SEEDS: u64 = 200;
const OPS: usize = 200;

fn pct(a: u64, b: u64) -> f64 {
    if b == 0 {
        0.0
    } else {
        100.0 * a as f64 / b as f64
    }
}

fn row(cfg: Config, base: u64) -> Stats {
    let s = sweep(cfg, base, SEEDS);
    println!(
        "{:>1} {:>2} {:>2} {:>4} {:>6} {:>8.1} {:>8.1} {:>7} {:>7.2} {:>6}",
        cfg.n,
        cfg.w,
        cfg.r,
        cfg.w + cfg.r,
        cfg.overlaps(),
        pct(s.writes_ok, s.writes),
        pct(s.reads_ok, s.reads),
        s.stale,
        pct(s.stale, s.reads_ok),
        s.ghost
    );
    s
}

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args
        .next()
        .map_or(42, |a| a.parse().expect("seed must be a u64"));
    let loss: f64 = args
        .next()
        .map_or(0.2, |a| a.parse().expect("loss must be a float"));
    println!("Lab (b) quorum - seed {seed}, loss {loss}, {SEEDS} seeds x {OPS} ops (50% writes)");
    println!("\n== 1. success rates and stale reads (ghost = reads that saw a FAILED write) ==");
    println!("n  w  r  w+r  w+r>n  write%   read%   stale  stale%  ghost");
    let mut combos: Vec<(usize, usize, usize)> = (1..=3)
        .flat_map(|w| (1..=3).map(move |r| (3, w, r)))
        .collect();
    combos.extend([
        (5, 3, 3),
        (5, 2, 2),
        (5, 1, 5),
        (5, 5, 1),
        (5, 3, 2),
        (5, 4, 1),
    ]);
    for (n, w, r) in combos {
        row(Config::new(n, w, r, loss, false, OPS), seed);
    }
    println!("\n== 2. read repair, same seeds ==");
    for (n, w, r) in [(3usize, 1usize, 2usize), (5, 2, 2), (5, 2, 3)] {
        let off = sweep(Config::new(n, w, r, loss, false, OPS), seed, SEEDS);
        let on = sweep(Config::new(n, w, r, loss, true, OPS), seed, SEEDS);
        println!(
            "n={n} w={w} r={r}: stale {:>5} ({:.2}%) -> {:>5} ({:.2}%) with read repair",
            off.stale,
            pct(off.stale, off.reads_ok),
            on.stale,
            pct(on.stale, on.reads_ok)
        );
    }
    let cfg = Config::new(3, 1, 1, loss, false, 50);
    println!("\n== 2b. minimal stale-read trace, n=3 w=1 r=1 ==");
    if let Some((s, t)) = shortest_stale_trace(cfg, seed, 1000) {
        println!("seed {s}:");
        for e in &t {
            println!("  {}", e.describe(3));
        }
    }
    println!("\n== 3. scripted trace n=3 w=2 r=2: write v1 reached only A ==");
    let plain = scripted_trace(false);
    for l in &plain.log {
        println!("  {l}");
    }
    println!("  -- with ABD write-back, instead:");
    for l in scripted_trace(true)
        .log
        .iter()
        .filter(|l| !plain.log.contains(l))
    {
        println!("  {l}");
    }
    println!("\n== 4. sloppy quorum n=3 w=2 r=2 + 2 standbys, loss 0.4 ==");
    let sc = SloppyConfig {
        n: 3,
        w: 2,
        r: 2,
        standbys: 2,
        loss: 0.4,
        rounds: 100,
        deliver_hints: true,
    };
    let never = SloppyConfig {
        deliver_hints: false,
        ..sc
    };
    for (label, mode, cfg) in [
        ("Strict", Mode::Strict, sc),
        ("Sloppy", Mode::Sloppy, sc),
        ("Sloppy, no handoff", Mode::Sloppy, never),
    ] {
        let s = sweep_sloppy(cfg, mode, seed, SEEDS);
        println!(
            "{label}: writes ok {} rejected {} | stale before handoff {}/{} after {}/{} | hints delivered {}",
            s.writes_ok,
            s.writes_rejected,
            s.stale_before,
            s.reads_before,
            s.stale_after,
            s.reads_after,
            s.hints_delivered
        );
    }
}
