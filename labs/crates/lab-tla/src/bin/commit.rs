//! Lab (c): atomic commit. `TCommit` is the requirement, `TwoPhase` the protocol, each with a
//! deliberately broken variant, and `TwoPhase => TCommit` is checked as a refinement.
//!
//!   cargo run -q -p lab-tla --bin commit [NUM_RMS]     (default 3)
//!
//! The Rust models mirror `specs/TCommit.tla` and `specs/TwoPhase.tla`.

use lab_tla::commit::{TCommit, TwoPhase};
use lab_tla::mc::{check, check_refinement, render_report, render_trace};

fn main() {
    let rms: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(3);

    // CHECK_DEADLOCK FALSE, as in TCommit.cfg: all-committed and all-aborted have no successor.
    for (model, title) in [
        (TCommit::new(rms), "TCommit"),
        (
            TCommit::buggy(rms),
            "TCommit with Buggy = TRUE (abort without notCommitted)",
        ),
    ] {
        let report = check(&model, &model.invariants(), false);
        print!(
            "{}",
            render_report(
                &format!("{title}, RM = {rms}"),
                &report,
                &|s| TCommit::show(s)
            )
        );
        println!();
    }

    // TwoPhase.cfg keeps the default deadlock check.
    for (model, title) in [
        (TwoPhase::new(rms), "TwoPhase"),
        (
            TwoPhase::buggy(rms),
            "TwoPhase with Buggy = TRUE (TMCommit without tmPrepared = RM)",
        ),
    ] {
        let report = check(&model, &model.invariants(), true);
        print!(
            "{}",
            render_report(&format!("{title}, RM = {rms}"), &report, &TwoPhase::show)
        );
        match check_refinement(&model, &TCommit::new(rms), TwoPhase::project) {
            Ok(stats) => println!(
                "refinement TwoPhase => TCommit: every step of the {} states maps to a TCommit step or a stutter",
                stats.distinct
            ),
            Err(failure) => {
                println!(
                    "refinement TwoPhase => TCommit: FAILS; shortest behaviour ending in a step TCommit does not have ({} states):",
                    failure.trace.len()
                );
                print!(
                    "{}",
                    render_trace::<TwoPhase>(&failure.trace, &TwoPhase::show)
                );
            }
        }
        println!();
    }
}
