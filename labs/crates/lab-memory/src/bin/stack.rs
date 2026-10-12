//! Lab (e): stack frames and stack overflow.
//!
//!   cargo run -p lab-memory --bin stack              # frame addresses, stack limits
//!   cargo run -p lab-memory --bin stack -- overflow  # overflow a 256 KiB thread stack
//!
//! Works on Linux and macOS; on Linux it also prints the [stack] line of /proc/self/maps.

use std::hint::black_box;

/// Recurses `depth` times and records the address of a local in every frame.
#[inline(never)]
fn descend(depth: usize, out: &mut Vec<usize>) {
    let local = [0_u8; 64]; // a local array, so every frame has some size
    out.push(black_box(&local).as_ptr() as usize);
    if depth > 0 {
        descend(depth - 1, out);
    }
    black_box(&local);
}

/// Recursion that never stops in practice: each frame keeps a 4 KiB local alive.
/// `black_box(true)` hides from the compiler that the base case is unreachable.
#[inline(never)]
fn overflow(depth: u64) -> u64 {
    let local = [depth as u8; 4096];
    black_box(&local);
    if black_box(true) {
        overflow(depth + 1) + u64::from(local[0])
    } else {
        depth
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("overflow") {
        let handle = std::thread::Builder::new()
            .name("small-stack".into())
            .stack_size(256 << 10)
            .spawn(|| overflow(0))
            .expect("spawn");
        // Never reached: the overflow aborts the whole process.
        println!("result {:?}", handle.join());
        return;
    }

    let mut addrs = Vec::with_capacity(8);
    descend(5, &mut addrs);
    println!("address of a local in each nested call (main thread):");
    for (i, pair) in addrs.windows(2).enumerate() {
        println!(
            "  depth {i}: {:#x}  next frame is {} bytes lower",
            pair[0],
            pair[0] as isize - pair[1] as isize
        );
    }

    let mut lim = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: `lim` is writable memory of the right type.
    if unsafe { libc::getrlimit(libc::RLIMIT_STACK, lim.as_mut_ptr()) } == 0 {
        // SAFETY: getrlimit returned 0, so `lim` is initialised.
        let lim = unsafe { lim.assume_init() };
        let show = |v: libc::rlim_t| {
            if v == libc::RLIM_INFINITY {
                "unlimited".to_string()
            } else {
                format!("{} KiB", v / 1024)
            }
        };
        println!(
            "RLIMIT_STACK soft={} hard={}",
            show(lim.rlim_cur),
            show(lim.rlim_max)
        );
    }

    #[cfg(target_os = "linux")]
    if let Ok(maps) = lab_memory::read_maps()
        && let Some(entry) = lab_memory::region_of(&maps, addrs[0])
    {
        println!(
            "main-thread stack mapping: {} ({} KiB mapped right now)",
            entry.line,
            (entry.end - entry.start) / 1024
        );
    }

    std::thread::spawn(|| {
        let local = 0_u8;
        println!(
            "a spawned thread's local lives at {:#x} (a different stack)",
            &raw const local as usize
        );
    })
    .join()
    .expect("join");
}
