//! Lab (c): row layout vs column layout, for an aggregate (OLAP) and for whole-row fetches (OLTP).
//!
//! Run: cargo run -q --release -p lab-data --bin layout_scan [rows]
//! (build with --release: debug builds measure the interpreter-like overhead of the loops, not memory.)

use std::hint::black_box;
use std::time::Instant;

use lab_data::layout::{self, Aggregate, Columns, Order, Rle};
use lab_data::util::thousands;

/// Runs `f` `reps` times; returns (best, median) in milliseconds.
fn time_ms<T>(reps: usize, mut f: impl FnMut() -> T) -> (f64, f64) {
    black_box(f()); // warm-up
    let mut times: Vec<f64> = (0..reps)
        .map(|_| {
            let t = Instant::now();
            black_box(f());
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    (times[0], times[times.len() / 2])
}

fn line(name: &str, bytes: u64, (best, median): (f64, f64)) {
    println!(
        "{name:<34} {:>15} {best:>9.3} {median:>11.3} {:>12.1}",
        thousands(bytes),
        bytes as f64 / (best / 1e3) / 1e9
    );
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(2_000_000);
    let country = 7u16;
    let rows: Vec<Order> = layout::generate(n, 200, 1);
    let cols = Columns::from_rows(&rows);

    println!("== table `orders`: {} rows x 14 columns, {} bytes per row ==", thousands(n as u64), layout::ROW_BYTES);
    println!(
        "row store    : {} MB in one array of 64-byte rows",
        n * layout::ROW_BYTES / 1_000_000
    );
    println!("column store : {} MB in 14 arrays (u64 x3, u32 x9, u16 x2)\n", n * layout::ROW_BYTES / 1_000_000);

    println!("== OLAP: SELECT SUM(price_cents), COUNT(*) FROM orders WHERE country = {country} ==");
    let a = layout::aggregate_rows(&rows, country);
    let b = layout::aggregate_columns(&cols, country);
    assert_eq!(a, b);
    let Aggregate { sum_price_cents, matches } = a;
    println!("result (identical in both layouts): sum_price_cents={} matches={}\n", thousands(sum_price_cents), thousands(matches));

    println!("{:<34} {:>15} {:>9} {:>11} {:>12}", "layout", "bytes streamed", "best ms", "median ms", "GB/s (best)");
    let reps = 11;
    line("row store (scan all 64 B rows)", layout::bytes_streamed_rows(n), time_ms(reps, || layout::aggregate_rows(black_box(&rows), country)));
    line("column store (country + price)", layout::bytes_streamed_columns(n), time_ms(reps, || layout::aggregate_columns(black_box(&cols), country)));

    let sorted_rows = layout::sorted_by_country(&rows);
    let sorted_cols = Columns::from_rows(&sorted_rows);
    let rle = Rle::encode(&sorted_cols.country);
    assert_eq!(layout::aggregate_sorted(&sorted_cols, &rle, country), a);
    let touched = matches * 8;
    line("sorted by country + RLE (slice)", touched, time_ms(reps, || layout::aggregate_sorted(black_box(&sorted_cols), &rle, country)));
    println!(
        "\nsorted + run-length encoded `country` column: {} runs, {} B encoded vs {} B as a plain u16 array",
        rle.runs(),
        thousands(rle.encoded_bytes() as u64),
        thousands(n as u64 * 2)
    );
    println!("(the third line streams only the {} matching prices; \"bytes streamed\" there is matches x 8)\n", thousands(matches));

    println!("== OLTP: fetch whole rows by id (1,000,000 random ids) ==");
    let ids = layout::lookup_ids(n, 1_000_000, 5);
    assert_eq!(layout::fetch_rows(&rows, &ids), layout::fetch_columns(&cols, &ids));
    println!("{:<34} {:>15} {:>9} {:>11}", "layout", "arrays touched", "best ms", "ns / row");
    for (name, arrays, t) in [
        ("row store (1 contiguous 64 B row)", 1, time_ms(reps, || layout::fetch_rows(black_box(&rows), &ids))),
        ("column store (rebuild from 14 arrays)", 14, time_ms(reps, || layout::fetch_columns(black_box(&cols), &ids))),
    ] {
        println!("{name:<34} {arrays:>15} {:>9.2} {:>11.1}", t.0, t.0 * 1e6 / ids.len() as f64);
    }
}
