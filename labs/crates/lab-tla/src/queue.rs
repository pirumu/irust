//! The at-least-once queue with an idempotent consumer, P13 chapter 09, section 3, as a
//! [`Model`]: what `pcal.trans` would produce from `specs/AtLeastOnce.tla` (the dedup record and
//! the side effect in one atomic step) and `specs/AtLeastOnceSplit.tla` (two steps).
//!
//! A broker redelivers any in-flight message at any time (a visibility timeout, collapsed into one
//! action); a consumer receives a message, applies its side effect, then acknowledges or "crashes"
//! (the acknowledgement is lost and the message will be redelivered).

use std::collections::BTreeSet;
use std::fmt;

use crate::mc::{Invariant, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Ready,
    Inflight,
    Acked,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Ready => "ready",
            Status::Inflight => "inflight",
            Status::Acked => "acked",
        }
    }
}

/// The label a consumer is at. `Handle` exists in the atomic variant, `Effect`/`Record` in the
/// split one; `Receive` is the loop head.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Label {
    Receive,
    Handle,
    Effect,
    Record,
    Ack,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Consumer {
    pub pc: Label,
    /// `cur`: the last message received (`NULL` before the first one; never reset).
    pub cur: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub status: Vec<Status>,
    /// Side effects applied per message.
    pub effects: Vec<u8>,
    /// The consumers' dedup table (they share one database), as a bit set over message ids.
    pub seen: BTreeSet<usize>,
    /// Budget used by lost acknowledgements.
    pub crashes: u8,
    pub consumers: Vec<Consumer>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Redeliver(usize),
    Receive { consumer: usize, msg: usize },
    Handle(usize),
    Effect(usize),
    Record(usize),
    AckOk(usize),
    AckCrash(usize),
}

fn m(i: usize) -> String {
    format!("m{}", i + 1)
}

fn c(i: usize) -> String {
    format!("c{}", i + 1)
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Redeliver(i) => write!(f, "Redeliver(broker, {})", m(*i)),
            Action::Receive { consumer, msg } => {
                write!(f, "Receive({}, {})", c(*consumer), m(*msg))
            }
            Action::Handle(i) => write!(f, "Handle({})", c(*i)),
            Action::Effect(i) => write!(f, "Effect({})", c(*i)),
            Action::Record(i) => write!(f, "Record({})", c(*i)),
            Action::AckOk(i) => write!(f, "Ack({}) [acked]", c(*i)),
            Action::AckCrash(i) => write!(f, "Ack({}) [crash]", c(*i)),
        }
    }
}

pub struct Queue {
    pub name: &'static str,
    pub msgs: usize,
    pub consumers: usize,
    pub max_crashes: u8,
    /// `Dedup`: skip the side effect for a message already in `seen`.
    pub dedup: bool,
    /// `true`: `Handle` is split into `Effect` then `Record` (two atomic steps).
    pub split: bool,
}

impl Queue {
    /// `specs/AtLeastOnce.tla` with `Dedup = TRUE`: effect and dedup record in one step.
    pub fn idempotent(msgs: usize, consumers: usize, max_crashes: u8) -> Self {
        Self {
            name: "AtLeastOnce (Dedup = TRUE)",
            msgs,
            consumers,
            max_crashes,
            dedup: true,
            split: false,
        }
    }

    /// `specs/AtLeastOnce.tla` with `Dedup = FALSE`: no dedup at all.
    pub fn naive(msgs: usize, consumers: usize, max_crashes: u8) -> Self {
        Self {
            name: "AtLeastOnce (Dedup = FALSE)",
            msgs,
            consumers,
            max_crashes,
            dedup: false,
            split: false,
        }
    }

    /// `specs/AtLeastOnceSplit.tla`: dedup is on, but recorded in a separate step.
    pub fn split(msgs: usize, consumers: usize, max_crashes: u8) -> Self {
        Self {
            name: "AtLeastOnceSplit (Dedup = TRUE)",
            msgs,
            consumers,
            max_crashes,
            dedup: true,
            split: true,
        }
    }

    pub fn invariants(&self) -> Vec<Invariant<State>> {
        let max = self.max_crashes;
        vec![
            Invariant::new("CrashBudget", move |s: &State| s.crashes <= max),
            // The property the design needs: no message has its side effect applied twice.
            Invariant::new("AtMostOneEffect", |s: &State| {
                s.effects.iter().all(|&e| e <= 1)
            }),
            // The inductive invariant of the chapter: `effects[m] = 1` exactly when `m` is in `seen`.
            Invariant::new("EffectsIffSeen", |s: &State| {
                s.effects
                    .iter()
                    .enumerate()
                    .all(|(i, &e)| e <= 1 && (e == 1) == s.seen.contains(&i))
            }),
        ]
    }

