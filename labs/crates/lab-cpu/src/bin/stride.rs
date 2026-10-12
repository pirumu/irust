//! Lab 4: cache line, stride và thứ tự truy cập.
//!
//!   cargo run -q -p lab-cpu --release --bin stride            # giả định dòng cache 64 B
//!   cargo run -q -p lab-cpu --release --bin stride -- 128     # giả định dòng 128 B
//!
//! Tham số tùy chọn: kích thước dòng cache (byte) dùng để *tính* số dòng bị chạm và
//! chọn khoảng cách ở phần B. Chương trình KHÔNG đọc kích thước dòng thật của máy.

use lab_cpu::{XorShift, best_of};
use std::hint::black_box;

const ELEM: usize = size_of::<u32>();
/// 64 MiB: lớn hơn cache của phần lớn máy (nhận xét của người soạn).
const LEN: usize = (64 << 20) / ELEM;

fn main() {
    let line: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .filter(|l: &usize| l.is_power_of_two() && *l >= ELEM)
        .unwrap_or(64);
    let reps = 7;

    if cfg!(debug_assertions) {
        eprintln!("cảnh báo: đang chạy bản debug; hãy chạy lại với --release");
    }
    let mut buf = vec![1_u32; LEN];
    println!(
        "buffer = {} MiB ({} phần tử u32), dòng cache giả định = {line} B",
        (LEN * ELEM) >> 20,
        LEN
    );

    println!(
        "\n== A. Stride tăng dần: mỗi lượt đi hết buffer, cộng 1 vào mỗi phần tử được chạm =="
    );
    println!(
        "{:>7} {:>10} {:>12} {:>12} {:>10} {:>12}",
        "stride", "stride(B)", "accesses", "lines(est)", "ms/pass", "ns/access"
    );
    let mut stride = 1;
    while stride * ELEM <= 4 * line {
        // `stride` đến lúc chạy nên LLVM không đặc hóa vòng lặp cho stride = 1: mọi dòng của
        // bảng dùng cùng một vòng lặp máy.
        let step = black_box(stride);
        let (t, touched) = best_of(reps, || {
            let mut n = 0_usize;
            for x in buf.iter_mut().step_by(step) {
                *x = x.wrapping_add(1);
                n += 1;
            }
            n
        });
        // Số dòng bị chạm: nếu stride nhỏ hơn dòng thì mọi dòng đều bị chạm.
        let lines = if stride * ELEM < line {
            LEN * ELEM / line
        } else {
            touched
        };
        println!(
            "{stride:>7} {:>10} {touched:>12} {lines:>12} {:>10.2} {:>12.2}",
            stride * ELEM,
            t.as_secs_f64() * 1e3,
            t.as_secs_f64() * 1e9 / touched as f64
        );
        stride *= 2;
    }

    println!(
        "\n== B. Cùng số lần truy cập (mỗi dòng đúng 1 phần tử), thứ tự tuần tự vs ngẫu nhiên =="
    );
    let per_line = line / ELEM;
    let mut order: Vec<u32> = (0..LEN / per_line).map(|i| (i * per_line) as u32).collect();
    let sum_in_order = |order: &[u32], buf: &[u32]| {
        let mut s = 0_u64;
        for &i in order {
            s += u64::from(buf[i as usize]);
        }
        s
    };
    let (t_seq, s_seq) = best_of(reps, || sum_in_order(&order, &buf));
    // Fisher-Yates với PRNG cố định.
    let mut rng = XorShift::new(0xDEAD_BEEF_CAFE_F00D);
    for i in (1..order.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    let (t_rand, s_rand) = best_of(reps, || sum_in_order(&order, &buf));
    assert_eq!(s_seq, s_rand, "cùng tập chỉ số nên tổng phải bằng nhau");
    let accesses = order.len() as f64;
    println!("{} lần truy cập, mỗi lần một dòng khác nhau", order.len());
    println!(
        "tuần tự   : {:>8.2} ms  {:>8.2} ns/access",
        t_seq.as_secs_f64() * 1e3,
        t_seq.as_secs_f64() * 1e9 / accesses
    );
    println!(
        "ngẫu nhiên: {:>8.2} ms  {:>8.2} ns/access",
        t_rand.as_secs_f64() * 1e3,
        t_rand.as_secs_f64() * 1e9 / accesses
    );
    black_box(&mut buf);
}
