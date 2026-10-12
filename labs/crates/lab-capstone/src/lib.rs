//! P7 capstone: shared pieces of the two reference solutions.
//!
//! - [`http`]: a minimal HTTP/1.1 request parser and response builder (plain std, any OS).
//! - `sys`: thin wrappers over the Linux syscalls both servers use (epoll, accept4, fcntl,
//!   read, send). Linux only.
//!
//! The binaries are `epoll_http` (project 1), `mini_runtime` (project 2) and `load_client`.

pub mod http;

#[cfg(target_os = "linux")]
pub mod sys;

/// Send buffer size the lab servers request for every accepted socket.
///
/// A deliberately small buffer makes `send` return short counts and `EAGAIN` on the
/// 1 MiB `/big` response, so the partial-write path really runs during the lab instead of
/// depending on the kernel's autotuned buffer size.
pub const LAB_SNDBUF: usize = 16 * 1024;
