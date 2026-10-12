//! Logical clocks lab: Lamport clocks, vector clocks, the `(t, node)` total order, last-write-wins
//! versus multi-value registers, and a Lamport-timestamp total order broadcast (TOB).
//!
//! Everything is a pure function of a seed. Time is virtual (integer milliseconds); there is no
//! wall clock. The *ground truth* of causality (the happens-before relation) is computed from the
//! program order and the message edges only, never from the clocks, so the clocks can be checked
//! against it.

use crate::rng::Rng;
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------------------------
// Clocks
// ---------------------------------------------------------------------------------------------

/// A Lamport clock: one counter per process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lamport {
    /// Current counter value.
    pub t: u64,
}

impl Lamport {
    /// A local event or a send: `t := t + 1`. Returns the new value (the event's timestamp).
    pub fn tick(&mut self) -> u64 {
        self.t += 1;
        self.t
    }

    /// Receive a message carrying `t'`: `t := max(t, t') + 1`. Returns the new value.
    pub fn recv(&mut self, t_msg: u64) -> u64 {
        self.t = self.t.max(t_msg) + 1;
        self.t
    }
}

/// A Lamport timestamp made unique by the node id. Derived `Ord` compares `t` first and `node`
/// second, which is exactly the total order `(t, node)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stamp {
    /// Lamport counter value.
    pub t: u64,
    /// Node that produced the event (tie-break; any fixed order between nodes works).
    pub node: usize,
}

impl fmt::Display for Stamp {
    /// Prints `(t,X)` where `X` is the node letter (`0 -> A`, `1 -> B`, ...).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({},{})", self.t, node_name(self.node))
    }
}

/// Node `0..26` as a letter `A..Z`.
pub fn node_name(node: usize) -> char {
    (b'A' + (node % 26) as u8) as char
}

/// Result of comparing two vector clocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Causal {
    /// `self < other`: every entry `<=` and at least one `<` (self happened before other).
    Before,
    /// `self > other`.
    After,
    /// All entries equal.
    Equal,
    /// Neither `<=` the other: the events are concurrent.
    Concurrent,
}

/// A vector clock with one entry per node, indexed by node id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorClock(Vec<u64>);

impl VectorClock {
    /// All zeros for `n` nodes.
    pub fn new(n: usize) -> Self {
        Self(vec![0; n])
    }

    /// The entries.
    pub fn as_slice(&self) -> &[u64] {
        &self.0
    }

    /// Local event or send at node `i`: increment entry `i`.
    pub fn tick(&mut self, i: usize) {
        self.0[i] += 1;
    }

    /// Entry-wise maximum with `other` (no increment).
    pub fn merge(&mut self, other: &VectorClock) {
        assert_eq!(
            self.0.len(),
            other.0.len(),
            "vector clocks of different size"
        );
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a = (*a).max(*b);
        }
    }

    /// Receive at node `i` a message carrying `other`: entry-wise max, then increment entry `i`.
    pub fn merge_recv(&mut self, i: usize, other: &VectorClock) {
        self.merge(other);
        self.tick(i);
    }

    /// Compares by the partial order "all entries `<=`".
    pub fn compare(&self, other: &VectorClock) -> Causal {
        assert_eq!(
            self.0.len(),
            other.0.len(),
            "vector clocks of different size"
        );
        let (mut le, mut ge) = (true, true);
        for (a, b) in self.0.iter().zip(&other.0) {
            le &= a <= b;
            ge &= a >= b;
        }
        match (le, ge) {
            (true, true) => Causal::Equal,
            (true, false) => Causal::Before,
            (false, true) => Causal::After,
            (false, false) => Causal::Concurrent,
        }
    }
}

impl fmt::Display for VectorClock {
    /// Prints `<1,0,0>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<")?;
        for (i, x) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{x}")?;
        }
        write!(f, ">")
    }
}

/// How a receive updates the Lamport clock. Only `MaxPlusOne` is correct; the other two exist so
/// the checkers can be shown to catch real bugs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecvRule {
    /// `t := max(t, t') + 1` (Lamport 1978).
    MaxPlusOne,
    /// `t := max(t, t')`: forgets the `+1`, so a receive can get the same value as its send.
    MaxOnly,
    /// `t := t + 1`: ignores the timestamp in the message.
    IgnoreMessage,
}

// ---------------------------------------------------------------------------------------------
// Executions with ground-truth causality
// ---------------------------------------------------------------------------------------------

/// What an event is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// Purely local event.
    Local,
    /// Send message `msg` to node `to`.
    Send {
        /// Message id (index into the execution's message list).
        msg: usize,
        /// Destination node.
        to: usize,
    },
    /// Receive message `msg` (sent by event `send_event`).
    Recv {
        /// Message id.
        msg: usize,
        /// Id of the matching send event.
        send_event: usize,
    },
}

/// One event with its clocks and its ground-truth set of causal ancestors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// Index in the global creation order (a linear extension of happens-before).
    pub id: usize,
    /// Node on which the event happened.
    pub node: usize,
    /// Local, send or receive.
    pub kind: EventKind,
    /// Lamport timestamp assigned by the clock rule.
    pub lamport: u64,
    /// Vector timestamp.
    pub vc: VectorClock,
    /// `ancestors[a]` is true iff event `a` happens before this event (strictly).
    ancestors: Vec<bool>,
}

