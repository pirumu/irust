// Not part of the Cargo build. Compile by hand to see the error:
//   rustc --edition 2024 compile-fail/rc_across_threads.rs -o /tmp/p03-cf
use std::rc::Rc;
use std::thread;

fn main() {
    let shared = Rc::new(0);
    let clone = Rc::clone(&shared);
    let handle = thread::spawn(move || println!("{clone}"));
    handle.join().unwrap();
}
