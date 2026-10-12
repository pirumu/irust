//! Lab (f): ways for a Rust process to end badly, one per case (chapter 05).
//!
//!   crash_lab <case> [arg]
//!
//!   panic         a `panic!` on the main thread: unwinds, exit code 101, no signal
//!   panic-thread  a panic on a worker thread: the process survives and exits 0
//!   hook          a custom panic hook that prints the location, the payload and a backtrace
//!   backtrace     `Backtrace::capture()` vs `force_capture()` without any panic
//!   abort         `std::process::abort()`: SIGABRT
//!   segv          a write through a null pointer: SIGSEGV in a release build, a compiler-inserted check (then abort) in a debug build
//!   stack         unbounded recursion on a small thread stack: "has overflowed its stack", then SIGABRT
//!   hang [secs]   two threads deadlock (AB / BA); a watchdog exits with code 3 after `secs` (default 3, 0 = never)
//!   preflight     why would a core file (not) appear? RLIMIT_CORE, core_pattern, coredump_filter
//!
//! Every case prints one line to stderr before it fails, so the shell shows which case ran.

use lab_observability::corecheck::CoreLimit;
use parking_lot::Mutex;
use std::backtrace::Backtrace;
use std::hint::black_box;
use std::sync::{Arc, Barrier};
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    let case = args.next().unwrap_or_default();
    let arg = args.next();
    eprintln!("crash_lab: case `{case}` pid={}", std::process::id());
    match case.as_str() {
        "panic" => panic_main(),
        "panic-thread" => panic_thread(),
        "hook" => hook(),
        "backtrace" => backtrace_demo(),
        "abort" => std::process::abort(),
        "segv" => segv(),
        "stack" => stack(),
        "hang" => hang(arg.and_then(|a| a.parse().ok()).unwrap_or(3)),
        "preflight" => preflight(),
        _ => {
            eprintln!("usage: crash_lab <panic|panic-thread|hook|backtrace|abort|segv|stack|hang [secs]|preflight>");
            std::process::exit(2);
        }
    }
}

#[inline(never)]
fn validate(order_id: u64) -> u64 {
    if black_box(order_id) % 2 == 1 {
        panic!("order {order_id}: odd ids are not allowed");
    }
    order_id
}

#[inline(never)]
fn handle(order_id: u64) -> u64 {
    validate(order_id) + 1
}

fn panic_main() {
    println!("handled: {}", handle(7));
}

fn panic_thread() {
    let worker = std::thread::Builder::new().name("worker".into()).spawn(|| handle(9)).expect("spawn");
    match worker.join() {
        Ok(v) => println!("main: worker returned {v}"),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("(non-string payload)");
            println!("main: the worker thread panicked ({msg}); the process carries on");
        }
    }
}

fn hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let place = info.location().map_or_else(|| "?".to_string(), |l| format!("{}:{}", l.file(), l.line()));
        let text = info.payload_as_str().unwrap_or("(non-string payload)");
        let backtrace = Backtrace::force_capture();
        eprintln!("HOOK: panic at {place}: {text}");
        eprintln!("HOOK: backtrace ({:?}):\n{backtrace}", backtrace.status());
        default_hook(info); // then the standard message, as before
    }));
    println!("handled: {}", handle(3));
}

#[inline(never)]
fn level_two() -> Backtrace {
    black_box(Backtrace::capture())
}

#[inline(never)]
fn level_one() -> (Backtrace, Backtrace) {
    (level_two(), black_box(Backtrace::force_capture()))
}

fn backtrace_demo() {
    let (by_env, forced) = level_one();
    println!("capture():       status = {:?}", by_env.status());
    println!("force_capture(): status = {:?}", forced.status());
    let frames = |bt: &Backtrace| bt.to_string().lines().filter(|l| l.trim_start().starts_with(|c: char| c.is_ascii_digit())).count();
    println!("frames printed:  capture() = {}, force_capture() = {}", frames(&by_env), frames(&forced));
    println!("lines of force_capture() that mention this program:");
    for line in forced.to_string().lines().filter(|l| l.contains("crash_lab")) {
        println!("  {line}");
    }
}

fn segv() {
    let p: *mut u32 = black_box(std::ptr::null_mut());
    // SAFETY: none — this is the bug under study. `black_box` hides that the pointer is null, so the
    // compiler cannot prove the write is dead. In a release build the CPU performs the store and the
    // kernel sends SIGSEGV; a debug build checks the pointer first (observed on 1.95: "null pointer
    // dereference occurred", a non-unwinding panic, then abort).
    unsafe { *p = 1 };
    println!("unreachable: the write returned");
}

