//! Lab (b): a target for `strace`, `perf trace`, `bpftrace` and uprobes (chapter 02).
//!
//! The program counts its own `write(2)` calls and its own allocator calls and prints them at
//! the end — the *ground truth* the reader compares a tracer's numbers with. It also exports one
//! function with a fixed, unmangled name, `handle_request`, so that `uprobe:<binary>:handle_request`
//! can attach to it without demangling anything (chapter 02 §9).
//!
//!   trace_target <unbuffered|buffered> [requests=20000] [wait_secs=0]
//!
//! * `unbuffered`: one `format!` and one `write_all` on the raw `File` per request: one `write(2)`
//!   and at least one heap allocation per request.
//! * `buffered`: the line is formatted into a reused `String` and written through a `BufWriter`:
//!   far fewer `write(2)` calls and no allocation in the formatting.
//!
//! `handle_request` allocates once in both modes, so the allocation count never reaches zero.
//! The loop itself lives in `lab_observability::iotrace`, where the unit tests pin it down.

use lab_observability::alloc::CountingAlloc;
use lab_observability::iotrace::{Mode, run_with};
use std::fs::OpenOptions;
use std::hint::black_box;
use std::time::Duration;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// Where the lines go. Writing to the null device keeps the disk out of the picture: the syscalls
/// still happen, the cost of the device does not.
const SINK: &str = "/dev/null";

/// The function a uprobe attaches to. `no_mangle` keeps the symbol name stable, `extern "C"`
/// makes the first argument (`id`) live in the first integer argument register, and
/// `inline(never)` guarantees that a function with that name exists.
#[unsafe(no_mangle)]
#[inline(never)]
pub extern "C" fn handle_request(id: u64) -> u64 {
    let body = format!("request {id} handled");
    black_box(&body).len() as u64
}

/// `run_with` takes a plain Rust `fn`; this adapter forwards to the exported symbol.
fn handle(id: u64) -> u64 {
    handle_request(id)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(mode) = args.next().as_deref().and_then(Mode::parse) else {
        eprintln!("usage: trace_target <unbuffered|buffered> [requests=20000] [wait_secs=0]");
        std::process::exit(2);
    };
    let requests: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(20_000);
    let wait_secs: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);

    eprintln!("trace_target: pid={} sink={SINK} handle_request is an unmangled symbol", std::process::id());
    if wait_secs > 0 {
        eprintln!("trace_target: waiting {wait_secs} s so a tracer can attach (bpftrace -p, strace -p, perf trace -p)");
        std::thread::sleep(Duration::from_secs(wait_secs));
    }

    let report = OpenOptions::new().write(true).open(SINK).and_then(|file| run_with(mode, requests, file, handle));
    match report {
        Ok(r) => println!(
            "mode={} requests={requests} bytes={} write_calls={} allocs={} reallocs={} frees={} elapsed_ms={:.2}",
            mode.name(),
            r.bytes,
            r.write_calls,
            r.allocs.allocs,
            r.allocs.reallocs,
            r.allocs.frees,
            r.elapsed.as_secs_f64() * 1e3
        ),
        Err(e) => {
            eprintln!("trace_target: {e}");
            std::process::exit(1);
        }
    }
}
