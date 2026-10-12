//! Lab (a): size, alignment, field offsets, niche optimization and wide pointers.
//!
//! Run: `cargo run -p lab-internals --bin type_layout`
//! Numbers depend on the target; the doc shows output from aarch64-apple-darwin
//! and x86_64/aarch64 Linux may differ for `repr(Rust)` types.

use std::fmt::Debug;
use std::marker::PhantomData;
use std::mem::{MaybeUninit, offset_of, size_of_val};
use std::num::NonZeroU32;
use std::ptr::NonNull;

macro_rules! row {
    ($t:ty) => {
        println!(
            "{:<34} size={:>2} align={:>2}",
            stringify!($t),
            size_of::<$t>(),
            align_of::<$t>()
        );
    };
}

// The same three fields under different representations.
#[allow(dead_code)]
struct RustRepr {
    a: u8,
    b: u32,
    c: u16,
}

#[allow(dead_code)]
#[repr(C)]
struct CRepr {
    a: u8,
    b: u32,
    c: u16,
}

#[allow(dead_code)]
#[repr(C, packed)]
struct PackedRepr {
    a: u8,
    b: u32,
    c: u16,
}

#[allow(dead_code)]
#[repr(transparent)]
struct Meters(f64);

#[allow(dead_code)]
enum FieldlessRust {
    A,
    B,
    C,
}

#[allow(dead_code)]
#[repr(u32)]
enum FieldlessU32 {
    A,
    B,
    C,
}

#[allow(dead_code)]
enum WithPayload {
    A(u32),
    B(u64),
    C(u8),
}

struct Nothing;

#[allow(dead_code)]
struct Tagged<T> {
    id: u32,
    _unit: PhantomData<T>,
}

trait Shape {
    fn area(&self) -> f64;
}

struct Square(f64);
struct Circle {
    r: f64,
    _label: [u8; 16],
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.0 * self.0
    }
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.0 * self.r * self.r
    }
}

fn main() {
    println!("== primitives & pointers ==");
    row!(u8);
    row!(u16);
    row!(u32);
    row!(u64);
    row!(u128);
    row!(usize);
    row!(char);
    row!(bool);
    row!(());

    println!("\n== same fields, different repr ==");
    row!(RustRepr);
    row!(CRepr);
    row!(PackedRepr);
    println!(
        "RustRepr   offsets: a={} b={} c={}",
        offset_of!(RustRepr, a),
        offset_of!(RustRepr, b),
        offset_of!(RustRepr, c)
    );
    println!(
        "CRepr      offsets: a={} b={} c={}",
        offset_of!(CRepr, a),
        offset_of!(CRepr, b),
        offset_of!(CRepr, c)
    );
    println!(
        "PackedRepr offsets: a={} b={} c={}",
        offset_of!(PackedRepr, a),
        offset_of!(PackedRepr, b),
        offset_of!(PackedRepr, c)
    );
    row!(Meters);
    row!(f64);

    println!("\n== enums ==");
    row!(FieldlessRust);
    row!(FieldlessU32);
    row!(WithPayload);

    println!("\n== niche optimization ==");
    row!(&u64);
    row!(Option<&u64>);
    row!(Box<u64>);
    row!(Option<Box<u64>>);
    row!(NonNull<u64>);
    row!(Option<NonNull<u64>>);
    row!(*const u64);
    row!(Option<*const u64>);
    row!(u32);
    row!(Option<u32>);
    row!(NonZeroU32);
    row!(Option<NonZeroU32>);
    row!(bool);
    row!(Option<bool>);
    row!(Option<MaybeUninit<bool>>);
    row!(fn(u8) -> u8);
    row!(Option<fn(u8) -> u8>);

    println!("\n== zero-sized types ==");
    row!(Nothing);
    row!([u64; 0]);
    row!(PhantomData<String>);
    row!(Tagged<String>);
    let v: Vec<()> = vec![(); 1_000_000];
    println!("Vec<()> with len={} has capacity={}", v.len(), v.capacity());

    println!("\n== wide (fat) pointers ==");
    row!(&[u8; 4]);
    row!(&[u8]);
    row!(&str);
    row!(&dyn Shape);
    row!(Box<dyn Shape>);
    row!(*const [u8]);
    row!(Option<&[u8]>);
    row!(Option<Box<dyn Shape>>);

    // Slice metadata = length.
    let arr = [10u32, 20, 30, 40, 50];
    let s: &[u32] = &arr[1..4];
    let raw: *const [u32] = s;
    println!(
        "slice: data ptr == &arr[1]? {}  len metadata = {}  size_of_val = {}",
        std::ptr::eq(s.as_ptr(), &arr[1]),
        raw.len(),
        size_of_val(s)
    );

    // Trait-object metadata = vtable; size_of_val reads the size from it.
    let sq = Square(2.0);
    let ci = Circle {
        r: 1.0,
        _label: [0; 16],
    };
    let shapes: [&dyn Shape; 2] = [&sq, &ci];
    for (name, obj) in ["Square", "Circle"].iter().zip(shapes) {
        let data = obj as *const dyn Shape as *const ();
        println!(
            "dyn Shape -> {name:<6}: size_of_val = {:>2}  area = {:>4}  data ptr points at the concrete value? {}",
            size_of_val(obj),
            obj.area(),
            if *name == "Square" {
                std::ptr::eq(data, (&sq as *const Square).cast())
            } else {
                std::ptr::eq(data, (&ci as *const Circle).cast())
            }
        );
    }

    let debug_obj: &dyn Debug = &"hello";
    println!("&dyn Debug of &str: size_of_val = {}", size_of_val(debug_obj));
}
