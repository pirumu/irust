//! Lab (g): delivery semantics, idempotency keys and the outbox, with injected crashes.
//! Usage: `delivery_dedup [seed]` (default 42).

use lab_distributed::delivery::*;

// Parameters of the default invocation.
const EVENTS: usize = 8;
const BATCH: usize = 2;
const CRASH_P: f64 = 0.15;
const RUNS: u32 = 1000;
const EXH_EVENTS: usize = 4;
const EXH_K: u32 = 14;
const PAY_INTENTS: u32 = 1000;
const PAY_EXH_K: u32 = 12;
const DUAL_OPS: usize = 20;
const DUAL_P: f64 = 0.10;
const DUAL_RUNS: u32 = 500;

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .map_or(42, |s| s.parse().expect("seed must be a u64"));
    println!("== Lab (g) delivery_dedup  seed={seed} ==");
    part1(seed);
    part2(seed);
    part3(seed);
    part4(seed);
}

fn part1(seed: u64) {
    println!(
        "\n-- 1. consumer: {EVENTS} events, batch {BATCH}, crash p={CRASH_P:.2}/step, {RUNS} runs ({} events) --",
        u64::from(RUNS) * EVENTS as u64
    );
    println!(
        "{:<20} {:>5} {:>5} {:>6} | exhaustive: {EXH_EVENTS} events, all 2^{EXH_K} crash patterns -> runs with loss / dup",
        "design", "lost", "dup", "exact"
    );
    for d in ConsumerDesign::ALL {
        let t = sweep_consumer(d, EVENTS, BATCH, CRASH_P, RUNS, seed);
        let x = exhaustive_consumer(d, EXH_EVENTS, BATCH, EXH_K);
        println!(
            "{:<20} {:>5} {:>5} {:>6} | {:>6} / {:>6}",
            d.label(),
            t.lost,
            t.dup,
            t.exact,
            x.runs_lost,
            x.runs_dup
        );
    }
}

fn part2(seed: u64) {
    let (messages, runs) = (20u64, 1000u32);
    println!(
        "\n-- 2. producer: {messages} msgs/run, request lost 0.20, ack lost 0.20, {runs} runs --"
    );
    println!(
        "{:<26} {:>8} {:>6} {:>10} {:>9}",
        "producer", "log", "dups", "misordered", "rejected"
    );
    for (idempotent, window) in [(false, 1), (false, 3), (true, 1), (true, 3)] {
        let p = ProducerParams {
            messages,
            window,
            p_request_lost: 0.2,
            p_ack_lost: 0.2,
            idempotent,
        };
        let t = sweep_producer(p, runs, seed);
        let name = format!(
            "{} window={window}",
            if idempotent {
                "idempotent (pid,seq)"
            } else {
                "plain retry"
            }
        );
        println!(
            "{:<26} {:>8} {:>6} {:>10} {:>9}",
            name, t.log_entries, t.duplicates, t.misordered_runs, t.rejected
        );
    }
}

fn part3(seed: u64) {
    println!("\n-- 3. POST /pay with Idempotency-Key (lease {LEASE_TTL}) --");
    let (lines, charges) = script_four_situations(PayDesign::KeyAtomic);
    for l in &lines {
        let r = l.resp;
        let note = match (r.status, r.replay) {
            (201, true) => format!("201 replay of charge #{}", r.charge_id.unwrap_or(0)),
            (201, false) => format!("201 created charge #{}", r.charge_id.unwrap_or(0)),
            (s, _) => s.to_string(),
        };
        println!(
            "t={} key={} amount={:<3} {:<32} -> {note}",
            l.time, l.req.key, l.req.amount, l.what
        );
    }
    println!(
        "charges after script: {charges}; stale handler after lease takeover: non-atomic {} charges, atomic {}",
        script_stale_handler(PayDesign::KeyNonAtomic),
        script_stale_handler(PayDesign::KeyAtomic)
    );
    println!(
        "{PAY_INTENTS} intentions, step/reply fail p={CRASH_P:.2}, <= {MAX_ATTEMPTS} attempts; exhaustive = 1 intention x 2^{PAY_EXH_K} patterns"
    );
    println!(
        "{:<16} {:>7} {:>7} {:>9} {:>5} | {:>10} {:>7}",
        "design", "double", "never", "confirmed", "409s", "exh double", "of"
    );
    for d in PayDesign::ALL {
        let t = run_payments(d, PAY_INTENTS, CRASH_P, seed);
        let x = exhaustive_payments(d, PAY_EXH_K);
        println!(
            "{:<16} {:>7} {:>7} {:>9} {:>5} | {:>10} {:>7}",
            d.label(),
            t.double_charged,
            t.never_charged,
            t.confirmed,
            t.conflicts,
            x.double_charged,
            x.intents
        );
    }
}

fn part4(seed: u64) {
    println!(
        "\n-- 4. update DB + publish event: {DUAL_OPS} ops/run, crash p={DUAL_P:.2}/step, {DUAL_RUNS} runs --"
    );
    println!(
        "{:<22} {:>9} {:>5} {:>8} {:>5} {:>11} {:>12}",
        "design", "committed", "lost", "phantom", "dup", "effect dup", "effect dup"
    );
    println!(
        "{:<22} {:>9} {:>5} {:>8} {:>5} {:>11} {:>12}",
        "", "", "", "", "msgs", "(no dedup)", "(dedup by id)"
    );
    for d in DualDesign::ALL {
        let t = sweep_dual(d, DUAL_OPS, DUAL_P, DUAL_RUNS, seed);
        println!(
            "{:<22} {:>9} {:>5} {:>8} {:>5} {:>11} {:>12}",
            d.label(),
            t.committed,
            t.lost,
            t.phantom,
            t.dup_events,
            t.effect_dup_naive,
            t.effect_dup_dedup
        );
    }
}
