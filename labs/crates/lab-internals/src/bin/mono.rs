//! Lab (c): count monomorphized copies and compare static vs dynamic dispatch.
//!
//! Run:   cargo run -p lab-internals --release --bin mono
//! Count: nm target/release/mono | grep describe_generic
//! Asm:   cargo rustc -p lab-internals --release --bin mono -- --emit=asm

use std::fmt::Debug;
use std::hint::black_box;

trait Shape {
    fn area(&self) -> f64;
}

struct Square(f64);
struct Rect(f64, f64);

impl Shape for Square {
    fn area(&self) -> f64 {
        self.0 * self.0
    }
}

impl Shape for Rect {
    fn area(&self) -> f64 {
        self.0 * self.1
    }
}

/// One copy of machine code per concrete `T` (static dispatch).
#[inline(never)]
fn describe_generic<T: Debug>(x: T) -> String {
    format!("{x:?}")
}

/// One copy of machine code for every type; calls go through the vtable.
#[inline(never)]
fn describe_dyn(x: &dyn Debug) -> String {
    format!("{x:?}")
}

#[inline(never)]
fn total_area_static<S: Shape>(xs: &[S]) -> f64 {
    xs.iter().map(Shape::area).sum()
}

#[inline(never)]
fn total_area_dyn(xs: &[&dyn Shape]) -> f64 {
    xs.iter().map(|s| s.area()).sum()
}

fn main() {
    let generic = [
        describe_generic(black_box(7u8)),
        describe_generic(black_box(-7i64)),
        describe_generic(black_box("seven")),
        describe_generic(black_box(vec![7u16])),
    ];
    let dynamic = [
        describe_dyn(black_box(&7u8)),
        describe_dyn(black_box(&-7i64)),
        describe_dyn(black_box(&"seven")),
        describe_dyn(black_box(&vec![7u16])),
    ];
    println!("generic: {generic:?}");
    println!("dyn:     {dynamic:?}");

    let squares = [Square(1.0), Square(2.0)];
    let rects = [Rect(1.0, 2.0), Rect(3.0, 4.0)];
    let mixed: [&dyn Shape; 2] = [&squares[1], &rects[1]];
    println!(
        "static squares={} rects={}  dyn mixed={}",
        total_area_static(black_box(&squares)),
        total_area_static(black_box(&rects)),
        total_area_dyn(black_box(&mixed))
    );
}
