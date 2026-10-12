//! Lab (b): a counting #[global_allocator] that wraps `System`, to watch what
//! Vec, String, Box and Rc ask the allocator for.
//!
//! Runs on any OS: cargo run -p lab-memory --bin count_alloc

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);
static DEALLOCS: AtomicUsize = AtomicUsize::new(0);
static REALLOCS: AtomicUsize = AtomicUsize::new(0);
static LAST_SIZE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the caller's arguments unchanged,
// so `System` upholds the GlobalAlloc contract; the counters do not allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        LAST_SIZE.store(layout.size(), Relaxed);
        // SAFETY: same contract as our caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOCS.fetch_add(1, Relaxed);
        // SAFETY: same contract as our caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        REALLOCS.fetch_add(1, Relaxed);
        LAST_SIZE.store(new_size, Relaxed);
        // SAFETY: same contract as our caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

#[derive(Clone, Copy, PartialEq)]
struct Counts {
    alloc: usize,
    dealloc: usize,
    realloc: usize,
}

fn counts() -> Counts {
    Counts {
        alloc: ALLOCS.load(Relaxed),
        dealloc: DEALLOCS.load(Relaxed),
        realloc: REALLOCS.load(Relaxed),
    }
}

/// Runs `f` and returns how many alloc/dealloc/realloc calls it made.
/// Nothing is printed inside the measured region, because printing may allocate.
fn measure<R>(f: impl FnOnce() -> R) -> (R, Counts) {
    let before = counts();
    let r = f();
    let after = counts();
    (
        r,
        Counts {
            alloc: after.alloc - before.alloc,
            dealloc: after.dealloc - before.dealloc,
            realloc: after.realloc - before.realloc,
        },
    )
}

fn show(what: &str, c: Counts) {
    println!(
        "{what:<44} alloc={} realloc={} dealloc={}",
        c.alloc, c.realloc, c.dealloc
    );
}

struct Noisy(&'static str);

impl Drop for Noisy {
    fn drop(&mut self) {
        println!("  drop({})", self.0);
    }
}

fn main() {
    // Warm up stdout so its buffer allocation does not land inside a measurement.
    println!("== sizes on the stack (size_of) ==");
    use std::mem::{align_of, size_of};
    println!(
        "u64={} &u64={} Box<u64>={} Option<Box<u64>>={} &[u8]={} Vec<u8>={} String={} Rc<u64>={}",
        size_of::<u64>(),
        size_of::<&u64>(),
        size_of::<Box<u64>>(),
        size_of::<Option<Box<u64>>>(),
        size_of::<&[u8]>(),
        size_of::<Vec<u8>>(),
        size_of::<String>(),
        size_of::<Rc<u64>>(),
    );
    println!(
        "align_of: u8={} u32={} u64={} (u8,u64)={} size_of (u8,u64)={}",
        align_of::<u8>(),
        align_of::<u32>(),
        align_of::<u64>(),
        align_of::<(u8, u64)>(),
        size_of::<(u8, u64)>(),
    );

    println!("\n== Vec<u64>::push growth ==");
    let mut v: Vec<u64> = Vec::new();
    println!("Vec::new(): len={} capacity={}", v.len(), v.capacity());
    let mut last_cap = v.capacity();
    let mut events = Vec::with_capacity(32); // reserved up front so recording does not allocate
    let (_, total) = measure(|| {
        for i in 0..1000_u64 {
            let before = counts();
            v.push(i);
            if v.capacity() != last_cap {
                let after = counts();
                events.push((
                    v.len(),
                    last_cap,
                    v.capacity(),
                    after.alloc - before.alloc,
                    after.realloc - before.realloc,
                    LAST_SIZE.load(Relaxed),
                ));
                last_cap = v.capacity();
            }
        }
    });
    for (len, old, new, a, r, bytes) in &events {
        println!(
            "push #{len:<5} capacity {old:>4} -> {new:<5} alloc={a} realloc={r} bytes requested={bytes}"
        );
    }
    show("total for 1000 pushes", total);

    let (_, c) = measure(|| {
        let mut w: Vec<u64> = Vec::with_capacity(1000);
        for i in 0..1000 {
            w.push(i);
        }
        drop(black_box(w));
    });
    show("Vec::with_capacity(1000) + 1000 pushes + drop", c);

    let (_, c) = measure(|| {
        v.clear();
        for i in 0..1000 {
            v.push(i);
        }
    });
    show("clear() then refill same Vec to len 1000", c);

    println!("\n== Box, String, Rc ==");
    let (_, c) = measure(|| drop(black_box(Box::new(5_u64))));
    show("Box::new(5u64), then dropped", c);
    let (_, c) = measure(|| drop(black_box(Box::new(()))));
    show("Box::new(()) (zero-sized), then dropped", c);
    let (s, c) = measure(|| black_box(String::from("hello")));
    show("String::from(\"hello\")", c);
    let (moved, c) = measure(move || s); // a move copies ptr/len/cap, the heap buffer stays put
    show("move the String into another binding", c);
    let (_, c) = measure(move || drop(moved));
    show("drop the moved String", c);
    let ((strong, bytes), c) = measure(|| {
        let a = Rc::new(5_u64);
        let b = Rc::clone(&a);
        (Rc::strong_count(&b), LAST_SIZE.load(Relaxed))
    });
    show("Rc::new + Rc::clone, then both dropped", c);
    println!("  strong_count was {strong}; the one allocation asked for {bytes} bytes");

    println!("\n== leaking is safe ==");
    let (_, c) = measure(|| std::mem::forget(Box::new(1_u64)));
    show("mem::forget(Box::new(1))", c);
    let (leaked, c) = measure(|| Box::leak(Box::new(2_u64)));
    show("Box::leak(Box::new(2)) -> &'static mut u64", c);
    *leaked += 1;
    println!("leaked value is still usable: {leaked}");

    println!("\n== drop order ==");
    {
        let _a = Noisy("a, declared first");
        let _b = Noisy("b, declared second");
        let _pair = (Noisy("tuple.0"), Noisy("tuple.1"));
        let moved = Noisy("moved into drop() early");
        drop(moved);
        println!("  end of scope");
    }
}
