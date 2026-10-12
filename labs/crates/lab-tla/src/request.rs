//! `RequestService`, P13 chapter 05, section 6 (mirrored in `specs/RequestService.tla`): requests
//! go idle -> pending -> done, a worker serves them and can crash and restart. The property is
//! `AllServed == \A r : (req[r] = "pending") ~> (req[r] = "done")`, checked under three levels of
//! fairness: none (`Spec0`), `WF` on `Process` (`Spec1`) and `SF` on `Process` (`Spec2`).

use std::fmt;

use crate::live::{Fairness, Lasso, check_leads_to};
use crate::mc::{Graph, Model, explore};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Req {
    Idle,
    Pending,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct State {
    pub req: Vec<Req>,
    /// `up`: the worker is alive.
    pub up: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Submit(usize),
    Process(usize),
    Crash,
    Restart,
}

impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Submit(r) => write!(f, "Submit(r{})", r + 1),
            Action::Process(r) => write!(f, "Process(r{})", r + 1),
            Action::Crash => write!(f, "Crash"),
            Action::Restart => write!(f, "Restart"),
        }
    }
}

/// The three specs of the chapter: `Init /\ [][Next]_vars /\ Fairness` with different `Fairness`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spec {
    /// `Spec0`: no fairness at all.
    Spec0,
    /// `Spec1`: `WF(Restart)` and `WF(Process(r))`.
    Spec1,
    /// `Spec2`: `WF(Restart)` and `SF(Process(r))`.
    Spec2,
}

pub struct RequestService {
    pub reqs: usize,
}

impl RequestService {
    pub fn fairness(&self, spec: Spec) -> Vec<Fairness<Action>> {
        let mut out = Vec::new();
        if spec != Spec::Spec0 {
            out.push(Fairness::weak("WF_vars(Restart)", |a| {
                *a == Action::Restart
            }));
            for r in 0..self.reqs {
                let is_process = move |a: &Action| *a == Action::Process(r);
                out.push(match spec {
                    Spec::Spec1 => {
                        Fairness::weak(format!("WF_vars(Process(r{}))", r + 1), is_process)
                    }
                    _ => Fairness::strong(format!("SF_vars(Process(r{}))", r + 1), is_process),
                });
            }
        }
        out
    }

    /// `(req[r] = "pending") ~> (req[r] = "done")` for one request.
    pub fn all_served(&self, g: &Graph<Self>, spec: Spec, r: usize) -> Option<Lasso<Self>> {
        check_leads_to(
            g,
            &self.fairness(spec),
            move |s| s.req[r] == Req::Pending,
            move |s| s.req[r] == Req::Done,
        )
    }

    pub fn graph(&self) -> Graph<Self> {
        explore(self)
    }

    pub fn show(s: &State) -> String {
        let req: Vec<String> = s
            .req
            .iter()
            .enumerate()
            .map(|(i, r)| format!("r{} |-> \"{}\"", i + 1, format!("{r:?}").to_lowercase()))
            .collect();
        format!(
            "/\\ req = [{}]\n/\\ up = {}",
            req.join(", "),
            if s.up { "TRUE" } else { "FALSE" }
        )
    }
}

impl Model for RequestService {
    type State = State;
    type Action = Action;

    fn init(&self) -> Vec<State> {
        vec![State {
            req: vec![Req::Idle; self.reqs],
            up: true,
        }]
    }

    /// The order of `Next`: `\E r : Submit(r) \/ Process(r)`, then `Crash`, then `Restart`.
    fn actions(&self) -> Vec<Action> {
        let mut out: Vec<Action> = (0..self.reqs)
            .flat_map(|r| [Action::Submit(r), Action::Process(r)])
            .collect();
        out.extend([Action::Crash, Action::Restart]);
        out
    }

    fn step(&self, s: &State, action: &Action) -> Option<State> {
        let mut next = s.clone();
        match *action {
            Action::Submit(r) if s.req[r] == Req::Idle => next.req[r] = Req::Pending,
            Action::Process(r) if s.up && s.req[r] == Req::Pending => next.req[r] = Req::Done,
            Action::Crash if s.up => next.up = false,
            Action::Restart if !s.up => next.up = true,
            _ => return None,
        }
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_state_graph_is_small_and_has_no_deadlock() {
        let g = RequestService { reqs: 2 }.graph();
        // 3 phases per request, 2 worker states: 3 * 3 * 2 = 18, and every one is reachable.
        assert_eq!(g.stats.distinct, 18);
        assert!(
            (0..g.states.len()).all(|i| !g.edges[i].is_empty()),
            "Crash or Restart is always enabled"
        );
    }

    #[test]
    fn no_fairness_a_pending_request_can_wait_forever() {
        let model = RequestService { reqs: 2 };
        let g = model.graph();
        for r in 0..2 {
            let lasso = model
                .all_served(&g, Spec::Spec0, r)
                .expect("stuttering after Submit");
            assert_eq!(lasso.prefix.len(), 2, "Init, then Submit(r{})", r + 1);
            assert_eq!(lasso.cycle.len(), 1);
            assert!(lasso.cycle[0].0.is_none(), "the cycle is a stuttering step");
        }
    }

    #[test]
    fn weak_fairness_is_not_enough_when_the_worker_can_crash_and_restart() {
        let model = RequestService { reqs: 2 };
        let g = model.graph();
        for r in 0..2 {
            let lasso = model
                .all_served(&g, Spec::Spec1, r)
                .expect("Crash, Restart, Crash, Restart, ...");
            let cycle: Vec<Action> = lasso.cycle.iter().filter_map(|(a, _)| *a).collect();
            assert_eq!(cycle, vec![Action::Crash, Action::Restart]);
            // The request stays pending along the whole cycle.
            assert!(lasso.cycle.iter().all(|(_, s)| s.req[r] == Req::Pending));
        }
    }

    #[test]
    fn strong_fairness_on_process_makes_every_request_get_served() {
        let model = RequestService { reqs: 2 };
        let g = model.graph();
        for r in 0..2 {
            assert!(model.all_served(&g, Spec::Spec2, r).is_none(), "r{}", r + 1);
        }
    }

    #[test]
    fn if_the_worker_cannot_crash_weak_fairness_suffices() {
        // Chapter 05, step 2: without Crash, Process(r) stays enabled until it runs, so WF is enough.
        struct NoCrash(RequestService);
        impl Model for NoCrash {
            type State = State;
            type Action = Action;
            fn init(&self) -> Vec<State> {
                self.0.init()
            }
            fn actions(&self) -> Vec<Action> {
                self.0
                    .actions()
                    .into_iter()
                    .filter(|a| *a != Action::Crash)
                    .collect()
            }
            fn step(&self, s: &State, a: &Action) -> Option<State> {
                self.0.step(s, a)
            }
        }
        let model = NoCrash(RequestService { reqs: 2 });
        let g = explore(&model);
        let fairness: Vec<Fairness<Action>> = (0..2)
            .map(|r| Fairness::weak("WF(Process)", move |a: &Action| *a == Action::Process(r)))
            .collect();
        for r in 0..2 {
            let lasso = check_leads_to(
                &g,
                &fairness,
                |s| s.req[r] == Req::Pending,
                |s| s.req[r] == Req::Done,
            );
            assert!(lasso.is_none());
        }
    }
}
