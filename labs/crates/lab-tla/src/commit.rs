//! Transaction commit, P13 chapter 08, sections 2 and 3, written to match the specs in
//! `tlaplus/Examples` (`transaction_commit/TCommit.tla` and `TwoPhase.tla`) that are mirrored
//! in `specs/TCommit.tla` and `specs/TwoPhase.tla`.
//!
//! * [`TCommit`] is the *requirement*: resource managers (RMs) move working -> prepared ->
//!   committed/aborted, and nobody may commit while another has aborted.
//! * [`TwoPhase`] is the *protocol*: a transaction manager (TM) and a set `msgs` of every message
//!   ever sent. [`check_refinement`](crate::mc::check_refinement) shows it implements `TCommit`.
//!
//! Both have a deliberate bug switch (`buggy`) that removes exactly one guard.

use std::collections::BTreeSet;
use std::fmt;

use crate::mc::{Invariant, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rm {
    Working,
    Prepared,
    Committed,
    Aborted,
}

impl Rm {
    fn name(self) -> &'static str {
        match self {
            Rm::Working => "working",
            Rm::Prepared => "prepared",
            Rm::Committed => "committed",
            Rm::Aborted => "aborted",
        }
    }
}

/// `TCConsistent`: no RM is aborted while another is committed.
pub fn tc_consistent(rm_state: &[Rm]) -> bool {
    !(rm_state.contains(&Rm::Aborted) && rm_state.contains(&Rm::Committed))
}

/// RM `i` (0-based inside Rust, printed as `r{i+1}` like the model values `r1, r2, r3`).
fn r(i: usize) -> String {
    format!("r{}", i + 1)
}

fn rm_function(rm_state: &[Rm]) -> String {
    let items: Vec<String> = rm_state
        .iter()
        .enumerate()
        .map(|(i, s)| format!("{} |-> \"{}\"", r(i), s.name()))
        .collect();
    format!("[{}]", items.join(", "))
}

// ---------------------------------------------------------------- TCommit

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TcAction {
    Prepare(usize),
    /// First branch of `Decide(rm)`: commit.
    DecideCommit(usize),
    /// Second branch of `Decide(rm)`: abort.
    DecideAbort(usize),
}

impl fmt::Debug for TcAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TcAction::Prepare(i) => write!(f, "Prepare({})", r(*i)),
            TcAction::DecideCommit(i) => write!(f, "Decide({}) [commit]", r(*i)),
            TcAction::DecideAbort(i) => write!(f, "Decide({}) [abort]", r(*i)),
        }
    }
}

pub struct TCommit {
    pub rms: usize,
    /// `true`: the abort branch of `Decide` forgets `notCommitted` (the bug of chapter 04).
    pub buggy: bool,
}

impl TCommit {
    pub fn new(rms: usize) -> Self {
        Self { rms, buggy: false }
    }

    pub fn buggy(rms: usize) -> Self {
        Self { rms, buggy: true }
    }

    pub fn invariants(&self) -> Vec<Invariant<Vec<Rm>>> {
        vec![Invariant::new("TCConsistent", |s: &Vec<Rm>| {
            tc_consistent(s)
        })]
    }

    pub fn show(s: &[Rm]) -> String {
        format!("/\\ rmState = {}", rm_function(s))
    }
}

impl Model for TCommit {
    type State = Vec<Rm>;
    type Action = TcAction;

    fn init(&self) -> Vec<Vec<Rm>> {
        vec![vec![Rm::Working; self.rms]]
    }

    fn actions(&self) -> Vec<TcAction> {
        (0..self.rms)
            .flat_map(|i| {
                [
                    TcAction::Prepare(i),
                    TcAction::DecideCommit(i),
                    TcAction::DecideAbort(i),
                ]
            })
            .collect()
    }

    fn step(&self, s: &Vec<Rm>, action: &TcAction) -> Option<Vec<Rm>> {
        // canCommit == \A rm \in RM : rmState[rm] \in {"prepared", "committed"}
        let can_commit = s.iter().all(|&x| x == Rm::Prepared || x == Rm::Committed);
        // notCommitted == \A rm \in RM : rmState[rm] # "committed"
        let not_committed = !s.contains(&Rm::Committed);
        let (i, enabled, to) = match *action {
            TcAction::Prepare(i) => (i, s[i] == Rm::Working, Rm::Prepared),
            TcAction::DecideCommit(i) => (i, s[i] == Rm::Prepared && can_commit, Rm::Committed),
            TcAction::DecideAbort(i) => (
                i,
                matches!(s[i], Rm::Working | Rm::Prepared) && (self.buggy || not_committed),
                Rm::Aborted,
            ),
        };
        if !enabled {
            return None;
        }
        let mut next = s.clone();
        next[i] = to;
        Some(next)
    }
}

// --------------------------------------------------------------- TwoPhase

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tm {
    Init,
    Committed,
    Aborted,
}

impl Tm {
    fn name(self) -> &'static str {
        match self {
            Tm::Init => "init",
            Tm::Committed => "committed",
            Tm::Aborted => "aborted",
        }
    }
}

