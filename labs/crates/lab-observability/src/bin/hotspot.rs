//! Lab (a): a CPU-bound program with a known hot function, a warm one, a cold one and a
//! blocking wait, to point `perf stat` / `perf record` / `perf report` at (chapter 01).
//!
//! The call chain is `main -> run_round -> process_batch -> hot_checksum` for the hot path,
//! `main -> run_round -> warm_format` and `main -> run_round -> cold_validate` for the others;
//! every function is `#[inline(never)]`, so each one stays a separate symbol and a separate stack
//! frame in an optimized build. The program times its own phases with the wall clock *and* the
//! thread CPU clock: that is the ground truth a sampling profiler should reproduce — for the CPU
//! phases. The `sleep` phase has wall time but no CPU time, which is what an on-CPU profile cannot see.
//!
//!   cargo run -q --release -p lab-observability --bin hotspot -- [rounds=30] [sleep_ms=10]

use lab_observability::{crc, thread_cpu_time};
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Bytes hashed by one `hot_checksum` call.
const BUF_LEN: usize = 64 * 1024;
/// `hot_checksum` calls per round.
const HOT_CALLS: u32 = 120;
/// Strings formatted by `warm_format` per round.
const WARM_ITEMS: usize = 15_000;
/// `cold_validate` runs once every this many rounds, on a 4 KiB block.
const COLD_EVERY: u64 = 8;

#[derive(Clone, Copy, Default)]
struct Phase {
    wall: Duration,
    cpu: Duration,
    /// Work units: bytes hashed, strings formatted, validations run, ... (not time).
    units: u64,
}

impl Phase {
    /// Runs `f`, charging its wall time and thread CPU time to this phase.
    fn time<R>(&mut self, units: u64, f: impl FnOnce() -> R) -> R {
        let (wall0, cpu0) = (Instant::now(), thread_cpu_time());
        let out = f();
        self.wall += wall0.elapsed();
        self.cpu += thread_cpu_time() - cpu0;
        self.units += units;
        out
    }
}

#[derive(Default)]
struct Phases {
    cold: Phase,
    hot: Phase,
    warm: Phase,
    sleep: Phase,
}

/// Cold: builds the CRC table once.
#[inline(never)]
fn cold_build_table() -> [u32; 256] {
    crc::build_table()
}

/// Cold: deterministic pseudo-random input, built once.
#[inline(never)]
fn cold_fill_buffer(len: usize) -> Vec<u8> {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    (0..len)
        .map(|_| {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 56) as u8
        })
        .collect()
}

/// Cold: cross-checks the table-driven CRC against the bit-at-a-time reference on a small block.
#[inline(never)]
fn cold_validate(table: &[u32; 256], data: &[u8]) -> bool {
    crc::checksum(table, data) == crc::checksum_bitwise(data)
}

/// Hot: the table-driven CRC loop. In the CPU phases, almost every sample should land here.
#[inline(never)]
fn hot_checksum(table: &[u32; 256], seed: u32, data: &[u8]) -> u32 {
    crc::update(table, seed, data)
}

/// Warm: allocation and formatting. Visible in a profile, an order of magnitude below the hot loop.
#[inline(never)]
fn warm_format(round: u64, items: usize) -> u64 {
    let mut total = 0u64;
    for i in 0..items {
        let s = format!("req-{round}-{i}-{:08x}", (round << 16) ^ i as u64);
        total += s.len() as u64 + u64::from(s.as_bytes()[s.len() - 1]);
    }
    black_box(total)
}

/// One level between `run_round` and the hot function: gives the call stack some depth.
#[inline(never)]
fn process_batch(table: &[u32; 256], buf: &[u8], mut state: u32) -> u32 {
    for _ in 0..HOT_CALLS {
        state = hot_checksum(table, state, buf);
    }
    state
}

#[inline(never)]
fn run_round(round: u64, table: &[u32; 256], buf: &[u8], state: u32, sleep_ms: u64, ph: &mut Phases) -> (u32, u64) {
    let state = ph.hot.time(u64::from(HOT_CALLS) * BUF_LEN as u64, || process_batch(table, buf, state));
    let warm = ph.warm.time(WARM_ITEMS as u64, || warm_format(round, WARM_ITEMS));
    if round.is_multiple_of(COLD_EVERY) {
        let ok = ph.cold.time(1, || cold_validate(table, &buf[..4096]));
        assert!(ok, "table-driven CRC disagrees with the reference");
    }
    ph.sleep.time(1, || std::thread::sleep(Duration::from_millis(sleep_ms)));
    (state, warm)
}

fn arg(n: usize, default: u64) -> u64 {
    std::env::args().nth(n).and_then(|a| a.parse().ok()).unwrap_or(default)
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn main() {
    let (rounds, sleep_ms) = (arg(1, 30), arg(2, 10));
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let load = lab_observability::load_average().map_or_else(|| "n/a".to_string(), |l| format!("{l:.2}"));
    println!(
        "hotspot: rounds={rounds} hot_calls/round={HOT_CALLS} buffer={BUF_LEN} B warm_items/round={WARM_ITEMS} sleep={sleep_ms} ms/round pid={}",
        std::process::id()
    );
    println!("machine: cores={cores} load1={load} (wall time includes time spent waiting for a CPU when load1 is near or above cores)");

    let mut ph = Phases::default();
    let table = ph.cold.time(256, cold_build_table);
    let buf = ph.cold.time(BUF_LEN as u64, || cold_fill_buffer(BUF_LEN));

    let (mut state, mut warm_sum) = (!0u32, 0u64);
    let total_start = Instant::now();
    for round in 0..rounds {
        let (s, w) = run_round(round, &table, &buf, state, sleep_ms, &mut ph);
        state = s;
        warm_sum = warm_sum.wrapping_add(w);
    }
    let total_wall = total_start.elapsed();

    let rows = [
        ("cold (setup + validate)", ph.cold),
        ("hot_checksum", ph.hot),
        ("warm_format", ph.warm),
        ("sleep (blocked)", ph.sleep),
    ];
    println!("{:<24} {:>10} {:>10} {:>16}", "phase", "wall_ms", "cpu_ms", "work");
    for (name, p) in &rows {
        println!("{name:<24} {:>10.1} {:>10.1} {:>16}", ms(p.wall), ms(p.cpu), p.units);
    }
    let (wall, cpu): (Duration, Duration) = rows.iter().fold((Duration::ZERO, Duration::ZERO), |(w, c), (_, p)| (w + p.wall, c + p.cpu));
    println!("{:<24} {:>10.1} {:>10.1}", "total", ms(wall), ms(cpu));
    let share = |d: Duration, whole: Duration| 100.0 * d.as_secs_f64() / whole.as_secs_f64().max(1e-12);
    println!(
        "share of wall: hot {:.1}%  warm {:.1}%  cold {:.1}%  sleep {:.1}%",
        share(ph.hot.wall, wall),
        share(ph.warm.wall, wall),
        share(ph.cold.wall, wall),
        share(ph.sleep.wall, wall)
    );
    println!(
        "share of cpu:  hot {:.1}%  warm {:.1}%  cold {:.1}%  sleep {:.1}%",
        share(ph.hot.cpu, cpu),
        share(ph.warm.cpu, cpu),
        share(ph.cold.cpu, cpu),
        share(ph.sleep.cpu, cpu)
    );
    println!("elapsed {:.1} ms (main loop), cpu/wall = {:.2}", ms(total_wall), cpu.as_secs_f64() / wall.as_secs_f64().max(1e-12));
    println!("crc={:#010x} warm={warm_sum}", !state);
}
