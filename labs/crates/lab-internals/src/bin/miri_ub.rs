//! Lab (d): deliberate Undefined Behavior, meant ONLY for Miri.
//!
//! Natively this binary refuses to run the UB: a program with UB has no
//! meaning, so "it printed something" would prove nothing.
//!
//!   cargo +nightly miri run -p lab-internals --bin miri_ub -- <case>
//!   cases: uaf | oob | alias | invalid-bool | ok

fn use_after_free() -> i32 {
    let p: *mut i32 = Box::into_raw(Box::new(42));
    // SAFETY: none — `p` is freed on the next line and then read. This is the bug.
    unsafe {
        drop(Box::from_raw(p));
        *p
    }
}

fn out_of_bounds() -> i32 {
    let a = [1, 2, 3];
    let p = a.as_ptr();
    // SAFETY: none — index 3 is one past the end; reading it is the bug.
    unsafe { p.add(3).read() }
}

fn aliasing() -> i32 {
    let mut x = 0;
    let raw = &raw mut x;
    // SAFETY: none — two live `&mut` to the same place; using `a` after `b` is the bug.
    unsafe {
        let a = &mut *raw;
        let b = &mut *raw;
        *b = 2;
        *a = 1;
        *a
    }
}

fn invalid_bool() -> bool {
    let byte = 3u8;
    // SAFETY: none — 3 is not a valid `bool` (must be 0 or 1); producing it is the bug.
    unsafe { (&raw const byte).cast::<bool>().read() }
}

fn well_defined() -> i32 {
    let mut x = 0;
    let raw = &raw mut x;
    // SAFETY: `raw` comes from a live local and no reference to `x` exists meanwhile.
    unsafe {
        *raw += 1;
        *raw
    }
}

fn main() {
    let case = std::env::args().nth(1).unwrap_or_else(|| "ok".to_string());
    if !cfg!(miri) && case != "ok" {
        eprintln!(
            "refusing to run UB case `{case}` natively; run it under Miri:\n  \
             cargo +nightly miri run -p lab-internals --bin miri_ub -- {case}"
        );
        std::process::exit(2);
    }
    match case.as_str() {
        "uaf" => println!("uaf read {}", use_after_free()),
        "oob" => println!("oob read {}", out_of_bounds()),
        "alias" => println!("alias read {}", aliasing()),
        "invalid-bool" => println!("bool is {}", invalid_bool()),
        "ok" => println!("well-defined raw pointer use: {}", well_defined()),
        other => {
            eprintln!("unknown case `{other}`; use uaf | oob | alias | invalid-bool | ok");
            std::process::exit(2);
        }
    }
}
