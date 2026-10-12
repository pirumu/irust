//! # Bài 3 — Generic hay `dyn`?
//!
//! Cùng một bài toán ("tính tổng diện tích") viết hai kiểu: generic (static dispatch, một bản
//! đã-monomorphize cho mỗi kiểu) và trait object (một bản duy nhất, gọi qua vtable). Rồi chọn
//! đúng công cụ cho từng việc: khi nào *bắt buộc* `dyn`, khi nào generic.
//!
//! Hai đoạn sau bị từ chối. Đọc và giải thích:
//!
//! Một `Vec<S>` chỉ có một kiểu phần tử, nên không trộn `Square` và `Rect` được.
//!
//! ```compile_fail,E0308
//! use lab_language::ex3_dispatch::{Rect, Square};
//! let v = vec![Square(1), Rect(1, 2)];
//! ```
//!
//! Trait có method trả `Self` (không có `where Self: Sized`) không dyn-compatible.
//!
//! ```compile_fail,E0038
//! trait Dup {
//!     fn dup(&self) -> Self;
//! }
//! fn f(_: &dyn Dup) {}
//! ```

/// Hình có diện tích nguyên (tránh so sánh số thực trong test).
pub trait Shape {
    fn area(&self) -> u32;
}

pub struct Square(pub u32);
pub struct Rect(pub u32, pub u32);

impl Shape for Square {
    fn area(&self) -> u32 {
        self.0 * self.0
    }
}

impl Shape for Rect {
    fn area(&self) -> u32 {
        self.0 * self.1
    }
}

/// Generic: mọi phần tử cùng kiểu `S`.
#[cfg(not(feature = "solution"))]
pub fn total_area_static<S: Shape>(items: &[S]) -> u32 {
    todo!("3.1")
}

/// Trait object: các phần tử có thể là kiểu khác nhau.
#[cfg(not(feature = "solution"))]
pub fn total_area_dyn(items: &[Box<dyn Shape>]) -> u32 {
    todo!("3.2")
}

/// Phần tử có diện tích lớn nhất; nếu hòa, lấy phần tử đứng **trước**. Rỗng → `None`.
/// Kết quả mượn từ `items` (lifetime được elision gán).
#[cfg(not(feature = "solution"))]
pub fn largest(items: &[Box<dyn Shape>]) -> Option<&dyn Shape> {
    todo!("3.3: lưu ý max_by_key trả phần tử CUỐI khi hòa")
}

/// Chọn kiểu lúc **chạy** từ chuỗi: `"square:3"` → `Square(3)`, `"rect:2x5"` → `Rect(2, 5)`.
/// Sai định dạng/kiểu lạ → `None`. Đây là việc generic không làm được: kiểu trả về
/// không biết lúc biên dịch.
#[cfg(not(feature = "solution"))]
pub fn make_shape(spec: &str) -> Option<Box<dyn Shape>> {
    todo!("3.4: split_once(':'), rồi split_once('x') cho rect; parse::<u32>().ok()?")
}

/// Áp lần lượt các bước (kiểu khác nhau, đặt trong `Vec<Box<dyn Fn>>`) lên `x`.
#[cfg(not(feature = "solution"))]
pub fn apply_pipeline(stages: &[Box<dyn Fn(i32) -> i32>], x: i32) -> i32 {
    todo!("3.5: fold")
}

/// `compose(f, g)(x) == g(f(x))`. Trả `impl Fn` — không cần `Box` vì chỉ có một kiểu cụ thể.
#[cfg(not(feature = "solution"))]
pub fn compose<F, G>(f: F, g: G) -> impl Fn(i32) -> i32
where
    F: Fn(i32) -> i32,
    G: Fn(i32) -> i32,
{
    move |x| todo!("3.6: g(f(x))")
}

#[cfg(feature = "solution")]
pub use crate::solutions::ex3::{
    apply_pipeline, compose, largest, make_shape, total_area_dyn, total_area_static,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_total() {
        assert_eq!(total_area_static(&[Square(2), Square(3)]), 13);
        assert_eq!(total_area_static::<Rect>(&[]), 0);
    }

    #[test]
    fn dyn_total_mixes_types() {
        let items: Vec<Box<dyn Shape>> = vec![Box::new(Square(2)), Box::new(Rect(2, 5))];
        assert_eq!(total_area_dyn(&items), 14);
        assert_eq!(total_area_dyn(&[]), 0);
    }

    #[test]
    fn largest_prefers_first_on_tie() {
        let items: Vec<Box<dyn Shape>> = vec![
            Box::new(Square(2)),  // 4
            Box::new(Rect(1, 9)), // 9
            Box::new(Rect(3, 3)), // 9, hòa
        ];
        let best = largest(&items).unwrap();
        assert_eq!(best.area(), 9);
        // phải là phần tử index 1, không phải index 2:
        assert!(std::ptr::addr_eq(best, &*items[1]));
        assert!(largest(&[]).is_none());
    }

    #[test]
    fn make_shape_parses_at_runtime() {
        assert_eq!(make_shape("square:3").unwrap().area(), 9);
        assert_eq!(make_shape("rect:2x5").unwrap().area(), 10);
        for bad in ["circle:1", "square:x", "rect:2", "rect:2x", "square", ""] {
            assert!(make_shape(bad).is_none(), "{bad:?} phải bị từ chối");
        }
        let total: u32 = ["square:1", "rect:2x2", "square:3"]
            .iter()
            .filter_map(|s| make_shape(s))
            .map(|s| s.area())
            .sum();
        assert_eq!(total, 14);
    }

    #[test]
    fn pipeline_of_boxed_closures() {
        let offset = 10;
        let stages: Vec<Box<dyn Fn(i32) -> i32>> = vec![
            Box::new(|x| x + 1),
            Box::new(move |x| x * offset),
            Box::new(|x| x - 3),
        ];
        assert_eq!(apply_pipeline(&stages, 2), 27);
        assert_eq!(apply_pipeline(&[], 5), 5);
    }

    #[test]
    fn compose_returns_impl_fn() {
        let f = compose(|x| x + 1, |x| x * 2);
        assert_eq!(f(3), 8);
        let g = compose(f, |x| x - 1);
        assert_eq!(g(0), 1);
    }
}
