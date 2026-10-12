//! `Simple.tla` from `tlaplus/Examples` (`specifications/TeachingConcurrency`), the PlusCal
//! example of P13 chapter 06, section 7: `N` processes in a ring, each doing
//! `a: x[self] := 1; b: y[self] := x[(self-1) % N]`.
//!
//! It is here as a *calibration* of how a PlusCal translation is counted: the repository's
//! `manifest.json` records what TLC found for `Simple.cfg` (`N = 5`), and the translation has the
//! `Terminating` action whose self-loops count as generated states.

use std::fmt;

use crate::mc::{Invariant, Model};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pc {
    A,
    B,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub x: Vec<u8>,
    pub y: Vec<u8>,
    pub pc: Vec<Pc>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    A(usize),
    B(usize),
    Terminating,
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::A(i) => write!(f, "a({i})"),
            Action::B(i) => write!(f, "b({i})"),
            Action::Terminating => write!(f, "Terminating"),
        }
    }
}

pub struct Simple {
    pub n: usize,
}

impl Simple {
    /// `PCorrect`, `TypeOK`'s non-trivial half, and the last conjuncts of `Inv` from `Simple.cfg`.
    pub fn invariants(&self) -> Vec<Invariant<State>> {
        vec![
            Invariant::new("PCorrect", |s: &State| {
                !s.pc.iter().all(|&p| p == Pc::Done) || s.y.contains(&1)
            }),
            Invariant::new("TypeOK", |s: &State| {
                s.x.iter().chain(&s.y).all(|&v| v <= 1)
            }),
            Invariant::new("Inv", |s: &State| {
                let started = s.pc.iter().zip(&s.x).all(|(&p, &x)| p == Pc::A || x == 1);
                let someone_pending = s.pc.iter().any(|&p| p != Pc::Done);
                started && (someone_pending || s.y.contains(&1))
            }),
        ]
    }
}

impl Model for Simple {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        vec![State {
            x: vec![0; self.n],
            y: vec![0; self.n],
            pc: vec![Pc::A; self.n],
        }]
    }

    fn actions(&self) -> Vec<Action> {
        let mut out: Vec<Action> = (0..self.n)
            .flat_map(|i| [Action::A(i), Action::B(i)])
            .collect();
        out.push(Action::Terminating);
        out
    }

    fn step(&self, s: &State, action: &Action) -> Option<State> {
        let mut next = s.clone();
        match *action {
            Action::A(i) if s.pc[i] == Pc::A => {
                next.x[i] = 1;
                next.pc[i] = Pc::B;
            }
            Action::B(i) if s.pc[i] == Pc::B => {
                // `(self-1) % N` in TLA+ is the mathematical modulus: 0 - 1 wraps to N - 1.
                next.y[i] = s.x[(i + self.n - 1) % self.n];
                next.pc[i] = Pc::Done;
            }
            Action::Terminating if s.pc.iter().all(|&p| p == Pc::Done) => {}
            _ => return None,
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::check;

    /// `manifest.json` of `specifications/TeachingConcurrency` at the pinned commit lists
    /// `Simple.cfg` (`N = 5`) with 723 distinct states, 1842 total, depth 11.
    #[test]
    fn simple_matches_the_examples_manifest() {
        let model = Simple { n: 5 };
        let report = check(&model, &model.invariants(), true);
        assert!(
            report.ok(),
            "Simple.cfg checks PCorrect, TypeOK and Inv and TLC reports success"
        );
        assert_eq!(
            (
                report.stats.distinct,
                report.stats.generated,
                report.stats.depth
            ),
            (723, 1842, 11)
        );
    }
}
