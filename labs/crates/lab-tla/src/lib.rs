//! Phase 13 labs: TLA+ and model checking, practised in Rust.
//!
//! The TLA+ specs in `specs/` are files for the TLA+ tools (Java: `pcal.trans`, `tlc2.TLC`).
//! Nothing in this crate runs them. What runs is the Rust side, which does three jobs:
//!
//! 1. [`mc`] is a small explicit-state model checker built the way TLC works (breadth-first
//!    search, invariants on every state, shortest counterexample, TLC-style statistics), and
//!    [`live`] adds `P ~> Q` under weak/strong fairness with lasso counterexamples.
//! 2. [`counter`], [`commit`], [`queue`], [`request`] and [`simple`] re-encode the *same* models
//!    as the specs, so `cargo run --bin ...` prints the number of distinct states and either
//!    "holds" or the shortest counterexample; [`simple`] and [`commit`] also reproduce the state
//!    counts that the `tlaplus/Examples` repository records for TLC.
//! 3. [`sut`] is a concrete two-phase-commit implementation (with injectable bugs) and
//!    [`conform`] is the differential harness that tests it against the model, in both directions.
//!
//! Binaries (from `labs/`): `calibrate`, `lost_update`, `commit`, `queue`, `liveness`, `conformance`.

pub mod commit;
pub mod conform;
pub mod counter;
pub mod live;
pub mod mc;
pub mod queue;
pub mod request;
pub mod simple;
pub mod sut;
