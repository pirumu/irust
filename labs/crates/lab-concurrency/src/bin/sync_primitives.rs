//! Extra lab: the blocking primitives of std::sync in one program.
//!
//!   cargo run -p lab-concurrency --bin sync_primitives
//!
//! 1. Condvar: a bounded queue; producer and consumer wait in a `while` loop on a predicate.
//! 2. mpsc: three producers, one consumer; the loop ends when every Sender is dropped.
//! 3. Poisoning: a thread panics while holding a Mutex; the next lock() returns Err.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Condvar, Mutex};
use std::thread;

fn condvar_queue() {
    const CAPACITY: usize = 2;
    const ITEMS: u32 = 6;
    let queue = Mutex::new(VecDeque::new());
    let not_empty = Condvar::new();
    let not_full = Condvar::new();

    thread::scope(|s| {
        s.spawn(|| {
            for i in 0..ITEMS {
                let mut q = queue.lock().unwrap();
                // Re-check after every wake-up: wake-ups can be spurious, and another
                // thread may have changed the state before we got the lock back.
                while q.len() == CAPACITY {
                    q = not_full.wait(q).unwrap();
                }
                q.push_back(i);
                println!("  producer pushed {i} (len {})", q.len());
                not_empty.notify_one();
            }
        });
        s.spawn(|| {
            for _ in 0..ITEMS {
                let mut q = queue.lock().unwrap();
                while q.is_empty() {
                    q = not_empty.wait(q).unwrap();
                }
                let item = q.pop_front().unwrap();
                println!("  consumer popped {item} (len {})", q.len());
                not_full.notify_one();
            }
        });
    });
}

fn channel() {
    let (tx, rx) = mpsc::channel();
    for id in 0..3 {
        let tx = tx.clone();
        thread::spawn(move || {
            for n in 0..2 {
                tx.send(format!("producer {id} message {n}")).unwrap();
            }
        });
    }
    // Drop the original Sender: the receive loop ends once every clone is gone too.
    drop(tx);
    let mut got: Vec<String> = rx.iter().collect();
    got.sort();
    for m in &got {
        println!("  {m}");
    }
    println!(
        "  received {} messages; rx.recv() now = {:?}",
        got.len(),
        rx.recv()
    );
}

fn poisoning() {
    let m = Mutex::new(vec![1, 2, 3]);
    let r = thread::scope(|s| {
        s.spawn(|| {
            let mut v = m.lock().unwrap();
            v.push(4);
            panic!("panic while holding the lock");
        })
        .join()
    });
    println!("  worker join() is_err = {}", r.is_err());
    println!("  is_poisoned() = {}", m.is_poisoned());
    match m.lock() {
        Ok(v) => println!("  lock() = Ok({v:?})"),
        Err(poisoned) => {
            let v = poisoned.into_inner();
            println!("  lock() = Err(PoisonError); into_inner() still gives the data: {v:?}");
        }
    }
}

fn main() {
    println!("-- 1. Condvar bounded queue (capacity 2)");
    condvar_queue();
    println!("-- 2. mpsc: 3 producers, 1 consumer");
    channel();
    println!("-- 3. Mutex poisoning");
    poisoning();
}
