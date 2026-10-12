//! Extra lab: a lock-order deadlock, detected with a timeout, and the fix.
//!
//!   cargo run -p lab-concurrency --bin deadlock
//!   ./labs/linux.sh cargo run -p lab-concurrency --bin deadlock   (also shows /proc state)
//!
//! Thread 1 locks A then B, thread 2 locks B then A. A Barrier makes both threads hold
//! their first lock before asking for the second, so the circular wait always happens.

use std::sync::mpsc;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

fn worker(
    name: &str,
    first: Arc<Mutex<u32>>,
    second: Arc<Mutex<u32>>,
    barrier: Arc<Barrier>,
    done: mpsc::Sender<String>,
) {
    let name = name.to_string();
    thread::Builder::new()
        .name(name.clone())
        .spawn(move || {
            let mut a = first.lock().unwrap();
            barrier.wait();
            let mut b = second.lock().unwrap();
            *a += 1;
            *b += 1;
            done.send(name).unwrap();
        })
        .unwrap();
}

fn run(order_t2_ab: bool) -> bool {
    let a = Arc::new(Mutex::new(0));
    let b = Arc::new(Mutex::new(0));
    // With a consistent order, the second thread cannot pass the barrier while holding A,
    // so it gets a barrier of its own size; with opposite orders both wait on one barrier.
    let (tx, rx) = mpsc::channel();
    if order_t2_ab {
        worker(
            "t1 A->B",
            a.clone(),
            b.clone(),
            Arc::new(Barrier::new(1)),
            tx.clone(),
        );
        worker(
            "t2 A->B",
            a.clone(),
            b.clone(),
            Arc::new(Barrier::new(1)),
            tx,
        );
    } else {
        let barrier = Arc::new(Barrier::new(2));
        worker("t1 A->B", a.clone(), b.clone(), barrier.clone(), tx.clone());
        worker("t2 B->A", b.clone(), a.clone(), barrier, tx);
    }
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(name) => println!("  {name} finished"),
            Err(_) => return false,
        }
    }
    true
}

#[cfg(target_os = "linux")]
fn show_threads() {
    // Field 3 of /proc/<pid>/task/<tid>/stat is the state; wchan names the kernel function
    // a sleeping task waits in (may read "0" if the kernel hides it).
    let mut tasks: Vec<_> = std::fs::read_dir("/proc/self/task")
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    tasks.sort();
    for t in tasks {
        let comm = std::fs::read_to_string(t.join("comm")).unwrap_or_default();
        let stat = std::fs::read_to_string(t.join("stat")).unwrap_or_default();
        let state = stat.rsplit(") ").next().and_then(|r| r.split(' ').next());
        let wchan = std::fs::read_to_string(t.join("wchan")).unwrap_or_default();
        println!(
            "  tid {:>6} comm={:<10} state={} wchan={}",
            t.file_name().unwrap().to_string_lossy(),
            comm.trim(),
            state.unwrap_or("?"),
            wchan.trim()
        );
    }
}

#[cfg(not(target_os = "linux"))]
fn show_threads() {
    println!("  (run in ./labs/linux.sh to see each thread's state in /proc)");
}

fn main() {
    println!("-- consistent order: both threads lock A then B");
    println!("  completed = {}", run(true));
    println!("-- opposite order: t1 locks A then B, t2 locks B then A");
    let ok = run(false);
    println!("  completed = {ok}");
    if !ok {
        println!("  no progress after 2 s: deadlock (circular wait). Threads of this process:");
        show_threads();
        // The two workers can never finish, so end the process instead of joining them.
        std::process::exit(0);
    }
}
