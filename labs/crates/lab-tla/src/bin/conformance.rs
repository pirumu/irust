//! Lab (f): differential testing of a concrete two-phase commit against the `TwoPhase` model,
//! in both directions, for the intended code and three injected bugs (P13 chapter 10).
//!
//!   cargo run -q -p lab-tla --bin conformance
//!
//! * model => code: replay a shortest path to every model state, then every step the model
//!   enables (model-based test-case generation, exhaustive for the finite model).
//! * code => model: proptest drives the code with arbitrary action sequences; every step the code
//!   accepts must be a model step (trace validation / state-machine property test).

use lab_tla::commit::{TpAction, TwoPhase};
use lab_tla::conform::{Divergence, DivergenceOf};
use lab_tla::sut::{Mutant, campaign, cover, replay};

const MUTANTS: [(Mutant, &str); 4] = [
    (Mutant::None, "intended code"),
    (Mutant::CountVotes, "coordinator counts votes"),
    (Mutant::AbortAfterVote, "RM may abort after voting"),
    (Mutant::StrictAbort, "TM never aborts after a vote"),
];

fn names(actions: &[TpAction]) -> String {
    actions
        .iter()
        .map(|a| format!("{a:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("{:34}{l}", ""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The divergence in words, with the model state in the layout of a TLC state.
fn describe(d: &DivergenceOf<TwoPhase>) -> String {
    match d {
        Divergence::ForbiddenStep {
            index,
            action,
            model_state,
        } => format!(
            "action {} ({action:?}): the code took it, the model forbids it in\n{}",
            index + 1,
            indent(&TwoPhase::show(model_state))
        ),
        Divergence::RefusedStep {
            action,
            model_state,
        } => {
            format!(
                "the code refused {action:?}, which the model enables in\n{}",
                indent(&TwoPhase::show(model_state))
            )
        }
        Divergence::ObservationMismatch {
            index,
            action,
            model,
            code,
        } => {
            format!(
                "action {} ({action:?}): model says {model}, code says {code}",
                index + 1
            )
        }
        Divergence::InitialMismatch { model, code } => {
            format!("initial state: model says {model}, code says {code}")
        }
    }
}

fn main() {
    println!("== model => code: transition cover of the TwoPhase graph, RM = 3 ==");
    for (mutant, what) in MUTANTS {
        match cover(3, mutant) {
            Ok(c) => println!(
                "{what:<30} ok: {} states, {} transitions replayed on the code",
                c.states, c.transitions
            ),
            Err(d) => println!("{what:<30} DIVERGES: {}", describe(&d).trim_start()),
        }
    }

    const RMS: usize = 2;
    const CASES: u32 = 10_000;
    const MAX_LEN: usize = 40;
    const SEED: u8 = 7;
    println!(
        "\n== code => model: {CASES} proptest walks of 1..={MAX_LEN} actions, RM = {RMS}, seed {SEED} =="
    );
    for (mutant, what) in MUTANTS {
        match campaign(RMS, mutant, CASES, MAX_LEN, SEED) {
            Ok(s) => println!(
                "{what:<30} passed: {} walks, {} of {} attempted steps accepted; {} walks ended committed, {} aborted",
                s.walks, s.accepted, s.attempted, s.committed, s.aborted
            ),
            Err(minimal) => {
                println!(
                    "{what:<30} DIVERGES; shrunk to {} actions: [{}]",
                    minimal.len(),
                    names(&minimal)
                );
                if let Some(d) = replay(RMS, mutant, &minimal) {
                    println!("{:30} {}", "", describe(&d).trim_start());
                }
            }
        }
    }
}