/// `Message ==  [type : {"Prepared"}, rm : RM] \cup [type : {"Commit", "Abort"}]`
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Msg {
    Prepared(usize),
    Commit,
    Abort,
}

impl fmt::Debug for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Msg::Prepared(i) => write!(f, "[type |-> \"Prepared\", rm |-> {}]", r(*i)),
            Msg::Commit => write!(f, "[type |-> \"Commit\"]"),
            Msg::Abort => write!(f, "[type |-> \"Abort\"]"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TpState {
    pub rm: Vec<Rm>,
    pub tm: Tm,
    /// `tmPrepared`: the RMs whose `Prepared` message the TM has received.
    pub tm_prepared: BTreeSet<usize>,
    /// `msgs`: every message ever sent; never shrinks.
    pub msgs: BTreeSet<Msg>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TpAction {
    TmCommit,
    TmAbort,
    TmRcvPrepared(usize),
    RmPrepare(usize),
    RmChooseToAbort(usize),
    RmRcvCommitMsg(usize),
    RmRcvAbortMsg(usize),
}

impl fmt::Debug for TpAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TpAction::TmCommit => write!(f, "TMCommit"),
            TpAction::TmAbort => write!(f, "TMAbort"),
            TpAction::TmRcvPrepared(i) => write!(f, "TMRcvPrepared({})", r(*i)),
            TpAction::RmPrepare(i) => write!(f, "RMPrepare({})", r(*i)),
            TpAction::RmChooseToAbort(i) => write!(f, "RMChooseToAbort({})", r(*i)),
            TpAction::RmRcvCommitMsg(i) => write!(f, "RMRcvCommitMsg({})", r(*i)),
            TpAction::RmRcvAbortMsg(i) => write!(f, "RMRcvAbortMsg({})", r(*i)),
        }
    }
}

pub struct TwoPhase {
    pub rms: usize,
    /// `true`: `TMCommit` no longer waits for `tmPrepared = RM` (the bug of chapter 08, section 7).
    pub buggy: bool,
}

impl TwoPhase {
    pub fn new(rms: usize) -> Self {
        Self { rms, buggy: false }
    }

    pub fn buggy(rms: usize) -> Self {
        Self { rms, buggy: true }
    }

    /// `rmState`, the only variable `TCommit` knows about: the refinement mapping.
    pub fn project(s: &TpState) -> Vec<Rm> {
        s.rm.clone()
    }

    pub fn invariants(&self) -> Vec<Invariant<TpState>> {
        vec![Invariant::new("TCConsistent", |s: &TpState| {
            tc_consistent(&s.rm)
        })]
    }

    pub fn show(s: &TpState) -> String {
        let set = |items: Vec<String>| format!("{{{}}}", items.join(", "));
        format!(
            "/\\ rmState = {}\n/\\ tmState = \"{}\"\n/\\ tmPrepared = {}\n/\\ msgs = {}",
            rm_function(&s.rm),
            s.tm.name(),
            set(s.tm_prepared.iter().map(|&i| r(i)).collect()),
            set(s.msgs.iter().map(|m| format!("{m:?}")).collect()),
        )
    }
}

impl Model for TwoPhase {
    type State = TpState;
    type Action = TpAction;

    fn init(&self) -> Vec<TpState> {
        vec![TpState {
            rm: vec![Rm::Working; self.rms],
            tm: Tm::Init,
            tm_prepared: BTreeSet::new(),
            msgs: BTreeSet::new(),
        }]
    }

    /// The order of `TPNext`: the two TM actions, then `\E rm` over the five RM-indexed actions.
    fn actions(&self) -> Vec<TpAction> {
        let mut out = vec![TpAction::TmCommit, TpAction::TmAbort];
        for i in 0..self.rms {
            out.extend([
                TpAction::TmRcvPrepared(i),
                TpAction::RmPrepare(i),
                TpAction::RmChooseToAbort(i),
                TpAction::RmRcvCommitMsg(i),
                TpAction::RmRcvAbortMsg(i),
            ]);
        }
        out
    }

