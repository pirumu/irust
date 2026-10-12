//! # Bài 7 — `macro_rules!` nhỏ
//!
//! Viết ba macro (stub hiện trả giá trị sai để test **đỏ** nhưng vẫn biên dịch):
//!
//! 1. `count_exprs!(a, b, c)` → `3usize`; `count_exprs!()` → `0`. Gợi ý: đệ quy, và **không**
//!    được đánh giá các biểu thức (chỉ đếm).
//! 2. `hashmap!{ k => v, ... }` → `HashMap` (dấu phẩy cuối tùy chọn; `hashmap!{}` ra map rỗng).
//!    Dùng `HashMap::with_capacity` với số cặp đếm bằng `$crate::count_exprs!`.
//! 3. `max_of!(a, b, c, ...)` → giá trị lớn nhất (`PartialOrd`), **mỗi đối số đánh giá đúng
//!    một lần, từ trái qua phải** (bind vào biến cục bộ, đừng lặp `$x`).
//!
//! Hygiene: biến `let` do macro tạo ra không "rò" vào code gọi. Đoạn sau bị từ chối:
//!
//! ```compile_fail,E0425
//! macro_rules! make_x {
//!     () => {
//!         let x = 1;
//!     };
//! }
//! fn main() {
//!     make_x!();
//!     println!("{x}");
//! }
//! ```

#[cfg(not(feature = "solution"))]
#[macro_export]
macro_rules! count_exprs {
    ($($t:tt)*) => {
        0usize // TODO: đệ quy: () => 0 ; ($head:expr $(, $tail:expr)*) => 1 + count!(tail)
    };
}

#[cfg(not(feature = "solution"))]
#[macro_export]
macro_rules! hashmap {
    ($($t:tt)*) => {
        ::std::collections::HashMap::new() // TODO: () và ($($k:expr => $v:expr),+ $(,)?)
    };
}

#[cfg(not(feature = "solution"))]
#[macro_export]
macro_rules! max_of {
    ($($t:tt)*) => {
        0 // TODO: ($x:expr $(,)?) và ($x:expr, $($rest:expr),+ $(,)?)
    };
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    #[test]
    fn count_exprs_counts_without_evaluating() {
        assert_eq!(crate::count_exprs!(), 0);
        assert_eq!(crate::count_exprs!(1), 1);
        assert_eq!(crate::count_exprs!(1, "a", 3.0), 3);
        // nếu macro đánh giá đối số, dòng này sẽ panic:
        assert_eq!(crate::count_exprs!(panic!("không được đánh giá"), 2), 2);
    }

    #[test]
    fn hashmap_builds_map() {
        let m: HashMap<&str, i32> = crate::hashmap! { "a" => 1, "b" => 2 };
        assert_eq!(m.len(), 2);
        assert_eq!(m["a"], 1);
        assert_eq!(m["b"], 2);
    }

    #[test]
    fn hashmap_accepts_trailing_comma_and_empty() {
        let m: HashMap<i32, &str> = crate::hashmap! { 1 => "x", 2 => "y", };
        assert_eq!(m.len(), 2);
        let e: HashMap<String, u8> = crate::hashmap! {};
        assert!(e.is_empty());
    }

    #[test]
    fn hashmap_keys_and_values_are_arbitrary_expressions() {
        let base = 10;
        let m: HashMap<i32, String> =
            crate::hashmap! { base + 1 => "a".to_string(), base * 2 => String::from("b") };
        assert_eq!(m[&11], "a");
        assert_eq!(m[&20], "b");
    }

    #[test]
    fn hashmap_preallocates_by_pair_count() {
        let m: HashMap<i32, i32> = crate::hashmap! { 1 => 1, 2 => 2, 3 => 3, 4 => 4, 5 => 5 };
        assert!(m.capacity() >= 5);
    }

    #[test]
    fn max_of_picks_largest() {
        assert_eq!(crate::max_of!(7), 7);
        assert_eq!(crate::max_of!(1, 9), 9);
        assert_eq!(crate::max_of!(4, 2, 8, 3,), 8);
        assert_eq!(crate::max_of!(2.5_f64 as i32, 1), 2);
        assert_eq!(crate::max_of!(-1_i64, -5, -3), -1);
    }

    #[test]
    fn max_of_is_hygienic_with_callers_variables() {
        // macro dùng biến cục bộ riêng; biến `a`, `b` của người gọi không bị đè.
        let a = 1;
        let b = 9;
        assert_eq!(crate::max_of!(a, b), 9);
        assert_eq!(crate::max_of!(b, a), 9);
        assert_eq!((a, b), (1, 9));
    }

    #[test]
    fn max_of_evaluates_each_argument_once_left_to_right() {
        let calls = Cell::new(0);
        let next = || {
            calls.set(calls.get() + 1);
            calls.get()
        };
        let m = crate::max_of!(next(), next(), next());
        assert_eq!(calls.get(), 3);
        assert_eq!(m, 3);
    }
}