    pub fn show(&self, s: &State) -> String {
        let status: Vec<String> = s
            .status
            .iter()
            .enumerate()
            .map(|(i, st)| format!("{} |-> \"{}\"", m(i), st.name()))
            .collect();
        let effects: Vec<String> = s
            .effects
            .iter()
            .enumerate()
            .map(|(i, e)| format!("{} |-> {e}", m(i)))
            .collect();
        let seen: Vec<String> = s.seen.iter().map(|&i| m(i)).collect();
        let pc: Vec<String> = s
            .consumers
            .iter()
            .enumerate()
            .map(|(i, x)| format!("{} |-> \"{:?}\"", c(i), x.pc))
            .collect();
        let cur: Vec<String> = s
            .consumers
            .iter()
            .enumerate()
            .map(|(i, x)| format!("{} |-> {}", c(i), x.cur.map_or("NULL".to_string(), m)))
            .collect();
        format!(
            "/\\ status = [{}]\n/\\ effects = [{}]\n/\\ seen = {{{}}}\n/\\ crashes = {}\n/\\ pc = [{}]\n/\\ cur = [{}]",
            status.join(", "),
            effects.join(", "),
            seen.join(", "),
            s.crashes,
            pc.join(", "),
            cur.join(", "),
        )
    }
}

impl Model for Queue {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        vec![State {
            status: vec![Status::Ready; self.msgs],
            effects: vec![0; self.msgs],
            seen: BTreeSet::new(),
            crashes: 0,
            consumers: vec![
                Consumer {
                    pc: Label::Receive,
                    cur: None
                };
                self.consumers
            ],
        }]
    }

    fn actions(&self) -> Vec<Action> {
        let mut out: Vec<Action> = (0..self.msgs).map(Action::Redeliver).collect();
        for consumer in 0..self.consumers {
            out.extend((0..self.msgs).map(|msg| Action::Receive { consumer, msg }));
            if self.split {
                out.extend([Action::Effect(consumer), Action::Record(consumer)]);
            } else {
                out.push(Action::Handle(consumer));
            }
            out.extend([Action::AckOk(consumer), Action::AckCrash(consumer)]);
        }
        out
    }

    fn step(&self, s: &State, action: &Action) -> Option<State> {
        let mut next = s.clone();
        match *action {
            Action::Redeliver(i) => {
                if s.status[i] != Status::Inflight {
                    return None;
                }
                next.status[i] = Status::Ready;
            }
            Action::Receive { consumer, msg } => {
                if s.consumers[consumer].pc != Label::Receive || s.status[msg] != Status::Ready {
                    return None;
                }
                next.status[msg] = Status::Inflight;
                next.consumers[consumer] = Consumer {
                    pc: if self.split {
                        Label::Effect
                    } else {
                        Label::Handle
                    },
                    cur: Some(msg),
                };
            }
            Action::Handle(i) => {
                let cur = self.at(s, i, Label::Handle)?;
                // if ~(Dedup /\ cur \in seen) then effects[cur] += 1; seen := seen \union {cur}
                if !(self.dedup && s.seen.contains(&cur)) {
                    next.effects[cur] += 1;
                    next.seen.insert(cur);
                }
                next.consumers[i].pc = Label::Ack;
            }
            Action::Effect(i) => {
                let cur = self.at(s, i, Label::Effect)?;
                if !(self.dedup && s.seen.contains(&cur)) {
                    next.effects[cur] += 1;
                }
                next.consumers[i].pc = Label::Record;
            }
            Action::Record(i) => {
                let cur = self.at(s, i, Label::Record)?;
                next.seen.insert(cur);
                next.consumers[i].pc = Label::Ack;
            }
            Action::AckOk(i) => {
                let cur = self.at(s, i, Label::Ack)?;
                next.status[cur] = Status::Acked;
                next.consumers[i].pc = Label::Receive;
            }
            Action::AckCrash(i) => {
                self.at(s, i, Label::Ack)?;
                if s.crashes >= self.max_crashes {
                    return None;
                }
                next.crashes += 1;
                next.consumers[i].pc = Label::Receive;
            }
        }
        Some(next)
    }
}

