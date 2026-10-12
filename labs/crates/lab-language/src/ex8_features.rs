//! # Bài 8 — Cargo features
//!
//! `Cargo.toml` của crate này khai báo hai feature: `solution` và `shout`. Bạn viết:
//!
//! - `enabled_features()`: trả tên các feature **đang bật khi biên dịch**, theo thứ tự
//!   `["solution", "shout"]` (chỉ giữ cái đang bật). Dùng macro `cfg!(feature = "...")`.
//! - `shout(name)` chỉ tồn tại khi bật feature `shout`: trả `name` viết hoa kèm `!`.
//!   Đây là feature **cộng thêm** (additive): bật nó chỉ thêm hàm, không đổi hành vi hàm khác.
//!
//! Chạy test ở ba cấu hình và để ý test nào được biên dịch / bị loại ở mỗi cấu hình:
//! `--features solution`, `--features solution,shout`, `--all-features`.
//! Phần lệnh workspace/`cargo tree` nằm trong tài liệu lab.

/// Tên các feature đang bật, theo thứ tự `solution`, `shout`.
#[cfg(not(feature = "solution"))]
pub fn enabled_features() -> Vec<&'static str> {
    todo!("cfg!(feature = \"...\") là một biểu thức bool tại thời điểm biên dịch")
}

#[cfg(feature = "solution")]
pub use crate::solutions::ex8::enabled_features;

/// `shout("an")` → `"AN!"`.
#[cfg(all(feature = "shout", not(feature = "solution")))]
pub fn shout(name: &str) -> String {
    todo!()
}

#[cfg(all(feature = "shout", feature = "solution"))]
pub use crate::solutions::ex8::shout;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_list_matches_cfg() {
        let got = enabled_features();
        assert_eq!(got.contains(&"solution"), cfg!(feature = "solution"));
        assert_eq!(got.contains(&"shout"), cfg!(feature = "shout"));
        assert_eq!(
            got.len(),
            usize::from(cfg!(feature = "solution")) + usize::from(cfg!(feature = "shout"))
        );
    }

    #[test]
    fn features_order_is_stable() {
        let got = enabled_features();
        if got.len() == 2 {
            assert_eq!(got, ["solution", "shout"]);
        }
    }

    #[cfg(feature = "shout")]
    #[test]
    fn shout_uppercases_and_adds_bang() {
        assert_eq!(shout("an"), "AN!");
        assert_eq!(shout(""), "!");
    }
}
