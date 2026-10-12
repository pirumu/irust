// Not part of the Cargo build. Compile by hand to see the error:
//   rustc --edition 2024 compile-fail/two_mut_borrows.rs -o /tmp/p03-cf
use std::thread;

fn main() {
    let mut counter = 0;
    thread::scope(|s| {
        s.spawn(|| counter += 1);
        s.spawn(|| counter += 1);
    });
    println!("{counter}");
}
