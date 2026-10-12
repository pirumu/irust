//! Lab (f): Raft leader election and log replication in a deterministic simulation.
//!
//! `raft_election [seed] [sweep_seeds]`: `seed` (default 42) picks the election timeline and the
//! Figure 8 replay; every sweep covers the seeds `0..sweep_seeds` (default 300). With
//! `sweep_seeds = 0` only the timeline is printed.

use lab_distributed::raft::*;
use std::process::ExitCode;

const WIDTHS: [Millis; 6] = [0, 5, 10, 25, 50, 150];
const NODES: usize = 5;
const HORIZON: Millis = 4000;
const DEADLINE: Millis = 2000;

fn ms(x: Option<Millis>) -> String {
    x.map_or_else(|| "-".to_string(), |v| v.to_string())
}

fn parse(arg: Option<String>, default: u64) -> Option<u64> {
    match arg {
        None => Some(default),
        Some(s) => s.parse().ok(),
    }
}

/// `"n2 grants its vote to n3 (term 1)"` -> `(2, 3, 1)`.
fn grant(line: &str) -> Option<(usize, usize, u64)> {
    let w: Vec<&str> = line.split_whitespace().collect();
    if w.len() == 8 && w[1] == "grants" {
        Some((
            w[0][1..].parse().ok()?,
            w[5][1..].parse().ok()?,
            w[7].trim_end_matches(')').parse().ok()?,
        ))
    } else {
        None
    }
}

/// Prints a trace, folding the consecutive votes of one election into a single line.
fn print_trace(trace: &[(Millis, String)]) {
    let mut i = 0;
    while i < trace.len() {
        let (t, line) = &trace[i];
        let Some((_, cand, term)) = grant(line) else {
            println!("t={t:>9} ms  {line}");
            i += 1;
            continue;
        };
        let (mut voters, mut end) = (Vec::new(), *t);
        while let Some((v, c, tm)) = trace.get(i).and_then(|(_, l)| grant(l)) {
            if (c, tm) != (cand, term) {
                break;
            }
            voters.push(format!("n{v}"));
            end = trace[i].0;
            i += 1;
        }
        println!(
            "t={:>9} ms  {} vote for n{cand} (term {term})",
            format!("{t}..{end}"),
            voters.join(",")
        );
    }
}

fn timeline(seed: u64) {
    println!(
        "== 1. Election timeline (seed {seed}): leader crashes at 900 ms, next leader is cut off 1500-2000 ms =="
    );
    let o = lab_distributed::raft::timeline(seed);
    let shown: Vec<(Millis, String)> = o
        .trace
        .iter()
        .filter(|(_, l)| !l.contains("commits up to"))
        .cloned()
        .collect();
    print_trace(&shown);
    let again = lab_distributed::raft::timeline(seed);
    println!(
        "trace hash {:#018x}, re-run identical: {}; invariants violated: {}",
        o.trace_hash,
        again.trace_hash == o.trace_hash,
        o.violations.len()
    );
}

fn scenarios(seeds: u64) {
    println!(
        "== 2. Scenarios: {NODES} nodes x {seeds} seeds x {HORIZON} ms (faults stop at {} ms, then all is healed) ==",
        HORIZON - QUIET_TAIL
    );
    println!(
        "{:<13} {:>13} {:>9} {:>4} {:>9} {:>9} {:>19} {:>9} {:>5}",
        "scenario",
        "1st leader ms",
        "elections",
        "max",
        "committed",
        "overwrit.",
        "recovery ms",
        "converged",
        "viol."
    );
    println!(
        "{:<13} {:>13} {:>9} {:>4} {:>9} {:>9} {:>19} {:>9} {:>5}",
        "", "med / p95", "per run", "term", "per run", "per run", "med / p95 (n)", "", ""
    );
    for s in Scenario::ALL {
        let r = sweep(s, NODES, HORIZON, 0..seeds);
        let n = r.seeds as f64;
        println!(
            "{:<13} {:>6} / {:>4} {:>9.2} {:>4} {:>9.1} {:>9.2} {:>5} / {:>3} ({:>4}) {:>5}/{:<3} {:>5}",
            s.name(),
            ms(r.median_first_leader()),
            ms(r.p95_first_leader()),
            r.elections as f64 / n,
            r.max_term,
            r.committed as f64 / n,
            r.overwritten as f64 / n,
            ms(r.median_recovery()),
            ms(r.p95_recovery()),
            r.recoveries.len(),
            r.converged,
            r.seeds,
            r.violations,
        );
    }
}

