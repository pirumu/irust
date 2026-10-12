//! The PlusCal counter of P13, chapter 06, section 8, as a [`Model`]: several threads each run
//! `counter := counter + 1` as separate steps. The model is what `pcal.trans` would produce
//! from `specs/CounterLost.tla`, `specs/CounterLock.tla` and `specs/CounterLockSplit.tla`:
//! one action per label per thread, a `pc` per thread, and the extra `Terminating` action that
//! the translator adds so that "everybody is Done" is not reported as a deadlock.

use std::fmt;

use crate::mc::{Invariant, Model};

/// What a label does. Each label is one atomic step, exactly like a PlusCal label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `await lock = 0; lock := self`
    Acquire,
    /// `tmp := counter`
    Load,
    /// `counter := tmp + 1`
    Store,
    /// `lock := 0`
    Release,
}

/// A thread's program: its labels in order. Falling off the end is the implicit label `Done`.
pub type Program = &'static [(&'static str, Op)];

/// `specs/CounterLost.tla`: `Load: tmp := counter; Store: counter := tmp + 1`.
pub const LOST: Program = &[("Load", Op::Load), ("Store", Op::Store)];

/// `specs/CounterLock.tla`: one lock around the whole read-modify-write.
pub const LOCKED: Program = &[
    ("Acquire", Op::Acquire),
    ("Load", Op::Load),
    ("Store", Op::Store),
    ("Release", Op::Release),
];

/// `specs/CounterLockSplit.tla`: the lock is released between the read and the write.
pub const SPLIT: Program = &[
    ("AcquireR", Op::Acquire),
    ("Load", Op::Load),
    ("ReleaseR", Op::Release),
    ("AcquireW", Op::Acquire),
    ("Store", Op::Store),
    ("ReleaseW", Op::Release),
];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub counter: u8,
    /// 0 = free, otherwise the 1-based id of the holder. Stays 0 when the program has no lock.
    pub lock: u8,
    /// `tmp[t - 1]`: thread `t`'s local copy.
    pub tmp: Vec<u8>,
    /// `pc[t - 1]`: index of the label thread `t` is about to run; `program.len()` means `Done`.
    pub pc: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Label `label` of thread `thread` (1-based, like `Threads == 1..NumThreads`).
    Step { label: &'static str, thread: usize },
    /// The translator's "everything is done, nothing happens" action.
    Terminating,
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Step { label, thread } => write!(f, "{label}({thread})"),
            Action::Terminating => write!(f, "Terminating"),
        }
    }
}

pub struct Counter {
    pub name: &'static str,
    pub threads: usize,
    pub program: Program,
    /// Indices of the labels between taking and releasing the lock (empty: no lock).
    pub critical: &'static [usize],
}

impl Counter {
    pub fn lost_update(threads: usize) -> Self {
        Self {
            name: "CounterLost",
            threads,
            program: LOST,
            critical: &[],
        }
    }

    pub fn with_lock(threads: usize) -> Self {
        Self {
            name: "CounterLock",
            threads,
            program: LOCKED,
            critical: &[1, 2, 3],
        }
    }

    pub fn lock_split(threads: usize) -> Self {
        Self {
            name: "CounterLockSplit",
            threads,
            program: SPLIT,
            critical: &[1, 2, 4, 5],
        }
    }

    fn done(&self) -> u8 {
        self.program.len() as u8
    }

    /// `AllDone`: every thread has reached `Done`.
    pub fn all_done(&self, s: &State) -> bool {
        s.pc.iter().all(|&p| p == self.done())
    }

    /// The invariants of the specs: `TypeOK`, `Correct` and, when there is a lock,
    /// `MutualExclusion`.
    pub fn invariants(&self) -> Vec<Invariant<State>> {
        let n = self.threads as u8;
        let done = self.done();
        let critical = self.critical;
        let mut out = vec![
            Invariant::new("TypeOK", move |s: &State| {
                s.counter <= n
                    && s.lock <= n
                    && s.tmp.iter().all(|&v| v <= n)
                    && s.pc.iter().all(|&p| p <= done)
            }),
            Invariant::new("Correct", move |s: &State| {
                !s.pc.iter().all(|&p| p == done) || s.counter == n
            }),
        ];
        if !critical.is_empty() {
            out.push(Invariant::new("MutualExclusion", move |s: &State| {
                s.pc.iter()
                    .filter(|&&p| critical.contains(&(p as usize)))
                    .count()
                    <= 1
            }));
        }
        out
    }

    /// The state as TLC would list the variables of the translated spec.
    pub fn show(&self, s: &State) -> String {
        let tuple = |items: Vec<String>| format!("<<{}>>", items.join(", "));
        let pcs = tuple(
            s.pc.iter()
                .map(|&p| match self.program.get(p as usize) {
                    Some((label, _)) => format!("\"{label}\""),
                    None => "\"Done\"".to_string(),
                })
                .collect(),
        );
        let tmps = tuple(s.tmp.iter().map(u8::to_string).collect());
        let mut out = format!("/\\ counter = {}", s.counter);
        if !self.critical.is_empty() {
            out.push_str(&format!("\n/\\ lock = {}", s.lock));
        }
        out.push_str(&format!("\n/\\ pc = {pcs}\n/\\ tmp = {tmps}"));
        out
    }
}

