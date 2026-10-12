//! Lab P9 — Rust language. Mỗi bài là một module `exN_*`, có đề trong doc của module,
//! phần `todo!()` để học viên điền, và test có sẵn ở cuối file.
//!
//! - Không bật feature: các hàm là stub `todo!()`, test sẽ **đỏ** cho tới khi bạn cài xong.
//! - `--features solution`: biên dịch lời giải mẫu trong `src/solutions/` (dùng để kiểm lab).
//!
//! Đề bài đầy đủ: `docs/labs/09-rust-language-lab.md`.

// Ở chế độ stub, tham số/import chưa dùng là bình thường vì thân hàm là `todo!()`.
#![cfg_attr(
    not(feature = "solution"),
    allow(unused_variables, dead_code, unused_imports, clippy::ptr_arg, clippy::drop_non_drop)
)]

pub mod ex1_borrow;
pub mod ex2_trait;
pub mod ex3_dispatch;
pub mod ex4_myrc;
pub mod ex5_error;
pub mod ex6_iter;
pub mod ex7_macro;
pub mod ex8_features;

#[cfg(feature = "solution")]
mod solutions;
