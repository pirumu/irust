//! Lab 3: branch prediction — cùng dữ liệu, thứ tự khác.
//!
//!   cargo run -q -p lab-cpu --release --bin branch            # 1 Mi phần tử, 15 lần đo
//!   cargo run -q -p lab-cpu --release --bin branch -- 4194304 25
//!
//! Tham số tùy chọn: số phần tử, số lần đo (lấy lần nhanh nhất).
//! Phải chạy `--release`: ở debug, mọi thứ chậm đi cùng một kiểu và thí nghiệm mất ý nghĩa.

use lab_cpu::{XorShift, best_of, lab_sum_branchless, lab_sum_branchy, lab_sum_plain};

/// Chữ ký chung của ba biến thể.
type SumFn = fn(&[u8]) -> u64;

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(1 << 20);
    let reps: usize = args.next().and_then(|a| a.parse().ok()).unwrap_or(15);

    let mut rng = XorShift::new(0x1234_5678_9ABC_DEF1);
    let random: Vec<u8> = (0..n).map(|_| (rng.next_u64() >> 32) as u8).collect();
    let mut sorted = random.clone();
    sorted.sort_unstable();

    if cfg!(debug_assertions) {
        eprintln!("cảnh báo: đang chạy bản debug; hãy chạy lại với --release");
    }
    println!("n = {n}, reps = {reps} (lấy lần nhanh nhất)");
    println!(
        "{:<28} {:>16} {:>16}",
        "variant", "random (µs)", "sorted (µs)"
    );

    let variants: [(&str, SumFn); 3] = [
        ("branchy (if + black_box)", lab_sum_branchy),
        ("branchless (mask)", lab_sum_branchless),
        ("plain if (no black_box)", lab_sum_plain),
    ];
    let mut reference = None;
    for (name, f) in variants {
        let (t_rand, s_rand) = best_of(reps, || f(&random));
        let (t_sorted, s_sorted) = best_of(reps, || f(&sorted));
        // Cùng tập giá trị nên tổng phải bằng nhau, giữa hai thứ tự và giữa các biến thể.
        assert_eq!(s_rand, s_sorted, "{name}: tổng phụ thuộc thứ tự?");
        assert_eq!(
            *reference.get_or_insert(s_rand),
            s_rand,
            "{name}: khác biến thể đầu"
        );
        println!(
            "{name:<28} {:>16.1} {:>16.1}",
            t_rand.as_secs_f64() * 1e6,
            t_sorted.as_secs_f64() * 1e6
        );
    }
    println!("tổng = {}", reference.unwrap_or(0));
}
