//! # Bài 2 — Trait, blanket impl, orphan rule và newtype
//!
//! Ba việc: (a) viết một *blanket impl*, (b) bọc kiểu ngoài bằng *newtype* để cài trait ngoài,
//! (c) viết *extension trait* cho mọi iterator.
//!
//! Hai đoạn sau bị từ chối — hãy đọc rồi giải thích bằng lời của bạn vì sao:
//!
//! Orphan rule: trait `Display` và kiểu `Vec<i32>` đều không thuộc crate này.
//!
//! ```compile_fail,E0117
//! use std::fmt;
//! impl fmt::Display for Vec<i32> {
//!     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//!         write!(f, "x")
//!     }
//! }
//! ```
//!
//! Coherence: đã có blanket impl phủ mọi `T: Display`, mà `i32: Display`.
//!
//! ```compile_fail,E0119
//! trait Describe {
//!     fn describe(&self) -> String;
//! }
//! impl<T: std::fmt::Display> Describe for T {
//!     fn describe(&self) -> String {
//!         String::new()
//!     }
//! }
//! impl Describe for i32 {
//!     fn describe(&self) -> String {
//!         String::new()
//!     }
//! }
//! ```

use std::fmt;

/// Mô tả một giá trị. Bạn cài blanket impl bên dưới để **mọi** kiểu `Display` có `describe`.
pub trait Describe {
    fn describe(&self) -> String;
}

// (a) Blanket impl: `describe()` trả `«{giá trị theo Display}»`, ví dụ `5.describe() == "«5»"`.
// `?Sized` để `str` (không có kích thước biên dịch) cũng dùng được.
#[cfg(not(feature = "solution"))]
impl<T: fmt::Display + ?Sized> Describe for T {
    fn describe(&self) -> String {
        todo!("(a) format!(\"«{{}}»\", self)")
    }
}

/// (b) Newtype quanh `Vec<i32>`. Không thể `impl Display for Vec<i32>` (orphan rule), nhưng
/// `Csv` là kiểu của crate này nên được. `Display` in các số cách nhau bằng `,`, không có
/// dấu cách và không có dấu phẩy thừa: `Csv(vec![1, 2, 3])` → `1,2,3`; rỗng → chuỗi rỗng.
pub struct Csv(pub Vec<i32>);

#[cfg(not(feature = "solution"))]
impl fmt::Display for Csv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!("(b) có thể dùng join_with của (c), hoặc vòng lặp + write!")
    }
}

/// (c) Extension trait: nối các phần tử của bất kỳ iterator nào bằng `sep`.
pub trait JoinWith: Iterator {
    fn join_with(self, sep: &str) -> String
    where
        Self: Sized,
        Self::Item: fmt::Display;
}

#[cfg(not(feature = "solution"))]
impl<I: Iterator> JoinWith for I {
    fn join_with(self, sep: &str) -> String
    where
        Self: Sized,
        Self::Item: fmt::Display,
    {
        todo!("(c) không dùng itertools; String + write! hoặc map/collect")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanket_impl_covers_many_display_types() {
        assert_eq!(5.describe(), "«5»");
        assert_eq!("hi".describe(), "«hi»");
        assert_eq!(String::from("x").describe(), "«x»");
        assert_eq!(2.5_f64.describe(), "«2.5»");
        let s: &str = "unsized";
        assert_eq!(s.describe(), "«unsized»");
    }

    #[test]
    fn newtype_displays_and_gets_describe_for_free() {
        let c = Csv(vec![1, 2, 3]);
        assert_eq!(c.to_string(), "1,2,3");
        assert_eq!(c.describe(), "«1,2,3»");
        assert_eq!(Csv(vec![]).to_string(), "");
        assert_eq!(Csv(vec![7]).to_string(), "7");
    }

    #[test]
    fn describe_is_usable_as_trait_object() {
        let items: Vec<Box<dyn Describe>> =
            vec![Box::new(1), Box::new("a"), Box::new(Csv(vec![7, 8]))];
        let got: Vec<String> = items.iter().map(|d| d.describe()).collect();
        assert_eq!(got, ["«1»", "«a»", "«7,8»"]);
    }

    #[test]
    fn join_with_works_on_any_iterator() {
        assert_eq!([1, 2, 3].iter().join_with("-"), "1-2-3");
        assert_eq!("a b c".split(' ').join_with(", "), "a, b, c");
        assert_eq!(std::iter::empty::<i32>().join_with(","), "");
        assert_eq!((1..=3).map(|x| x * x).join_with("+"), "1+4+9");
        assert_eq!(std::iter::once("solo").join_with("/"), "solo");
    }
}
