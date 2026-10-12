// Not part of the Cargo build. Compile by hand to see the error:
//   rustc --edition 2024 compile-fail/refcell_shared.rs -o /tmp/p03-cf
use std::cell::RefCell;
use std::thread;

fn main() {
    let cell = RefCell::new(0);
    thread::scope(|s| {
        s.spawn(|| *cell.borrow_mut() += 1);
    });
    println!("{}", cell.borrow());
}
