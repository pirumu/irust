//! A small explicit-state model checker that works the way TLC does (P13, chapter 04):
//! breadth-first search over the reachable states, invariants evaluated on every state,
//! the shortest counterexample rebuilt from parent links, and TLC-style statistics.
//!
//! A [`Model`] is written like a TLA+ spec: initial states plus a finite family of
//! parameterised actions, each with an enabling condition and an effect.

use std::collections::HashMap;
use std::fmt::{Debug, Write as _};
use std::hash::Hash;

/// A transition system in the shape of a TLA+ spec (`Init` and `Next`).
pub trait Model {
    type State: Clone + Eq + Hash + Debug;
    type Action: Clone + Debug;

    /// The states that satisfy `Init`.
    fn init(&self) -> Vec<Self::State>;

    /// Every action instance, i.e. every disjunct of `Next` after expanding `\E`
    /// (`Load(1)`, `Load(2)`, ...), whether or not it is enabled anywhere.
    fn actions(&self) -> Vec<Self::Action>;

    /// The state reached by `action` from `state`, or `None` when `action` is not enabled there.
    /// A step may return a state equal to `state`: TLC counts such a successor as generated.
    fn step(&self, state: &Self::State, action: &Self::Action) -> Option<Self::State>;
}

/// The three numbers TLC prints at the end of a complete run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stats {
    /// States generated: the initial states plus every successor produced, duplicates included.
    pub generated: usize,
    /// Distinct states found.
    pub distinct: usize,
    /// Number of states on the longest of the shortest paths from an initial state (an initial
    /// state alone is depth 1); this is TLC's "depth of the complete state graph search".
    pub depth: usize,
}

/// A behaviour prefix: `(None, initial state)` followed by `(Some(action), state reached)`.
pub type Trace<M> = Vec<(Option<<M as Model>::Action>, <M as Model>::State)>;

/// The whole reachable state graph, built breadth first. Node order is BFS order, so node 0 is
/// an initial state and depths never decrease along the vector.
pub struct Graph<M: Model> {
    pub states: Vec<M::State>,
    /// BFS level of each node; initial states are level 1.
    pub depth: Vec<usize>,
    /// The BFS-tree edge that first reached each node (`None` for initial states).
    pub parent: Vec<Option<(usize, M::Action)>>,
    /// Every enabled step out of each node, self-loops included, in action order.
    pub edges: Vec<Vec<(M::Action, usize)>>,
    pub initial: Vec<usize>,
    pub stats: Stats,
}

impl<M: Model> Graph<M> {
    /// The BFS-tree path from an initial state to node `i`: a shortest behaviour reaching it.
    pub fn trace_to(&self, mut i: usize) -> Trace<M> {
        let mut reversed = Vec::new();
        loop {
            match &self.parent[i] {
                Some((p, action)) => {
                    reversed.push((Some(action.clone()), self.states[i].clone()));
                    i = *p;
                }
                None => {
                    reversed.push((None, self.states[i].clone()));
                    break;
                }
            }
        }
        reversed.reverse();
        reversed
    }

    /// How many reachable states satisfy `pred`. Useful to show that an invariant of the form
    /// `Done => ...` is not vacuous: states where `Done` holds really are reachable.
    pub fn count(&self, pred: impl Fn(&M::State) -> bool) -> usize {
        self.states.iter().filter(|s| pred(s)).count()
    }
}

