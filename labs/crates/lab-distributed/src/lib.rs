//! P12b labs: deterministic, seedable simulations for the distributed-data chapters of
//! `docs/12-he-thong-du-lieu/` (replication, quorums, partitioning, fencing, ordering, Raft,
//! streams, Redis patterns). The write-up is `docs/labs/12b-du-lieu-phan-tan-lab.md`.
//!
//! Every simulation is a pure function of a seed: no wall clock, no threads, no `HashMap`
//! iteration order in any output. The binaries in `src/bin/` print the same text on every
//! machine for the same seed.

pub mod cache;
pub mod clocks;
pub mod delivery;
pub mod fencing;
pub mod limiter;
pub mod partition;
pub mod quorum;
pub mod raft;
pub mod redis_loop;
pub mod replication;
pub mod rng;
pub mod windows;
