//! Helpers dùng chung và các hàm "đối tượng đọc assembly" cho lab P2 (CPU).
//!
//! Các hàm `lab_*` có `#[unsafe(no_mangle)]` để symbol trong file `.s` mang đúng tên hàm.
//! Xem asm bằng:
//!
//!   cd labs && cargo rustc -p lab-cpu --lib --release -- --emit asm
//!
//! Hàm `extern "C"` dùng ABI của nền tảng (tham số/trả về theo chuẩn C); hàm `pub fn`
//! thường dùng ABI Rust (không ổn định, xem chương 04 mục 8).

use std::hint::black_box;
use std::time::{Duration, Instant};

/// xorshift64: PRNG nhỏ, seed cố định để hai lần chạy cho cùng dữ liệu.
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> Self {
        // xorshift không được có state 0.
        Self(if seed == 0 {
            0x9E37_79B9_7F4A_7C15
        } else {
            seed
        })
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// Chạy `f` `reps` lần, trả về thời gian của lần nhanh nhất và kết quả lần cuối.
/// Kết quả đi qua `black_box` để compiler không bỏ phép tính.
pub fn best_of<R>(reps: usize, mut f: impl FnMut() -> R) -> (Duration, R) {
    let mut best = Duration::MAX;
    let mut last = black_box(f());
    for _ in 0..reps {
        let t = Instant::now();
        last = black_box(f());
        best = best.min(t.elapsed());
    }
    (best, last)
}

// ---------------------------------------------------------------------------
// Lab 1: đọc asm của hàm Rust
// ---------------------------------------------------------------------------

/// Hàm nhỏ nhất: so asm `opt-level=0` (có kiểm tra tràn số) với `--release`.
#[unsafe(no_mangle)]
pub extern "C" fn lab_add(a: u64, b: u64) -> u64 {
    a + b
}

/// `v[i]` trên slice: tìm `cmp` + nhánh tới `panic_bounds_check`.
#[unsafe(no_mangle)]
pub fn lab_get(v: &[u32], i: usize) -> u32 {
    v[i]
}

/// Bản bỏ kiểm tra biên.
///
/// # Safety
/// `i` phải nhỏ hơn `v.len()`; nếu không là undefined behavior.
#[unsafe(no_mangle)]
pub unsafe fn lab_get_unchecked(v: &[u32], i: usize) -> u32 {
    // SAFETY: người gọi đảm bảo `i < v.len()` (hợp đồng ở phần `# Safety`).
    unsafe { *v.get_unchecked(i) }
}

/// Vòng lặp theo `0..v.len()`: compiler có thể chứng minh chỉ số hợp lệ.
#[unsafe(no_mangle)]
pub fn lab_sum_iter(v: &[u32]) -> u32 {
    let mut s = 0u32;
    #[allow(clippy::needless_range_loop)] // cố ý: giữ dạng `v[i]` để xem bounds check bị bỏ
    for i in 0..v.len() {
        s = s.wrapping_add(v[i]);
    }
    s
}

/// Chỉ số `n` đến từ ngoài: kiểm tra phải còn (hoặc được đưa ra trước vòng lặp).
#[unsafe(no_mangle)]
pub fn lab_sum_idx(v: &[u32], n: usize) -> u32 {
    let mut s = 0u32;
    #[allow(clippy::needless_range_loop)]
    // cố ý: giữ dạng `v[i]` với `n` từ ngoài để xem bounds check
    for i in 0..n {
        s = s.wrapping_add(v[i]);
    }
    s
}

// ---------------------------------------------------------------------------
// Lab 2: calling convention và stack frame
// ---------------------------------------------------------------------------

/// Hàm không inline để `lab_two` buộc phải `call`.
#[inline(never)]
#[unsafe(no_mangle)]
pub extern "C" fn lab_ext(x: u64) -> u64 {
    x ^ 0x55
}

/// Tám tham số nguyên: x86-64 SysV chỉ có 6 thanh ghi tham số nguyên, AArch64 có 8 (x0-x7).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // cố ý: 8 tham số để vượt số thanh ghi tham số của x86-64
pub extern "C" fn lab_sum8(a: u64, b: u64, c: u64, d: u64, e: u64, f: u64, g: u64, h: u64) -> u64 {
    a + b + c + d + e + f + g + h
}

/// Giá trị cần sống qua hai lời gọi: phải nằm trong thanh ghi callee-saved
/// và hàm phải có prologue/epilogue để lưu/khôi phục chúng.
#[unsafe(no_mangle)]
pub extern "C" fn lab_two(x: u64) -> u64 {
    let a = lab_ext(x);
    let b = lab_ext(a + 1);
    a * b
}

/// Struct hai trường: trả về trong cặp thanh ghi.
#[repr(C)]
pub struct Two {
    pub a: u64,
    pub b: u64,
}

/// Struct ba trường (24 byte): trả về qua con trỏ kết quả do caller cấp.
#[repr(C)]
pub struct Big {
    pub a: u64,
    pub b: u64,
    pub c: u64,
}

#[unsafe(no_mangle)]
pub extern "C" fn lab_mk2(x: u64) -> Two {
    Two { a: x, b: x + 1 }
}

#[unsafe(no_mangle)]
pub extern "C" fn lab_mk(x: u64) -> Big {
    Big {
        a: x,
        b: x + 1,
        c: x + 2,
    }
}

/// Hàm lá có mảng cục bộ: xem frame nằm dưới `rsp` (red zone) hay trừ SP thật.
#[unsafe(no_mangle)]
pub extern "C" fn lab_leaf_local(i: usize) -> u64 {
    let mut a = [0u64; 4];
    a[i & 3] = i as u64;
    a[1] + a[2]
}

// ---------------------------------------------------------------------------
// Lab 3: branch prediction
// ---------------------------------------------------------------------------

/// Có nhánh `if` thật: `black_box` chặn compiler đổi `if` thành `cmov`/`csel`.
#[inline(never)]
#[unsafe(no_mangle)]
pub fn lab_sum_branchy(data: &[u8]) -> u64 {
    let mut s = 0u64;
    for &x in data {
        if x >= 128 {
            s = black_box(s + u64::from(x));
        }
    }
    s
}

/// Không nhánh: mặt nạ toàn bit 1 nếu `x >= 128`, 0 nếu không.
#[inline(never)]
#[unsafe(no_mangle)]
pub fn lab_sum_branchless(data: &[u8]) -> u64 {
    let mut s = 0u64;
    for &x in data {
        let x = black_box(x);
        let mask = u64::from(x >> 7).wrapping_neg();
        s += u64::from(x) & mask;
    }
    s
}

/// `if` viết thẳng, không `black_box`: compiler tự do chọn cách dịch.
#[inline(never)]
#[unsafe(no_mangle)]
pub fn lab_sum_plain(data: &[u8]) -> u64 {
    let mut s = 0u64;
    for &x in data {
        if x >= 128 {
            s += u64::from(x);
        }
    }
    s
}
