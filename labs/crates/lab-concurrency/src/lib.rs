//! Phase 4 labs: concurrency.
//!
//! The library holds the pieces shared by the binaries in `src/bin/`:
//! a spin lock written after *Rust Atomics and Locks*, chapter 4, a tiny timing helper,
//! and the `compile_fail` doc tests that show which data races the compiler rejects.
//!
//! # Data races that do not compile
//!
//! Each block below is a doc test marked `compile_fail` with the expected error code;
//! `cargo test -p lab-concurrency --doc` passes only if every one of them is rejected
//! with that code. The full compiler messages are produced from the same programs in
//! `compile-fail/*.rs` (see `docs/labs/04-concurrency-lab.md`).
//!
//! An `Rc` moved into a spawned thread: `Rc` is not `Send`.
//!
//! ```compile_fail,E0277
//! use std::rc::Rc;
//! let shared = Rc::new(0);
//! let clone = Rc::clone(&shared);
//! std::thread::spawn(move || println!("{clone}"));
//! ```
//!
//! Two threads mutating the same local variable: two `&mut` borrows at once.
//!
//! ```compile_fail,E0499
//! let mut counter = 0;
//! std::thread::scope(|s| {
//!     s.spawn(|| counter += 1);
//!     s.spawn(|| counter += 1);
//! });
//! ```
//!
//! A `RefCell` shared between threads: `RefCell` is not `Sync`.
//!
//! ```compile_fail,E0277
//! use std::cell::RefCell;
//! let cell = RefCell::new(0);
//! std::thread::scope(|s| {
//!     s.spawn(|| *cell.borrow_mut() += 1);
//! });
//! ```
//!
//! The same programs with the right types compile:
//!
//! ```
//! use std::sync::{Arc, Mutex};
//! let shared = Arc::new(Mutex::new(0));
//! let clone = Arc::clone(&shared);
//! std::thread::spawn(move || *clone.lock().unwrap() += 1).join().unwrap();
//! assert_eq!(*shared.lock().unwrap(), 1);
//! ```

pub mod spinlock;

use std::time::{Duration, Instant};

/// Runs `f` once and returns its result together with the wall-clock time it took.
pub fn timed<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let out = f();
    (out, start.elapsed())
}

/// Reads a positive integer from the first command-line argument, or returns `default`.
pub fn arg_or(default: u64) -> u64 {
    std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

/// Printed by binaries that only make sense on Linux when they are built for another OS.
pub fn not_linux(bin: &str) {
    println!(
        "[{bin}] this lab needs Linux (futex, /proc). Run it in the container:\n  \
         ./labs/linux.sh cargo run -p lab-concurrency --bin {bin}"
    );
}
