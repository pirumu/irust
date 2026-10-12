//! Lab d: lease + process pause + fencing token. Usage: `fencing [seed] [runs]`.

use lab_distributed::fencing::*;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(42);
    let runs: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2000);
    println!(
        "== lab d: lease + pause + fencing token (seed {seed}, ttl {TTL_MS} ms, virtual time) =="
    );

    println!("\n-- 0. check-then-act: same 15 s pause, Unfenced --");
    for (label, before_check) in [
        ("pause before check", true),
        ("pause between check and write", false),
    ] {
        let r = scenario_check_then_act(before_check);
        println!(
            "  {label:30} aborted_by_check={} stale_accepted={} lost={}",
            r.aborted_by_check,
            r.stale_accepted(),
            r.lost_updates()
        );
    }

    println!("\n-- 1. A (token 33) pauses 15 s between check and write; B takes over --");
    let unfenced = scenario_pause(Mode::Unfenced);
    let fenced = scenario_pause(Mode::Fenced);
    println!("[Unfenced]");
    print_log(&unfenced, &[]);
    println!(
        "  => final value {} (accepted {}, lost {}), holder intervals overlap: {}",
        unfenced.final_value,
        unfenced.accepted.len(),
        unfenced.lost_updates(),
        unfenced.overlaps > 0
    );
    println!("[Fenced] same events; only the last line differs:");
    print_log(&fenced, &unfenced.log);
    println!(
        "  => final value {} (accepted {}, lost {}), holder intervals overlap: {}",
        fenced.final_value,
        fenced.accepted.len(),
        fenced.lost_updates(),
        fenced.overlaps > 0
    );

    println!("\n-- 2. residual window: A's late write (33) lands before B's write (34) --");
    for mode in [Mode::Fenced, Mode::FencedCas] {
        let r = scenario_residual(mode);
        let acc: Vec<String> = r
            .accepted
            .iter()
            .map(|w| format!("{}:t{}", client_name(w.client), w.token))
            .collect();
        let rej: Vec<String> = r
            .rejected
            .iter()
            .map(|w| {
                format!(
                    "{}:t{} ({})",
                    client_name(w.client),
                    w.token,
                    why(&w.reason)
                )
            })
            .collect();
        println!(
            "  {:9} accepted [{}] rejected [{}] final {} lost {}",
            mode.name(),
            acc.join(" "),
            rej.join(" "),
            r.final_value,
            r.lost_updates()
        );
    }

    println!("\n-- 3. token source: lock service fails over at t=5000, counter rolled back? --");
    for survives in [false, true] {
        let r = scenario_token_source(Mode::Fenced, survives);
        let toks: Vec<String> = r
            .grants
            .iter()
            .map(|g| format!("{}={}", client_name(g.client), g.token))
            .collect();
        let acc: Vec<String> = r
            .accepted
            .iter()
            .map(|w| format!("{}:t{}", client_name(w.client), w.token))
            .collect();
        let rej: Vec<String> = r
            .rejected
            .iter()
            .map(|w| format!("{}:t{}", client_name(w.client), w.token))
            .collect();
        println!(
            "  counter {:9} grants [{}] accepted [{}] rejected [{}] lost {}",
            if survives { "survives" } else { "rolled back" },
            toks.join(" "),
            acc.join(" "),
            rej.join(" "),
            r.lost_updates()
        );
    }

    println!(
        "\n-- 4. clock skew, no pause: client wall clock 8 s behind, 12 s after the request --"
    );
    let s = scenario_clock_skew(-8_000, 12_000);
    println!(
        "  truly valid: {}   wall-clock check says valid: {}   monotonic-since-send says valid: {}",
        s.truly_valid, s.wall_clock_says_valid, s.monotonic_says_valid
    );
    println!(
        "  A's write with token 33 after B (token {}) wrote: Unfenced accepts: {}, Fenced accepts: {}",
        s.other_token, s.unfenced_accepts, s.fenced_accepts
    );

    println!(
        "\n-- sweep: {runs} runs per mode, 2 clients x {} rounds, pause p={} per step up to {} ms --",
        SweepParams::LAB.rounds,
        SweepParams::LAB.p_pause,
        3 * TTL_MS
    );
    println!(
        "{:<10} {:>7} {:>9} {:>8} {:>9} {:>9} {:>9} {:>8} {:>8} {:>11}",
        "mode",
        "overlap",
        "stale_acc",
        "sup_acc",
        "stale_rej",
        "valid_rej",
        "lost_runs",
        "lost_upd",
        "tok_back",
        "check_abort"
    );
    for mode in Mode::ALL {
        let r = sweep(seed, runs, mode, SweepParams::LAB);
        println!(
            "{:<10} {:>7} {:>9} {:>8} {:>9} {:>9} {:>9} {:>8} {:>8} {:>11}",
            mode.name(),
            r.runs_overlap,
            r.stale_accepted,
            r.superseded_accepted,
            r.stale_rejected,
            r.valid_rejected,
            r.runs_lost,
            r.lost_updates,
            r.runs_token_regress,
            r.aborted_by_check
        );
    }
    // Control: the same sweep without any pause, summed over the three modes.
    let calm: Vec<SweepRow> = Mode::ALL
        .iter()
        .map(|&m| sweep(seed, runs, m, SweepParams::NO_PAUSE))
        .collect();
    println!(
        "no pauses (p=0), all 3 modes: overlap {}, stale writes {}, lost updates {}, check aborts {}",
        calm.iter().map(|r| r.runs_overlap).sum::<u32>(),
        calm.iter()
            .map(|r| r.stale_accepted + r.stale_rejected + r.valid_rejected)
            .sum::<u64>(),
        calm.iter().map(|r| r.lost_updates).sum::<u64>(),
        calm.iter().map(|r| r.aborted_by_check).sum::<u64>()
    );
}

fn why(r: &Reject) -> String {
    match r {
        Reject::StaleToken { token, highest } => format!("{token}<{highest}"),
        Reject::VersionMismatch { read, current } => format!("read v{read}, now v{current}"),
    }
}

/// Prints the log; lines already present in `skip` are omitted.
fn print_log(r: &RunReport, skip: &[LogEntry]) {
    for e in r.log.iter().filter(|e| !skip.contains(e)) {
        let who = e.client.map_or('-', client_name);
        println!("  t={:>6} {who} {}", e.t, e.text);
    }
}