impl Event {
    /// The `(t, node)` stamp of this event.
    pub fn stamp(&self) -> Stamp {
        Stamp {
            t: self.lamport,
            node: self.node,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Message {
    send_event: usize,
    to: usize,
    t: u64,
    vc: VectorClock,
    delivered: bool,
}

/// A distributed execution being built event by event. Events are appended in an order that is a
/// linear extension of happens-before (a receive is created after its send).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Execution {
    /// Number of nodes.
    pub n: usize,
    /// All events in creation order.
    pub events: Vec<Event>,
    rule: RecvRule,
    clocks: Vec<Lamport>,
    vcs: Vec<VectorClock>,
    last: Vec<Option<usize>>,
    msgs: Vec<Message>,
}

impl Execution {
    /// Empty execution with `n` nodes and the correct Lamport receive rule.
    pub fn new(n: usize) -> Self {
        Self::with_rule(n, RecvRule::MaxPlusOne)
    }

    /// Empty execution with a (possibly buggy) Lamport receive rule. Vector clocks are always correct.
    pub fn with_rule(n: usize, rule: RecvRule) -> Self {
        Self {
            n,
            events: Vec::new(),
            rule,
            clocks: vec![Lamport::default(); n],
            vcs: vec![VectorClock::new(n); n],
            last: vec![None; n],
            msgs: Vec::new(),
        }
    }

    /// Ancestors by program order only; message edges are added by the caller.
    fn base_ancestors(&self, node: usize) -> Vec<bool> {
        let mut anc = vec![false; self.events.len() + 1];
        if let Some(prev) = self.last[node] {
            for (slot, &a) in anc.iter_mut().zip(&self.events[prev].ancestors) {
                *slot = a;
            }
            anc[prev] = true;
        }
        anc
    }

    fn push(&mut self, node: usize, kind: EventKind, ancestors: Vec<bool>) -> usize {
        let id = self.events.len();
        self.events.push(Event {
            id,
            node,
            kind,
            lamport: self.clocks[node].t,
            vc: self.vcs[node].clone(),
            ancestors,
        });
        self.last[node] = Some(id);
        id
    }

    /// A local event on `node`. Returns the event id.
    pub fn local(&mut self, node: usize) -> usize {
        let anc = self.base_ancestors(node);
        self.clocks[node].tick();
        self.vcs[node].tick(node);
        self.push(node, EventKind::Local, anc)
    }

    /// `node` sends a message to `to`. Returns `(event id, message id)`.
    pub fn send(&mut self, node: usize, to: usize) -> (usize, usize) {
        assert_ne!(node, to, "a node does not message itself");
        let anc = self.base_ancestors(node);
        self.clocks[node].tick();
        self.vcs[node].tick(node);
        let msg = self.msgs.len();
        let ev = self.push(node, EventKind::Send { msg, to }, anc);
        self.msgs.push(Message {
            send_event: ev,
            to,
            t: self.clocks[node].t,
            vc: self.vcs[node].clone(),
            delivered: false,
        });
        (ev, msg)
    }

    /// Message ids addressed to `node` that have not been received yet.
    pub fn in_flight_to(&self, node: usize) -> Vec<usize> {
        (0..self.msgs.len())
            .filter(|&m| self.msgs[m].to == node && !self.msgs[m].delivered)
            .collect()
    }

    /// `node` receives message `msg` (must be addressed to it and not yet delivered).
    pub fn recv(&mut self, node: usize, msg: usize) -> usize {
        assert_eq!(
            self.msgs[msg].to, node,
            "message {msg} is not addressed to node {node}"
        );
        assert!(!self.msgs[msg].delivered, "message {msg} delivered twice");
        let mut anc = self.base_ancestors(node);
        let send_event = self.msgs[msg].send_event;
        // Ground truth: everything before the send, and the send itself, happens before this receive.
        for (slot, &a) in anc.iter_mut().zip(&self.events[send_event].ancestors) {
            *slot |= a;
        }
        anc[send_event] = true;
        self.msgs[msg].delivered = true;
        let t_msg = self.msgs[msg].t;
        let c = &mut self.clocks[node];
        match self.rule {
            RecvRule::MaxPlusOne => {
                c.recv(t_msg);
            }
            RecvRule::MaxOnly => c.t = c.t.max(t_msg),
            RecvRule::IgnoreMessage => {
                c.tick();
            }
        }
        let vc_msg = self.msgs[msg].vc.clone();
        self.vcs[node].merge_recv(node, &vc_msg);
        self.push(node, EventKind::Recv { msg, send_event }, anc)
    }

    /// Ground truth: does event `a` happen before event `b`?
    pub fn happens_before(&self, a: usize, b: usize) -> bool {
        self.events[b].ancestors.get(a).copied().unwrap_or(false)
    }

    /// Ground truth: neither `a -> b` nor `b -> a` and `a != b`.
    pub fn concurrent(&self, a: usize, b: usize) -> bool {
        a != b && !self.happens_before(a, b) && !self.happens_before(b, a)
    }
}

/// Labels of [`worked_example`] events, in creation order.
pub const WORKED_LABELS: [&str; 7] = [
    "A: a1",
    "A: send m1",
    "B: b1",
    "B: recv m1",
    "B: send m2",
    "C: c1",
    "C: recv m2",
];

/// Expected `(Lamport t, vector <A,B,C>)` per event of [`worked_example`]: the table of chapter 09 section 3.
pub const WORKED_EXPECTED: [(u64, [u64; 3]); 7] = [
    (1, [1, 0, 0]),
    (2, [2, 0, 0]),
    (1, [0, 1, 0]),
    (3, [2, 2, 0]),
    (4, [2, 3, 0]),
    (1, [0, 0, 1]),
    (5, [2, 3, 2]),
];

/// The chapter 09 section 3 example: A has a1 then sends m1 to B; B has b1, receives m1, sends m2
/// to C; C has c1, then receives m2.
pub fn worked_example() -> Execution {
    let mut e = Execution::new(3);
    e.local(0); // a1
    let (_, m1) = e.send(0, 1);
    e.local(1); // b1
    e.recv(1, m1);
    let (_, m2) = e.send(1, 2);
    e.local(2); // c1
    e.recv(2, m2);
    e
}

/// A random execution of `n` nodes with exactly `steps` events. Each step picks a node, then a
/// local event (40%), a send to a random other node (30%), or a receive of a random in-flight
/// message addressed to it (30%, falling back to a local event if there is none). Choosing a random
/// in-flight message reorders deliveries between different senders and even from one sender.
pub fn generate(seed: u64, n: usize, steps: usize, rule: RecvRule) -> Execution {
    assert!(n >= 2);
    let mut rng = Rng::new(seed);
    let mut e = Execution::with_rule(n, rule);
    for _ in 0..steps {
        let p = rng.below(n as u64) as usize;
        let r = rng.below(10);
        if r < 3 {
            let inflight = e.in_flight_to(p);
            if let Some(&m) = rng.choose(&inflight) {
                e.recv(p, m);
                continue;
            }
            e.local(p);
        } else if r < 7 {
            e.local(p);
        } else {
            let mut to = rng.below(n as u64 - 1) as usize;
            if to >= p {
                to += 1;
            }
            e.send(p, to);
        }
    }
    e
}

/// Counts from checking clocks against ground truth over all unordered event pairs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckReport {
    /// Number of events.
    pub events: u64,
    /// Unordered pairs of distinct events.
    pub pairs: u64,
    /// Pairs related by happens-before.
    pub causal_pairs: u64,
    /// Concurrent pairs.
    pub concurrent_pairs: u64,
    /// Pairs with `a -> b` but `L(a) >= L(b)` (Clock Condition violations).
    pub clock_violations: u64,
    /// Concurrent pairs where the Lamport values nevertheless differ, i.e. `L(a) < L(b)` for some
    /// orientation although `a` does not happen before `b`: the converse of the Clock Condition fails.
    pub converse_counterexamples: u64,
    /// Pairs where vector-clock comparison disagrees with ground truth
    /// (`a -> b` iff `Before`, `b -> a` iff `After`, concurrent iff `Concurrent`).
    pub vc_mismatches: u64,
    /// Pairs where the `(t, node)` order contradicts causality, or two events share a stamp.
    pub order_violations: u64,
}

impl CheckReport {
    /// Adds another report's counts into this one.
    pub fn add(&mut self, o: &CheckReport) {
        self.events += o.events;
        self.pairs += o.pairs;
        self.causal_pairs += o.causal_pairs;
        self.concurrent_pairs += o.concurrent_pairs;
        self.clock_violations += o.clock_violations;
        self.converse_counterexamples += o.converse_counterexamples;
        self.vc_mismatches += o.vc_mismatches;
        self.order_violations += o.order_violations;
    }
}

/// Checks every unordered pair of events of `e` against the ground truth.
pub fn check(e: &Execution) -> CheckReport {
    let mut r = CheckReport {
        events: e.events.len() as u64,
        ..Default::default()
    };
    for b in 0..e.events.len() {
        for a in 0..b {
            let (ea, eb) = (&e.events[a], &e.events[b]);
            r.pairs += 1;
            let ab = e.happens_before(a, b);
            let ba = e.happens_before(b, a);
            debug_assert!(
                !ba,
                "events are created in a linear extension of happens-before"
            );
            // Lamport: Clock Condition and its converse.
            if ab {
                r.causal_pairs += 1;
                if ea.lamport >= eb.lamport {
                    r.clock_violations += 1;
                }
            } else {
                r.concurrent_pairs += 1;
                if ea.lamport != eb.lamport {
                    r.converse_counterexamples += 1;
                }
            }
            // Vector clocks.
            let expected = if ab {
                Causal::Before
            } else {
                Causal::Concurrent
            };
            if ea.vc.compare(&eb.vc) != expected {
                r.vc_mismatches += 1;
            }
            // Total order by (t, node): a strict total order extending causality.
            if ea.stamp() == eb.stamp() || (ab && ea.stamp() > eb.stamp()) {
                r.order_violations += 1;
            }
        }
    }
    r
}

/// Runs `executions` random executions (seeds `base_seed + i`, node count `3 + i % 3`, `steps`
/// events each) and returns the summed [`CheckReport`] per node count.
pub fn sweep(base_seed: u64, executions: u64, steps: usize) -> BTreeMap<usize, CheckReport> {
    let mut out: BTreeMap<usize, CheckReport> = BTreeMap::new();
    for i in 0..executions {
        let n = 3 + (i % 3) as usize;
        let e = generate(base_seed.wrapping_add(i), n, steps, RecvRule::MaxPlusOne);
        out.entry(n).or_default().add(&check(&e));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Registers: LWW versus multi-value
// ---------------------------------------------------------------------------------------------

/// Last-write-wins register: keeps the value with the greatest stamp. `S` is a physical timestamp
/// or a [`Stamp`]; ties keep the existing value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LwwRegister<S, V> {
    current: Option<(S, V)>,
}

impl<S: Ord + Copy, V: Clone> LwwRegister<S, V> {
    /// Empty register.
    pub fn new() -> Self {
        Self { current: None }
    }

    /// Applies a write; returns whether it was accepted (strictly newer than the current stamp).
    pub fn put(&mut self, stamp: S, value: V) -> bool {
        match &self.current {
            Some((s, _)) if *s >= stamp => false,
            _ => {
                self.current = Some((stamp, value));
                true
            }
        }
    }

    /// Current value and stamp.
    pub fn get(&self) -> Option<&(S, V)> {
        self.current.as_ref()
    }
}

impl<S: Ord + Copy, V: Clone> Default for LwwRegister<S, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Multi-value register: keeps every write that is not causally overwritten (siblings).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MvRegister<V> {
    siblings: Vec<(VectorClock, V)>,
}

impl<V: Clone + Ord> MvRegister<V> {
    /// Empty register.
    pub fn new() -> Self {
        Self {
            siblings: Vec::new(),
        }
    }

    /// Applies a write with vector clock `vc`: ignored if an existing sibling is equal or later;
    /// otherwise it removes the siblings it supersedes and is added.
    pub fn put(&mut self, vc: VectorClock, value: V) {
        if self
            .siblings
            .iter()
            .any(|(s, _)| matches!(vc.compare(s), Causal::Before | Causal::Equal))
        {
            return;
        }
        self.siblings
            .retain(|(s, _)| s.compare(&vc) != Causal::Before);
        self.siblings.push((vc, value));
    }

    /// Sibling values, sorted (arrival order must not leak into results).
    pub fn values(&self) -> Vec<V> {
        let mut v: Vec<V> = self.siblings.iter().map(|(_, x)| x.clone()).collect();
        v.sort();
        v
    }

    /// Entry-wise maximum of all sibling clocks: the context a client passes back after reading.
    pub fn context(&self, n: usize) -> VectorClock {
        let mut c = VectorClock::new(n);
        for (s, _) in &self.siblings {
            c.merge(s);
        }
        c
    }
}

impl<V: Clone + Ord> Default for MvRegister<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Outcomes of the register demos (see [`lww_demo`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LwwDemo {
    /// Physical timestamps `(A's write, B's write)` in ms, with A's clock 30 ms fast.
    pub physical_ts: (i64, i64),
    /// Value kept by LWW on physical timestamps (B's write is causally later).
    pub physical_value: i32,
    /// Lamport stamps `(A's write, B's write)`.
    pub lamport_stamps: (Stamp, Stamp),
    /// Value kept by LWW on Lamport stamps.
    pub lamport_value: i32,
    /// Siblings kept by the multi-value register for the causal pair (A then B, B read A's write).
    pub mv_causal_values: Vec<i32>,
    /// Lamport stamps of two truly concurrent writes `(A: x=1, B: x=2)`.
    pub concurrent_stamps: (Stamp, Stamp),
    /// Value LWW keeps for the concurrent pair (identical for both arrival orders).
    pub concurrent_lww_value: i32,
    /// Siblings the multi-value register keeps for the concurrent pair.
    pub concurrent_mv_values: Vec<i32>,
    /// Vector clocks of the concurrent writes `(A, B)`.
    pub concurrent_vcs: (VectorClock, VectorClock),
    /// Siblings after B reads both and writes x=3 with the merged context.
    pub resolved_values: Vec<i32>,
}

/// The chapter 08 section 6 figure plus a concurrent-writes variant.
///
/// Real time: A writes `x=1` at 10 ms (its clock is 30 ms fast, so it stamps 40); B reads `x=1` at
/// 15 ms and writes `x=2` at 20 ms (correct clock, stamps 20). Clients are nodes 0 (A) and 1 (B).
pub fn lww_demo() -> LwwDemo {
    const A: usize = 0;
    const B: usize = 1;
    let skew_a: i64 = 30;
    let (real_a, real_b): (i64, i64) = (10, 20);
    let physical_ts = (real_a + skew_a, real_b);

    // Physical-timestamp LWW: the replica sees A's write first, then B's; B's stamp 20 < 40.
    let mut phys: LwwRegister<i64, i32> = LwwRegister::new();
    phys.put(physical_ts.0, 1);
    phys.put(physical_ts.1, 2);
    let physical_value = phys.get().map(|(_, v)| *v).unwrap_or(-1);

    // Lamport stamps: B observes A's write (the read returns A's stamp) before writing.
    let mut ca = Lamport::default();
    let mut cb = Lamport::default();
    let sa = Stamp {
        t: ca.tick(),
        node: A,
    };
    cb.recv(sa.t); // the read of x=1 carries A's stamp
    let sb = Stamp {
        t: cb.tick(),
        node: B,
    };
    let mut lamport: LwwRegister<Stamp, i32> = LwwRegister::new();
    lamport.put(sa, 1);
    lamport.put(sb, 2);
    let lamport_value = lamport.get().map(|(_, v)| *v).unwrap_or(-1);

    // Same causal pair with a multi-value register: B's write supersedes A's.
    let mut vca = VectorClock::new(2);
    vca.tick(A);
    let mut vcb = vca.clone(); // B read x=1 with context vca
    vcb.tick(B);
    let mut mv_causal = MvRegister::new();
    mv_causal.put(vca, 1);
    mv_causal.put(vcb, 2);

    // Truly concurrent writes: neither client has seen the other's write.
    let mut ca = Lamport::default();
    let mut cb = Lamport::default();
    let conc = (
        Stamp {
            t: ca.tick(),
            node: A,
        },
        Stamp {
            t: cb.tick(),
            node: B,
        },
    );
    let mut lww_ab: LwwRegister<Stamp, i32> = LwwRegister::new();
    lww_ab.put(conc.0, 1);
    lww_ab.put(conc.1, 2);
    let mut lww_ba: LwwRegister<Stamp, i32> = LwwRegister::new();
    lww_ba.put(conc.1, 2);
    lww_ba.put(conc.0, 1);
    assert_eq!(
        lww_ab.get(),
        lww_ba.get(),
        "LWW converges; it just converges to one of the writes"
    );
    let concurrent_lww_value = lww_ab.get().map(|(_, v)| *v).unwrap_or(-1);

    let mut vc1 = VectorClock::new(2);
    vc1.tick(A);
    let mut vc2 = VectorClock::new(2);
    vc2.tick(B);
    let mut mv_ab = MvRegister::new();
    mv_ab.put(vc1.clone(), 1);
    mv_ab.put(vc2.clone(), 2);
    let mut mv_ba = MvRegister::new();
    mv_ba.put(vc2.clone(), 2);
    mv_ba.put(vc1.clone(), 1);
    assert_eq!(mv_ab.values(), mv_ba.values());

    // B reads both siblings, then writes x=3 with the merged context.
    let mut vc3 = mv_ab.context(2);
    vc3.tick(B);
    mv_ab.put(vc3, 3);

    LwwDemo {
        physical_ts,
        physical_value,
        lamport_stamps: (sa, sb),
        lamport_value,
        mv_causal_values: mv_causal.values(),
        concurrent_stamps: conc,
        concurrent_lww_value,
        concurrent_mv_values: mv_ba.values(),
        concurrent_vcs: (vc1, vc2),
        resolved_values: mv_ab.values(),
    }
}

// ---------------------------------------------------------------------------------------------
// Total order broadcast by Lamport timestamps
// ---------------------------------------------------------------------------------------------

/// A non-commutative operation on a string state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Replace the whole state.
    Set(&'static str),
    /// Append to the state.
    Append(&'static str),
}

impl fmt::Display for Op {
    /// `=a` for `Set("a")`, `+3` for `Append("3")`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Set(s) => write!(f, "={s}"),
            Op::Append(s) => write!(f, "+{s}"),
        }
    }
}

impl Op {
    /// Deterministic transition function.
    pub fn apply(&self, state: &mut String) {
        match self {
            Op::Set(s) => {
                state.clear();
                state.push_str(s);
            }
            Op::Append(s) => state.push_str(s),
        }
    }
}

/// How a replica applies the operations it hears about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TobMode {
    /// Apply each operation as soon as it arrives (own operations immediately).
    Arrival,
    /// Hold operations and apply the one with the smallest `(t, node)` stamp only when something
    /// with `t' >= t` has been heard from every other node (Lamport 1978, FIFO channels).
    Lamport,
}

/// Parameters of a TOB run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TobConfig {
    /// Number of replicas (each also issues operations).
    pub nodes: usize,
    /// Operations issued by each replica.
    pub ops_per_node: usize,
    /// Operations are issued at random virtual times in `0..horizon` ms.
    pub horizon: u64,
    /// One-way message delay is uniform in `1..=max_delay` ms (FIFO per channel is enforced).
    pub max_delay: u64,
    /// `Some((node, t))`: the node crashes at virtual time `t` ms (stops issuing, receiving, sending).
    pub crash: Option<(usize, u64)>,
}

impl Default for TobConfig {
    fn default() -> Self {
        Self {
            nodes: 3,
            ops_per_node: 3,
            horizon: 20,
            max_delay: 6,
            crash: None,
        }
    }
}

/// Result of a TOB run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TobRun {
    /// Final state of every replica.
    pub states: Vec<String>,
    /// Per replica, the operations applied, in application order, with their stamps.
    pub logs: Vec<Vec<(Stamp, Op)>>,
    /// Per replica, operations received but not yet delivered when the simulation ran dry.
    pub pending: Vec<usize>,
    /// Operations actually issued (a crashed node issues none after its crash).
    pub issued: usize,
}

