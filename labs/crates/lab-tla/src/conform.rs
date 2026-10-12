//! Differential testing of code against a model (P13 chapter 10): does a concrete system only
//! take steps that the model allows, and can it take every step the model allows?
//!
//! The two directions are different questions:
//!
//! * [`check_trace`] / [`find_divergence`]: *code => model*. Drive the code with arbitrary actions,
//!   including ones the model forbids; every step the code accepts must be a step of the model
//!   (or a stuttering one) and the observable state must match. This is trace validation (Cirstea
//!   et al.) and a proptest "state machine" test with the model as reference.
//! * [`transition_cover`]: *model => code*. Replay a shortest path to every model state, then try
//!   every step the model enables from it. This is model-based test-case generation (MongoDB's
//!   MBTCG), here exhaustive for the finite model.

use std::fmt::Debug;

use proptest::collection::vec;
use proptest::prelude::{ProptestConfig, TestCaseError};
use proptest::sample::select;
use proptest::test_runner::{RngAlgorithm, TestError, TestRng, TestRunner};

use crate::mc::{Model, explore};

/// A system under test, driven by the model's action vocabulary.
pub trait System<A> {
    /// What a log of the system would show: a projection of the model state.
    type Obs: PartialEq + Debug;

    /// Perform `action` if the code accepts it in its current state; `false` when its guard says no.
    fn apply(&mut self, action: &A) -> bool;

    fn observe(&self) -> Self::Obs;
}

/// Where code and model part ways. `A` is the action type and `S` the model's state type.
#[derive(Debug, PartialEq, Eq)]
pub enum Divergence<A: Debug, S: Debug> {
    /// The code performed a step the model does not allow in the state the model is in.
    ForbiddenStep {
        index: usize,
        action: A,
        model_state: S,
    },
    /// The code refused a step that the model enables and that changes the model state.
    RefusedStep { action: A, model_state: S },
    /// Code and model took the same step but their observable states differ.
    ObservationMismatch {
        index: usize,
        action: A,
        model: String,
        code: String,
    },
    /// The code does not even start in the model's initial state.
    InitialMismatch { model: String, code: String },
}

/// [`Divergence`] for a given model.
pub type DivergenceOf<M> = Divergence<<M as Model>::Action, <M as Model>::State>;

fn single_init<M: Model>(model: &M) -> M::State {
    let mut init = model.init();
    assert_eq!(init.len(), 1, "the harness expects a single initial state");
    init.remove(0)
}

/// Run `actions` on `sys` and on the model in lockstep (*code => model*). A refused action leaves
/// both unchanged: a less permissive implementation is still an implementation.
pub fn check_trace<M, S>(
    model: &M,
    sys: &mut S,
    actions: &[M::Action],
    project: &dyn Fn(&M::State) -> S::Obs,
) -> Result<(), DivergenceOf<M>>
where
    M: Model,
    S: System<M::Action>,
{
    let mut state = single_init(model);
    if sys.observe() != project(&state) {
        return Err(Divergence::InitialMismatch {
            model: format!("{:?}", project(&state)),
            code: format!("{:?}", sys.observe()),
        });
    }
    for (index, action) in actions.iter().enumerate() {
        if sys.apply(action) {
            match model.step(&state, action) {
                Some(next) => state = next,
                None => {
                    return Err(Divergence::ForbiddenStep {
                        index,
                        action: action.clone(),
                        model_state: state,
                    });
                }
            }
        }
        if sys.observe() != project(&state) {
            return Err(Divergence::ObservationMismatch {
                index,
                action: action.clone(),
                model: format!("{:?}", project(&state)),
                code: format!("{:?}", sys.observe()),
            });
        }
    }
    Ok(())
}

/// What a successful [`transition_cover`] covered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub states: usize,
    pub transitions: usize,
}

/// *Model => code*: for every reachable model state, replay a shortest path on a fresh system,
/// then try every step the model enables from that state. The code must accept each step (unless
/// the step leaves the model state unchanged) and land on the same observable state.
pub fn transition_cover<M, S>(
    model: &M,
    fresh: impl Fn() -> S,
    project: &dyn Fn(&M::State) -> S::Obs,
) -> Result<Coverage, DivergenceOf<M>>
where
    M: Model,
    S: System<M::Action> + Clone,
{
    let g = explore(model);
    let mut transitions = 0;
    for i in 0..g.states.len() {
        let mut sys = fresh();
        let mut state = single_init(model);
        let path: Vec<M::Action> = g.trace_to(i).into_iter().filter_map(|(a, _)| a).collect();
        for (index, action) in path.iter().enumerate() {
            let next = model
                .step(&state, action)
                .expect("a path of the model graph is enabled");
            if !sys.apply(action) {
                return Err(Divergence::RefusedStep {
                    action: action.clone(),
                    model_state: state,
                });
            }
            state = next;
            if sys.observe() != project(&state) {
                return Err(Divergence::ObservationMismatch {
                    index,
                    action: action.clone(),
                    model: format!("{:?}", project(&state)),
                    code: format!("{:?}", sys.observe()),
                });
            }
        }
        for (action, j) in &g.edges[i] {
            let mut probe = sys.clone();
            let accepted = probe.apply(action);
            if !accepted && *j != i {
                return Err(Divergence::RefusedStep {
                    action: action.clone(),
                    model_state: g.states[i].clone(),
                });
            }
            if probe.observe() != project(&g.states[*j]) {
                return Err(Divergence::ObservationMismatch {
                    index: path.len(),
                    action: action.clone(),
                    model: format!("{:?}", project(&g.states[*j])),
                    code: format!("{:?}", probe.observe()),
                });
            }
            transitions += 1;
        }
    }
    Ok(Coverage {
        states: g.states.len(),
        transitions,
    })
}

