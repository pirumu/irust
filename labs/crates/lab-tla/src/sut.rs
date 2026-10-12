//! A production-style two-phase commit to test against [`TwoPhase`]: participants and a
//! coordinator written as ordinary Rust state machines with guards, the way a service would, plus
//! a "wire" that remembers every packet the network has carried. The model's action enum is only
//! the vocabulary used to drive it (the "log lines"); nothing else is shared.
//!
//! [`Mutant`] switches in one realistic bug at a time:
//!
//! * `CountVotes`: the coordinator counts `Prepared` packets instead of remembering *which*
//!   participants sent one, the shortcut Cirstea et al. report finding by trace validation.
//! * `AbortAfterVote`: a participant may still abort on its own after it voted `Prepared`.
//! * `StrictAbort`: the coordinator refuses to abort once it has seen a vote. Safe, but less
//!   permissive than the specification.

use std::cell::Cell;
use std::collections::BTreeSet;

use crate::commit::{Msg, Rm, Tm, TpAction, TpState, TwoPhase};
use crate::conform::{
    Coverage, DivergenceOf, System, check_trace, find_divergence, transition_cover,
};
use crate::mc::Model;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutant {
    None,
    CountVotes,
    AbortAfterVote,
    StrictAbort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Working,
    Prepared,
    Committed,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decision {
    Undecided,
    Commit,
    Abort,
}

/// How the coordinator remembers votes.
#[derive(Clone, Debug)]
enum Votes {
    Who(BTreeSet<usize>),
    HowMany(usize),
}

impl Votes {
    fn record(&mut self, rm: usize) {
        match self {
            Votes::Who(set) => {
                set.insert(rm);
            }
            Votes::HowMany(n) => *n += 1,
        }
    }

    fn complete(&self, rms: usize) -> bool {
        match self {
            Votes::Who(set) => set.len() == rms,
            Votes::HowMany(n) => *n >= rms,
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Votes::Who(set) => set.is_empty(),
            Votes::HowMany(n) => *n == 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Packet {
    Prepared(usize),
    Commit,
    Abort,
}

/// The whole deployment: `rms.len()` participants, one coordinator and the network.
#[derive(Clone, Debug)]
pub struct Cluster {
    mutant: Mutant,
    rms: Vec<Phase>,
    decision: Decision,
    votes: Votes,
    /// Every packet the network has carried; delivering one again is a retransmission.
    wire: BTreeSet<Packet>,
}

impl Cluster {
    pub fn new(rms: usize, mutant: Mutant) -> Self {
        let votes = if mutant == Mutant::CountVotes {
            Votes::HowMany(0)
        } else {
            Votes::Who(BTreeSet::new())
        };
        Self {
            mutant,
            rms: vec![Phase::Working; rms],
            decision: Decision::Undecided,
            votes,
            wire: BTreeSet::new(),
        }
    }

    // ---- participant side

    fn rm_prepare(&mut self, i: usize) -> bool {
        if self.rms[i] != Phase::Working {
            return false;
        }
        self.rms[i] = Phase::Prepared;
        self.wire.insert(Packet::Prepared(i));
        true
    }

    fn rm_abort_on_its_own(&mut self, i: usize) -> bool {
        let allowed = match self.mutant {
            Mutant::AbortAfterVote => matches!(self.rms[i], Phase::Working | Phase::Prepared),
            _ => self.rms[i] == Phase::Working,
        };
        if !allowed {
            return false;
        }
        self.rms[i] = Phase::Aborted;
        true
    }

    fn rm_on_commit(&mut self, i: usize) -> bool {
        if !self.wire.contains(&Packet::Commit) || self.rms[i] != Phase::Prepared {
            return false;
        }
        self.rms[i] = Phase::Committed;
        true
    }

    fn rm_on_abort(&mut self, i: usize) -> bool {
        if !self.wire.contains(&Packet::Abort)
            || !matches!(self.rms[i], Phase::Working | Phase::Prepared)
        {
            return false;
        }
        self.rms[i] = Phase::Aborted;
        true
    }

    // ---- coordinator side

    fn tm_on_prepared(&mut self, i: usize) -> bool {
        if self.decision != Decision::Undecided || !self.wire.contains(&Packet::Prepared(i)) {
            return false;
        }
        self.votes.record(i);
        true
    }

    fn tm_commit(&mut self) -> bool {
        if self.decision != Decision::Undecided || !self.votes.complete(self.rms.len()) {
            return false;
        }
        self.decision = Decision::Commit;
        self.wire.insert(Packet::Commit);
        true
    }

    fn tm_abort(&mut self) -> bool {
        if self.decision != Decision::Undecided
            || (self.mutant == Mutant::StrictAbort && !self.votes.is_empty())
        {
            return false;
        }
        self.decision = Decision::Abort;
        self.wire.insert(Packet::Abort);
        true
    }
}

/// What a log of the deployment shows, in the model's vocabulary: the participants' states, the
/// coordinator's decision and the set of packets sent. The coordinator's vote book is *not*
/// logged, which is exactly why a wrong vote book is only visible through the steps it enables.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Observed {
    pub rm: Vec<Rm>,
    pub tm: Tm,
    pub msgs: BTreeSet<Msg>,
}

/// The projection of a model state onto what the code logs.
pub fn observed_model(s: &TpState) -> Observed {
    Observed {
        rm: s.rm.clone(),
        tm: s.tm,
        msgs: s.msgs.clone(),
    }
}

impl System<TpAction> for Cluster {
    type Obs = Observed;

    fn apply(&mut self, action: &TpAction) -> bool {
        match *action {
            TpAction::RmPrepare(i) => self.rm_prepare(i),
            TpAction::RmChooseToAbort(i) => self.rm_abort_on_its_own(i),
            TpAction::RmRcvCommitMsg(i) => self.rm_on_commit(i),
            TpAction::RmRcvAbortMsg(i) => self.rm_on_abort(i),
            TpAction::TmRcvPrepared(i) => self.tm_on_prepared(i),
            TpAction::TmCommit => self.tm_commit(),
            TpAction::TmAbort => self.tm_abort(),
        }
    }

    fn observe(&self) -> Observed {
        Observed {
            rm: self
                .rms
                .iter()
                .map(|p| match p {
                    Phase::Working => Rm::Working,
                    Phase::Prepared => Rm::Prepared,
                    Phase::Committed => Rm::Committed,
                    Phase::Aborted => Rm::Aborted,
                })
                .collect(),
            tm: match self.decision {
                Decision::Undecided => Tm::Init,
                Decision::Commit => Tm::Committed,
                Decision::Abort => Tm::Aborted,
            },
            msgs: self
                .wire
                .iter()
                .map(|p| match *p {
                    Packet::Prepared(i) => Msg::Prepared(i),
                    Packet::Commit => Msg::Commit,
                    Packet::Abort => Msg::Abort,
                })
                .collect(),
        }
    }
}

/// *Model => code*: [`transition_cover`] of the `TwoPhase` graph on a fresh cluster.
pub fn cover(rms: usize, mutant: Mutant) -> Result<Coverage, DivergenceOf<TwoPhase>> {
    transition_cover(
        &TwoPhase::new(rms),
        || Cluster::new(rms, mutant),
        &observed_model,
    )
}

/// Replay one action sequence (*code => model*); `None` when the code conforms.
pub fn replay(rms: usize, mutant: Mutant, actions: &[TpAction]) -> Option<DivergenceOf<TwoPhase>> {
    let mut cluster = Cluster::new(rms, mutant);
    check_trace(&TwoPhase::new(rms), &mut cluster, actions, &observed_model).err()
}

/// What a passing random campaign saw: evidence that it was not vacuous.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WalkStats {
    pub walks: u32,
    pub attempted: usize,
    pub accepted: usize,
    /// Walks that ended with the coordinator having committed / aborted.
    pub committed: usize,
    pub aborted: usize,
}

/// A counting wrapper: how many steps the code accepted out of those attempted.
struct Counting<'a> {
    inner: Cluster,
    attempted: &'a Cell<usize>,
    accepted: &'a Cell<usize>,
}

impl System<TpAction> for Counting<'_> {
    type Obs = Observed;

    fn apply(&mut self, action: &TpAction) -> bool {
        self.attempted.set(self.attempted.get() + 1);
        let took = self.inner.apply(action);
        if took {
            self.accepted.set(self.accepted.get() + 1);
        }
        took
    }

    fn observe(&self) -> Observed {
        self.inner.observe()
    }
}