#[derive(Clone, Copy, Debug)]
struct NetMsg {
    from: usize,
    to: usize,
    stamp: Stamp,
    /// `None` is an acknowledgement (carries only a timestamp).
    op: Option<Op>,
}

enum Ev {
    Issue { node: usize, op: Op },
    Arrive(NetMsg),
}

struct Replica {
    clock: Lamport,
    /// Largest `t` heard from each node.
    heard: Vec<u64>,
    pending: BTreeMap<Stamp, Op>,
    state: String,
    log: Vec<(Stamp, Op)>,
}

/// Event queue plus FIFO channels. Events are ordered by `(time, insertion sequence)`.
struct Net {
    queue: BTreeMap<(u64, u64), Ev>,
    seq: u64,
    last_arrival: Vec<Vec<u64>>,
    rng: Rng,
    nodes: usize,
    max_delay: u64,
}

impl Net {
    fn schedule(&mut self, time: u64, ev: Ev) {
        self.queue.insert((time, self.seq), ev);
        self.seq += 1;
    }

    /// Sends `op` (or an ack) from `from` to every other node at time `now`. Delay is random, but
    /// a message never overtakes an earlier one on the same channel (FIFO).
    fn broadcast(&mut self, now: u64, from: usize, stamp: Stamp, op: Option<Op>) {
        for to in (0..self.nodes).filter(|&j| j != from) {
            let at = (now + self.rng.range(1, self.max_delay + 1)).max(self.last_arrival[from][to]);
            self.last_arrival[from][to] = at;
            self.schedule(
                at,
                Ev::Arrive(NetMsg {
                    from,
                    to,
                    stamp,
                    op,
                }),
            );
        }
    }
}

