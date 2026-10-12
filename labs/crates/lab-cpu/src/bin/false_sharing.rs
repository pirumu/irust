//! Lab 5: false sharing.
//!
//!   cargo run -q -p lab-cpu --release --bin false_sharing             # 50 triệu vòng/thread
//!   cargo run -q -p lab-cpu --release --bin false_sharing -- 20000000
//!
//! Mỗi thread chỉ ghi vào atomic của riêng nó (load rồi store, `Relaxed`, không `fetch_add`);
//! các biến thể chỉ khác bố cục bộ nhớ của hai atomic.

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::thread;
use std::time::Instant;

#[repr(align(64))]
struct Pad64(AtomicU64);

#[repr(align(128))]
struct Pad128(AtomicU64);

/// Mỗi thread tăng atomic của nó `n` lần; trả về thời gian (giây) của cả cặp.
fn run_pair(a: &AtomicU64, b: &AtomicU64, n: u64) -> f64 {
    let t = Instant::now();
    thread::scope(|s| {
        s.spawn(|| {
            for _ in 0..n {
                a.store(a.load(Relaxed) + 1, Relaxed);
            }
        });
        s.spawn(|| {
            for _ in 0..n {
                b.store(b.load(Relaxed) + 1, Relaxed);
            }
        });
    });
    t.elapsed().as_secs_f64()
}

/// Một thread, một atomic: mốc so sánh không có tranh chấp.
fn run_single(a: &AtomicU64, n: u64) -> f64 {
    let t = Instant::now();
    for _ in 0..n {
        a.store(a.load(Relaxed) + 1, Relaxed);
    }
    t.elapsed().as_secs_f64()
}

fn same_line(a: &AtomicU64, b: &AtomicU64, line: usize) -> bool {
    (a as *const AtomicU64 as usize) / line == (b as *const AtomicU64 as usize) / line
}

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(50_000_000);
    if cfg!(debug_assertions) {
        eprintln!("cảnh báo: đang chạy bản debug; hãy chạy lại với --release");
    }
    println!(
        "available_parallelism = {:?}",
        thread::available_parallelism().map(|p| p.get())
    );
    println!(
        "size_of: [AtomicU64;2]={} Pad64={} Pad128={}",
        size_of::<[AtomicU64; 2]>(),
        size_of::<Pad64>(),
        size_of::<Pad128>()
    );

    let adjacent = [AtomicU64::new(0), AtomicU64::new(0)];
    let p64 = [Pad64(AtomicU64::new(0)), Pad64(AtomicU64::new(0))];
    let p128 = [Pad128(AtomicU64::new(0)), Pad128(AtomicU64::new(0))];

    println!(
        "\n{:<26} {:>10} {:>12}  chung dòng 64B? chung dòng 128B?",
        "layout", "seconds", "ns/iter"
    );
    let rows: [(&str, &AtomicU64, &AtomicU64); 3] = [
        ("adjacent [AtomicU64; 2]", &adjacent[0], &adjacent[1]),
        ("repr(align(64))", &p64[0].0, &p64[1].0),
        ("repr(align(128))", &p128[0].0, &p128[1].0),
    ];
    for (name, a, b) in rows {
        let secs = run_pair(a, b, n);
        println!(
            "{name:<26} {secs:>10.3} {:>12.2}  {:<14} {}",
            secs * 1e9 / n as f64,
            same_line(a, b, 64),
            same_line(a, b, 128)
        );
    }

    let secs = run_single(&adjacent[0], n);
    println!(
        "{:<26} {secs:>10.3} {:>12.2}  (một thread, không tranh chấp)",
        "baseline 1 thread",
        secs * 1e9 / n as f64
    );
    println!(
        "giá trị cuối (kiểm tra mỗi atomic đã được tăng): {}",
        adjacent[0].load(Relaxed)
    );
}