    fn step(&self, s: &TpState, action: &TpAction) -> Option<TpState> {
        let mut next = s.clone();
        match *action {
            TpAction::TmRcvPrepared(i) => {
                if s.tm != Tm::Init || !s.msgs.contains(&Msg::Prepared(i)) {
                    return None;
                }
                next.tm_prepared.insert(i);
            }
            TpAction::TmCommit => {
                let all_prepared = s.tm_prepared.len() == self.rms;
                if s.tm != Tm::Init || !(self.buggy || all_prepared) {
                    return None;
                }
                next.tm = Tm::Committed;
                next.msgs.insert(Msg::Commit);
            }
            TpAction::TmAbort => {
                if s.tm != Tm::Init {
                    return None;
                }
                next.tm = Tm::Aborted;
                next.msgs.insert(Msg::Abort);
            }
            TpAction::RmPrepare(i) => {
                if s.rm[i] != Rm::Working {
                    return None;
                }
                next.rm[i] = Rm::Prepared;
                next.msgs.insert(Msg::Prepared(i));
            }
            TpAction::RmChooseToAbort(i) => {
                if s.rm[i] != Rm::Working {
                    return None;
                }
                next.rm[i] = Rm::Aborted;
            }
            // These two have no guard on `rmState[rm]` in the original spec; message ordering
            // (the invariants of `TwoPhase_proof.tla`) is what keeps them safe.
            TpAction::RmRcvCommitMsg(i) => {
                if !s.msgs.contains(&Msg::Commit) {
                    return None;
                }
                next.rm[i] = Rm::Committed;
            }
            TpAction::RmRcvAbortMsg(i) => {
                if !s.msgs.contains(&Msg::Abort) {
                    return None;
                }
                next.rm[i] = Rm::Aborted;
            }
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::{check, check_refinement, explore};

    /// Closed forms derived by hand in P13 chapter 04 (section 3, "Đi sâu") for `TCommit`:
    /// `3^n + 2^n - 1` distinct states and `1 + n (3^n + 2^(n-1))` generated.
    #[test]
    fn tcommit_counts_match_the_hand_derivation() {
        for n in 1..=6u32 {
            let s = explore(&TCommit::new(n as usize)).stats;
            assert_eq!(s.distinct, 3usize.pow(n) + 2usize.pow(n) - 1, "n = {n}");
            assert_eq!(
                s.generated,
                1 + n as usize * (3usize.pow(n) + 2usize.pow(n - 1)),
                "n = {n}"
            );
            // Longest shortest path: n prepares, then n commits.
            assert_eq!(s.depth, 2 * n as usize + 1, "n = {n}");
        }
    }

    /// The numbers `manifest.json` of tlaplus/Examples records for `TCommit.cfg`, `RM = {r1, r2, r3}`.
    #[test]
    fn tcommit_matches_the_examples_manifest() {
        let model = TCommit::new(3);
        let report = check(&model, &model.invariants(), false);
        assert!(report.ok());
        assert_eq!(
            (
                report.stats.distinct,
                report.stats.generated,
                report.stats.depth
            ),
            (34, 94, 7)
        );
        // `TCommit.cfg` says `CHECK_DEADLOCK FALSE`: all-committed and all-aborted have no step.
        assert_eq!(report.deadlocks, 2);
        assert!(
            check(&model, &[], true).deadlocks > 0,
            "with the default deadlock check TLC would stop"
        );
    }

    #[test]
    fn dropping_not_committed_breaks_consistency_in_n_plus_2_steps() {
        for n in 2..=4 {
            let model = TCommit::buggy(n);
            let report = check(&model, &model.invariants(), false);
            let v = report
                .violation("TCConsistent")
                .expect("abort after commit must be possible");
            // n prepares, one commit, one abort: n + 2 steps, n + 3 states.
            assert_eq!(v.trace.len(), n + 3, "n = {n}");
        }
    }

    /// `TwoPhase.cfg` at the pinned commit: `manifest.json` says 288 / 1146 / 11, and the comment
    /// at the end of `TwoPhase.tla` says 50816 reachable states for six RMs.
    #[test]
    fn two_phase_matches_the_examples_numbers() {
        let s = explore(&TwoPhase::new(3));
        assert_eq!(
            (s.stats.distinct, s.stats.generated, s.stats.depth),
            (288, 1146, 11)
        );
        assert_eq!(explore(&TwoPhase::new(6)).stats.distinct, 50816);
    }

    #[test]
    fn two_phase_is_consistent_deadlock_free_and_refines_tcommit() {
        let model = TwoPhase::new(3);
        let report = check(&model, &model.invariants(), true);
        assert!(
            report.ok(),
            "TwoPhase.cfg keeps the default deadlock check and TLC reports success"
        );
        let tc = TCommit::new(3);
        assert!(check_refinement(&model, &tc, TwoPhase::project).is_ok());
    }

    #[test]
    fn a_tm_that_commits_early_breaks_both_consistency_and_refinement() {
        let model = TwoPhase::buggy(2);
        let report = check(&model, &model.invariants(), true);
        let v = report
            .violation("TCConsistent")
            .expect("commit before all prepared");
        // RMChooseToAbort(r1), TMCommit, RMRcvCommitMsg(r2): 3 steps.
        assert_eq!(v.trace.len(), 4);
        let failure = check_refinement(&model, &TCommit::new(2), TwoPhase::project)
            .expect_err("not a TCommit step");
        // The first step that no TCommit step explains is the RM acting on the premature Commit.
        assert!(matches!(
            failure.trace.last(),
            Some((Some(TpAction::RmRcvCommitMsg(_)), _))
        ));
    }

    #[test]
    fn counterexample_traces_are_real_behaviours() {
        let model = TwoPhase::buggy(3);
        let report = check(&model, &model.invariants(), true);
        let v = report.violation("TCConsistent").unwrap();
        assert_eq!(model.init(), vec![v.trace[0].1.clone()]);
        for pair in v.trace.windows(2) {
            let (Some(a), after) = &pair[1] else { panic!() };
            assert_eq!(model.step(&pair[0].1, a).as_ref(), Some(after));
        }
    }
}