/// Explore every reachable state of `model`.
pub fn explore<M: Model>(model: &M) -> Graph<M> {
    let alphabet = model.actions();
    let mut index: HashMap<M::State, usize> = HashMap::new();
    let mut g: Graph<M> = Graph {
        states: Vec::new(),
        depth: Vec::new(),
        parent: Vec::new(),
        edges: Vec::new(),
        initial: Vec::new(),
        stats: Stats {
            generated: 0,
            distinct: 0,
            depth: 0,
        },
    };
    let mut generated = 0;

    for s in model.init() {
        generated += 1;
        if !index.contains_key(&s) {
            index.insert(s.clone(), g.states.len());
            g.initial.push(g.states.len());
            g.states.push(s);
            g.depth.push(1);
            g.parent.push(None);
            g.edges.push(Vec::new());
        }
    }

    // `g.states` doubles as TLC's queue: `head` is the next state whose successors are unknown.
    let mut head = 0;
    while head < g.states.len() {
        let current = g.states[head].clone();
        for action in &alphabet {
            let Some(next) = model.step(&current, action) else {
                continue;
            };
            generated += 1;
            let j = match index.get(&next) {
                Some(&j) => j,
                None => {
                    let j = g.states.len();
                    index.insert(next.clone(), j);
                    g.states.push(next);
                    g.depth.push(g.depth[head] + 1);
                    g.parent.push(Some((head, action.clone())));
                    g.edges.push(Vec::new());
                    j
                }
            };
            g.edges[head].push((action.clone(), j));
        }
        head += 1;
    }

    g.stats = Stats {
        generated,
        distinct: g.states.len(),
        depth: g.depth.iter().copied().max().unwrap_or(0),
    };
    g
}

/// A named state predicate, i.e. an `INVARIANT` in a TLC configuration.
pub struct Invariant<S> {
    pub name: String,
    pub holds: Box<dyn Fn(&S) -> bool>,
}

impl<S> Invariant<S> {
    pub fn new(name: impl Into<String>, holds: impl Fn(&S) -> bool + 'static) -> Self {
        Self {
            name: name.into(),
            holds: Box::new(holds),
        }
    }
}

/// An invariant that fails somewhere, with the shortest behaviour that exposes it.
pub struct Violation<M: Model> {
    pub invariant: String,
    /// How many distinct reachable states violate the invariant.
    pub violating_states: usize,
    pub trace: Trace<M>,
}

/// The result of exploring everything and evaluating every invariant on every state.
/// Unlike a default TLC run, exploration does not stop at the first violation, so `stats`
/// describes the whole graph (compare TLC's `-continue`).
pub struct Report<M: Model> {
    pub stats: Stats,
    /// Names of invariants that hold in every reachable state.
    pub held: Vec<String>,
    pub violations: Vec<Violation<M>>,
    /// Reachable states with no successor at all (TLC's "deadlock").
    pub deadlocks: usize,
    /// A shortest behaviour reaching a deadlock state, if any.
    pub deadlock_trace: Option<Trace<M>>,
    /// Whether a deadlock counts as an error (TLC's default; `CHECK_DEADLOCK FALSE` turns it off).
    pub check_deadlock: bool,
}

impl<M: Model> Report<M> {
    /// True when no invariant is violated and (if checked) there is no deadlock.
    pub fn ok(&self) -> bool {
        self.violations.is_empty() && !(self.check_deadlock && self.deadlocks > 0)
    }

    pub fn violation(&self, invariant: &str) -> Option<&Violation<M>> {
        self.violations.iter().find(|v| v.invariant == invariant)
    }
}

/// Explore `model` and evaluate `invariants` on every reachable state.
pub fn check<M: Model>(
    model: &M,
    invariants: &[Invariant<M::State>],
    check_deadlock: bool,
) -> Report<M> {
    check_graph(&explore(model), invariants, check_deadlock)
}

/// Like [`check`], on a graph that was already explored.
pub fn check_graph<M: Model>(
    g: &Graph<M>,
    invariants: &[Invariant<M::State>],
    check_deadlock: bool,
) -> Report<M> {
    let mut held = Vec::new();
    let mut violations = Vec::new();
    for inv in invariants {
        // BFS order: the first violating node is a shortest one.
        let mut first = None;
        let mut count = 0;
        for (i, s) in g.states.iter().enumerate() {
            if !(inv.holds)(s) {
                first.get_or_insert(i);
                count += 1;
            }
        }
        match first {
            None => held.push(inv.name.clone()),
            Some(i) => violations.push(Violation {
                invariant: inv.name.clone(),
                violating_states: count,
                trace: g.trace_to(i),
            }),
        }
    }
    let terminal: Vec<usize> = (0..g.states.len())
        .filter(|&i| g.edges[i].is_empty())
        .collect();
    Report {
        stats: g.stats,
        held,
        violations,
        deadlocks: terminal.len(),
        deadlock_trace: terminal.first().map(|&i| g.trace_to(i)),
        check_deadlock,
    }
}