impl Model for Counter {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        vec![State {
            counter: 0,
            lock: 0,
            tmp: vec![0; self.threads],
            pc: vec![0; self.threads],
        }]
    }

    fn actions(&self) -> Vec<Action> {
        let mut out = Vec::new();
        for thread in 1..=self.threads {
            for &(label, _) in self.program {
                out.push(Action::Step { label, thread });
            }
        }
        out.push(Action::Terminating);
        out
    }

    fn step(&self, s: &State, action: &Action) -> Option<State> {
        match *action {
            Action::Terminating => self.all_done(s).then(|| s.clone()),
            Action::Step { label, thread } => {
                let i = thread - 1;
                let at = s.pc[i] as usize;
                let &(here, op) = self.program.get(at)?;
                if here != label {
                    return None;
                }
                let mut next = s.clone();
                match op {
                    Op::Acquire => {
                        if s.lock != 0 {
                            return None;
                        }
                        next.lock = thread as u8;
                    }
                    Op::Load => next.tmp[i] = s.counter,
                    Op::Store => next.counter = s.tmp[i] + 1,
                    Op::Release => next.lock = 0,
                }
                next.pc[i] += 1;
                Some(next)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::{check, explore};

    /// Replay a counterexample from the initial state: every step must be enabled and land on the
    /// state the trace claims.
    fn replay(model: &Counter, trace: &crate::mc::Trace<Counter>) {
        assert_eq!(trace[0].0, None);
        assert_eq!(model.init(), vec![trace[0].1.clone()]);
        for pair in trace.windows(2) {
            let (Some(action), after) = &pair[1] else {
                panic!("only the first state has no action")
            };
            assert_eq!(
                model.step(&pair[0].1, action).as_ref(),
                Some(after),
                "{action:?} must be enabled"
            );
        }
    }

    #[test]
    fn lost_update_has_a_counterexample_of_exactly_2n_steps() {
        for n in [2, 3] {
            let model = Counter::lost_update(n);
            let report = check(&model, &model.invariants(), true);
            // TypeOK holds: the counter never leaves 0..=N, the lost update is *wrong*, not ill-typed.
            assert_eq!(report.held, vec!["TypeOK".to_string()], "n = {n}");
            let v = report
                .violation("Correct")
                .expect("lost update must be found");
            // `Correct` can only fail once everybody is Done, and each thread needs 2 steps.
            assert_eq!(v.trace.len(), 2 * n + 1, "n = {n}");
            let last = &v.trace.last().unwrap().1;
            assert!(model.all_done(last));
            assert!(last.counter < n as u8);
            replay(&model, &v.trace);
            assert_eq!(
                report.deadlocks, 0,
                "Terminating keeps all-Done from being a deadlock"
            );
        }
    }

    #[test]
    fn two_threads_lost_update_walks_through_the_chapter_trace() {
        let model = Counter::lost_update(2);
        let report = check(&model, &model.invariants(), true);
        let v = report.violation("Correct").unwrap();
        let last = &v.trace.last().unwrap().1;
        // Both threads read 0 and both wrote 1.
        assert_eq!((last.counter, last.tmp.clone()), (1, vec![0, 0]));
        // A shortest counterexample must read twice before the first write.
        let labels: Vec<&str> = v
            .trace
            .iter()
            .filter_map(|(a, _)| match a {
                Some(Action::Step { label, .. }) => Some(*label),
                _ => None,
            })
            .collect();
        assert_eq!(labels.len(), 4);
        assert_eq!(&labels[..2], &["Load", "Load"], "{labels:?}");
        assert_eq!(&labels[2..], &["Store", "Store"], "{labels:?}");
    }

    #[test]
    fn the_lock_fixes_it_and_the_check_is_not_vacuous() {
        for n in [2, 3] {
            let model = Counter::with_lock(n);
            let g = explore(&model);
            let report = crate::mc::check_graph(&g, &model.invariants(), true);
            assert!(report.ok(), "n = {n}");
            assert_eq!(report.held, ["TypeOK", "Correct", "MutualExclusion"]);
            // `Correct` is an implication from `AllDone`: make sure `AllDone` is reachable.
            let finished = g.count(|s| model.all_done(s));
            assert!(finished > 0, "n = {n}");
            // With the lock every schedule is a permutation of whole critical sections, so the
            // counter is n in every finished state.
            assert!(
                g.states
                    .iter()
                    .filter(|s| model.all_done(s))
                    .all(|s| s.counter == n as u8)
            );
        }
    }

    #[test]
    fn a_lock_with_the_wrong_scope_keeps_mutual_exclusion_but_still_loses_updates() {
        let model = Counter::lock_split(2);
        let report = check(&model, &model.invariants(), true);
        assert_eq!(report.held, ["TypeOK", "MutualExclusion"]);
        let v = report
            .violation("Correct")
            .expect("the lock is released between Load and Store");
        // Everybody must be Done before `Correct` can fail: 6 labels per thread.
        assert_eq!(v.trace.len(), 2 * 6 + 1);
        replay(&model, &v.trace);
    }

    #[test]
    fn state_counts_are_stable() {
        // distinct / generated / depth; generated includes the Terminating self-loops.
        // CounterLost with two threads was also counted by hand (13 states by depth: 1, 2, 3, 4, 3;
        // 17 successors + 1 initial = 18), and CounterLock with n threads has 1 + 4 (n + n(n-1) + ...)
        // states, i.e. 17 for n = 2 and 61 for n = 3.
        let expected = [
            (Counter::lost_update(2), (13, 18, 5)),
            (Counter::lost_update(3), (84, 128, 7)),
            (Counter::with_lock(2), (17, 19, 9)),
            (Counter::with_lock(3), (61, 67, 13)),
            (Counter::lock_split(2), (41, 46, 13)),
        ];
        for (model, (distinct, generated, depth)) in expected {
            let s = explore(&model).stats;
            assert_eq!(
                (s.distinct, s.generated, s.depth),
                (distinct, generated, depth),
                "{} {}",
                model.name,
                model.threads
            );
        }
    }
}
