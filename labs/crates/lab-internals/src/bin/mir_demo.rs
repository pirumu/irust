//! Lab (b): a tiny program whose MIR we read.
//!
//! Dump MIR on stable (no nightly needed):
//!   cargo rustc -p lab-internals --bin mir_demo -- --emit=mir
//! The file lands in `target/debug/deps/mir_demo-<hash>.mir`.

#[inline(never)]
fn add_then_double(a: i32, b: i32) -> i32 {
    let s = a + b;
    s * 2
}

/// Accepted thanks to non-lexical lifetimes: `first` is last used before `push`.
#[inline(never)]
fn nll_ok() -> usize {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    let copied = *first;
    v.push(copied);
    v.len()
}

fn main() {
    println!("add_then_double(3, 4) = {}", add_then_double(3, 4));
    println!("nll_ok() = {}", nll_ok());
}
