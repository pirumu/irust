// Not a cargo target: compile by hand to see the borrow checker reject it.
//   rustc --edition 2024 --crate-type lib labs/crates/lab-internals/snippets/nll_error.rs -o /tmp/nll_error.rlib
// `first` is still used after `push`, so the shared borrow overlaps the mutable one.
pub fn nll_error() -> i32 {
    let mut v = vec![1, 2, 3];
    let first = &v[0];
    v.push(4);
    *first
}