/// Render a trace in a layout close to TLC's error traces (this is *not* TLC's output).
pub fn render_trace<M: Model>(trace: &Trace<M>, show: &dyn Fn(&M::State) -> String) -> String {
    let mut out = String::new();
    for (k, (action, state)) in trace.iter().enumerate() {
        let label = match action {
            None => "<Initial predicate>".to_string(),
            Some(a) => format!("<{a:?}>"),
        };
        let _ = writeln!(out, "State {}: {label}", k + 1);
        let _ = writeln!(out, "{}", show(state));
    }
    out
}

/// Render a whole report: statistics, which invariants hold, and the shortest counterexample of
/// the first violated invariant (like TLC, which stops at the first one). Further violated
/// invariants get a summary line only; their traces are in `report.violations`.
pub fn render_report<M: Model>(
    title: &str,
    report: &Report<M>,
    show: &dyn Fn(&M::State) -> String,
) -> String {
    let s = report.stats;
    let mut out = String::new();
    let _ = writeln!(out, "== {title} ==");
    let _ = writeln!(
        out,
        "whole state graph: {} distinct states, {} states generated, depth {}",
        s.distinct, s.generated, s.depth
    );
    for name in &report.held {
        let _ = writeln!(out, "invariant {name}: holds in all {} states", s.distinct);
    }
    for (k, v) in report.violations.iter().enumerate() {
        let _ = writeln!(
            out,
            "invariant {}: VIOLATED in {} of {} states; shortest counterexample has {} states ({} steps){}",
            v.invariant,
            v.violating_states,
            s.distinct,
            v.trace.len(),
            v.trace.len() - 1,
            if k == 0 { ":" } else { " (trace not shown)" }
        );
        if k == 0 {
            out.push_str(&render_trace::<M>(&v.trace, show));
        }
    }
    let _ = match (report.check_deadlock, report.deadlocks) {
        (true, 0) => writeln!(out, "deadlock check on: no state without a successor"),
        (true, n) => writeln!(
            out,
            "deadlock check on: {n} states without a successor (TLC would stop with a deadlock error)"
        ),
        (false, n) => writeln!(
            out,
            "deadlock check off (CHECK_DEADLOCK FALSE): {n} states without a successor"
        ),
    };
    out
}

/// Why `low` does not refine `high` under a projection.
pub struct RefinementFailure<L: Model> {
    /// A shortest behaviour of `low` up to the offending step, which comes last.
    pub trace: Trace<L>,
    /// True when the failing state is an initial state (no step is involved).
    pub at_init: bool,
}