/// Recurses without end; each frame carries a 512-byte array so the stack fills quickly.
#[allow(unconditional_recursion)]
#[inline(never)]
fn recurse(depth: u64) -> u64 {
    let pad = [depth as u8; 512];
    black_box(&pad);
    // The addition after the call stops the compiler from turning this into a loop.
    recurse(depth + 1) + u64::from(pad[7])
}

fn stack() {
    let t = std::thread::Builder::new().name("recurser".into()).stack_size(256 * 1024).spawn(|| recurse(0)).expect("spawn");
    println!("unreachable: {:?}", t.join());
}

fn hang(watchdog_secs: u64) {
    let (a, b) = (Arc::new(Mutex::new(())), Arc::new(Mutex::new(())));
    let both_hold_one = Arc::new(Barrier::new(2));
    for (name, first, second) in [("worker-AB", a.clone(), b.clone()), ("worker-BA", b, a)] {
        let both_hold_one = both_hold_one.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _first = first.lock();
                both_hold_one.wait(); // now each thread holds one lock and wants the other
                let _second = second.lock();
                println!("unreachable: {name} got both locks");
            })
            .expect("spawn");
    }
    eprintln!("crash_lab: two threads are about to deadlock; watchdog = {watchdog_secs} s (0 = wait forever)");
    let mut waited = 0;
    while watchdog_secs == 0 || waited < watchdog_secs {
        std::thread::sleep(Duration::from_secs(1));
        waited += 1;
    }
    eprintln!("crash_lab: watchdog: no thread finished in {watchdog_secs} s; exiting with code 3");
    std::process::exit(3);
}

fn show_limit(raw: libc::rlim_t) -> String {
    match CoreLimit::from_rlim(raw, libc::RLIM_INFINITY) {
        CoreLimit::Zero => "0 (no core file is created)".to_string(),
        CoreLimit::Unlimited => "unlimited".to_string(),
        CoreLimit::Bytes(n) => format!("{n} bytes (a larger core is truncated)"),
    }
}

fn preflight() {
    let mut lim = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
    // SAFETY: `lim` is a valid, writable `rlimit`; RLIMIT_CORE exists on Linux and macOS.
    if unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut lim) } != 0 {
        eprintln!("getrlimit(RLIMIT_CORE) failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
    println!("RLIMIT_CORE soft = {}", show_limit(lim.rlim_cur));
    println!("RLIMIT_CORE hard = {}", show_limit(lim.rlim_max));
    println!("RLIM_INFINITY    = {:#x}", libc::RLIM_INFINITY);
    kernel_core_settings(CoreLimit::from_rlim(lim.rlim_cur, libc::RLIM_INFINITY));
}

/// The kernel-side half of the checklist: `core_pattern`, `core_uses_pid`, `coredump_filter`.
#[cfg(target_os = "linux")]
fn kernel_core_settings(limit: CoreLimit) {
    use lab_observability::corecheck::{Verdict, classify_core_pattern, filter_names, parse_coredump_filter, verdict};

    let read = |path: &str| std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"));
    match (read("/proc/sys/kernel/core_pattern"), read("/proc/sys/kernel/core_uses_pid")) {
        (Ok(pattern), Ok(uses_pid)) => {
            let pattern = classify_core_pattern(&pattern);
            let uses_pid = uses_pid.trim() != "0";
            println!("core_pattern     = {pattern:?}");
            println!("core_uses_pid    = {uses_pid}");
            match verdict(limit, &pattern, uses_pid) {
                Verdict::NoCore(why) => println!("verdict: no core file: {why:?}"),
                Verdict::File { template, truncated_to } => {
                    println!("verdict: a file named `{template}` (truncated to {truncated_to:?} bytes)");
                }
                Verdict::HandlerDecides { command } => println!("verdict: piped to `{command}`; RLIMIT_CORE is ignored, the handler decides"),
            }
        }
        (Err(e), _) | (_, Err(e)) => println!("could not read the kernel's core settings: {e}"),
    }
    match read("/proc/self/coredump_filter").map(|raw| parse_coredump_filter(&raw)) {
        Ok(Ok(mask)) => println!("coredump_filter  = {mask:#x}: {}", filter_names(mask).join(", ")),
        Ok(Err(e)) => println!("coredump_filter unreadable: {e}"),
        Err(e) => println!("{e}"),
    }
}

#[cfg(not(target_os = "linux"))]
fn kernel_core_settings(_limit: CoreLimit) {
    lab_observability::not_linux("crash_lab", "preflight", "reading /proc/sys/kernel/core_pattern and /proc/self/coredump_filter");
}