fn liveness_table(seeds: u64) {
    println!(
        "== 3. Election timeout in [150, 150+w) ms: {NODES} nodes, constant 10 ms latency, no faults, deadline {DEADLINE} ms =="
    );
    println!(
        "{:>6} {:>11} {:>17} {:>16} {:>13}",
        "w (ms)", "leader<=2s", "1st leader med/p95", "mean term of 1st", "campaigns/run"
    );
    for w in WIDTHS {
        let r = liveness(NODES, w, 0..seeds, DEADLINE);
        let term = if r.elected() == 0 {
            "-".to_string()
        } else {
            format!("{:.2}", r.term_sum as f64 / r.elected() as f64)
        };
        println!(
            "{:>6} {:>7}/{:<3} {:>8} / {:>6} {:>16} {:>13.2}",
            w,
            r.elected(),
            r.seeds,
            ms(percentile(&r.first_leader, 50)),
            ms(percentile(&r.first_leader, 95)),
            term,
            r.candidacies as f64 / r.seeds as f64,
        );
    }
}

fn mutants(seeds: u64) {
    println!(
        "== 4. Mutants: the checker must bite; the correct algorithm runs on the same seeds =="
    );
    println!(
        "{:<24} {:<25} {:<20} {:>10} {:>9} {:>8}",
        "mutant", "faults used", "first violation", "first seed", "violating", "correct"
    );
    for m in Mutant::ALL {
        let r = sweep_mutant(m, 0..seeds);
        let (kind, first) = match &r.first {
            Some((s, v)) => (v.invariant.to_string(), s.to_string()),
            None => ("none".to_string(), "-".to_string()),
        };
        println!(
            "{:<24} {:<25} {:<20} {:>10} {:>5}/{:<3} {:>8}",
            m.name(),
            r.label,
            kind,
            first,
            r.violating,
            r.seeds,
            r.correct_violations
        );
    }
}

fn figure8(seed: u64) {
    println!(
        "== 5. Figure 8 replay (seed {seed}), CommitOldTermByCounting: S1..S5 = n0..n4, elections forced by a script =="
    );
    let mut c = Cluster::new(figure8_config(Some(Mutant::CommitOldTermByCounting)), seed);
    c.run_until(1600);
    let key = |t: Millis, l: &str| {
        t >= 700
            && (l.contains("wins term")
                || l.contains("CRASHES")
                || (t < 1000 && l.contains("commits up to index 2")))
    };
    let shown: Vec<(Millis, String)> = c
        .trace()
        .iter()
        .filter(|(t, l)| key(*t, l))
        .cloned()
        .collect();
    print_trace(&shown);
    for v in c.outcome().violations {
        println!("{v}");
    }
    let mut ok = Cluster::new(figure8_config(None), seed);
    ok.run_until(1600);
    let logs: Vec<String> = (0..NODES)
        .map(|i| {
            let l: Vec<String> = ok
                .log(i)
                .iter()
                .map(|e| format!("({},{})", e.term, e.cmd))
                .collect();
            format!("[{}]", l.join(" "))
        })
        .collect();
    println!(
        "correct algorithm: {} violations, {} entries overwritten, converged: {}, every log {}",
        ok.outcome().violations.len(),
        ok.overwritten(),
        ok.converged(),
        if logs.iter().all(|l| *l == logs[0]) {
            logs[0].clone()
        } else {
            logs.join(" ")
        }
    );
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let (Some(seed), Some(seeds)) = (parse(args.next(), 42), parse(args.next(), 300)) else {
        eprintln!("usage: raft_election [seed] [sweep_seeds]");
        return ExitCode::from(2);
    };
    if seeds == 0 {
        println!("Lab (f) raft_election | seed {seed} | timeline only");
    } else {
        println!("Lab (f) raft_election | seed {seed} | sweeps over seeds 0..{seeds}");
    }
    println!(
        "virtual ms; election timeout 150-300, heartbeat 50, latency 5-15, one client command per 100 ms"
    );
    timeline(seed);
    if seeds > 0 {
        scenarios(seeds);
        liveness_table(seeds);
        mutants(seeds);
        figure8(seed);
    }
    ExitCode::SUCCESS
}
