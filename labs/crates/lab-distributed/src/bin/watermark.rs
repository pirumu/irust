//! Lab (h): event time, tumbling windows, watermarks and late events. Usage: `watermark [seed]`.

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(42);
    print!("{}", lab_distributed::windows::report(seed));
}