/// *Code => model* by proptest: `cases` random action sequences of length `1..=max_len` over the
/// whole action alphabet, a fixed `seed`. `Err` carries the shrunk sequence that diverges.
pub fn campaign(
    rms: usize,
    mutant: Mutant,
    cases: u32,
    max_len: usize,
    seed: u8,
) -> Result<WalkStats, Vec<TpAction>> {
    let model = TwoPhase::new(rms);
    let walks = Cell::new(0u32);
    let attempted = Cell::new(0usize);
    let accepted = Cell::new(0usize);
    let committed = Cell::new(0usize);
    let aborted = Cell::new(0usize);
    find_divergence(model.actions(), cases, max_len, seed, |actions| {
        walks.set(walks.get() + 1);
        let mut sys = Counting {
            inner: Cluster::new(rms, mutant),
            attempted: &attempted,
            accepted: &accepted,
        };
        check_trace(&model, &mut sys, actions, &observed_model).map_err(|d| format!("{d:?}"))?;
        match sys.inner.decision {
            Decision::Commit => committed.set(committed.get() + 1),
            Decision::Abort => aborted.set(aborted.get() + 1),
            Decision::Undecided => {}
        }
        Ok(())
    })?;
    Ok(WalkStats {
        walks: walks.get(),
        attempted: attempted.get(),
        accepted: accepted.get(),
        committed: committed.get(),
        aborted: aborted.get(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::TpAction::*;
    use crate::conform::Divergence;

    const CASES: u32 = 10_000;
    const MAX_LEN: usize = 40;

    #[test]
    fn the_intended_code_conforms_in_both_directions() {
        for rms in [2, 3] {
            let covered =
                cover(rms, Mutant::None).expect("every model step can be replayed on the code");
            // The cover walks the whole TwoPhase graph: 288 states for 3 RMs (see `commit`).
            assert_eq!(covered.states, if rms == 2 { 56 } else { 288 });
        }
        let stats = campaign(2, Mutant::None, CASES, MAX_LEN, 7).expect("no forbidden step");
        assert_eq!(stats.walks, CASES);
        // The campaign is not vacuous: many steps are accepted and both outcomes are reached.
        assert!(
            stats.accepted > 0 && stats.accepted < stats.attempted,
            "{stats:?}"
        );
        assert!(stats.committed > 0 && stats.aborted > 0, "{stats:?}");
    }

    #[test]
    fn counting_votes_instead_of_remembering_voters_commits_too_early() {
        let minimal = campaign(2, Mutant::CountVotes, CASES, MAX_LEN, 7)
            .expect_err("a duplicate Prepared is reachable");
        // Shrunk to a minimal sequence: some RM prepares, its packet is delivered twice, the TM commits.
        let TmCommit = *minimal.last().unwrap() else {
            panic!("must end with the premature commit: {minimal:?}")
        };
        let deliveries: Vec<_> = minimal
            .iter()
            .filter(|a| matches!(a, TmRcvPrepared(_)))
            .collect();
        assert_eq!(deliveries.len(), 2, "{minimal:?}");
        assert_eq!(
            deliveries[0], deliveries[1],
            "the same Prepared is counted twice: {minimal:?}"
        );
        assert!(minimal.len() <= 5, "{minimal:?}");
        // The reported divergence is the commit that the model forbids.
        assert!(matches!(
            replay(2, Mutant::CountVotes, &minimal),
            Some(Divergence::ForbiddenStep {
                action: TmCommit,
                ..
            })
        ));
        // The same bug is invisible to the cover: it never retransmits a packet.
        assert!(cover(2, Mutant::CountVotes).is_ok());
        assert!(cover(3, Mutant::CountVotes).is_ok());
    }

    #[test]
    fn aborting_after_voting_is_a_forbidden_step_found_in_two_actions() {
        let minimal = campaign(2, Mutant::AbortAfterVote, CASES, MAX_LEN, 7)
            .expect_err("RMChooseToAbort after RMPrepare");
        assert_eq!(minimal.len(), 2, "{minimal:?}");
        let (RmPrepare(a), RmChooseToAbort(b)) = (minimal[0], minimal[1]) else {
            panic!("{minimal:?}")
        };
        assert_eq!(a, b, "the same RM votes and then aborts on its own");
        assert!(
            cover(2, Mutant::AbortAfterVote).is_ok(),
            "the cover never tries a step the model forbids"
        );
    }

    #[test]
    fn a_stricter_coordinator_conforms_one_way_only() {
        // Code => model: refusing is always allowed, so random walks see nothing wrong...
        assert!(campaign(2, Mutant::StrictAbort, CASES, MAX_LEN, 7).is_ok());
        // ...but model => code: the spec lets the TM abort after a vote, the code refuses.
        match cover(2, Mutant::StrictAbort) {
            Err(Divergence::RefusedStep {
                action: TmAbort,
                model_state,
            }) => {
                assert_eq!(model_state.tm, Tm::Init);
                assert!(!model_state.tm_prepared.is_empty(), "{model_state:?}");
            }
            other => panic!("expected a refused TMAbort, got {other:?}"),
        }
    }
}
