//! Liveness under fairness: `P ~> Q` checked on the explored state graph the way TLC does it
//! (P13, chapter 05, section 8): look for a strongly connected part of the graph in which the
//! behaviour can stay in "not Q" forever *and* every `WF`/`SF` condition is respected.
//!
//! Stuttering is an implicit self-loop on every state, as in `[][Next]_vars`.

use crate::mc::{Graph, Model, Trace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    /// `WF_vars(A)`: if `A` is enabled continuously, a step of `A` happens.
    Weak,
    /// `SF_vars(A)`: if `A` is enabled again and again, a step of `A` happens.
    Strong,
}

/// A fairness condition on the steps whose action satisfies `is_step`. Following TLA+, a step
/// counts only if it changes the state (`<<A>>_vars`), and `A` is enabled in a state when such a
/// step leaves it.
pub struct Fairness<A> {
    pub name: String,
    pub strength: Strength,
    pub is_step: Box<dyn Fn(&A) -> bool>,
}

impl<A> Fairness<A> {
    pub fn weak(name: impl Into<String>, is_step: impl Fn(&A) -> bool + 'static) -> Self {
        Self {
            name: name.into(),
            strength: Strength::Weak,
            is_step: Box::new(is_step),
        }
    }

    pub fn strong(name: impl Into<String>, is_step: impl Fn(&A) -> bool + 'static) -> Self {
        Self {
            name: name.into(),
            strength: Strength::Strong,
            is_step: Box::new(is_step),
        }
    }
}

/// A counterexample to `P ~> Q`: a finite prefix and then a cycle repeated forever.
pub struct Lasso<M: Model> {
    /// From an initial state to the first state of the cycle (inclusive).
    pub prefix: Trace<M>,
    /// The steps that take the last prefix state around the cycle and back to it;
    /// `None` is a stuttering step.
    pub cycle: Vec<(Option<M::Action>, M::State)>,
}

impl<M: Model> Lasso<M> {
    /// Render in a layout close to TLC's liveness counterexamples (this is *not* TLC's output).
    pub fn render(&self, show: &dyn Fn(&M::State) -> String) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let mut k = 0;
        for (action, state) in &self.prefix {
            k += 1;
            let label = match action {
                None => "<Initial predicate>".to_string(),
                Some(a) => format!("<{a:?}>"),
            };
            let _ = writeln!(out, "State {k}: {label}\n{}", show(state));
        }
        let back_to = self.prefix.len();
        let last = self.cycle.len() - 1;
        for (i, (action, state)) in self.cycle.iter().enumerate() {
            match action {
                None => {
                    let _ = writeln!(out, "Stuttering: state {back_to} repeats forever");
                }
                Some(a) if i == last => {
                    let _ = writeln!(out, "Back to state {back_to}: <{a:?}>");
                }
                Some(a) => {
                    k += 1;
                    let _ = writeln!(out, "State {k}: <{a:?}>\n{}", show(state));
                }
            }
        }
        out
    }
}

struct Ctx<'a, M: Model> {
    g: &'a Graph<M>,
    fairness: &'a [Fairness<M::Action>],
    /// `enabled[k][u]`: a state-changing step of fairness condition `k` leaves node `u`.
    enabled: Vec<Vec<bool>>,
}

impl<M: Model> Ctx<'_, M> {
    /// A step of condition `k` that starts and ends inside `inside`, if there is one.
    fn step_inside(&self, k: usize, inside: &[bool]) -> Option<(usize, M::Action, usize)> {
        let is_step = &self.fairness[k].is_step;
        for (u, edges) in self.g.edges.iter().enumerate() {
            if !inside[u] {
                continue;
            }
            for (a, v) in edges {
                if *v != u && inside[*v] && is_step(a) {
                    return Some((u, a.clone(), *v));
                }
            }
        }
        None
    }
}

