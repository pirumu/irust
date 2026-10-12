//! Lab (c): watch std::sync::Mutex turn into futex(2) calls only under contention.
//!
//!   ./labs/linux.sh bash -c 'cargo build -p lab-concurrency --bin futex_contention &&
//!       strace -f -e trace=futex,write ./target-linux/debug/futex_contention'
//!
//! The program writes a marker line to stderr (one write(2) each) before every phase, so
//! the strace log shows which futex calls belong to which phase.

#[cfg(target_os = "linux")]
fn main() {
    use std::hint::black_box;
    use std::io::Write;
    use std::sync::Mutex;
    use std::thread;

    fn marker(text: &str) {
        // One write_all of a short buffer to unbuffered stderr = one write(2) in strace.
        std::io::stderr().write_all(text.as_bytes()).unwrap();
    }

    const UNCONTENDED: u64 = 100_000;
    const THREADS: u64 = 4;
    const CONTENDED: u64 = 20_000;

    let m = Mutex::new(0u64);
    marker(&format!(
        "phase0 mutex at {:p}, size {} bytes\n",
        &m,
        size_of_val(&m)
    ));

    marker("phase1 start: 1 thread, uncontended\n");
    for _ in 0..UNCONTENDED {
        *m.lock().unwrap() += 1;
    }
    marker("phase1 end\n");

    marker("phase2 start: 4 threads, contended\n");
    thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                for _ in 0..CONTENDED {
                    let mut g = m.lock().unwrap();
                    // A little work inside the critical section so the lock is held long
                    // enough for the other threads to find it taken.
                    for _ in 0..50 {
                        *g = black_box(*g + 1);
                    }
                    *g -= 49;
                }
            });
        }
    });
    marker("phase2 end\n");

    let total = m.into_inner().unwrap();
    println!(
        "total = {total} (expected {})",
        UNCONTENDED + THREADS * CONTENDED
    );
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_concurrency::not_linux("futex_contention");
}