/// Property-based *code => model* testing. `run` replays one action sequence on a fresh system
/// and reports a divergence as `Err`. Sequences are drawn from `alphabet` (any action, enabled or
/// not) by proptest with a fixed seed, so the outcome is reproducible; on failure proptest shrinks
/// the sequence and the shortest one it found is returned.
pub fn find_divergence<A: Clone + Debug + 'static>(
    alphabet: Vec<A>,
    cases: u32,
    max_len: usize,
    seed: u8,
    run: impl Fn(&[A]) -> Result<(), String>,
) -> Result<(), Vec<A>> {
    let config = ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let rng = TestRng::from_seed(RngAlgorithm::ChaCha, &[seed; 32]);
    let mut runner = TestRunner::new_with_rng(config, rng);
    let strategy = vec(select(alphabet), 1..=max_len);
    match runner.run(&strategy, |actions| {
        run(&actions).map_err(TestCaseError::fail)
    }) {
        Ok(()) => Ok(()),
        Err(TestError::Fail(_, minimal)) => Err(minimal),
        Err(TestError::Abort(reason)) => panic!("proptest aborted: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toy model: a counter that can only step by 1, up to 3.
    struct Up;

    impl Model for Up {
        type State = u8;
        type Action = u8; // the increment attempted

        fn init(&self) -> Vec<u8> {
            vec![0]
        }

        fn actions(&self) -> Vec<u8> {
            vec![1, 2]
        }

        fn step(&self, s: &u8, by: &u8) -> Option<u8> {
            (*by == 1 && *s < 3).then_some(s + 1)
        }
    }

    /// The code under test: `ceiling` bounds the counter; `allows_two` is a bug.
    #[derive(Clone)]
    struct Code {
        value: u8,
        ceiling: u8,
        allows_two: bool,
    }

    impl System<u8> for Code {
        type Obs = u8;

        fn apply(&mut self, by: &u8) -> bool {
            if (*by == 1 || (*by == 2 && self.allows_two)) && self.value + by <= self.ceiling {
                self.value += by;
                true
            } else {
                false
            }
        }

        fn observe(&self) -> u8 {
            self.value
        }
    }

    fn code(ceiling: u8, allows_two: bool) -> Code {
        Code {
            value: 0,
            ceiling,
            allows_two,
        }
    }

    #[test]
    fn a_faithful_implementation_passes_both_directions() {
        let cover = transition_cover(&Up, || code(3, false), &|s| *s).unwrap();
        assert_eq!(
            cover,
            Coverage {
                states: 4,
                transitions: 3
            }
        );
        let mut c = code(3, false);
        assert_eq!(
            check_trace(&Up, &mut c, &[1, 2, 1, 2, 1, 1], &|s| *s),
            Ok(())
        );
    }

    #[test]
    fn code_that_does_more_than_the_model_is_caught_by_code_to_model() {
        let mut c = code(3, true);
        assert_eq!(
            check_trace(&Up, &mut c, &[1, 2], &|s| *s),
            Err(Divergence::ForbiddenStep {
                index: 1,
                action: 2,
                model_state: 1
            })
        );
        // ...but the transition cover never tries an action the model forbids, so it passes.
        assert!(transition_cover(&Up, || code(3, true), &|s| *s).is_ok());
    }

    #[test]
    fn code_that_does_less_than_the_model_is_caught_by_model_to_code() {
        // The ceiling stops the code at 2 while the model goes to 3: a safe but weaker system.
        let mut c = code(2, false);
        // Seen from the code => model side nothing is wrong: every step the code takes is a model
        // step, and refusing the rest is allowed.
        assert_eq!(check_trace(&Up, &mut c, &[1, 1, 1, 1], &|s| *s), Ok(()));
        assert_eq!(
            transition_cover(&Up, || code(2, false), &|s| *s),
            Err(Divergence::RefusedStep {
                action: 1,
                model_state: 2
            })
        );
    }

    #[test]
    fn a_wrong_observation_is_reported_even_when_the_step_is_allowed() {
        /// Allowed step, wrong result: adds 1 but reports twice the value.
        #[derive(Clone)]
        struct Liar(u8);
        impl System<u8> for Liar {
            type Obs = u8;
            fn apply(&mut self, by: &u8) -> bool {
                self.0 += by;
                *by == 1
            }
            fn observe(&self) -> u8 {
                self.0 + u8::from(self.0 > 1)
            }
        }
        let mut sys = Liar(0);
        let result = check_trace(&Up, &mut sys, &[1, 1], &|s| *s);
        assert_eq!(
            result,
            Err(Divergence::ObservationMismatch {
                index: 1,
                action: 1,
                model: "2".into(),
                code: "3".into()
            })
        );
    }

    #[test]
    fn proptest_finds_and_shrinks_the_forbidden_step() {
        let run = |actions: &[u8]| {
            let mut c = code(3, true);
            check_trace(&Up, &mut c, actions, &|s| *s).map_err(|d| format!("{d:?}"))
        };
        let minimal =
            find_divergence(vec![1, 2], 200, 12, 1, run).expect_err("the bug is reachable");
        // The shortest way to take a step of 2 is to take it first.
        assert_eq!(minimal, vec![2]);
        // And the faithful code survives the same campaign.
        let run = |actions: &[u8]| {
            let mut c = code(3, false);
            check_trace(&Up, &mut c, actions, &|s| *s).map_err(|d| format!("{d:?}"))
        };
        assert_eq!(find_divergence(vec![1, 2], 200, 12, 1, run), Ok(()));
    }
}
