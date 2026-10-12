//! Lab (b): one shared counter, several ways to increment it from many threads.
//!
//!   cargo run --release -p lab-concurrency --bin counter [ITERS_PER_THREAD]
//!
//! Every variant runs THREADS threads that each add 1 ITERS times. The expected total is
//! THREADS * ITERS; the "racy" variant shows a race condition that is NOT a data race:
//! each access is atomic, but load-then-store is not, so increments get lost.

use std::hint::black_box;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::thread;
use std::time::Duration;

use lab_concurrency::spinlock::SpinLock;
use lab_concurrency::{arg_or, timed};

const THREADS: u64 = 4;

fn run_threads(iters: u64, body: impl Fn() + Sync) {
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..iters {
                    body();
                }
            });
        }
    });
}

fn report(name: &str, got: u64, expected: u64, took: Duration) {
    let verdict = if got == expected {
        "ok"
    } else {
        "LOST UPDATES"
    };
    println!(
        "{name:<28} total={got:>10} expected={expected:>10} {verdict:<12} {:>8.1} ms",
        took.as_secs_f64() * 1e3
    );
}

fn main() {
    let iters = arg_or(1_000_000);
    let expected = THREADS * iters;
    println!(
        "threads={THREADS} iters/thread={iters} available_parallelism={:?}",
        thread::available_parallelism().map(|n| n.get()).ok()
    );

    let m = Mutex::new(0u64);
    let ((), t) = timed(|| run_threads(iters, || *m.lock().unwrap() += 1));
    report("Mutex<u64>", m.into_inner().unwrap(), expected, t);

    let sl = SpinLock::new(0u64);
    let ((), t) = timed(|| run_threads(iters, || *sl.lock() += 1));
    report("SpinLock<u64> (ch.4)", sl.into_inner(), expected, t);

    let a = AtomicU64::new(0);
    let ((), t) = timed(|| {
        run_threads(iters, || {
            a.fetch_add(1, SeqCst);
        })
    });
    report("AtomicU64 fetch_add SeqCst", a.into_inner(), expected, t);

    let a = AtomicU64::new(0);
    let ((), t) = timed(|| {
        run_threads(iters, || {
            a.fetch_add(1, Relaxed);
        })
    });
    report("AtomicU64 fetch_add Relaxed", a.into_inner(), expected, t);

    // Two separate atomic operations: another thread can store in between.
    let a = AtomicU64::new(0);
    let ((), t) = timed(|| {
        run_threads(iters, || {
            let v = a.load(Relaxed);
            a.store(v + 1, Relaxed);
        })
    });
    report("racy load+store Relaxed", a.into_inner(), expected, t);

    // No sharing while counting: each thread sums locally and publishes once.
    let a = AtomicU64::new(0);
    let ((), t) = timed(|| {
        thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    let mut local = 0u64;
                    for _ in 0..iters {
                        local = black_box(local + 1);
                    }
                    a.fetch_add(local, Relaxed);
                });
            }
        })
    });
    report("thread-local, 1 fetch_add", a.into_inner(), expected, t);
}
