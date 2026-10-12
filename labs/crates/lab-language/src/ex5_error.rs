//! # Bài 5 — Kiểu lỗi riêng, `From` và toán tử `?`
//!
//! Parse một file cấu hình dạng `key=value` (xem test để biết định dạng). Bạn viết:
//! `Display`, `Error::source`, `From<ParseIntError>` (để `?` tự chuyển lỗi), `parse_config`,
//! và `load_port` trả `Box<dyn Error + Send + Sync>`.
//!
//! Quy tắc parse (`parse_config`):
//! - bỏ qua dòng trống và dòng bắt đầu bằng `#` (sau khi cắt khoảng trắng đầu/cuối);
//! - mỗi dòng còn lại phải có dấu `=`, nếu không → `ConfigError::Syntax { line }` với `line`
//!   đánh số từ 1 tính **cả** dòng trống/chú thích;
//! - key và value được cắt khoảng trắng; chỉ `port` (`u16`) và `workers` (`usize`) được nhận,
//!   key khác bị bỏ qua; số sai → `ConfigError::InvalidNumber(e)`;
//! - thiếu key bắt buộc → `ConfigError::Missing("port")` / `Missing("workers")` (kiểm `port`
//!   trước); lỗi `Syntax`/`InvalidNumber` gặp trong lúc duyệt dòng luôn đến trước `Missing`.

use std::num::ParseIntError;

#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    pub port: u16,
    pub workers: usize,
}

#[derive(Debug, PartialEq)]
pub enum ConfigError {
    /// Dòng (đánh số từ 1) không có dấu `=`.
    Syntax { line: usize },
    /// Thiếu key bắt buộc.
    Missing(&'static str),
    /// Giá trị không phải số hợp lệ. Giữ lỗi gốc để `source()` trả về nó.
    InvalidNumber(ParseIntError),
}

// Thông điệp cố định (test kiểm đúng chuỗi):
//   Syntax        → "line {line}: expected key=value"
//   Missing(k)    → "missing key `{k}`"
//   InvalidNumber → "invalid number: {lỗi gốc}"
#[cfg(not(feature = "solution"))]
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        todo!("match self {{ ... }} với write!")
    }
}

// `InvalidNumber` phải trả lỗi gốc qua `source()`; hai biến thể còn lại trả `None`.
#[cfg(not(feature = "solution"))]
impl std::error::Error for ConfigError {}

// Cho phép `?` trên `Result<_, ParseIntError>` trong hàm trả `Result<_, ConfigError>`.
#[cfg(not(feature = "solution"))]
impl From<ParseIntError> for ConfigError {
    fn from(e: ParseIntError) -> Self {
        todo!()
    }
}

#[cfg(not(feature = "solution"))]
pub fn parse_config(text: &str) -> Result<Config, ConfigError> {
    todo!("duyệt lines().enumerate(); dùng `?` cho cả parse số lẫn ok_or(..)")
}

/// Trả cổng đã parse. Cổng `0` bị từ chối bằng lỗi có thông điệp đúng `"port must not be 0"`
/// (tạo từ `&str`/`String` bằng `.into()`). Lỗi của `parse_config` đi lên bằng `?` và phải
/// còn `downcast_ref::<ConfigError>()` được.
#[cfg(not(feature = "solution"))]
pub fn load_port(text: &str) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    todo!()
}

#[cfg(feature = "solution")]
pub use crate::solutions::ex5::{load_port, parse_config};

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn parses_valid_config_with_comments_and_spaces() {
        let text = "# demo\n\n  port = 8080 \nworkers=4\nunknown=whatever\n";
        assert_eq!(
            parse_config(text),
            Ok(Config {
                port: 8080,
                workers: 4
            })
        );
    }

    #[test]
    fn syntax_error_reports_line_counting_blank_and_comment_lines() {
        let text = "# c\n\nport=1\nbroken line\nworkers=2";
        let err = parse_config(text).unwrap_err();
        assert_eq!(err, ConfigError::Syntax { line: 4 });
        assert_eq!(err.to_string(), "line 4: expected key=value");
    }

    #[test]
    fn missing_key_is_reported_port_first() {
        let err = parse_config("workers=2").unwrap_err();
        assert_eq!(err, ConfigError::Missing("port"));
        assert_eq!(err.to_string(), "missing key `port`");
        assert_eq!(
            parse_config("port=1").unwrap_err(),
            ConfigError::Missing("workers")
        );
        assert_eq!(parse_config("").unwrap_err(), ConfigError::Missing("port"));
    }

    #[test]
    fn invalid_number_keeps_source_through_question_mark() {
        let err = parse_config("port=abc\nworkers=1").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidNumber(_)));
        assert!(err.to_string().starts_with("invalid number: "));
        let source = err.source().expect("InvalidNumber phải có source");
        assert!(source.downcast_ref::<ParseIntError>().is_some());
    }

    #[test]
    fn out_of_range_port_is_invalid_number() {
        let err = parse_config("port=70000\nworkers=1").unwrap_err();
        assert!(matches!(err, ConfigError::InvalidNumber(_)));
        let neg = parse_config("port=1\nworkers=-1").unwrap_err();
        assert!(matches!(neg, ConfigError::InvalidNumber(_)));
    }

    #[test]
    fn other_variants_have_no_source() {
        assert!(ConfigError::Missing("port").source().is_none());
        assert!(ConfigError::Syntax { line: 1 }.source().is_none());
    }

    #[test]
    fn from_impl_is_what_question_mark_uses() {
        let e: ConfigError = "x".parse::<u16>().unwrap_err().into();
        assert!(matches!(e, ConfigError::InvalidNumber(_)));
    }

    #[test]
    fn load_port_ok_and_zero_rejected() {
        assert_eq!(load_port("port=8080\nworkers=1").unwrap(), 8080);
        let err = load_port("port=0\nworkers=1").unwrap_err();
        assert_eq!(err.to_string(), "port must not be 0");
        assert!(err.downcast_ref::<ConfigError>().is_none());
    }

    #[test]
    fn load_port_propagates_config_error_downcastable() {
        let err = load_port("workers=1").unwrap_err();
        assert_eq!(
            err.downcast_ref::<ConfigError>(),
            Some(&ConfigError::Missing("port"))
        );
    }
}