impl Queue {
    /// The message consumer `i` is working on, if it is at `label`.
    fn at(&self, s: &State, i: usize, label: Label) -> Option<usize> {
        let consumer = &s.consumers[i];
        if consumer.pc == label {
            consumer.cur
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::{check, explore};

    fn labels(trace: &crate::mc::Trace<Queue>) -> Vec<String> {
        trace
            .iter()
            .filter_map(|(a, _)| a.map(|a| format!("{a:?}")))
            .collect()
    }

    #[test]
    fn without_dedup_one_message_one_consumer_one_crash_is_enough() {
        let model = Queue::naive(1, 1, 1);
        let report = check(&model, &model.invariants(), false);
        let v = report
            .violation("AtMostOneEffect")
            .expect("at-least-once delivers twice");
        // Receive, Handle, Ack [crash], Redeliver, Receive, Handle: 6 steps (the chapter's trace)
        // or Receive, Redeliver, Handle, Ack [crash], Receive, Handle (the same length).
        assert_eq!(v.trace.len(), 7, "{:?}", labels(&v.trace));
        assert!(
            labels(&v.trace)
                .iter()
                .filter(|l| l.starts_with("Handle"))
                .count()
                == 2
        );
        assert_eq!(v.trace.last().unwrap().1.effects, vec![2]);
    }

    #[test]
    fn a_slow_consumer_is_enough_even_without_any_crash() {
        let model = Queue::naive(1, 2, 0);
        let report = check(&model, &model.invariants(), false);
        let v = report
            .violation("AtMostOneEffect")
            .expect("the visibility timeout alone redelivers");
        // Receive(c1), Handle(c1), Redeliver, Receive(c2), Handle(c2): 5 steps, no crash used.
        assert_eq!(v.trace.len(), 6, "{:?}", labels(&v.trace));
        assert_eq!(v.trace.last().unwrap().1.crashes, 0);
    }

    #[test]
    fn dedup_in_the_same_atomic_step_makes_the_effect_idempotent() {
        for (msgs, consumers, crashes) in [(1, 1, 1), (1, 2, 2), (2, 2, 1)] {
            let model = Queue::idempotent(msgs, consumers, crashes);
            let report = check(&model, &model.invariants(), false);
            assert!(
                report.ok(),
                "{msgs} msgs, {consumers} consumers, {crashes} crashes: {:?}",
                report.violations.len()
            );
            assert_eq!(
                report.held,
                ["CrashBudget", "AtMostOneEffect", "EffectsIffSeen"]
            );
            // The check must not be vacuous: some state really has the effect applied.
            assert!(explore(&model).count(|s| s.effects.iter().all(|&e| e == 1)) > 0);
        }
    }

    #[test]
    fn splitting_the_dedup_record_from_the_effect_brings_the_duplicates_back() {
        let model = Queue::split(1, 2, 0);
        let report = check(&model, &model.invariants(), false);
        let v = report
            .violation("AtMostOneEffect")
            .expect("dedup is on, yet the effect runs twice");
        // Receive(c1), Effect(c1), Redeliver, Receive(c2), Effect(c2): 5 steps.
        assert_eq!(v.trace.len(), 6, "{:?}", labels(&v.trace));
        assert_eq!(v.trace.last().unwrap().1.crashes, 0, "no crash is needed");
        // `EffectsIffSeen` is broken earlier: right after the first Effect, before Record.
        let inductive = report.violation("EffectsIffSeen").unwrap();
        assert_eq!(inductive.trace.len(), 3);
    }

    #[test]
    fn with_a_single_consumer_the_split_is_harmless_in_this_model() {
        // One consumer cannot overlap with itself, and a crash is always after Record.
        let model = Queue::split(1, 1, 2);
        assert!(check(&model, &model.invariants()[..2], false).ok());
    }

    #[test]
    fn the_broker_alone_can_stall_everything_so_deadlock_must_stay_off() {
        let model = Queue::idempotent(1, 1, 1);
        let report = check(&model, &model.invariants(), true);
        // Once the message is acked nobody can do anything: TLC would call that a deadlock,
        // which is why the cfg of the spec says CHECK_DEADLOCK FALSE.
        assert!(report.deadlocks > 0);
        assert!(!report.ok());
    }

    #[test]
    fn state_counts_are_stable() {
        // One message, one consumer, one lost ack was also counted by hand: 13 states, 17
        // successors + 1 initial = 18 generated, depth 8, and two states without a successor.
        let expected = [
            (Queue::naive(1, 1, 1), (13, 18, 8)),
            (Queue::naive(1, 2, 0), (24, 43, 8)),
            (Queue::idempotent(1, 2, 2), (96, 202, 15)),
            (Queue::split(1, 2, 0), (51, 97, 10)),
            (Queue::idempotent(2, 2, 1), (621, 1617, 14)),
        ];
        for (model, (distinct, generated, depth)) in expected {
            let s = explore(&model).stats;
            assert_eq!(
                (s.distinct, s.generated, s.depth),
                (distinct, generated, depth),
                "{}",
                model.name
            );
        }
        assert_eq!(check(&Queue::naive(1, 1, 1), &[], false).deadlocks, 2);
    }
}
