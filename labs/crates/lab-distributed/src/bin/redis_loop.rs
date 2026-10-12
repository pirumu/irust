//! Lab (i): single-threaded event loop, atomicity boundary, async replication + WAIT.
//! Usage: `redis_loop [seed=42] [crashes_per_row=2000]`

use lab_distributed::redis_loop::render_report;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed = args
        .next()
        .map_or(42, |s| s.parse().expect("seed must be a u64"));
    let runs = args
        .next()
        .map_or(2000, |s| s.parse().expect("crashes must be a u64"));
    print!("{}", render_report(seed, runs));
}
