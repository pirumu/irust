//! A counting `GlobalAlloc`, per thread (chapter 03 §6).
//!
//! The counters are `thread_local!` `Cell<u64>`s with a `const` initialiser and no destructor,
//! so reading or bumping them never allocates (an allocator that allocates would recurse) and
//! a measurement only sees what the *calling thread* asked the allocator for. That is what lets
//! the unit tests of this crate run in parallel without polluting each other's counts, and lets
//! each worker thread of the `latency` lab report its own allocations per request.
//!
//! A library crate cannot decide for its binaries whether to install a global allocator, so
//! each binary that wants the counts declares
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: lab_observability::alloc::CountingAlloc = lab_observability::alloc::CountingAlloc;
//! ```
//!
//! and the unit tests of this crate install it under `#[cfg(test)]` below.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static REALLOCS: Cell<u64> = const { Cell::new(0) };
    static FREES: Cell<u64> = const { Cell::new(0) };
    static BYTES: Cell<u64> = const { Cell::new(0) };
}

fn bump(counter: &'static std::thread::LocalKey<Cell<u64>>, by: u64) {
    counter.with(|c| c.set(c.get() + by));
}

/// Forwards everything to [`System`] and counts what passes through, for the calling thread.
pub struct CountingAlloc;

// SAFETY: every method forwards to `System` with the caller's arguments unchanged, so `System`
// upholds the GlobalAlloc contract; the counters are plain thread-local integers and never allocate.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        bump(&ALLOCS, 1);
        bump(&BYTES, layout.size() as u64);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        bump(&ALLOCS, 1);
        bump(&BYTES, layout.size() as u64);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        bump(&FREES, 1);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        bump(&REALLOCS, 1);
        bump(&BYTES, new_size as u64);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// What the calling thread has asked the allocator for. `bytes` adds up the *requested* size of
/// every `alloc` and the *new* size of every `realloc`; it is not live memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub allocs: u64,
    pub reallocs: u64,
    pub frees: u64,
    pub bytes: u64,
}

impl Stats {
    /// Calls that reached the allocator to obtain or grow memory: `alloc` + `realloc`.
    pub fn events(&self) -> u64 {
        self.allocs + self.reallocs
    }

    fn since(self, earlier: Stats) -> Stats {
        Stats {
            allocs: self.allocs - earlier.allocs,
            reallocs: self.reallocs - earlier.reallocs,
            frees: self.frees - earlier.frees,
            bytes: self.bytes - earlier.bytes,
        }
    }
}

/// The calling thread's running totals. All zero unless a [`CountingAlloc`] is the global allocator.
pub fn thread_stats() -> Stats {
    Stats {
        allocs: ALLOCS.with(Cell::get),
        reallocs: REALLOCS.with(Cell::get),
        frees: FREES.with(Cell::get),
        bytes: BYTES.with(Cell::get),
    }
}

/// Runs `f` and returns its result plus what *this thread* asked the allocator for meanwhile.
/// Nothing is printed inside the measured region, because printing may allocate.
pub fn measure<R>(f: impl FnOnce() -> R) -> (R, Stats) {
    let before = thread_stats();
    let out = f();
    (out, thread_stats().since(before))
}

#[cfg(test)]
#[global_allocator]
static TEST_ALLOC: CountingAlloc = CountingAlloc;

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;

    #[test]
    fn counts_a_box_and_its_free() {
        let (b, s) = measure(|| black_box(Box::new([7u8; 100])));
        assert_eq!((s.allocs, s.reallocs, s.bytes), (1, 0, 100));
        assert_eq!(s.frees, 0, "the box is still alive");
        let ((), s) = measure(|| drop(b));
        assert_eq!((s.allocs, s.frees), (0, 1));
    }

    /// A `Vec` that grows by pushing reaches the allocator a logarithmic number of times;
    /// one built with the right capacity reaches it exactly once (chapter 03 §6: `with_capacity`).
    #[test]
    fn with_capacity_beats_growing_by_pushing() {
        const N: usize = 1000;
        let (v, grown) = measure(|| {
            let mut v = Vec::new();
            for i in 0..N {
                v.push(black_box(i as u64));
            }
            v
        });
        let (w, sized) = measure(|| {
            let mut w = Vec::with_capacity(N);
            for i in 0..N {
                w.push(black_box(i as u64));
            }
            w
        });
        assert_eq!(v, w);
        assert_eq!(sized.events(), 1);
        assert!(grown.events() > 4, "growing by pushing made {} allocator calls", grown.events());
        assert!(sized.bytes < grown.bytes, "{} >= {}", sized.bytes, grown.bytes);
    }

    /// Reusing a `String` after `clear()` keeps its capacity, so the second round allocates nothing.
    #[test]
    fn reused_buffer_stops_allocating() {
        use std::fmt::Write;
        let mut buf = String::new();
        let (_, first) = measure(|| write!(buf, "request {} ok", black_box(12345)).unwrap());
        assert!(first.events() >= 1);
        buf.clear();
        let (_, second) = measure(|| write!(buf, "request {} ok", black_box(67890)).unwrap());
        assert_eq!(second.events(), 0, "{second:?}");
    }

    /// The counters belong to the thread that allocated: work on another thread is invisible here.
    #[test]
    fn counts_are_per_thread() {
        let ((), mine) = measure(|| {
            std::thread::spawn(|| {
                let v: Vec<u8> = black_box(vec![0; 4096]);
                black_box(v.len());
            })
            .join()
            .unwrap();
        });
        // Spawning allocates on this thread (the Thread handle, the packet); the 4096-byte Vec does not.
        assert!(mine.bytes < 4096, "{mine:?}");
    }
}
