//! Lab (d): which system calls does the allocator make for small vs large blocks?
//!
//! Run under strace on Linux:
//!   ./labs/linux.sh bash -c 'cargo build -p lab-memory --bin big_alloc && \
//!     strace -e trace=brk,mmap,munmap,write ./target-linux/debug/big_alloc'
//!
//! Each step prints a marker on stderr first, so its write(2) shows up in the
//! strace output right before the brk/mmap/munmap calls of that step.

use std::hint::black_box;
use std::io::Write;

fn step(label: &str, size: usize) -> Vec<u8> {
    // One pre-formatted buffer = one write(2); `eprintln!` with arguments would split into several.
    let marker = format!("--- {label}: Vec<u8> of {size} bytes ---\n");
    let _ = std::io::stderr().write_all(marker.as_bytes());
    // vec![1; n] asks the global allocator (System -> malloc) for n bytes, then fills them.
    black_box(vec![1_u8; size])
}

fn main() {
    let small = step("small 1 KiB", 1 << 10);
    let medium = step("medium 64 KiB", 64 << 10);
    let large = step("large 1 MiB", 1 << 20);
    eprintln!("--- free the 1 MiB block ---");
    drop(large);
    // glibc raises its mmap threshold when an mmap'd block of up to
    // DEFAULT_MMAP_THRESHOLD_MAX is freed (mallopt(3)), so the same request may now
    // come from the heap instead of a fresh mmap.
    let again = step("large 1 MiB again, after the free", 1 << 20);
    eprintln!("--- done ---");
    black_box((&small, &medium, &again));
}
