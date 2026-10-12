//! Phase 11 labs: observability — profiling, tracing, latency percentiles, debugging, crashes
//! and ELF inspection (`docs/labs/11-do-dac-va-go-loi-lab.md`).
//!
//! The binaries in `src/bin/` are the labs. Everything that can be checked without a
//! profiler or a debugger lives here, so `cargo test -p lab-observability` can pin it down:
//!
//! * [`alloc`] — a `GlobalAlloc` that counts allocations per thread (labs b, c).
//! * [`crc`] — the CRC-32 kernel that `hotspot` makes hot (lab a).
//! * [`iotrace`] — the write loop of `trace_target`, buffered or not, with exact counts (lab b).
//! * [`latency`] — a service-like workload, a naive and a tuned variant, percentile helpers (lab c).
//! * [`spans`] — a `tracing` layer and the `Span::enter`-across-`.await` experiment (lab d).
//! * [`elf`] — a small ELF64 reader, in the spirit of `readelf` (labs e, g).
//! * [`corecheck`] — "why is there no core file?" as pure functions (lab f).

pub mod alloc;
pub mod corecheck;
pub mod crc;
pub mod elf;
pub mod iotrace;
pub mod latency;
pub mod spans;

use std::time::Duration;

/// CPU time consumed by the *calling thread* so far (user + system), not wall-clock time.
///
/// A thread that sleeps or waits on a lock does not advance this clock, which is exactly the
/// on-CPU / off-CPU split of chapter 01. It is per thread (not per process) so that other
/// threads — for example the other tests of `cargo test` — cannot pollute a measurement.
pub fn thread_cpu_time() -> Duration {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid, writable `timespec`; CLOCK_THREAD_CPUTIME_ID is a clock id on Linux and macOS.
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID) failed");
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// The 1-minute load average: how many processes were runnable (running or waiting for a CPU)
/// on average. A wall-clock measurement taken while this is near or above the number of cores
/// includes time spent waiting for a CPU, so the labs print it next to every timing.
pub fn load_average() -> Option<f64> {
    let mut loads = [0f64; 3];
    // SAFETY: `loads` has room for the 3 samples requested.
    let n = unsafe { libc::getloadavg(loads.as_mut_ptr(), 3) };
    (n >= 1).then_some(loads[0])
}

/// Printed when part of a lab binary needs Linux and this build is for another OS. `args` is
/// what follows `--` on the command line that reproduces the case in the lab container.
pub fn not_linux(bin: &str, args: &str, what: &str) {
    eprintln!(
        "{bin} {args}: {what} is Linux-only; this build is for another OS.\n\
         Run it in the lab container from the repository root:\n  \
         ./labs/linux.sh cargo run -q -p lab-observability --bin {bin} -- {args}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::hint::black_box;
    use std::time::Instant;

    /// The distinction the whole of chapter 01 §7 rests on: waiting is wall time, not CPU time.
    #[test]
    fn sleeping_advances_wall_time_but_not_cpu_time() {
        let (cpu0, wall0) = (thread_cpu_time(), Instant::now());
        std::thread::sleep(Duration::from_millis(60));
        let (cpu, wall) = (thread_cpu_time() - cpu0, wall0.elapsed());
        assert!(wall >= Duration::from_millis(60), "wall = {wall:?}");
        assert!(cpu < Duration::from_millis(30), "a sleeping thread burned {cpu:?} of CPU");
    }

    /// ... and burning cycles advances the CPU clock, which for one thread never runs ahead of
    /// the wall clock. The loop waits on the CPU clock itself (with a generous wall-clock
    /// ceiling) so a busy machine slows the test down instead of failing it.
    #[test]
    fn spinning_advances_cpu_time() {
        let (cpu0, wall0) = (thread_cpu_time(), Instant::now());
        let mut x = 1u64;
        while thread_cpu_time() - cpu0 < Duration::from_millis(20) && wall0.elapsed() < Duration::from_secs(5) {
            x = black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
        let (cpu, wall) = (thread_cpu_time() - cpu0, wall0.elapsed());
        assert!(cpu >= Duration::from_millis(20), "only {cpu:?} of CPU in {wall:?}");
        assert!(cpu <= wall + Duration::from_millis(5), "cpu {cpu:?} > wall {wall:?}");
    }
}
