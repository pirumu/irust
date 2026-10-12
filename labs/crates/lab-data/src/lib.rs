//! Phase 12 labs, one node: log-structured storage, a mini LSM-tree, row vs column layout, a tagged
//! binary encoding, MVCC with isolation anomalies, batch jobs and latency percentiles.
//!
//! Everything is `std` only. The binaries in `src/bin/` are the lab exercises; the modules here hold
//! the code they exercise, with unit tests that assert the behaviour the labs print.

pub mod batch;
pub mod bloom;
pub mod codec;
pub mod crc32c;
#[cfg(unix)]
pub mod kvlog;
pub mod latency;
pub mod layout;
#[cfg(unix)]
pub mod lsm;
pub mod mvcc;
#[cfg(unix)]
pub mod pagestore;
pub mod record;
pub mod rng;
#[cfg(unix)]
pub mod sstable;
pub mod util;

pub use util::need_unix;
