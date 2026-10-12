//! Lab (a): does this checker count states the way TLC does?
//!
//!   cargo run -q -p lab-tla --bin calibrate
//!
//! Four runs whose TLC numbers are written down in the `tlaplus/Examples` repository
//! (`manifest.json` next to each spec, and a comment in `TwoPhase.tla`). The Rust model is
//! compared with the recorded number; nothing here runs TLC.

use lab_tla::commit::{TCommit, TwoPhase};
use lab_tla::mc::{Stats, check, explore};
use lab_tla::simple::{Pc, Simple};

struct Row {
    model: &'static str,
    config: &'static str,
    ours: Stats,
    /// (distinct, generated, depth) as recorded; `None` where the source gives no number.
    recorded: (usize, Option<usize>, Option<usize>),
    /// The invariants of the cfg hold and the deadlock check behaves as the cfg says.
    ok: bool,
}

fn main() {
    let tcommit = TCommit::new(3);
    let two_phase3 = TwoPhase::new(3);
    let two_phase6 = TwoPhase::new(6);
    let simple = Simple { n: 5 };

    // TCommit.cfg says CHECK_DEADLOCK FALSE; TwoPhase.cfg and Simple.cfg keep the default.
    let rows = [
        {
            let r = check(&tcommit, &tcommit.invariants(), false);
            Row {
                model: "TCommit",
                config: "RM = {r1,r2,r3}",
                ours: r.stats,
                recorded: (34, Some(94), Some(7)),
                ok: r.ok(),
            }
        },
        {
            let r = check(&two_phase3, &two_phase3.invariants(), true);
            Row {
                model: "TwoPhase",
                config: "RM = {r1,r2,r3}",
                ours: r.stats,
                recorded: (288, Some(1146), Some(11)),
                ok: r.ok(),
            }
        },
        {
            let r = check(&two_phase6, &two_phase6.invariants(), true);
            Row {
                model: "TwoPhase",
                config: "six RMs",
                ours: r.stats,
                recorded: (50816, None, None),
                ok: r.ok(),
            }
        },
        {
            let r = check(&simple, &simple.invariants(), true);
            Row {
                model: "Simple (PlusCal)",
                config: "N = 5",
                ours: r.stats,
                recorded: (723, Some(1842), Some(11)),
                ok: r.ok(),
            }
        },
    ];

    println!(
        "{:<17} {:<16} {:>8} {:>10} {:>6}   {:<22} agree",
        "model", "config", "distinct", "generated", "depth", "recorded (d / g / depth)"
    );
    let mut all_agree = true;
    let mut all_ok = true;
    for row in &rows {
        let (d, g, depth) = row.recorded;
        let agree = row.ours.distinct == d
            && g.is_none_or(|g| g == row.ours.generated)
            && depth.is_none_or(|x| x == row.ours.depth);
        all_agree &= agree;
        all_ok &= row.ok;
        let show = |x: Option<usize>| x.map_or("-".to_string(), |x| x.to_string());
        println!(
            "{:<17} {:<16} {:>8} {:>10} {:>6}   {:<22} {}",
            row.model,
            row.config,
            row.ours.distinct,
            row.ours.generated,
            row.ours.depth,
            format!("{d} / {} / {}", show(g), show(depth)),
            if agree { "yes" } else { "NO" }
        );
    }
    println!("every recorded number is reproduced: {all_agree}");
    println!("the invariants of the cfg files hold, with each cfg's deadlock setting: {all_ok}");

    // Why `generated` is 1842 and not less: the PlusCal translation's `Terminating` action is a
    // step from every all-Done state to itself, and TLC counts it as a generated state.
    let g = explore(&simple);
    let finished = g.count(|s| s.pc.iter().all(|&p| p == Pc::Done));
    println!(
        "Simple: {finished} of {} states have every pc = Done and each has a Terminating step to itself; \
         without those self-loops `generated` would be {}, not {}",
        g.stats.distinct,
        g.stats.generated - finished,
        g.stats.generated
    );
}
