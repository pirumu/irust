//! Lab 2: khoảng cách giữa các stack frame của một hàm đệ quy.
//!
//!   cargo run -q -p lab-cpu --bin frames            # debug (opt-level=0)
//!   cargo run -q -p lab-cpu --release --bin frames  # release (opt-level=3)
//!
//! Chương trình chỉ in số; việc của bạn là đối chiếu với asm của `descend`
//! (`cargo rustc -p lab-cpu --bin frames --release -- --emit asm`).

use std::hint::black_box;

/// Đệ quy `depth` lần, ghi lại địa chỉ một biến cục bộ ở mỗi frame.
/// `#[inline(never)]` và `black_box` ngăn compiler gộp các lời gọi hoặc bỏ biến cục bộ.
#[inline(never)]
fn descend(depth: usize, out: &mut Vec<usize>) {
    let local = [0_u8; 64];
    out.push(black_box(&local).as_ptr() as usize);
    if depth > 0 {
        descend(depth - 1, out);
    }
    // Dùng `local` sau lời gọi đệ quy để nó phải sống qua lời gọi (không thành tail call).
    black_box(&local);
}

fn main() {
    let mut addrs = Vec::with_capacity(8);
    descend(5, &mut addrs);

    println!(
        "target_arch={} debug_assertions={}",
        std::env::consts::ARCH,
        cfg!(debug_assertions)
    );
    println!("depth  local address        delta to next frame (bytes)  delta % 16");
    for (depth, pair) in addrs.windows(2).enumerate() {
        // Stack thường mọc xuống: frame sâu hơn có địa chỉ nhỏ hơn. In có dấu để thấy chiều.
        let delta = pair[1] as i128 - pair[0] as i128;
        println!(
            "{depth:>5}  {:#018x}   {delta:>28}  {:>10}",
            pair[0],
            delta.rem_euclid(16)
        );
    }
    if let Some(last) = addrs.last() {
        println!("{:>5}  {last:#018x}", addrs.len() - 1);
    }
}
