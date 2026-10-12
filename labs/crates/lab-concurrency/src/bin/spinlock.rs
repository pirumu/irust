//! Lab (d): the hand-written spin lock from `lab_concurrency::spinlock`.
//!
//!   cargo run --release -p lab-concurrency --bin spinlock [ITERS_PER_THREAD]
//!
//! Part 1 checks that Acquire/Release is enough for mutual exclusion.
//! Part 2 runs the same loop through a lock whose orderings are all Relaxed (wrong on
//!        purpose) and reports whether increments went missing on this machine.
//! Part 3 runs more threads than CPUs, where a spinning waiter burns its time slice
//!        while the holder is descheduled, and compares with std::sync::Mutex.

use std::sync::Mutex;
use std::thread;

use lab_concurrency::spinlock::{BrokenRelaxedSpinLock, SpinLock};
use lab_concurrency::{arg_or, timed};

fn hammer(threads: u64, iters: u64, body: impl Fn() + Sync) {
    thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                for _ in 0..iters {
                    body();
                }
            });
        }
    });
}

fn main() {
    let iters = arg_or(1_000_000);
    let cpus = thread::available_parallelism().map_or(1, |n| n.get()) as u64;
    println!(
        "cpus={cpus} iters/thread={iters} arch={}",
        std::env::consts::ARCH
    );

    println!("-- part 1: SpinLock (Acquire/Release), 4 threads");
    let lock = SpinLock::new(0u64);
    let ((), t) = timed(|| hammer(4, iters, || *lock.lock() += 1));
    println!(
        "total={} expected={} in {:?}",
        lock.into_inner(),
        4 * iters,
        t
    );

    println!("-- part 2: BrokenRelaxedSpinLock (all Relaxed), 4 threads, 5 rounds");
    for round in 1..=5 {
        let lock = BrokenRelaxedSpinLock::new(0u64);
        hammer(4, iters, || lock.with(|v| *v += 1));
        let total = lock.into_inner();
        println!(
            "round {round}: total={total} expected={} lost={}",
            4 * iters,
            4 * iters - total
        );
    }

    let threads = 4 * cpus;
    let small = (iters / 20).max(1);
    println!("-- part 3: {threads} threads (4 x cpus), {small} iters/thread");
    let lock = SpinLock::new(0u64);
    let ((), t) = timed(|| hammer(threads, small, || *lock.lock() += 1));
    println!("SpinLock: total={} in {:?}", lock.into_inner(), t);
    let m = Mutex::new(0u64);
    let ((), t) = timed(|| hammer(threads, small, || *m.lock().unwrap() += 1));
    println!("Mutex:    total={} in {:?}", m.into_inner().unwrap(), t);
}
