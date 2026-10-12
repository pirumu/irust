//! # Bài 1 — Sửa lỗi borrow
//!
//! Mỗi mục dưới đây là một đoạn code **bị trình biên dịch từ chối** (doctest `compile_fail`
//! giữ cho đoạn đó luôn là đoạn sai). Hãy viết bản đúng ở hàm tương ứng; test ở cuối file
//! mô tả hành vi cần có. Đọc kỹ nhãn lỗi (`later used here`…) trước khi sửa.
//!
//! ## 1.1 `read_then_push` — đọc rồi ghi cùng một `Vec`
//!
//! ```compile_fail,E0502
//! let mut v = vec![1, 2, 3];
//! let first = &v[0];
//! v.push(4);
//! println!("{first}");
//! ```
//!
//! ## 1.2 `longest` — hàm trả về tham chiếu
//!
//! ```compile_fail,E0106
//! fn longest(a: &str, b: &str) -> &str {
//!     if a.len() >= b.len() { a } else { b }
//! }
//! ```
//!
//! ## 1.3 `take_first` — move ra khỏi chỉ mục
//!
//! ```compile_fail,E0507
//! let v = vec![String::from("a")];
//! let s = v[0];
//! ```
//!
//! ## 1.4 `remove_empty` — sửa collection khi đang duyệt
//!
//! ```compile_fail,E0502
//! let mut v = vec![String::new(), String::from("x")];
//! for (i, s) in v.iter().enumerate() {
//!     if s.is_empty() {
//!         v.remove(i);
//!     }
//! }
//! ```

/// Nếu `v` không rỗng: đọc phần tử đầu `f`, đẩy `f * 10` vào cuối `v`, trả `Some(f)`.
/// Nếu rỗng: trả `None` và không đổi `v`.
#[cfg(not(feature = "solution"))]
pub fn read_then_push(v: &mut Vec<i32>) -> Option<i32> {
    todo!("1.1: đọc v[0] ra giá trị (Copy) TRƯỚC khi push, để borrow chia sẻ chết trước &mut")
}

/// Trả về chuỗi dài hơn; nếu bằng nhau trả `a`. Kết quả không được sống lâu hơn cả hai đầu vào.
/// Gợi ý: chữ ký dưới đây đã có tham số lifetime — hãy tự giải thích vì sao thiếu nó thì E0106,
/// và thử viết `b: &str` (không `'a`) để đọc lỗi.
#[cfg(not(feature = "solution"))]
pub fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    todo!("1.2")
}

/// Lấy phần tử đầu ra khỏi `v` (move `String` ra), để lại `String::new()` tại chỗ — độ dài `v`
/// không đổi (vì thế nhận `&mut [String]`, không cần `Vec`). Rỗng thì `None`.
#[cfg(not(feature = "solution"))]
pub fn take_first(v: &mut [String]) -> Option<String> {
    todo!("1.3: không thể move ra khỏi v[0]; cần một API hoán đổi/lấy-và-thay")
}

/// Xóa mọi chuỗi rỗng khỏi `v` (giữ nguyên thứ tự các phần tử còn lại); trả số phần tử đã xóa.
#[cfg(not(feature = "solution"))]
pub fn remove_empty(v: &mut Vec<String>) -> usize {
    todo!("1.4: đừng gọi remove trong lúc iter(); dùng API sửa tại chỗ của Vec")
}

#[cfg(feature = "solution")]
pub use crate::solutions::ex1::{longest, read_then_push, remove_empty, take_first};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_then_push_appends_ten_times_first() {
        let mut v = vec![1, 2, 3];
        assert_eq!(read_then_push(&mut v), Some(1));
        assert_eq!(v, [1, 2, 3, 10]);
    }

    #[test]
    fn read_then_push_empty_is_noop() {
        let mut v = Vec::new();
        assert_eq!(read_then_push(&mut v), None);
        assert!(v.is_empty());
    }

    #[test]
    fn longest_picks_longer_and_prefers_first_on_tie() {
        assert_eq!(longest("ab", "c"), "ab");
        assert_eq!(longest("a", "bcd"), "bcd");
        let (a, b) = (String::from("xy"), String::from("zw"));
        assert!(std::ptr::eq(longest(&a, &b), a.as_str()));
    }

    #[test]
    fn longest_result_can_outlive_neither_input_but_inner_scope_is_ok() {
        let outer = String::from("outer string");
        let n;
        {
            let inner = String::from("in");
            n = longest(&outer, &inner).len();
        }
        assert_eq!(n, 12);
    }

    #[test]
    fn take_first_moves_out_and_leaves_empty_string() {
        let mut v = vec![String::from("a"), String::from("b")];
        assert_eq!(take_first(&mut v), Some(String::from("a")));
        assert_eq!(v, ["", "b"]);
        assert_eq!(take_first(&mut Vec::new()), None);
    }

    #[test]
    fn remove_empty_keeps_order_and_counts() {
        let mut v: Vec<String> = ["", "a", "", "b", ""]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(remove_empty(&mut v), 3);
        assert_eq!(v, ["a", "b"]);
        assert_eq!(remove_empty(&mut v), 0);
    }
}