/// Step simulation (P13, chapter 07): every initial state of `low` must project to an initial
/// state of `high`, and every step of `low` must project to a step of `high` or to a stuttering
/// step (same projected state). This is what TLC checks for `PROPERTY Low => High!Spec`, safety
/// part only. Returns the statistics of `low` on success.
pub fn check_refinement<L: Model, H: Model>(
    low: &L,
    high: &H,
    project: impl Fn(&L::State) -> H::State,
) -> Result<Stats, RefinementFailure<L>> {
    let g = explore(low);
    let high_init = high.init();
    let high_actions = high.actions();
    for &i in &g.initial {
        if !high_init.contains(&project(&g.states[i])) {
            return Err(RefinementFailure {
                trace: g.trace_to(i),
                at_init: true,
            });
        }
    }
    for i in 0..g.states.len() {
        let from = project(&g.states[i]);
        for (action, j) in &g.edges[i] {
            let to = project(&g.states[*j]);
            let stutter = from == to;
            if stutter
                || high_actions
                    .iter()
                    .any(|h| high.step(&from, h).as_ref() == Some(&to))
            {
                continue;
            }
            let mut trace = g.trace_to(i);
            trace.push((Some(action.clone()), g.states[*j].clone()));
            return Err(RefinementFailure {
                trace,
                at_init: false,
            });
        }
    }
    Ok(g.stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Die Hard 3 puzzle: a 5-gallon and a 3-gallon jug, measure exactly 4 gallons.
    struct DieHard;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Pour {
        FillSmall,
        FillBig,
        EmptySmall,
        EmptyBig,
        SmallToBig,
        BigToSmall,
    }

    impl Model for DieHard {
        type State = (u8, u8); // (small, big)
        type Action = Pour;

        fn init(&self) -> Vec<(u8, u8)> {
            vec![(0, 0)]
        }

        fn actions(&self) -> Vec<Pour> {
            use Pour::*;
            vec![
                FillSmall, FillBig, EmptySmall, EmptyBig, SmallToBig, BigToSmall,
            ]
        }

        fn step(&self, &(small, big): &(u8, u8), action: &Pour) -> Option<(u8, u8)> {
            Some(match action {
                Pour::FillSmall => (3, big),
                Pour::FillBig => (small, 5),
                Pour::EmptySmall => (0, big),
                Pour::EmptyBig => (small, 0),
                Pour::SmallToBig => {
                    let moved = small.min(5 - big);
                    (small - moved, big + moved)
                }
                Pour::BigToSmall => {
                    let moved = big.min(3 - small);
                    (small + moved, big - moved)
                }
            })
        }
    }

    #[test]
    fn bfs_returns_a_shortest_counterexample() {
        // `NotSolved == big # 4` is not a property of the system: its violation *is* the solution.
        let not_solved = Invariant::new("NotSolved", |&(_, big): &(u8, u8)| big != 4);
        let report = check(&DieHard, &[not_solved], true);
        let v = report
            .violation("NotSolved")
            .expect("4 gallons are measurable");
        // The classic solution takes 6 pours, so 7 states; BFS cannot return anything longer.
        assert_eq!(v.trace.len(), 7);
        assert_eq!(v.trace[0], (None, (0, 0)));
        assert_eq!(v.trace.last().map(|(_, s)| s.1), Some(4));
        // Every consecutive pair of the trace is a real step of the model.
        for pair in v.trace.windows(2) {
            let (_, before) = &pair[0];
            let (Some(action), after) = &pair[1] else {
                panic!("only the first state has no action")
            };
            assert_eq!(DieHard.step(before, action).as_ref(), Some(after));
        }
    }

    #[test]
    fn stats_count_generated_and_distinct_separately() {
        let g = explore(&DieHard);
        // Every successor counts as generated, so generated is never smaller than distinct.
        assert!(g.stats.generated >= g.stats.distinct);
        // Six actions are enabled in every state (none has a guard): 1 initial + 6 per state.
        assert_eq!(g.stats.generated, 1 + 6 * g.stats.distinct);
        assert_eq!(g.stats.depth, *g.depth.iter().max().unwrap());
        // The graph is a BFS: depths never decrease along `states`.
        assert!(g.depth.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn a_model_with_a_dead_end_reports_the_deadlock() {
        struct Countdown;
        impl Model for Countdown {
            type State = u8;
            type Action = ();
            fn init(&self) -> Vec<u8> {
                vec![3]
            }
            fn actions(&self) -> Vec<()> {
                vec![()]
            }
            fn step(&self, s: &u8, _: &()) -> Option<u8> {
                s.checked_sub(1)
            }
        }
        let report = check(&Countdown, &[], true);
        assert_eq!(report.deadlocks, 1);
        assert!(!report.ok());
        let trace = report.deadlock_trace.expect("state 0 has no successor");
        assert_eq!(
            trace.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
            vec![3, 2, 1, 0]
        );
        assert!(check(&Countdown, &[], false).ok());
    }

    #[test]
    fn refinement_rejects_a_step_the_abstract_model_does_not_have() {
        // Low: a counter that can add 1 or 2. High: a counter that can only add 1.
        struct Add(u8);
        impl Model for Add {
            type State = u8;
            type Action = u8;
            fn init(&self) -> Vec<u8> {
                vec![0]
            }
            fn actions(&self) -> Vec<u8> {
                (1..=self.0).collect()
            }
            fn step(&self, s: &u8, by: &u8) -> Option<u8> {
                (*s + by <= 4).then_some(*s + by)
            }
        }
        assert!(check_refinement(&Add(1), &Add(1), |s| *s).is_ok());
        let failure = check_refinement(&Add(2), &Add(1), |s| *s)
            .expect_err("adding 2 is not a high-level step");
        assert!(!failure.at_init);
        assert_eq!(
            failure.trace.last().map(|(a, s)| (*a, *s)),
            Some((Some(2), 2))
        );
    }
}
