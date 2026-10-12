//! Extra lab: false sharing. Two threads each update only their own counter; the only
//! difference between the two runs is whether the counters share a cache line.
//!
//!   cargo run --release -p lab-concurrency --bin false_sharing_counters [ITERS]
//!
//! The padded layout aligns each counter to 128 bytes: 64 bytes is the common line size,
//! and *Rust Atomics and Locks* ch. 7 notes some processors need 128-byte alignment.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::Relaxed;
use std::thread;
use std::time::Duration;

use lab_concurrency::{arg_or, timed};

#[repr(align(128))]
struct Padded(AtomicU64);

fn two_writers(a: &AtomicU64, b: &AtomicU64, iters: u64) -> Duration {
    let ((), t) = timed(|| {
        thread::scope(|s| {
            s.spawn(|| {
                for _ in 0..iters {
                    a.fetch_add(1, Relaxed);
                }
            });
            s.spawn(|| {
                for _ in 0..iters {
                    b.fetch_add(1, Relaxed);
                }
            });
        })
    });
    t
}

fn main() {
    let iters = arg_or(50_000_000);
    println!("iters/thread={iters}");

    let adjacent = [AtomicU64::new(0), AtomicU64::new(0)];
    let gap = (&adjacent[1] as *const _ as usize) - (&adjacent[0] as *const _ as usize);
    let t = two_writers(&adjacent[0], &adjacent[1], iters);
    println!(
        "adjacent (addresses {gap:>3} bytes apart): {:>8.1} ms",
        t.as_secs_f64() * 1e3
    );

    let padded = [Padded(AtomicU64::new(0)), Padded(AtomicU64::new(0))];
    let gap = (&padded[1] as *const _ as usize) - (&padded[0] as *const _ as usize);
    let t = two_writers(&padded[0].0, &padded[1].0, iters);
    println!(
        "padded   (addresses {gap:>3} bytes apart): {:>8.1} ms",
        t.as_secs_f64() * 1e3
    );
}