/// Strongly connected components of the subgraph induced by `mask` (self-loops ignored: every
/// node has the stuttering loop anyway), restricted to the components reachable from `nodes`.
fn tarjan<M: Model>(g: &Graph<M>, mask: &[bool], nodes: &[usize]) -> Vec<Vec<usize>> {
    let n = g.states.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0usize; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut next_index = 0;
    let mut out = Vec::new();
    for &root in nodes {
        if index[root] != usize::MAX {
            continue;
        }
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        // An explicit DFS stack of (node, next edge to look at) avoids recursion depth limits.
        let mut frames: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&(u, pos)) = frames.last() {
            if pos < g.edges[u].len() {
                if let Some(frame) = frames.last_mut() {
                    frame.1 += 1;
                }
                let v = g.edges[u][pos].1;
                if v == u || !mask[v] {
                    continue;
                }
                if index[v] == usize::MAX {
                    index[v] = next_index;
                    low[v] = next_index;
                    next_index += 1;
                    stack.push(v);
                    on_stack[v] = true;
                    frames.push((v, 0));
                } else if on_stack[v] {
                    low[u] = low[u].min(index[v]);
                }
            } else {
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    low[parent] = low[parent].min(low[u]);
                }
                if low[u] == index[u] {
                    let mut scc = Vec::new();
                    while let Some(w) = stack.pop() {
                        on_stack[w] = false;
                        scc.push(w);
                        if w == u {
                            break;
                        }
                    }
                    out.push(scc);
                }
            }
        }
    }
    out
}

/// Find every set of nodes, strongly connected inside `nodes`, in which some infinite behaviour
/// can visit every node and edge forever while satisfying every fairness condition.
///
/// A whole SCC `C` is the most permissive cycle for the "infinitely often" half of each
/// condition, so test `C` itself first. A weak condition that fails on `C` (enabled everywhere,
/// never taken) fails on every part of `C`. A strong condition that fails on `C` (enabled
/// somewhere, never taken inside `C`) can only be rescued by leaving out the states where it is
/// enabled, so remove them and search again.
fn find_fair<M: Model>(ctx: &Ctx<'_, M>, nodes: Vec<usize>, found: &mut Vec<Vec<usize>>) {
    let n = ctx.g.states.len();
    let mut mask = vec![false; n];
    for &u in &nodes {
        mask[u] = true;
    }
    for scc in tarjan(ctx.g, &mask, &nodes) {
        let mut inside = vec![false; n];
        for &u in &scc {
            inside[u] = true;
        }
        let mut remove = vec![false; n];
        let mut any_removed = false;
        let mut hopeless = false;
        for (k, f) in ctx.fairness.iter().enumerate() {
            if ctx.step_inside(k, &inside).is_some() {
                continue;
            }
            match f.strength {
                Strength::Weak => {
                    if scc.iter().all(|&u| ctx.enabled[k][u]) {
                        hopeless = true;
                        break;
                    }
                }
                Strength::Strong => {
                    for &u in &scc {
                        if ctx.enabled[k][u] {
                            remove[u] = true;
                            any_removed = true;
                        }
                    }
                }
            }
        }
        if hopeless {
            continue;
        }
        if !any_removed {
            found.push(scc);
            continue;
        }
        let rest: Vec<usize> = scc.into_iter().filter(|&u| !remove[u]).collect();
        if !rest.is_empty() {
            find_fair(ctx, rest, found);
        }
    }
}

/// Shortest path from `from` to `to` using state-changing steps inside `inside`.
fn path_in<M: Model>(
    g: &Graph<M>,
    inside: &[bool],
    from: usize,
    to: usize,
) -> Vec<(M::Action, usize)> {
    if from == to {
        return Vec::new();
    }
    let mut came: Vec<Option<(usize, M::Action)>> = vec![None; g.states.len()];
    let mut seen = vec![false; g.states.len()];
    seen[from] = true;
    let mut queue = std::collections::VecDeque::from([from]);
    while let Some(u) = queue.pop_front() {
        for (a, v) in &g.edges[u] {
            if *v == u || !inside[*v] || seen[*v] {
                continue;
            }
            seen[*v] = true;
            came[*v] = Some((u, a.clone()));
            if *v == to {
                let mut hops = Vec::new();
                let mut cur = to;
                while let Some((p, a)) = came[cur].take() {
                    hops.push((a, cur));
                    cur = p;
                }
                hops.reverse();
                return hops;
            }
            queue.push_back(*v);
        }
    }
    unreachable!("the nodes of a strongly connected component reach each other")
}

/// A closed walk being built inside one strongly connected component.
struct Walk<'a, M: Model> {
    g: &'a Graph<M>,
    inside: &'a [bool],
    cycle: Vec<(Option<M::Action>, M::State)>,
    /// The actions on the walk so far, and the nodes it has passed through.
    taken: Vec<M::Action>,
    visited: Vec<usize>,
    at: usize,
}

impl<M: Model> Walk<'_, M> {
    /// Take one step `action` to node `to`.
    fn take(&mut self, action: M::Action, to: usize) {
        self.cycle
            .push((Some(action.clone()), self.g.states[to].clone()));
        self.taken.push(action);
        self.visited.push(to);
        self.at = to;
    }

    /// Walk along a shortest path inside the component to `target`.
    fn go_to(&mut self, target: usize) {
        for (action, v) in path_in(self.g, self.inside, self.at, target) {
            self.take(action, v);
        }
    }
}