const SET_VALUES: [&str; 3] = ["a", "b", "c"];
const APPEND_VALUES: [&str; 6] = ["1", "2", "3", "4", "5", "6"];

/// The first operation of node `node` is `Set` of its own letter; the others append a random digit.
fn random_op(rng: &mut Rng, node: usize, index: usize) -> Op {
    if index == 0 {
        Op::Set(SET_VALUES[node % SET_VALUES.len()])
    } else {
        Op::Append(APPEND_VALUES[rng.below(APPEND_VALUES.len() as u64) as usize])
    }
}

/// Simulates `cfg.nodes` replicas that each issue random operations and broadcast them, in `mode`.
/// In [`TobMode::Lamport`] every replica acknowledges every operation it receives with a message
/// carrying a fresh Lamport timestamp; those acknowledgements are what lets a replica learn that
/// no earlier-stamped operation is still on its way.
pub fn run_tob(seed: u64, cfg: TobConfig, mode: TobMode) -> TobRun {
    let n = cfg.nodes;
    // Separate streams: the schedule of operations must not depend on how many delays were drawn.
    let mut root = Rng::new(seed);
    let mut sched_rng = root.fork(1);
    let net_rng = root.fork(2);

    let mut net = Net {
        queue: BTreeMap::new(),
        seq: 0,
        last_arrival: vec![vec![0u64; n]; n],
        rng: net_rng,
        nodes: n,
        max_delay: cfg.max_delay,
    };
    for node in 0..n {
        for index in 0..cfg.ops_per_node {
            let time = sched_rng.below(cfg.horizon);
            let op = random_op(&mut sched_rng, node, index);
            net.schedule(time, Ev::Issue { node, op });
        }
    }

    let mut reps: Vec<Replica> = (0..n)
        .map(|_| Replica {
            clock: Lamport::default(),
            heard: vec![0; n],
            pending: BTreeMap::new(),
            state: String::new(),
            log: Vec::new(),
        })
        .collect();
    let mut issued = 0usize;
    let crashed =
        |node: usize, time: u64| matches!(cfg.crash, Some((c, at)) if c == node && time >= at);

    while let Some(((now, _), ev)) = net.queue.pop_first() {
        match ev {
            Ev::Issue { node, op } => {
                if crashed(node, now) {
                    continue;
                }
                issued += 1;
                let r = &mut reps[node];
                let stamp = Stamp {
                    t: r.clock.tick(),
                    node,
                };
                match mode {
                    TobMode::Arrival => {
                        op.apply(&mut r.state);
                        r.log.push((stamp, op));
                    }
                    TobMode::Lamport => {
                        r.pending.insert(stamp, op);
                    }
                }
                net.broadcast(now, node, stamp, Some(op));
            }
            Ev::Arrive(m) => {
                if crashed(m.to, now) {
                    continue;
                }
                let r = &mut reps[m.to];
                r.clock.recv(m.stamp.t);
                r.heard[m.from] = r.heard[m.from].max(m.stamp.t);
                if let Some(op) = m.op {
                    match mode {
                        TobMode::Arrival => {
                            op.apply(&mut r.state);
                            r.log.push((m.stamp, op));
                        }
                        TobMode::Lamport => {
                            r.pending.insert(m.stamp, op);
                            let ack = Stamp {
                                t: r.clock.tick(),
                                node: m.to,
                            };
                            net.broadcast(now, m.to, ack, None);
                        }
                    }
                }
            }
        }
        if mode == TobMode::Lamport {
            // Deliver while the smallest pending stamp is "stable": every other node has been heard
            // at `t' >= t`. With FIFO channels and a strictly increasing per-node clock, anything a
            // node sends later has a larger `t`, so nothing smaller can still arrive.
            for (me, r) in reps.iter_mut().enumerate() {
                while let Some((&stamp, _)) = r.pending.first_key_value() {
                    let stable = (0..n).filter(|&j| j != me).all(|j| r.heard[j] >= stamp.t);
                    if !stable {
                        break;
                    }
                    if let Some((stamp, op)) = r.pending.pop_first() {
                        op.apply(&mut r.state);
                        r.log.push((stamp, op));
                    }
                }
            }
        }
    }

    TobRun {
        states: reps.iter().map(|r| r.state.clone()).collect(),
        pending: reps.iter().map(|r| r.pending.len()).collect(),
        logs: reps.into_iter().map(|r| r.log).collect(),
        issued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lamport_rules() {
        let mut c = Lamport::default();
        assert_eq!(c.tick(), 1);
        assert_eq!(c.recv(10), 11);
        assert_eq!(
            c.recv(3),
            12,
            "an old timestamp still advances the clock by one"
        );
        assert!(Stamp { t: 2, node: 0 } < Stamp { t: 2, node: 1 });
        assert!(Stamp { t: 2, node: 5 } < Stamp { t: 3, node: 0 });
    }

    #[test]
    fn vector_compare_cases() {
        let mk = |v: &[u64]| VectorClock(v.to_vec());
        assert_eq!(mk(&[1, 0, 0]).compare(&mk(&[1, 0, 0])), Causal::Equal);
        assert_eq!(mk(&[1, 0, 0]).compare(&mk(&[2, 0, 1])), Causal::Before);
        assert_eq!(mk(&[2, 0, 1]).compare(&mk(&[1, 0, 0])), Causal::After);
        assert_eq!(mk(&[0, 1, 0]).compare(&mk(&[2, 0, 0])), Causal::Concurrent);
        assert_eq!(mk(&[1, 2]).to_string(), "<1,2>");
    }

    /// The table of chapter 09 section 3, value for value.
    #[test]
    fn worked_example_matches_chapter_table() {
        let e = worked_example();
        let lamport: Vec<u64> = e.events.iter().map(|x| x.lamport).collect();
        assert_eq!(lamport, [1, 2, 1, 3, 4, 1, 5]);
        let vcs: Vec<&[u64]> = e.events.iter().map(|x| x.vc.as_slice()).collect();
        assert_eq!(
            vcs,
            [
                &[1, 0, 0][..],
                &[2, 0, 0],
                &[0, 1, 0],
                &[2, 2, 0],
                &[2, 3, 0],
                &[0, 0, 1],
                &[2, 3, 2]
            ]
        );
        for (ev, (t, v)) in e.events.iter().zip(WORKED_EXPECTED) {
            assert_eq!((ev.lamport, ev.vc.as_slice()), (t, &v[..]));
        }
        // Ground truth read off the message pattern: a1 -> a_send -> b_recv -> b_send -> c_recv.
        let (a1, asend, b1, brecv, bsend, c1, crecv) = (0, 1, 2, 3, 4, 5, 6);
        assert!(
            e.happens_before(a1, crecv)
                && e.happens_before(asend, bsend)
                && e.happens_before(b1, crecv)
        );
        assert!(e.happens_before(c1, crecv), "program order on C");
        // b1 has the *smaller* Lamport value (1 < 2) yet is concurrent with "A: send m1".
        assert!(e.concurrent(b1, asend));
        assert!(e.events[b1].lamport < e.events[asend].lamport);
        assert_eq!(
            e.events[b1].vc.compare(&e.events[asend].vc),
            Causal::Concurrent
        );
        assert_eq!(
            e.events[asend].vc.compare(&e.events[bsend].vc),
            Causal::Before
        );
        assert!(e.concurrent(c1, a1) && e.concurrent(c1, brecv));
        assert_eq!(check(&e).clock_violations, 0);
        assert_eq!(check(&e).vc_mismatches, 0);
    }

    /// Ground truth is independent of the clocks: it is exactly program order plus message edges, transitively.
    #[test]
    fn ground_truth_is_transitive_closure_of_program_order_and_messages() {
        for seed in 0..20 {
            let e = generate(seed, 4, 40, RecvRule::MaxPlusOne);
            // Direct edges.
            let mut direct = vec![vec![false; e.events.len()]; e.events.len()];
            let mut last: Vec<Option<usize>> = vec![None; e.n];
            for ev in &e.events {
                if let Some(p) = last[ev.node] {
                    direct[p][ev.id] = true;
                }
                last[ev.node] = Some(ev.id);
                if let EventKind::Recv { send_event, .. } = ev.kind {
                    direct[send_event][ev.id] = true;
                }
            }
            // Floyd-Warshall closure.
            let m = e.events.len();
            let mut reach = direct;
            for k in 0..m {
                for i in 0..m {
                    if reach[i][k] {
                        let row_k = reach[k].clone();
                        for (dst, src) in reach[i].iter_mut().zip(row_k) {
                            *dst |= src;
                        }
                    }
                }
            }
            for (i, row) in reach.iter().enumerate() {
                for (j, &expected) in row.iter().enumerate() {
                    assert_eq!(e.happens_before(i, j), expected, "seed {seed}: {i} -> {j}");
                }
            }
        }
    }

    /// Clock Condition, vector-clock iff, and total-order consistency over many seeds and sizes.
    #[test]
    fn clock_condition_vector_iff_and_total_order_over_many_seeds() {
        let by_n = sweep(1000, 300, 60);
        assert_eq!(by_n.len(), 3);
        let mut total = CheckReport::default();
        for (n, r) in &by_n {
            assert_eq!(r.clock_violations, 0, "n={n}");
            assert_eq!(r.vc_mismatches, 0, "n={n}");
            assert_eq!(r.order_violations, 0, "n={n}");
            assert!(
                r.causal_pairs > 0 && r.concurrent_pairs > 0,
                "n={n}: need both kinds of pair"
            );
            total.add(r);
        }
        assert_eq!(total.events, 300 * 60);
        assert_eq!(total.pairs, 300 * (60 * 59 / 2));
        assert_eq!(total.pairs, total.causal_pairs + total.concurrent_pairs);
    }

    /// The converse of the Clock Condition is false: explicit tiny trace, then statistics.
    #[test]
    fn converse_of_clock_condition_fails() {
        // A: x1, x2 (L=1,2). B: y1 (L=1). x2 and y1 are concurrent with L(y1) < L(x2).
        let mut e = Execution::new(2);
        e.local(0);
        let x2 = e.local(0);
        let y1 = e.local(1);
        assert!(e.concurrent(x2, y1));
        assert!(e.events[y1].lamport < e.events[x2].lamport);
        assert_eq!(
            e.events[y1].vc.compare(&e.events[x2].vc),
            Causal::Concurrent
        );
        // Pairs: (x1,x2) causal; (x1,y1) concurrent with equal L (not a counterexample); (x2,y1) is.
        assert_eq!(check(&e).converse_counterexamples, 1);

        let total: u64 = sweep(5, 100, 50)
            .values()
            .map(|r| r.converse_counterexamples)
            .sum();
        assert!(
            total > 1000,
            "random executions are full of such pairs: {total}"
        );
    }

    /// Counterexamples to buggy clocks: the checker is able to see violations.
    #[test]
    fn buggy_lamport_rules_violate_clock_condition() {
        for rule in [RecvRule::MaxOnly, RecvRule::IgnoreMessage] {
            let violations: u64 = (0..100)
                .map(|s| check(&generate(s, 3, 50, rule)).clock_violations)
                .sum();
            assert!(violations > 0, "{rule:?} should break the Clock Condition");
        }
        // The explicit tiny case for IgnoreMessage: B receives a message stamped 5 and ends at 1.
        let mut e = Execution::with_rule(2, RecvRule::IgnoreMessage);
        for _ in 0..4 {
            e.local(0);
        }
        let (s, m) = e.send(0, 1);
        let r = e.recv(1, m);
        assert!(e.happens_before(s, r));
        assert!(e.events[s].lamport >= e.events[r].lamport);
        // Correct rule on the same trace.
        let mut ok = Execution::new(2);
        for _ in 0..4 {
            ok.local(0);
        }
        let (s, m) = ok.send(0, 1);
        let r = ok.recv(1, m);
        assert!(ok.events[s].lamport < ok.events[r].lamport);
    }

    #[test]
    fn sorting_by_stamp_is_a_linear_extension_of_happens_before() {
        for seed in 0..50 {
            let e = generate(seed, 5, 60, RecvRule::MaxPlusOne);
            let mut ids: Vec<usize> = (0..e.events.len()).collect();
            ids.sort_by_key(|&i| e.events[i].stamp());
            let pos: Vec<usize> = {
                let mut p = vec![0; ids.len()];
                for (k, &i) in ids.iter().enumerate() {
                    p[i] = k;
                }
                p
            };
            for a in 0..ids.len() {
                for b in 0..ids.len() {
                    if e.happens_before(a, b) {
                        assert!(pos[a] < pos[b], "seed {seed}");
                    }
                }
            }
            // Stamps are unique, so the order is total.
            ids.windows(2)
                .for_each(|w| assert!(e.events[w[0]].stamp() < e.events[w[1]].stamp()));
        }
    }

    /// Concurrent events are ordered by the node id tie-break, which is arbitrary but fixed.
    #[test]
    fn total_order_orders_concurrent_events_arbitrarily() {
        let mut e = Execution::new(2);
        let a = e.local(0);
        let b = e.local(1);
        assert!(e.concurrent(a, b));
        assert_eq!(e.events[a].lamport, e.events[b].lamport);
        assert!(
            e.events[a].stamp() < e.events[b].stamp(),
            "tie broken by node id"
        );
    }

    #[test]
    fn lww_physical_loses_the_causally_later_write_lamport_keeps_it() {
        let d = lww_demo();
        assert_eq!(d.physical_ts, (40, 20));
        assert_eq!(
            d.physical_value, 1,
            "x=2 was written after reading x=1 but is dropped"
        );
        assert_eq!(
            d.lamport_stamps,
            (Stamp { t: 1, node: 0 }, Stamp { t: 3, node: 1 })
        );
        assert_eq!(d.lamport_value, 2);
        assert_eq!(
            d.mv_causal_values,
            [2],
            "vector clocks: B's write supersedes A's, no sibling"
        );
    }

    #[test]
    fn concurrent_writes_lww_drops_one_vector_clocks_keep_both() {
        let d = lww_demo();
        assert_eq!(
            d.concurrent_vcs.0.compare(&d.concurrent_vcs.1),
            Causal::Concurrent
        );
        assert_eq!(
            d.concurrent_lww_value, 2,
            "(1,A) < (1,B): A's write is silently lost"
        );
        assert_eq!(d.concurrent_mv_values, [1, 2]);
        assert_eq!(
            d.resolved_values,
            [3],
            "a write carrying the merged context replaces both siblings"
        );
    }

    #[test]
    fn mv_register_edge_cases() {
        let mut r: MvRegister<u8> = MvRegister::new();
        let mut a = VectorClock::new(2);
        a.tick(0);
        r.put(a.clone(), 1);
        r.put(a.clone(), 9); // equal clock: duplicate delivery is ignored
        assert_eq!(r.values(), [1]);
        let mut b = a.clone();
        b.tick(0);
        r.put(b, 2);
        r.put(a, 7); // stale write arriving late is ignored
        assert_eq!(r.values(), [2]);
    }

    /// Without ordering, replicas that apply operations as they arrive end in different states;
    /// with `(t, node)` ordering they always agree.
    #[test]
    fn tob_arrival_order_diverges_lamport_order_agrees() {
        let cfg = TobConfig::default();
        let (mut diverged, mut lamport_agree) = (0, 0);
        for seed in 0..200 {
            let arrival = run_tob(seed, cfg, TobMode::Arrival);
            if arrival.states.iter().any(|s| *s != arrival.states[0]) {
                diverged += 1;
            }
            let l = run_tob(seed, cfg, TobMode::Lamport);
            assert_eq!(l.issued, 9);
            assert!(
                l.pending.iter().all(|&p| p == 0),
                "seed {seed}: nothing stuck without failures"
            );
            assert!(l.logs.iter().all(|log| log.len() == 9), "seed {seed}");
            assert!(
                l.logs.iter().all(|log| *log == l.logs[0]),
                "seed {seed}: identical delivery sequence"
            );
            assert!(l.states.iter().all(|s| *s == l.states[0]));
            // The agreed sequence is the sorted order of all stamps.
            assert!(l.logs[0].windows(2).all(|w| w[0].0 < w[1].0), "seed {seed}");
            lamport_agree += 1;
        }
        assert_eq!(lamport_agree, 200);
        assert!(
            diverged > 100,
            "arrival order diverged in only {diverged}/200 seeds"
        );
    }

    /// A tiny deterministic counterexample: two ops from different nodes that arrive in opposite orders.
    #[test]
    fn arrival_order_counterexample_is_real_and_non_commutative() {
        let mut x = String::new();
        let mut y = String::new();
        let (set, app) = (Op::Set("a"), Op::Append("1"));
        set.apply(&mut x);
        app.apply(&mut x);
        app.apply(&mut y);
        set.apply(&mut y);
        assert_eq!((x.as_str(), y.as_str()), ("a1", "a"));
    }

    /// One crashed node: replicas stay consistent with each other (prefix of the same order) but
    /// operations stamped after the last thing heard from the crashed node never get delivered.
    #[test]
    fn tob_stalls_when_one_node_is_silent() {
        let mut stalled = 0;
        for seed in 0..100 {
            let cfg = TobConfig {
                crash: Some((2, 6)),
                ..TobConfig::default()
            };
            let run = run_tob(seed, cfg, TobMode::Lamport);
            let (a, b) = (&run.logs[0], &run.logs[1]);
            let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
            assert_eq!(
                &long[..short.len()],
                &short[..],
                "seed {seed}: safety holds: one is a prefix of the other"
            );
            assert!(long.windows(2).all(|w| w[0].0 < w[1].0));
            if run.pending[0] > 0 || run.pending[1] > 0 {
                stalled += 1;
            }
            // Nothing is delivered that was not issued; the rest is stuck, not lost.
            for k in 0..2 {
                assert!(run.logs[k].len() <= run.issued);
                assert!(
                    run.logs[k].len() + run.pending[k] <= run.issued,
                    "seed {seed}"
                );
            }
        }
        assert!(
            stalled >= 90,
            "with node C crashed at 6 ms almost every run should stall, got {stalled}/100"
        );
        // Crash after everything is done: no stall.
        let late = run_tob(
            7,
            TobConfig {
                crash: Some((2, 10_000)),
                ..TobConfig::default()
            },
            TobMode::Lamport,
        );
        assert!(late.pending.iter().all(|&p| p == 0));
    }

    /// Arrival mode does not stall (it just diverges): availability bought with consistency.
    #[test]
    fn arrival_mode_keeps_applying_with_a_crashed_node() {
        let cfg = TobConfig {
            crash: Some((2, 6)),
            ..TobConfig::default()
        };
        let run = run_tob(3, cfg, TobMode::Arrival);
        assert!(run.pending.iter().all(|&p| p == 0));
        assert!(
            run.logs[0].len() >= 6,
            "live nodes applied at least their own six operations"
        );
    }

    #[test]
    fn everything_is_deterministic() {
        assert_eq!(
            generate(9, 4, 50, RecvRule::MaxPlusOne),
            generate(9, 4, 50, RecvRule::MaxPlusOne)
        );
        assert_ne!(
            generate(9, 4, 50, RecvRule::MaxPlusOne),
            generate(10, 4, 50, RecvRule::MaxPlusOne)
        );
        assert_eq!(sweep(1, 30, 40), sweep(1, 30, 40));
        assert_eq!(lww_demo(), lww_demo());
        let cfg = TobConfig::default();
        for mode in [TobMode::Arrival, TobMode::Lamport] {
            assert_eq!(run_tob(4, cfg, mode), run_tob(4, cfg, mode));
        }
    }
}
