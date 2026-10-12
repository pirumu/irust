//! Lab (e): Lamport clocks, vector clocks, LWW vs siblings, total order broadcast.
//! Usage: `lamport [seed]` (default 42).

use lab_distributed::clocks::{
    CheckReport, TobConfig, TobMode, TobRun, WORKED_EXPECTED, WORKED_LABELS, lww_demo, node_name,
    run_tob, sweep, worked_example,
};

const EXECUTIONS: u64 = 200;
const STEPS: usize = 60;
const TOB_SEEDS: u64 = 200;

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .map_or(42, |s| s.parse().expect("seed must be a u64"));
    println!("== Lab (e) lamport — seed {seed} ==");

    table();
    random_executions(seed);
    registers();
    tob(seed);
}

fn table() {
    println!("\n-- 1. chapter 09 section 3: A/B/C worked example --");
    let e = worked_example();
    println!("event        Lamport  vector<A,B,C>");
    let mut matches = true;
    for ((ev, label), (t, v)) in e.events.iter().zip(WORKED_LABELS).zip(WORKED_EXPECTED) {
        println!("{:<12} {:>7}  {}", label, ev.lamport, ev.vc);
        matches &= ev.lamport == t && ev.vc.as_slice() == v;
    }
    assert!(matches, "table differs from chapter 09");
    println!("equals the table in chapter 09: {matches}");
    let (b1, asend) = (&e.events[2], &e.events[1]);
    println!(
        "b1 L={} < send m1 L={} but concurrent: {} vs {} are incomparable",
        b1.lamport, asend.lamport, b1.vc, asend.vc
    );
}

fn random_executions(seed: u64) {
    println!(
        "\n-- 2. {EXECUTIONS} random executions (seeds {seed}..{}), {STEPS} events each --",
        seed + EXECUTIONS - 1
    );
    println!(
        "{:>2} {:>5} {:>8} {:>8} {:>10} {:>9} {:>9} {:>8}",
        "N", "execs", "pairs", "a->b", "concurrent", "clock-vio", "converse", "vc-bad"
    );
    let by_n = sweep(seed, EXECUTIONS, STEPS);
    let mut total = CheckReport::default();
    for (n, r) in &by_n {
        let execs = r.events / STEPS as u64;
        println!(
            "{:>2} {:>5} {:>8} {:>8} {:>10} {:>9} {:>9} {:>8}",
            n,
            execs,
            r.pairs,
            r.causal_pairs,
            r.concurrent_pairs,
            r.clock_violations,
            r.converse_counterexamples,
            r.vc_mismatches
        );
        total.add(r);
    }
    println!(
        "{:>2} {:>5} {:>8} {:>8} {:>10} {:>9} {:>9} {:>8}",
        "*",
        EXECUTIONS,
        total.pairs,
        total.causal_pairs,
        total.concurrent_pairs,
        total.clock_violations,
        total.converse_counterexamples,
        total.vc_mismatches
    );
    println!(
        "converse = concurrent pairs with L(a) != L(b): {:.1}% of concurrent pairs look ordered to Lamport",
        100.0 * total.converse_counterexamples as f64 / total.concurrent_pairs as f64
    );
    println!(
        "(t,node) order: {} violations (contradicts a->b, or duplicate stamp)",
        total.order_violations
    );
    assert_eq!(total.clock_violations, 0);
    assert_eq!(total.vc_mismatches, 0);
    assert_eq!(total.order_violations, 0);
}

fn registers() {
    println!("\n-- 3. LWW: client A clock +30 ms, B correct --");
    let d = lww_demo();
    println!(
        "physical ts: A set x=1 @{} ; B read x=1, set x=2 @{}  => LWW keeps x={}",
        d.physical_ts.0, d.physical_ts.1, d.physical_value
    );
    println!(
        "Lamport:     A set x=1 @{} ; B read x=1, set x=2 @{}  => LWW keeps x={}",
        d.lamport_stamps.0, d.lamport_stamps.1, d.lamport_value
    );
    println!(
        "vector clk:  same causal pair                          => siblings {:?}",
        d.mv_causal_values
    );
    println!(
        "concurrent:  A x=1 @{}, B x=2 @{} (no read in between)",
        d.concurrent_stamps.0, d.concurrent_stamps.1
    );
    println!(
        "  LWW on (t,node)  => keeps x={} (either arrival order), x=1 lost",
        d.concurrent_lww_value
    );
    println!(
        "  vector {} vs {} => siblings {:?}; after B reads both and writes x=3: {:?}",
        d.concurrent_vcs.0, d.concurrent_vcs.1, d.concurrent_mv_values, d.resolved_values
    );
}

fn show(run: &TobRun, nodes: &[usize]) {
    for &i in nodes {
        println!(
            "  {} state={:<10} applied={}",
            node_name(i),
            format!("{:?}", run.states[i]),
            run.logs[i].len()
        );
    }
}

fn tob(seed: u64) {
    let cfg = TobConfig::default();
    println!(
        "\n-- 4. total order broadcast: {} replicas x {} ops (=set, +append), delay 1..={} ms, seed {seed} --",
        cfg.nodes, cfg.ops_per_node, cfg.max_delay
    );
    let arrival = run_tob(seed, cfg, TobMode::Arrival);
    println!("apply on arrival:");
    show(&arrival, &[0, 1, 2]);
    let ordered = run_tob(seed, cfg, TobMode::Lamport);
    println!("apply in (t,node) order after hearing t' >= t from every other node:");
    show(&ordered, &[0, 1, 2]);
    let seq: Vec<String> = ordered.logs[0]
        .iter()
        .map(|(s, op)| format!("{s}{op}"))
        .collect();
    println!("  delivery order on every replica: {}", seq.join(" "));

    let crashed = TobConfig {
        crash: Some((2, 6)),
        ..cfg
    };
    let run = run_tob(seed, crashed, TobMode::Lamport);
    println!(
        "node C crashes at 6 ms ({} ops issued in total):",
        run.issued
    );
    for i in 0..2 {
        println!(
            "  {} applied={} stuck-pending={} state={:?}",
            node_name(i),
            run.logs[i].len(),
            run.pending[i],
            run.states[i]
        );
    }

    let (mut diverged, mut agree, mut stalled) = (0, 0, 0);
    for s in seed..seed + TOB_SEEDS {
        let a = run_tob(s, cfg, TobMode::Arrival);
        diverged += a.states.iter().any(|x| *x != a.states[0]) as u32;
        let o = run_tob(s, cfg, TobMode::Lamport);
        agree += (o.states.iter().all(|x| *x == o.states[0]) && o.pending.iter().all(|&p| p == 0))
            as u32;
        let c = run_tob(s, crashed, TobMode::Lamport);
        stalled += (c.pending[0] > 0 || c.pending[1] > 0) as u32;
    }
    println!(
        "over {TOB_SEEDS} seeds: arrival order diverged {diverged}, (t,node) order agreed {agree}, stalled with C crashed {stalled}"
    );
}