/// Check `P ~> Q` (that is, `[](P => <>Q)`) on `g` under `fairness`. Returns `None` when the
/// property holds and a lasso when some fair behaviour violates it: a behaviour that reaches a
/// state with `P` and `~Q`, then never reaches `Q`.
pub fn check_leads_to<M: Model>(
    g: &Graph<M>,
    fairness: &[Fairness<M::Action>],
    p: impl Fn(&M::State) -> bool,
    q: impl Fn(&M::State) -> bool,
) -> Option<Lasso<M>> {
    let n = g.states.len();
    let not_q: Vec<bool> = g.states.iter().map(|s| !q(s)).collect();

    // Breadth first inside "not Q", from every state with P and not Q.
    let mut dist = vec![usize::MAX; n];
    let mut from: Vec<Option<usize>> = vec![None; n];
    let mut order: Vec<usize> = Vec::new();
    for u in 0..n {
        if not_q[u] && p(&g.states[u]) {
            dist[u] = 0;
            order.push(u);
        }
    }
    let mut head = 0;
    while head < order.len() {
        let u = order[head];
        head += 1;
        for (_, v) in &g.edges[u] {
            if *v != u && not_q[*v] && dist[*v] == usize::MAX {
                dist[*v] = dist[u] + 1;
                from[*v] = Some(u);
                order.push(*v);
            }
        }
    }

    let enabled = fairness
        .iter()
        .map(|f| {
            (0..n)
                .map(|u| g.edges[u].iter().any(|(a, v)| *v != u && (f.is_step)(a)))
                .collect()
        })
        .collect();
    let ctx = Ctx {
        g,
        fairness,
        enabled,
    };
    let mut components = Vec::new();
    find_fair(&ctx, order, &mut components);

    // Lasso, part 1: enter a fair component where the whole lasso is shortest: a shortest
    // behaviour to a state with P and not Q, then along `from` links (all inside "not Q") to the
    // entry of the cycle.
    let source_of = |mut u: usize| {
        while let Some(prev) = from[u] {
            u = prev;
        }
        u
    };
    let (entry, scc) = components
        .iter()
        .flat_map(|scc| scc.iter().map(move |&u| (u, scc)))
        .min_by_key(|&(u, _)| g.depth[source_of(u)] + dist[u])?;
    let mut chain = vec![entry];
    while let Some(prev) = from[*chain.last()?] {
        chain.push(prev);
    }
    chain.reverse();
    let mut prefix = g.trace_to(chain[0]);
    for pair in chain.windows(2) {
        let action = g.edges[pair[0]]
            .iter()
            .find(|(_, v)| *v == pair[1])
            .map(|(a, _)| a.clone())?;
        prefix.push((Some(action), g.states[pair[1]].clone()));
    }

    // Lasso, part 2: a closed walk inside the SCC with one witness per fairness condition: a
    // step of it, or (for a weak condition) a state on the walk where it is disabled. A strong
    // condition with no step inside is disabled on the whole SCC and needs nothing. Witnesses the
    // walk already contains are not added again, which keeps the cycle short.
    let mut inside = vec![false; n];
    for &u in scc {
        inside[u] = true;
    }
    let mut walk = Walk::<M> {
        g,
        inside: &inside,
        cycle: Vec::new(),
        taken: Vec::new(),
        visited: vec![entry],
        at: entry,
    };
    for (k, f) in fairness.iter().enumerate() {
        if walk.taken.iter().any(|a| (f.is_step)(a)) {
            continue;
        }
        if let Some((u, a, v)) = ctx.step_inside(k, &inside) {
            walk.go_to(u);
            walk.take(a, v);
        }
    }
    for (k, f) in fairness.iter().enumerate() {
        let has_step = ctx.step_inside(k, &inside).is_some();
        if f.strength == Strength::Weak
            && !has_step
            && !walk.visited.iter().any(|&u| !ctx.enabled[k][u])
        {
            let w = scc.iter().copied().find(|&u| !ctx.enabled[k][u])?;
            walk.go_to(w);
        }
    }
    walk.go_to(entry);
    let mut cycle = walk.cycle;
    if cycle.is_empty() {
        cycle.push((None, g.states[entry].clone()));
    }
    Some(Lasso { prefix, cycle })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mc::explore;

    /// The connection pool of P13 chapter 05 (`ConnPool1`): one connection, two clients.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    enum Phase {
        Idle,
        Waiting,
        Using,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PoolAction {
        Want(usize),
        Acquire(usize),
        Release(usize),
    }

    struct Pool;

    type PoolState = (Option<usize>, [Phase; 2]); // (holder, phase of each client)

    impl Model for Pool {
        type State = PoolState;
        type Action = PoolAction;

        fn init(&self) -> Vec<PoolState> {
            vec![(None, [Phase::Idle; 2])]
        }

        fn actions(&self) -> Vec<PoolAction> {
            (0..2)
                .flat_map(|c| {
                    [
                        PoolAction::Want(c),
                        PoolAction::Acquire(c),
                        PoolAction::Release(c),
                    ]
                })
                .collect()
        }

        fn step(&self, &(holder, phase): &PoolState, a: &PoolAction) -> Option<PoolState> {
            let mut next = (holder, phase);
            match *a {
                PoolAction::Want(c) if phase[c] == Phase::Idle => next.1[c] = Phase::Waiting,
                PoolAction::Acquire(c) if phase[c] == Phase::Waiting && holder.is_none() => {
                    next = (Some(c), phase);
                    next.1[c] = Phase::Using;
                }
                PoolAction::Release(c) if phase[c] == Phase::Using && holder == Some(c) => {
                    next = (None, phase);
                    next.1[c] = Phase::Idle;
                }
                _ => return None,
            }
            Some(next)
        }
    }

    fn pool_fairness(acquire: Strength) -> Vec<Fairness<PoolAction>> {
        let mut out = Vec::new();
        for c in 0..2 {
            let is_acquire = move |a: &PoolAction| *a == PoolAction::Acquire(c);
            out.push(match acquire {
                Strength::Weak => Fairness::weak(format!("WF(Acquire({c}))"), is_acquire),
                Strength::Strong => Fairness::strong(format!("SF(Acquire({c}))"), is_acquire),
            });
            out.push(Fairness::weak(format!("WF(Release({c}))"), move |a| {
                *a == PoolAction::Release(c)
            }));
        }
        out
    }

    fn starvation(
        g: &Graph<Pool>,
        fairness: &[Fairness<PoolAction>],
        c: usize,
    ) -> Option<Lasso<Pool>> {
        check_leads_to(
            g,
            fairness,
            |s| s.1[c] == Phase::Waiting,
            |s| s.1[c] == Phase::Using,
        )
    }

    #[test]
    fn weak_fairness_does_not_prevent_starvation_but_strong_does() {
        let g = explore(&Pool);
        let weak = pool_fairness(Strength::Weak);
        let strong = pool_fairness(Strength::Strong);

        // WF: client 1 can wait forever while client 0 takes the connection again and again.
        let lasso = starvation(&g, &weak, 1).expect("starvation is possible under weak fairness");
        let cycle: Vec<PoolAction> = lasso.cycle.iter().filter_map(|(a, _)| *a).collect();
        assert_eq!(
            cycle,
            vec![
                PoolAction::Want(0),
                PoolAction::Acquire(0),
                PoolAction::Release(0)
            ]
        );
        // The prefix ends in the first state of the cycle; client 1 is waiting there.
        assert_eq!(
            lasso.prefix.last().map(|(_, s)| s.1[1]),
            Some(Phase::Waiting)
        );
        // The cycle closes: it ends in the state it started from.
        assert_eq!(
            lasso.cycle.last().map(|(_, s)| *s),
            lasso.prefix.last().map(|(_, s)| *s)
        );

        // SF on Acquire: the same behaviours are unfair now, and nobody starves.
        assert!(starvation(&g, &strong, 0).is_none());
        assert!(starvation(&g, &strong, 1).is_none());
    }

    #[test]
    fn without_any_fairness_a_waiting_client_can_stutter_forever() {
        let g = explore(&Pool);
        let lasso = starvation(&g, &[], 0).expect("nothing forces the system to move");
        assert_eq!(lasso.cycle.len(), 1);
        assert!(lasso.cycle[0].0.is_none(), "the cycle is a stuttering step");
    }

    #[test]
    fn a_property_with_no_violating_state_holds_vacuously() {
        let g = explore(&Pool);
        // `Using` is reachable, so `Idle ~> true`-style properties cannot fail on Q alone.
        assert!(check_leads_to(&g, &[], |s| s.1[0] == Phase::Idle, |_| true).is_none());
        // P never holds: nothing to check.
        assert!(check_leads_to(&g, &[], |_| false, |s| s.1[0] == Phase::Using).is_none());
    }
}
