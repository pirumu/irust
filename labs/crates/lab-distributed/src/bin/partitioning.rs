//! `partitioning`: prints the lab (c) report. Usage: `partitioning [seed] [section 1-5]`.

use lab_distributed::partition::report;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed = args
        .next()
        .map_or(42, |s| s.parse().expect("seed must be a u64"));
    let only = args
        .next()
        .map(|s| s.parse().expect("section must be a number 1-5"));
    print!("{}", report(seed, only));
}
