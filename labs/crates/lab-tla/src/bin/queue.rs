//! Lab (d): a backend model, the at-least-once queue with an idempotent consumer
//! (P13 chapter 09, section 3).
//!
//!   cargo run -q -p lab-tla --bin queue
//!
//! The Rust models mirror `specs/AtLeastOnce.tla` (`Dedup = FALSE` and `Dedup = TRUE`) and
//! `specs/AtLeastOnceSplit.tla`. The deadlock check is off, as in the cfg files: once every
//! message is acknowledged nothing can move, and the broker's loop never terminates.

use lab_tla::mc::{check, explore, render_report};
use lab_tla::queue::Queue;

fn main() {
    let runs = [
        (
            Queue::naive(1, 1, 1),
            "one message, one consumer, one lost ack",
        ),
        (
            Queue::naive(1, 2, 0),
            "one message, two consumers, no crash at all",
        ),
        (
            Queue::idempotent(1, 2, 2),
            "dedup in the same atomic step as the effect",
        ),
        (
            Queue::split(1, 2, 0),
            "dedup recorded in a separate step from the effect",
        ),
    ];
    for (model, what) in runs {
        let report = check(&model, &model.invariants(), false);
        print!(
            "{}",
            render_report(&format!("{}: {what}", model.name), &report, &|s| model
                .show(s))
        );
        println!();
    }

    // How the state space grows with the parameters (chapter 04, section 8).
    println!("== growth of the idempotent model with its parameters ==");
    println!(
        "{:>5} {:>9} {:>8} {:>9} {:>10} {:>6}",
        "msgs", "consumers", "crashes", "distinct", "generated", "depth"
    );
    for (msgs, consumers, crashes) in [
        (1, 1, 1),
        (1, 2, 1),
        (2, 1, 1),
        (2, 2, 1),
        (2, 2, 2),
        (3, 2, 1),
    ] {
        let s = explore(&Queue::idempotent(msgs, consumers, crashes)).stats;
        println!(
            "{msgs:>5} {consumers:>9} {crashes:>8} {:>9} {:>10} {:>6}",
            s.distinct, s.generated, s.depth
        );
    }
}
