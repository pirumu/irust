//! Lab (b): two threads, `counter := counter + 1` in two steps: lost update, a lock, a lock with
//! the wrong scope.
//!
//!   cargo run -q -p lab-tla --bin lost_update [NUM_THREADS]     (default 2)
//!
//! The Rust model is what `pcal.trans` would produce from `specs/CounterLost.tla`,
//! `specs/CounterLock.tla` and `specs/CounterLockSplit.tla`.

use lab_tla::counter::Counter;
use lab_tla::mc::{check_graph, explore, render_report};

fn main() {
    let threads: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(2);
    for (model, title) in [
        (
            Counter::lost_update(threads),
            "CounterLost: Load, Store (no lock)",
        ),
        (
            Counter::with_lock(threads),
            "CounterLock: Acquire, Load, Store, Release",
        ),
        (
            Counter::lock_split(threads),
            "CounterLockSplit: lock released between Load and Store",
        ),
    ] {
        let graph = explore(&model);
        let report = check_graph(&graph, &model.invariants(), true);
        print!(
            "{}",
            render_report(&format!("{title}, NumThreads = {threads}"), &report, &|s| {
                model.show(s)
            })
        );
        // `Correct` is an implication from AllDone; it says nothing unless AllDone is reachable.
        println!(
            "AllDone holds in {} of {} states, so Correct is not vacuous",
            graph.count(|s| model.all_done(s)),
            graph.stats.distinct
        );
        println!();
    }
}
