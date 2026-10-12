//! Deterministic discrete-event simulation of Raft leader election and log replication.
//!
//! This is the *learning* implementation of the Raft paper's Figure 2 (Ongaro & Ousterhout):
//! terms, `RequestVote` (retried while a campaign lasts) with the up-to-date check,
//! `AppendEntries` with the `prevLogIndex` / `prevLogTerm` consistency check, conflict
//! truncation, `nextIndex` back-off with a conflict hint, commitment by counting `matchIndex`
//! only for entries of the leader's **current** term (Figure 8), and the leader's no-op entry
//! (section 8, switchable with [`Config::leader_noop`]).
//!
//! The whole cluster runs in one thread on a virtual clock (integer milliseconds). Events are
//! ordered by `(time, sequence number)`, every random choice comes from [`crate::rng::Rng`], so a
//! run is a pure function of `(Config, seed)`.
//!
//! After **every** event an online checker verifies the five properties of the paper's Figure 3:
//! Election Safety, Leader Append-Only, Log Matching, Leader Completeness and State Machine
//! Safety. [`Config::mutant`] switches on a deliberately broken variant of the algorithm so that
//! tests can prove the checker bites; [`simulate`] returns the first violation,
//! [`simulate_checked`] panics with the seed instead.
//!
//! What the model leaves out (it is *not* production Raft): snapshots / log compaction,
//! membership changes, pre-vote and `CheckQuorum`, linearizable reads (ReadIndex / leases), the
//! cost of `fsync` (persistent state is updated instantly), clock drift, client sessions and
//! de-duplication, Byzantine faults. Network messages are lost, delayed, reordered, duplicated or
//! cut by a partition, but never corrupted.

use crate::rng::Rng;
use std::collections::BTreeMap;
use std::fmt;

/// Virtual time in milliseconds.
pub type Millis = u64;
/// Node identifier (index in `0..Config::nodes`).
pub type NodeId = usize;
/// Raft term number.
pub type Term = u64;

/// Largest supported cluster (vote sets are bit masks, `matchIndex` sorting uses a stack array).
pub const MAX_NODES: usize = 8;

/// Number of nodes that form a strict majority of a cluster of `n` nodes.
pub const fn majority(n: usize) -> usize {
    n / 2 + 1
}

/// One log entry: a client command tagged with the term in which the leader received it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Term of the leader that created the entry.
    pub term: Term,
    /// Opaque state machine command: a unique client command id, or 0 for the leader's no-op.
    pub cmd: u64,
}

/// The three Raft server states (paper Figure 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Passive: answers RPCs, starts an election when the election timer fires.
    Follower,
    /// Campaigning for the current term.
    Candidate,
    /// Won the current term; replicates the log.
    Leader,
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Role::Follower => "follower",
            Role::Candidate => "candidate",
            Role::Leader => "leader",
        })
    }
}

/// Deliberately broken variants of the algorithm (all off by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutant {
    /// `votedFor` is not on stable storage: a restarted node may vote twice in one term.
    NoPersistVote,
    /// `RequestVote` is granted whatever the candidate's log looks like (no §5.4.1 check).
    NoUpToDateCheck,
    /// A candidate wins with `N / 2` votes (integer division) instead of a strict majority.
    MinorityLeader,
    /// The leader commits an entry of an *older* term as soon as it is on a majority (Figure 8).
    CommitOldTermByCounting,
    /// A follower checks that it has an entry at `prevLogIndex` but not that its term matches.
    NoPrevLogTermCheck,
}

impl Mutant {
    /// All mutants, in the order the lab prints them.
    pub const ALL: [Mutant; 5] = [
        Mutant::NoPersistVote,
        Mutant::NoUpToDateCheck,
        Mutant::MinorityLeader,
        Mutant::CommitOldTermByCounting,
        Mutant::NoPrevLogTermCheck,
    ];

    /// Short name used in tables.
    pub fn name(self) -> &'static str {
        match self {
            Mutant::NoPersistVote => "NoPersistVote",
            Mutant::NoUpToDateCheck => "NoUpToDateCheck",
            Mutant::MinorityLeader => "MinorityLeader",
            Mutant::CommitOldTermByCounting => "CommitOldTermByCounting",
            Mutant::NoPrevLogTermCheck => "NoPrevLogTermCheck",
        }
    }
}

/// The five properties of the paper's Figure 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Invariant {
    /// At most one leader can be elected in a given term.
    ElectionSafety,
    /// If two logs contain an entry with the same index and term, they are identical up to it.
    LogMatching,
    /// An entry committed in some term is in the log of the leaders of all higher terms.
    LeaderCompleteness,
    /// No two nodes apply different commands at the same index.
    StateMachineSafety,
    /// A leader never overwrites or deletes entries of its own log.
    LeaderAppendOnly,
}

impl fmt::Display for Invariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Invariant::ElectionSafety => "Election Safety",
            Invariant::LogMatching => "Log Matching",
            Invariant::LeaderCompleteness => "Leader Completeness",
            Invariant::StateMachineSafety => "State Machine Safety",
            Invariant::LeaderAppendOnly => "Leader Append-Only",
        })
    }
}

/// The first violated invariant of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// Which property broke.
    pub invariant: Invariant,
    /// Virtual time of the event after which the checker noticed it.
    pub at: Millis,
    /// Human-readable description (nodes, terms, indexes).
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} violated at t={} ms: {}",
            self.invariant, self.at, self.detail
        )
    }
}

/// Something that happens at a scheduled virtual time (fault injection and scripted scenarios).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Crash a node: volatile state is lost, `currentTerm`/`votedFor`/`log` survive.
    Crash(NodeId),
    /// Restart a crashed node as a follower.
    Restart(NodeId),
    /// Crash a node and restart it `Millis` later (skipped if that would leave too few nodes up).
    CrashFor(NodeId, Millis),
    /// Crash whichever node is leader now (if any) and restart it `Millis` later.
    CrashLeaderFor(Millis),
    /// Replace the partition by these disjoint groups; nodes in different groups cannot talk.
    Partition(Vec<Vec<NodeId>>),
    /// Cut the current leader (if any) off from every other node.
    IsolateLeader,
    /// Remove the partition.
    Heal,
    /// Remove the partition, set drop/duplication to 0 and restart every crashed node.
    HealAll,
    /// Make a node's election timer fire right now (used by scripted scenarios).
    ForceTimeout(NodeId),
    /// Deliver a client command straight to a node (ignored unless it is a live leader).
    Propose(NodeId, u64),
    /// The background client stops proposing.
    StopClient,
}

/// Parameters of one simulated cluster.
#[derive(Clone, Debug)]
pub struct Config {
    /// Number of nodes (`1..=MAX_NODES`; the lab uses 3 and 5).
    pub nodes: usize,
    /// Election timeout is uniform in `[lo, hi)`; `lo == hi` means a fixed timeout.
    pub election_timeout: (Millis, Millis),
    /// Leader heartbeat period (also the candidate's `RequestVote` retry period).
    pub heartbeat: Millis,
    /// One-way message latency, uniform in `[lo, hi]`.
    pub latency: (Millis, Millis),
    /// Per-message loss probability in 1/1000.
    pub drop_permille: u32,
    /// Per-message duplication probability in 1/1000.
    pub dup_permille: u32,
    /// A background client proposes a command every this many ms (0 disables it).
    pub client_interval: Millis,
    /// The client forgets "its" leader if no command was answered for this long.
    pub client_timeout: Millis,
    /// Maximum number of entries in one `AppendEntries`.
    pub max_batch: usize,
    /// End of the simulation.
    pub horizon: Millis,
    /// Fault plan and scripted actions.
    pub plan: Vec<(Millis, Action)>,
    /// Never crash another node when this many are already down (random crashes only).
    pub max_down: usize,
    /// Optional deliberately broken variant.
    pub mutant: Option<Mutant>,
    /// Stop as soon as the first leader is elected (liveness experiments).
    pub stop_at_first_leader: bool,
    /// Keep a human-readable trace of the interesting events.
    pub record_trace: bool,
    /// Chance (in 1/1000) that a node crashes right after it granted a vote: fault injection aimed
    /// at the one window in which a persisted `votedFor` matters.
    pub crash_after_vote_permille: u32,
    /// Downtime after such a crash, uniform in `[lo, hi]` ms.
    pub quick_restart: (Millis, Millis),
    /// Stop at the first violated invariant (the default). If `false` the run goes on and
    /// `Outcome::violations` lists the first violation of every invariant that broke.
    pub stop_at_violation: bool,
    /// A new leader appends a no-op entry of its own term right away (paper section 8), so that
    /// entries of earlier terms get committed without waiting for a client command.
    pub leader_noop: bool,
}

impl Config {
    /// Defaults: election timeout 150-300 ms, heartbeat 50 ms, latency 5-15 ms, one client
    /// command per 100 ms, no faults, 3 s horizon.
    pub fn new(nodes: usize) -> Config {
        Config {
            nodes,
            election_timeout: (150, 300),
            heartbeat: 50,
            latency: (5, 15),
            drop_permille: 0,
            dup_permille: 0,
            client_interval: 100,
            client_timeout: 400,
            max_batch: 16,
            horizon: 3000,
            plan: Vec::new(),
            max_down: nodes.saturating_sub(1) / 2,
            mutant: None,
            stop_at_first_leader: false,
            record_trace: false,
            crash_after_vote_permille: 0,
            quick_restart: (1, 5),
            stop_at_violation: true,
            leader_noop: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config::new(5)
    }
}

/// Everything observable about one finished run.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The seed of the run.
    pub seed: u64,
    /// First violated invariant, if any (with `stop_at_violation` the run ends right there).
    pub violation: Option<Violation>,
    /// The first violation of every invariant that broke, in order of occurrence.
    pub violations: Vec<Violation>,
    /// Virtual time at which some node first won an election.
    pub first_leader_at: Option<Millis>,
    /// Term of that first leader.
    pub first_leader_term: Term,
    /// Number of times a node became leader.
    pub elections: u32,
    /// Number of times a node started a campaign (term increments by timeout).
    pub candidacies: u32,
    /// Largest term reached by any node.
    pub max_term: Term,
    /// Highest log index ever committed by a leader.
    pub committed: usize,
    /// Log entries deleted from followers' logs by conflict truncation.
    pub overwritten: usize,
    /// Client commands accepted by a leader.
    pub accepted: u64,
    /// Number of simulated events.
    pub events: u64,
    /// FNV-style fingerprint of the event trace: per event it mixes the time, the message kind/term/flags, the
    /// target id and a digest of the target's term, role, log length and commit index (equal seeds must give
    /// equal values; it does not cover entry payloads).
    pub trace_hash: u64,
    /// Human-readable trace (only if `Config::record_trace`).
    pub trace: Vec<(Millis, String)>,
    /// For every time the leader was lost (crashed or cut off), the delay until another node won
    /// an election.
    pub recoveries: Vec<Millis>,
    /// At the end: every node up, a leader exists, all logs identical and every committed entry
    /// applied everywhere.
    pub converged: bool,
}

// ---------------------------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------------------------

/// Arguments of an `AppendEntries` RPC (the sender is the leader).
#[derive(Clone, Debug)]
struct AppendArgs {
    term: Term,
    prev_index: usize,
    prev_term: Term,
    entries: Vec<Entry>,
    leader_commit: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    RequestVote {
        term: Term,
        last_log_index: usize,
        last_log_term: Term,
    },
    VoteReply {
        term: Term,
        granted: bool,
    },
    Append(AppendArgs),
    AppendReply {
        term: Term,
        success: bool,
        match_index: usize,
        hint: usize,
    },
}

#[derive(Clone, Debug)]
enum Event {
    Deliver { from: NodeId, to: NodeId, msg: Msg },
    ElectionTimeout { node: NodeId, epoch: u64 },
    Tick { node: NodeId, epoch: u64 },
    ClientTick,
    Act(Action),
}

struct Node {
    // Persistent state (Figure 2): survives a crash.
    term: Term,
    voted_for: Option<NodeId>,
    log: Vec<Entry>,
    // Volatile state: lost at a crash.
    alive: bool,
    role: Role,
    commit_index: usize,
    last_applied: usize,
    leader_hint: Option<NodeId>,
    votes: u64,
    replied: u64,
    next_index: Vec<usize>,
    match_index: Vec<usize>,
    timer_epoch: u64,
    tick_epoch: u64,
    rng: Rng,
    /// The log changed during the current event (checker bookkeeping).
    dirty: bool,
}

impl Node {
    fn new(rng: Rng) -> Node {
        Node {
            term: 0,
            voted_for: None,
            log: Vec::new(),
            alive: true,
            role: Role::Follower,
            commit_index: 0,
            last_applied: 0,
            leader_hint: None,
            votes: 0,
            replied: 0,
            next_index: Vec::new(),
            match_index: Vec::new(),
            timer_epoch: 0,
            tick_epoch: 0,
            rng,
            dirty: false,
        }
    }

    fn last_log_term(&self) -> Term {
        self.log.last().map_or(0, |e| e.term)
    }
}

fn bit(i: NodeId) -> u64 {
    1u64 << i
}

fn uniform(rng: &mut Rng, lo: u64, hi_exclusive: u64) -> u64 {
    if hi_exclusive <= lo {
        lo
    } else {
        rng.range(lo, hi_exclusive)
    }
}

/// Highest 1-based index at which two logs have the same term, and the first 1-based index in
/// `1..=that` where they differ (if any): a Log Matching violation.
fn log_matching_violation(a: &[Entry], b: &[Entry]) -> Option<(usize, usize)> {
    let mut top = a.len().min(b.len());
    while top > 0 && a[top - 1].term != b[top - 1].term {
        top -= 1;
    }
    (0..top).find(|&k| a[k] != b[k]).map(|k| (top, k + 1))
}

/// The online invariant checker: global ground truth that no node can see.
struct Checker {
    /// The first violation of each invariant that broke, in order of occurrence.
    violations: Vec<Violation>,
    /// term -> the node that won it.
    leaders: BTreeMap<Term, NodeId>,
    /// index -> (entry, term of the leader that committed it).
    committed: BTreeMap<usize, (Entry, Term)>,
    /// index -> entry applied by some node.
    applied: BTreeMap<usize, Entry>,
    /// Copy of each current leader's log, to verify it only ever grows at the end.
    shadow: Vec<Option<Vec<Entry>>>,
}

impl Checker {
    fn new(n: usize) -> Checker {
        Checker {
            violations: Vec::new(),
            leaders: BTreeMap::new(),
            committed: BTreeMap::new(),
            applied: BTreeMap::new(),
            shadow: vec![None; n],
        }
    }

    fn fail(&mut self, invariant: Invariant, at: Millis, detail: String) {
        if !self.violations.iter().any(|v| v.invariant == invariant) {
            self.violations.push(Violation {
                invariant,
                at,
                detail,
            });
        }
    }

    fn on_become_leader(&mut self, node: NodeId, nodes: &[Node], at: Millis) {
        let term = nodes[node].term;
        match self.leaders.get(&term) {
            Some(&other) if other != node => {
                self.fail(
                    Invariant::ElectionSafety,
                    at,
                    format!("term {term} has two leaders: n{other} and n{node}"),
                );
            }
            _ => {
                self.leaders.insert(term, node);
            }
        }
        let log = &nodes[node].log;
        let missing = self
            .committed
            .iter()
            .find(|&(&idx, &(e, cterm))| cterm < term && log.get(idx - 1) != Some(&e))
            .map(|(&idx, &(e, cterm))| (idx, e, cterm));
        if let Some((idx, e, cterm)) = missing {
            self.fail(
                Invariant::LeaderCompleteness,
                at,
                format!(
                    "n{node} won term {term} but its log lacks index {idx} (term {}, cmd {}) committed in term {cterm}",
                    e.term, e.cmd
                ),
            );
        }
        self.shadow[node] = Some(log.clone());
    }

    fn on_commit(&mut self, leader: NodeId, old: usize, new: usize, nodes: &[Node], at: Millis) {
        let term = nodes[leader].term;
        for idx in old + 1..=new {
            let e = nodes[leader].log[idx - 1];
            match self.committed.get(&idx) {
                Some(&(prev, pterm)) if prev != e => {
                    self.fail(
                        Invariant::LeaderCompleteness,
                        at,
                        format!(
                            "n{leader} commits index {idx} as (term {}, cmd {}) in term {term}, but (term {}, cmd {}) was committed in term {pterm}",
                            e.term, e.cmd, prev.term, prev.cmd
                        ),
                    );
                }
                Some(_) => {}
                None => {
                    self.committed.insert(idx, (e, term));
                }
            }
            // A leader of a higher term that is already in office must hold the entry.
            let lacking = nodes.iter().enumerate().find(|&(j, nj)| {
                j != leader
                    && nj.alive
                    && nj.role == Role::Leader
                    && nj.term > term
                    && nj.log.get(idx - 1) != Some(&e)
            });
            if let Some((j, nj)) = lacking {
                let jt = nj.term;
                self.fail(
                    Invariant::LeaderCompleteness,
                    at,
                    format!(
                        "n{j} is leader of term {jt} but lacks index {idx} committed in term {term}"
                    ),
                );
            }
        }
    }

    fn on_apply(&mut self, node: NodeId, index: usize, e: Entry, at: Millis) {
        match self.applied.get(&index) {
            Some(&prev) if prev != e => self.fail(
                Invariant::StateMachineSafety,
                at,
                format!(
                    "n{node} applies (term {}, cmd {}) at index {index}, another node applied (term {}, cmd {})",
                    e.term, e.cmd, prev.term, prev.cmd
                ),
            ),
            Some(_) => {}
            None => {
                self.applied.insert(index, e);
            }
        }
    }

    /// Checks that depend on logs, run once per event for every node whose log changed.
    fn after_event(&mut self, nodes: &mut [Node], at: Millis) {
        for (sh, n) in self.shadow.iter_mut().zip(nodes.iter()) {
            if n.role != Role::Leader {
                *sh = None;
            }
        }
        for i in 0..nodes.len() {
            if !nodes[i].dirty {
                continue;
            }
            nodes[i].dirty = false;
            if nodes[i].role == Role::Leader {
                let log = &nodes[i].log;
                if let Some(sh) = self.shadow[i].as_mut() {
                    if log.len() < sh.len() || log[..sh.len()] != sh[..] {
                        let detail = format!(
                            "leader n{i} (term {}) changed entries it already had",
                            nodes[i].term
                        );
                        self.fail(Invariant::LeaderAppendOnly, at, detail);
                    } else {
                        let from = sh.len();
                        sh.extend_from_slice(&log[from..]);
                    }
                }
            }
            for j in 0..nodes.len() {
                if j == i {
                    continue;
                }
                if let Some((top, first)) = log_matching_violation(&nodes[i].log, &nodes[j].log) {
                    let term = nodes[i].log[top - 1].term;
                    self.fail(
                        Invariant::LogMatching,
                        at,
                        format!(
                            "n{i} and n{j} both hold an entry with term {term} at index {top} but their logs differ at index {first}"
                        ),
                    );
                }
            }
        }
    }
}

struct Client {
    active: bool,
    next_cmd: u64,
    believed: Option<NodeId>,
    last_ack: Millis,
    rng: Rng,
}

/// A simulated Raft cluster. Build it with [`Cluster::new`], drive it with
/// [`Cluster::run_until`], inspect it with the accessors. [`simulate`] wraps the common case.
pub struct Cluster {
    cfg: Config,
    seed: u64,
    now: Millis,
    seq: u64,
    queue: BTreeMap<(Millis, u64), Event>,
    nodes: Vec<Node>,
    partition: Option<Vec<u8>>,
    drop_permille: u32,
    dup_permille: u32,
    net_rng: Rng,
    act_rng: Rng,
    client: Client,
    checker: Checker,
    stopped: bool,
    events: u64,
    hash: u64,
    trace: Vec<(Millis, String)>,
    first_leader_at: Option<Millis>,
    first_leader_term: Term,
    elections: u32,
    candidacies: u32,
    max_term: Term,
    overwritten: usize,
    accepted: u64,
    leader_lost_at: Option<Millis>,
    recoveries: Vec<Millis>,
}

impl Cluster {
    /// Creates a cluster of `cfg.nodes` followers at virtual time 0 with fresh election timers.
    pub fn new(cfg: Config, seed: u64) -> Cluster {
        assert!(
            (1..=MAX_NODES).contains(&cfg.nodes),
            "1..={MAX_NODES} nodes supported"
        );
        let n = cfg.nodes;
        let mut master = Rng::new(seed);
        let net_rng = master.fork(1);
        let act_rng = master.fork(2);
        let client_rng = master.fork(3);
        let nodes: Vec<Node> = (0..n)
            .map(|i| Node::new(master.fork(100 + i as u64)))
            .collect();
        let mut c = Cluster {
            drop_permille: cfg.drop_permille,
            dup_permille: cfg.dup_permille,
            seed,
            now: 0,
            seq: 0,
            queue: BTreeMap::new(),
            nodes,
            partition: None,
            net_rng,
            act_rng,
            client: Client {
                active: cfg.client_interval > 0,
                next_cmd: 1,
                believed: None,
                last_ack: 0,
                rng: client_rng,
            },
            checker: Checker::new(n),
            stopped: false,
            events: 0,
            hash: 0xcbf2_9ce4_8422_2325,
            trace: Vec::new(),
            first_leader_at: None,
            first_leader_term: 0,
            elections: 0,
            candidacies: 0,
            max_term: 0,
            overwritten: 0,
            accepted: 0,
            leader_lost_at: None,
            recoveries: Vec::new(),
            cfg,
        };
        for i in 0..n {
            c.reset_election_timer(i);
        }
        if c.cfg.client_interval > 0 {
            let at = c.cfg.client_interval;
            c.schedule(at, Event::ClientTick);
        }
        for (t, a) in c.cfg.plan.clone() {
            c.schedule(t, Event::Act(a));
        }
        c
    }

    // ----- inspection --------------------------------------------------------------------

    /// Current virtual time.
    pub fn now(&self) -> Millis {
        self.now
    }
    /// Role of node `i`.
    pub fn role(&self, i: NodeId) -> Role {
        self.nodes[i].role
    }
    /// `currentTerm` of node `i`.
    pub fn term(&self, i: NodeId) -> Term {
        self.nodes[i].term
    }
    /// `votedFor` of node `i`.
    pub fn voted_for(&self, i: NodeId) -> Option<NodeId> {
        self.nodes[i].voted_for
    }
    /// Log of node `i` (index 1 is `log()[0]`).
    pub fn log(&self, i: NodeId) -> &[Entry] {
        &self.nodes[i].log
    }
    /// `commitIndex` of node `i`.
    pub fn commit_index(&self, i: NodeId) -> usize {
        self.nodes[i].commit_index
    }
    /// `lastApplied` of node `i`.
    pub fn last_applied(&self, i: NodeId) -> usize {
        self.nodes[i].last_applied
    }
    /// Whether node `i` is up.
    pub fn is_alive(&self, i: NodeId) -> bool {
        self.nodes[i].alive
    }
    /// The live leader with the highest term, if any.
    pub fn leader(&self) -> Option<NodeId> {
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.alive && n.role == Role::Leader)
            .max_by_key(|&(i, n)| (n.term, std::cmp::Reverse(i)))
            .map(|(i, _)| i)
    }
    /// Highest index ever committed by a leader.
    pub fn committed_len(&self) -> usize {
        self.checker
            .committed
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0)
    }
    /// The first violated invariant so far.
    pub fn violation(&self) -> Option<&Violation> {
        self.checker.violations.first()
    }
    /// Whether, right now, every node is up, a leader exists, all logs are identical and every
    /// entry ever committed has been applied by every node.
    pub fn converged(&self) -> bool {
        let committed = self.committed_len();
        self.leader().is_some()
            && self
                .nodes
                .iter()
                .all(|n| n.alive && n.log == self.nodes[0].log && n.last_applied >= committed)
    }
    /// Number of events processed so far.
    pub fn events(&self) -> u64 {
        self.events
    }
    /// Entries deleted from logs by conflict truncation so far.
    pub fn overwritten(&self) -> usize {
        self.overwritten
    }
    /// Human-readable trace so far (empty unless `Config::record_trace`).
    pub fn trace(&self) -> &[(Millis, String)] {
        &self.trace
    }

    /// Schedules an action at an absolute virtual time (`at >= now()`).
    pub fn schedule_action(&mut self, at: Millis, action: Action) {
        self.schedule(at.max(self.now), Event::Act(action));
    }

    /// Summary of the run so far.
    pub fn outcome(&self) -> Outcome {
        Outcome {
            seed: self.seed,
            violation: self.checker.violations.first().cloned(),
            violations: self.checker.violations.clone(),
            first_leader_at: self.first_leader_at,
            first_leader_term: self.first_leader_term,
            elections: self.elections,
            candidacies: self.candidacies,
            max_term: self.max_term,
            committed: self.committed_len(),
            overwritten: self.overwritten,
            accepted: self.accepted,
            events: self.events,
            trace_hash: self.hash,
            trace: self.trace.clone(),
            recoveries: self.recoveries.clone(),
            converged: self.converged(),
        }
    }

    // ----- engine ------------------------------------------------------------------------

    /// Processes events with `time <= t`; stops early at the first violated invariant (or, with
    /// `stop_at_first_leader`, at the first election win).
    pub fn run_until(&mut self, t: Millis) {
        while !self.stopped {
            let Some((&(time, _), _)) = self.queue.first_key_value() else {
                break;
            };
            if time > t {
                break;
            }
            let Some(((time, _), ev)) = self.queue.pop_first() else {
                break;
            };
            self.now = time;
            self.events += 1;
            let target = match &ev {
                Event::Deliver { to, .. } => *to,
                Event::ElectionTimeout { node, .. } | Event::Tick { node, .. } => *node,
                Event::ClientTick | Event::Act(_) => 0,
            };
            self.mix(time);
            self.mix(Self::kind_code(&ev));
            self.mix(target as u64);
            self.dispatch(ev);
            let n = &self.nodes[target];
            let digest = n.term
                ^ (n.role as u64) << 56
                ^ (n.log.len() as u64) << 40
                ^ (n.commit_index as u64) << 24;
            self.mix(digest);
            self.checker.after_event(&mut self.nodes, self.now);
            if self.cfg.stop_at_violation && !self.checker.violations.is_empty() {
                self.stopped = true;
            }
        }
    }

    fn kind_code(ev: &Event) -> u64 {
        match ev {
            Event::Deliver { msg, .. } => match msg {
                Msg::RequestVote { term, .. } => 1 | term << 8,
                Msg::VoteReply { term, granted } => 2 | (*granted as u64) << 4 | term << 8,
                Msg::Append(a) => 3 | (a.entries.len() as u64) << 4 | a.term << 8,
                Msg::AppendReply { term, success, .. } => 4 | (*success as u64) << 4 | term << 8,
            },
            Event::ElectionTimeout { .. } => 5,
            Event::Tick { .. } => 6,
            Event::ClientTick => 7,
            Event::Act(_) => 8,
        }
    }

    fn mix(&mut self, x: u64) {
        self.hash = (self.hash ^ x).wrapping_mul(0x0000_0100_0000_01B3);
    }

    fn schedule(&mut self, at: Millis, ev: Event) {
        self.seq += 1;
        self.queue.insert((at, self.seq), ev);
    }

    fn log_line(&mut self, f: impl FnOnce() -> String) {
        if self.cfg.record_trace {
            let line = f();
            self.trace.push((self.now, line));
        }
    }

    fn dispatch(&mut self, ev: Event) {
        match ev {
            Event::Deliver { from, to, msg } => {
                if !self.nodes[to].alive || !self.connected(from, to) {
                    return;
                }
                match msg {
                    Msg::RequestVote {
                        term,
                        last_log_index,
                        last_log_term,
                    } => self.on_request_vote(to, from, term, last_log_index, last_log_term),
                    Msg::VoteReply { term, granted } => self.on_vote_reply(to, from, term, granted),
                    Msg::Append(a) => self.on_append(to, from, &a),
                    Msg::AppendReply {
                        term,
                        success,
                        match_index,
                        hint,
                    } => self.on_append_reply(to, from, term, success, match_index, hint),
                }
            }
            Event::ElectionTimeout { node, epoch } => {
                let n = &self.nodes[node];
                if n.alive && n.timer_epoch == epoch && n.role != Role::Leader {
                    self.start_election(node);
                }
            }
            Event::Tick { node, epoch } => {
                let n = &self.nodes[node];
                if !n.alive || n.tick_epoch != epoch {
                    return;
                }
                match n.role {
                    Role::Leader => self.send_appends(node),
                    Role::Candidate => self.send_request_votes(node),
                    Role::Follower => return,
                }
                let at = self.now + self.cfg.heartbeat;
                self.schedule(at, Event::Tick { node, epoch });
            }
            Event::ClientTick => self.on_client_tick(),
            Event::Act(a) => self.do_action(a),
        }
    }

    // ----- network -----------------------------------------------------------------------

    fn connected(&self, a: NodeId, b: NodeId) -> bool {
        match &self.partition {
            None => true,
            Some(g) => g[a] == g[b],
        }
    }

    fn set_partition(&mut self, groups: &[Vec<NodeId>]) {
        let mut g = vec![groups.len() as u8; self.cfg.nodes];
        for (k, members) in groups.iter().enumerate() {
            for &m in members {
                g[m] = k as u8;
            }
        }
        self.partition = Some(g);
    }

    fn send(&mut self, from: NodeId, to: NodeId, msg: Msg) {
        if !self.connected(from, to) {
            return;
        }
        if self.drop_permille > 0 && self.net_rng.below(1000) < u64::from(self.drop_permille) {
            return;
        }
        let (lo, hi) = self.cfg.latency;
        let lat = uniform(&mut self.net_rng, lo, hi + 1);
        if self.dup_permille > 0 && self.net_rng.below(1000) < u64::from(self.dup_permille) {
            let lat2 = uniform(&mut self.net_rng, lo, hi + 1);
            self.schedule(
                self.now + lat2,
                Event::Deliver {
                    from,
                    to,
                    msg: msg.clone(),
                },
            );
        }
        self.schedule(self.now + lat, Event::Deliver { from, to, msg });
    }

    // ----- timers ------------------------------------------------------------------------

    fn reset_election_timer(&mut self, i: NodeId) {
        let (lo, hi) = self.cfg.election_timeout;
        let d = uniform(&mut self.nodes[i].rng, lo, hi);
        let n = &mut self.nodes[i];
        n.timer_epoch += 1;
        let epoch = n.timer_epoch;
        self.schedule(self.now + d, Event::ElectionTimeout { node: i, epoch });
    }

    fn start_tick(&mut self, i: NodeId) {
        let n = &mut self.nodes[i];
        n.tick_epoch += 1;
        let epoch = n.tick_epoch;
        self.schedule(
            self.now + self.cfg.heartbeat,
            Event::Tick { node: i, epoch },
        );
    }

    // ----- role changes ------------------------------------------------------------------

    /// Figure 2, "All servers": a larger term in any RPC makes the receiver a follower of it.
    fn observe_term(&mut self, i: NodeId, term: Term) {
        if term <= self.nodes[i].term {
            return;
        }
        let old = self.nodes[i].term;
        let was = self.nodes[i].role;
        self.nodes[i].term = term;
        self.nodes[i].voted_for = None;
        self.max_term = self.max_term.max(term);
        if was != Role::Follower {
            self.log_line(|| {
                format!("n{i} ({was}) sees term {term} > {old}: steps down to follower")
            });
            self.become_follower(i);
        }
    }

    fn become_follower(&mut self, i: NodeId) {
        let was = self.nodes[i].role;
        let n = &mut self.nodes[i];
        n.role = Role::Follower;
        n.votes = 0;
        n.replied = 0;
        n.tick_epoch += 1;
        if was == Role::Leader {
            // Leaders run no election timer; a deposed leader needs one again.
            self.reset_election_timer(i);
        }
    }

    fn wins(&self, votes: u64) -> bool {
        let v = votes.count_ones() as usize;
        let n = self.cfg.nodes;
        if self.cfg.mutant == Some(Mutant::MinorityLeader) {
            v >= n / 2
        } else {
            v >= majority(n)
        }
    }

    fn start_election(&mut self, i: NodeId) {
        let n = &mut self.nodes[i];
        n.term += 1;
        n.role = Role::Candidate;
        n.voted_for = Some(i);
        n.votes = bit(i);
        n.replied = bit(i);
        n.leader_hint = None;
        let term = n.term;
        self.max_term = self.max_term.max(term);
        self.candidacies += 1;
        self.log_line(|| format!("n{i} election timeout: candidate for term {term}"));
        self.reset_election_timer(i);
        self.start_tick(i);
        if self.wins(bit(i)) {
            self.become_leader(i);
        } else {
            self.send_request_votes(i);
        }
    }

    fn send_request_votes(&mut self, i: NodeId) {
        let n = &self.nodes[i];
        let msg = Msg::RequestVote {
            term: n.term,
            last_log_index: n.log.len(),
            last_log_term: n.last_log_term(),
        };
        let replied = n.replied;
        for p in 0..self.cfg.nodes {
            if replied & bit(p) == 0 {
                self.send(i, p, msg.clone());
            }
        }
    }

    fn become_leader(&mut self, i: NodeId) {
        let count = self.cfg.nodes;
        let n = &mut self.nodes[i];
        n.role = Role::Leader;
        n.leader_hint = Some(i);
        n.next_index = vec![n.log.len() + 1; count];
        n.match_index = vec![0; count];
        let len = n.log.len();
        if self.cfg.leader_noop {
            n.log.push(Entry {
                term: n.term,
                cmd: 0,
            });
            n.dirty = true;
        }
        n.match_index[i] = n.log.len();
        n.timer_epoch += 1;
        let (term, votes) = (n.term, n.votes.count_ones());
        self.elections += 1;
        if let Some(lost) = self.leader_lost_at.take() {
            self.recoveries.push(self.now - lost);
        }
        if self.first_leader_at.is_none() {
            self.first_leader_at = Some(self.now);
            self.first_leader_term = term;
        }
        self.log_line(|| {
            format!("n{i} wins term {term} with {votes}/{count} votes: LEADER (log length {len})")
        });
        self.checker.on_become_leader(i, &self.nodes, self.now);
        self.send_appends(i);
        self.advance_commit(i); // a single-node cluster is its own majority
        self.start_tick(i);
        if self.cfg.stop_at_first_leader {
            self.stopped = true;
        }
    }

    // ----- RequestVote -------------------------------------------------------------------

    fn on_request_vote(
        &mut self,
        to: NodeId,
        from: NodeId,
        term: Term,
        last_index: usize,
        last_term: Term,
    ) {
        self.observe_term(to, term);
        let skip_check = self.cfg.mutant == Some(Mutant::NoUpToDateCheck);
        let n = &mut self.nodes[to];
        let mut granted = false;
        if term >= n.term {
            let free = n.voted_for.is_none() || n.voted_for == Some(from);
            let my_term = n.last_log_term();
            let up_to_date =
                last_term > my_term || (last_term == my_term && last_index >= n.log.len());
            if free && (up_to_date || skip_check) {
                n.voted_for = Some(from);
                granted = true;
            }
        }
        let reply_term = n.term;
        if granted {
            self.log_line(|| format!("n{to} grants its vote to n{from} (term {reply_term})"));
            self.reset_election_timer(to);
        }
        self.send(
            to,
            from,
            Msg::VoteReply {
                term: reply_term,
                granted,
            },
        );
        let p = u64::from(self.cfg.crash_after_vote_permille);
        if granted && p > 0 && self.act_rng.below(1000) < p {
            // Fault injection: the voter dies right after its reply left the machine.
            let (lo, hi) = self.cfg.quick_restart;
            let down = uniform(&mut self.act_rng, lo, hi + 1);
            self.crash_for(to, down);
        }
    }

    fn on_vote_reply(&mut self, to: NodeId, from: NodeId, term: Term, granted: bool) {
        self.observe_term(to, term);
        let n = &mut self.nodes[to];
        if n.role != Role::Candidate || term != n.term {
            return;
        }
        n.replied |= bit(from);
        if granted {
            n.votes |= bit(from);
            let votes = n.votes;
            if self.wins(votes) {
                self.become_leader(to);
            }
        }
    }

    // ----- AppendEntries -----------------------------------------------------------------

    fn send_appends(&mut self, i: NodeId) {
        for p in 0..self.cfg.nodes {
            if p != i {
                self.send_append(i, p);
            }
        }
    }

    fn send_append(&mut self, i: NodeId, p: NodeId) {
        let n = &self.nodes[i];
        let prev = n.next_index[p] - 1;
        let prev_term = if prev == 0 { 0 } else { n.log[prev - 1].term };
        let end = (prev + self.cfg.max_batch).min(n.log.len());
        let msg = Msg::Append(AppendArgs {
            term: n.term,
            prev_index: prev,
            prev_term,
            entries: n.log[prev..end].to_vec(),
            leader_commit: n.commit_index,
        });
        self.send(i, p, msg);
    }

    fn on_append(&mut self, to: NodeId, from: NodeId, a: &AppendArgs) {
        let (term, prev_index, prev_term, leader_commit) =
            (a.term, a.prev_index, a.prev_term, a.leader_commit);
        let entries = &a.entries;
        self.observe_term(to, term);
        let cur = self.nodes[to].term;
        if term < cur {
            self.send(
                to,
                from,
                Msg::AppendReply {
                    term: cur,
                    success: false,
                    match_index: 0,
                    hint: 0,
                },
            );
            return;
        }
        // `from` is the legitimate leader of term `cur`: a candidate gives up, the timer restarts.
        if self.nodes[to].role != Role::Follower {
            self.log_line(|| format!("n{to} hears leader n{from} of term {cur}: back to follower"));
            self.become_follower(to);
        }
        self.nodes[to].leader_hint = Some(from);
        self.reset_election_timer(to);

        let skip_term = self.cfg.mutant == Some(Mutant::NoPrevLogTermCheck);
        let n = &mut self.nodes[to];
        let consistent = prev_index == 0
            || (n.log.len() >= prev_index
                && (skip_term || n.log[prev_index - 1].term == prev_term));
        if !consistent {
            let hint = if n.log.len() < prev_index {
                n.log.len() + 1
            } else {
                let t = n.log[prev_index - 1].term;
                n.log.iter().position(|e| e.term == t).map_or(1, |p| p + 1)
            };
            self.send(
                to,
                from,
                Msg::AppendReply {
                    term: cur,
                    success: false,
                    match_index: 0,
                    hint,
                },
            );
            return;
        }
        let mut changed = false;
        for (k, e) in entries.iter().enumerate() {
            let idx = prev_index + k + 1;
            if idx <= n.log.len() {
                if n.log[idx - 1].term != e.term {
                    // Conflict: delete the existing entry and everything after it.
                    self.overwritten += n.log.len() - (idx - 1);
                    n.log.truncate(idx - 1);
                    n.log.push(*e);
                    changed = true;
                }
            } else {
                n.log.push(*e);
                changed = true;
            }
        }
        n.dirty |= changed;
        let last_new = prev_index + entries.len();
        // Figure 2: min(leaderCommit, index of last NEW entry); never move commitIndex backwards.
        let new_commit = leader_commit.min(last_new);
        if new_commit > n.commit_index {
            n.commit_index = new_commit;
        }
        self.apply(to);
        self.send(
            to,
            from,
            Msg::AppendReply {
                term: cur,
                success: true,
                match_index: last_new,
                hint: 0,
            },
        );
    }

    fn on_append_reply(
        &mut self,
        to: NodeId,
        from: NodeId,
        term: Term,
        success: bool,
        match_index: usize,
        hint: usize,
    ) {
        self.observe_term(to, term);
        let n = &mut self.nodes[to];
        if n.role != Role::Leader || term != n.term {
            return;
        }
        if success {
            // A reply that does not advance matchIndex (a duplicate, or older than one already seen)
            // changes nothing: acting on it would multiply messages when the network duplicates.
            if match_index > n.match_index[from] {
                n.match_index[from] = match_index;
                n.next_index[from] = n.next_index[from].max(match_index + 1);
                let more = n.log.len() >= n.next_index[from];
                self.advance_commit(to);
                if more {
                    self.send_append(to, from);
                }
            }
        } else {
            // Back off to the follower's hint (never below what is known to match); a duplicate
            // rejection computes the same value and is therefore ignored.
            let new_next = hint.max(n.match_index[from] + 1).max(1);
            if new_next < n.next_index[from] {
                n.next_index[from] = new_next;
                self.send_append(to, from);
            }
        }
    }

    /// Leader rule: commit the highest `N` stored on a majority, but only if `log[N]` is from the
    /// leader's current term (the `CommitOldTermByCounting` mutant drops that second condition).
    fn advance_commit(&mut self, i: NodeId) {
        let count = self.cfg.nodes;
        let n = &self.nodes[i];
        let mut m = [0usize; MAX_NODES];
        m[..count].copy_from_slice(&n.match_index);
        m[..count].sort_unstable_by(|a, b| b.cmp(a));
        let cand = m[majority(count) - 1];
        let current_term_ok = n
            .log
            .get(cand.wrapping_sub(1))
            .is_some_and(|e| e.term == n.term);
        if cand > n.commit_index
            && (current_term_ok || self.cfg.mutant == Some(Mutant::CommitOldTermByCounting))
        {
            let old = n.commit_index;
            self.nodes[i].commit_index = cand;
            self.checker.on_commit(i, old, cand, &self.nodes, self.now);
            self.log_line(|| format!("n{i} commits up to index {cand}"));
            self.apply(i);
        }
    }

    fn apply(&mut self, i: NodeId) {
        while self.nodes[i].last_applied < self.nodes[i].commit_index {
            let n = &mut self.nodes[i];
            n.last_applied += 1;
            let idx = n.last_applied;
            let e = n.log[idx - 1];
            let answers_client = n.role == Role::Leader && e.term == n.term && e.cmd != 0;
            self.checker.on_apply(i, idx, e, self.now);
            if answers_client {
                self.client.last_ack = self.now;
                self.client.believed = Some(i);
            }
        }
    }

    // ----- client ------------------------------------------------------------------------

    /// A command from a client. `Err(hint)` carries the leader the node believes in, if any.
    fn propose(&mut self, i: NodeId, cmd: u64) -> Result<(), Option<NodeId>> {
        let n = &mut self.nodes[i];
        if !n.alive {
            return Err(None);
        }
        if n.role != Role::Leader {
            return Err(n.leader_hint);
        }
        n.log.push(Entry { term: n.term, cmd });
        n.dirty = true;
        n.match_index[i] = n.log.len();
        self.accepted += 1;
        self.send_appends(i);
        self.advance_commit(i);
        Ok(())
    }

    fn on_client_tick(&mut self) {
        if !self.client.active {
            return;
        }
        let at = self.now + self.cfg.client_interval;
        self.schedule(at, Event::ClientTick);
        if self.client.believed.is_some()
            && self.now - self.client.last_ack > self.cfg.client_timeout
        {
            self.client.believed = None;
        }
        let target = match self.client.believed {
            Some(t) => t,
            None => {
                self.client.last_ack = self.now;
                self.client.rng.below(self.cfg.nodes as u64) as usize
            }
        };
        let cmd = self.client.next_cmd;
        match self.propose(target, cmd) {
            Ok(()) => {
                self.client.next_cmd += 1;
                self.client.believed = Some(target);
            }
            Err(hint) => self.client.believed = hint,
        }
    }

    // ----- faults ------------------------------------------------------------------------

    fn down_count(&self) -> usize {
        self.nodes.iter().filter(|n| !n.alive).count()
    }

    fn crash(&mut self, i: NodeId) {
        if !self.nodes[i].alive {
            return;
        }
        if self.leader() == Some(i) {
            self.leader_lost_at.get_or_insert(self.now);
        }
        let n = &mut self.nodes[i];
        let was = n.role;
        n.alive = false;
        n.role = Role::Follower;
        n.commit_index = 0;
        n.last_applied = 0;
        n.votes = 0;
        n.replied = 0;
        n.next_index.clear();
        n.match_index.clear();
        n.leader_hint = None;
        n.timer_epoch += 1;
        n.tick_epoch += 1;
        self.log_line(|| format!("n{i} CRASHES (was {was})"));
    }

    fn restart(&mut self, i: NodeId) {
        let n = &mut self.nodes[i];
        if n.alive {
            return;
        }
        n.alive = true;
        if self.cfg.mutant == Some(Mutant::NoPersistVote) {
            n.voted_for = None;
        }
        let (term, len) = (n.term, n.log.len());
        self.log_line(|| format!("n{i} restarts as follower (term {term}, log length {len})"));
        self.reset_election_timer(i);
    }

    fn crash_for(&mut self, i: NodeId, down_for: Millis) {
        if self.nodes[i].alive && self.down_count() < self.cfg.max_down {
            self.crash(i);
            self.schedule(self.now + down_for, Event::Act(Action::Restart(i)));
        }
    }

    fn do_action(&mut self, a: Action) {
        match a {
            Action::Crash(i) => self.crash(i),
            Action::Restart(i) => self.restart(i),
            Action::CrashFor(i, d) => self.crash_for(i, d),
            Action::CrashLeaderFor(d) => {
                if let Some(l) = self.leader() {
                    self.crash_for(l, d);
                }
            }
            Action::Partition(groups) => {
                self.log_line(|| format!("network partition {groups:?}"));
                self.set_partition(&groups);
            }
            Action::IsolateLeader => {
                if let Some(l) = self.leader() {
                    let rest: Vec<NodeId> = (0..self.cfg.nodes).filter(|&p| p != l).collect();
                    self.log_line(|| format!("leader n{l} is cut off from the other nodes"));
                    self.leader_lost_at.get_or_insert(self.now);
                    self.set_partition(&[vec![l], rest]);
                }
            }
            Action::Heal => {
                self.log_line(|| "network healed".to_string());
                self.partition = None;
            }
            Action::HealAll => {
                self.partition = None;
                self.drop_permille = 0;
                self.dup_permille = 0;
                for i in 0..self.cfg.nodes {
                    self.restart(i);
                }
                self.log_line(|| "everything healed: all nodes up, network reliable".to_string());
            }
            Action::ForceTimeout(i) => {
                if self.nodes[i].alive && self.nodes[i].role != Role::Leader {
                    self.start_election(i);
                }
            }
            Action::Propose(i, cmd) => {
                let _ = self.propose(i, cmd);
            }
            Action::StopClient => self.client.active = false,
        }
    }
}

/// Runs a whole simulation and returns its outcome; the first violated invariant (if any) is in
/// `Outcome::violation`. Never panics on a violation.
pub fn simulate(cfg: &Config, seed: u64) -> Outcome {
    let mut c = Cluster::new(cfg.clone(), seed);
    c.run_until(cfg.horizon);
    c.outcome()
}

/// Like [`simulate`] but panics, naming the seed, the time and the invariant, on a violation.
pub fn simulate_checked(cfg: &Config, seed: u64) -> Outcome {
    let o = simulate(cfg, seed);
    if let Some(v) = &o.violation {
        panic!(
            "Raft invariant violated (seed {seed}, {} nodes, mutant {:?}): {v}",
            cfg.nodes, cfg.mutant
        );
    }
    o
}

// ---------------------------------------------------------------------------------------------
// Scenarios and sweeps
// ---------------------------------------------------------------------------------------------

/// Fault scenarios that the lab sweeps over seeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    /// Reliable network, nobody crashes.
    NoFaults,
    /// 10-20 % of the messages are lost (rate drawn per seed).
    Drops,
    /// Random crashes and restarts; half of them hit the current leader.
    Crashes,
    /// Partitions that heal. Most of them cut the current leader off, so it keeps accepting
    /// commands it can never commit and the new leader overwrites them after the heal.
    Partitions,
    /// 10 % loss, 3 % duplication, crashes and partitions together.
    Combined,
}

impl Scenario {
    /// All scenarios, in the order the lab prints them.
    pub const ALL: [Scenario; 5] = [
        Scenario::NoFaults,
        Scenario::Drops,
        Scenario::Crashes,
        Scenario::Partitions,
        Scenario::Combined,
    ];

    /// Short name used in tables.
    pub fn name(self) -> &'static str {
        match self {
            Scenario::NoFaults => "no faults",
            Scenario::Drops => "drops 10-20%",
            Scenario::Crashes => "crash+restart",
            Scenario::Partitions => "partitions",
            Scenario::Combined => "combined",
        }
    }
}

/// Faults stop this long before the horizon; then everything is healed and the client stops, so
/// that liveness after healing can be checked.
pub const QUIET_TAIL: Millis = 1000;

fn crash_plan(r: &mut Rng, cfg: &mut Config) {
    let n = cfg.nodes as u64;
    let end = cfg.horizon.saturating_sub(QUIET_TAIL);
    let mut t = r.range(300, 600);
    while t + 500 < end {
        let down = r.range(100, 500);
        let action = if r.chance(0.5) {
            Action::CrashLeaderFor(down)
        } else {
            Action::CrashFor(r.below(n) as usize, down)
        };
        cfg.plan.push((t, action));
        t += r.range(250, 700);
    }
}

fn partition_plan(r: &mut Rng, cfg: &mut Config) {
    let n = cfg.nodes;
    let mut t = r.range(400, 900);
    let end = cfg.horizon.saturating_sub(QUIET_TAIL);
    while t + 700 < end {
        let action = match r.below(10) {
            0..=5 => Action::IsolateLeader,
            6..=7 => {
                let lone = r.below(n as u64) as usize;
                Action::Partition(vec![vec![lone], (0..n).filter(|&p| p != lone).collect()])
            }
            _ => {
                let mut ids: Vec<NodeId> = (0..n).collect();
                r.shuffle(&mut ids);
                let minority = ids.split_off(n - r.range(1, (n as u64 - 1) / 2 + 1) as usize);
                Action::Partition(vec![minority, ids])
            }
        };
        let len = r.range(300, 700);
        cfg.plan.push((t, action));
        cfg.plan.push((t + len, Action::Heal));
        t += len + r.range(300, 700);
    }
}

/// The configuration of `scenario` for `seed`: the fault plan is drawn from the seed, so the same
/// seed always gives the same faults.
pub fn scenario_config(scenario: Scenario, nodes: usize, horizon: Millis, seed: u64) -> Config {
    let mut cfg = Config::new(nodes);
    cfg.horizon = horizon;
    let mut r = Rng::new(seed).fork(0xFA17);
    let (drops, crashes, partitions) = match scenario {
        Scenario::NoFaults => (false, false, false),
        Scenario::Drops => (true, false, false),
        Scenario::Crashes => (false, true, false),
        Scenario::Partitions => (false, false, true),
        Scenario::Combined => (true, true, true),
    };
    if drops {
        if scenario == Scenario::Combined {
            cfg.drop_permille = 100;
            cfg.dup_permille = 30;
        } else {
            cfg.drop_permille = r.range(100, 201) as u32;
        }
    }
    if crashes {
        crash_plan(&mut r, &mut cfg);
    }
    if partitions {
        partition_plan(&mut r, &mut cfg);
    }
    let heal_at = horizon.saturating_sub(QUIET_TAIL);
    cfg.plan.push((heal_at, Action::HealAll));
    cfg.plan.push((heal_at + 300, Action::StopClient));
    cfg
}

/// Nearest-rank percentile (`p` in `1..=100`) of an ascending slice.
pub fn percentile(sorted: &[u64], p: usize) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    Some(sorted[rank - 1])
}

/// Aggregate of one scenario over many seeds.
#[derive(Clone, Debug)]
pub struct ScenarioSummary {
    /// The scenario.
    pub scenario: Scenario,
    /// Cluster size.
    pub nodes: usize,
    /// Number of seeds run.
    pub seeds: usize,
    /// Ascending times of the first leader, one per seed that elected one.
    pub first_leader: Vec<Millis>,
    /// Total leader elections over all seeds.
    pub elections: u64,
    /// Total campaigns over all seeds.
    pub candidacies: u64,
    /// Largest term seen in any seed.
    pub max_term: Term,
    /// Total committed entries over all seeds.
    pub committed: u64,
    /// Total entries overwritten in followers' logs over all seeds.
    pub overwritten: u64,
    /// Total simulated events.
    pub events: u64,
    /// Seeds with a violated invariant.
    pub violations: usize,
    /// First violating seed and its violation.
    pub first_violation: Option<(u64, Violation)>,
    /// Ascending delays between losing the leader (crash or cut off) and the next election win.
    pub recoveries: Vec<Millis>,
    /// Seeds that ended converged (see [`Outcome::converged`]).
    pub converged: usize,
}

impl ScenarioSummary {
    /// Seeds in which a leader was elected within `ms` of the start.
    pub fn leader_within(&self, ms: Millis) -> usize {
        self.first_leader.iter().filter(|&&t| t <= ms).count()
    }
    /// Median time to the first leader.
    pub fn median_first_leader(&self) -> Option<Millis> {
        percentile(&self.first_leader, 50)
    }
    /// 95th percentile of the time to the first leader.
    pub fn p95_first_leader(&self) -> Option<Millis> {
        percentile(&self.first_leader, 95)
    }
    /// Median delay between losing the leader and electing the next one.
    pub fn median_recovery(&self) -> Option<Millis> {
        percentile(&self.recoveries, 50)
    }
    /// 95th percentile of that delay.
    pub fn p95_recovery(&self) -> Option<Millis> {
        percentile(&self.recoveries, 95)
    }
}

/// Runs `scenario` for every seed in `seeds` (checker on, never panics) and aggregates.
pub fn sweep(
    scenario: Scenario,
    nodes: usize,
    horizon: Millis,
    seeds: std::ops::Range<u64>,
) -> ScenarioSummary {
    let mut s = ScenarioSummary {
        scenario,
        nodes,
        seeds: 0,
        first_leader: Vec::new(),
        elections: 0,
        candidacies: 0,
        max_term: 0,
        committed: 0,
        overwritten: 0,
        events: 0,
        violations: 0,
        first_violation: None,
        recoveries: Vec::new(),
        converged: 0,
    };
    for seed in seeds {
        let o = simulate(&scenario_config(scenario, nodes, horizon, seed), seed);
        s.seeds += 1;
        s.first_leader.extend(o.first_leader_at);
        s.elections += u64::from(o.elections);
        s.candidacies += u64::from(o.candidacies);
        s.max_term = s.max_term.max(o.max_term);
        s.committed += o.committed as u64;
        s.overwritten += o.overwritten as u64;
        s.events += o.events;
        s.recoveries.extend(o.recoveries.iter().copied());
        s.converged += usize::from(o.converged);
        if let Some(v) = o.violation {
            s.violations += 1;
            s.first_violation.get_or_insert((seed, v));
        }
    }
    s.first_leader.sort_unstable();
    s.recoveries.sort_unstable();
    s
}

// ---------------------------------------------------------------------------------------------
// Liveness experiment: how much randomness do election timeouts need?
// ---------------------------------------------------------------------------------------------

/// Result of the "time to first leader" experiment for one timeout width.
#[derive(Clone, Debug)]
pub struct LivenessSummary {
    /// Election timeouts are drawn from `[150, 150 + width)` ms; width 0 means identical timeouts.
    pub width: Millis,
    /// Cluster size.
    pub nodes: usize,
    /// Seeds run.
    pub seeds: usize,
    /// Ascending time of the first leader, one per seed that elected one before the deadline.
    pub first_leader: Vec<Millis>,
    /// Sum over the seeds that elected a leader of that leader's term (1 = no split vote).
    pub term_sum: u64,
    /// Sum over all seeds of the campaigns started before the deadline or the first leader.
    pub candidacies: u64,
}

impl LivenessSummary {
    /// Seeds that elected a leader before the deadline.
    pub fn elected(&self) -> usize {
        self.first_leader.len()
    }
}

/// Starts `nodes` nodes at the same instant with election timeouts uniform in
/// `[150, 150 + width)` ms (`width == 0`: identical timeouts), constant 10 ms latency, no client,
/// no faults, and records when (if ever) a first leader appears within `deadline` ms.
pub fn liveness(
    nodes: usize,
    width: Millis,
    seeds: std::ops::Range<u64>,
    deadline: Millis,
) -> LivenessSummary {
    let mut s = LivenessSummary {
        width,
        nodes,
        seeds: 0,
        first_leader: Vec::new(),
        term_sum: 0,
        candidacies: 0,
    };
    for seed in seeds {
        let mut cfg = Config::new(nodes);
        cfg.election_timeout = (150, 150 + width);
        cfg.latency = (10, 10);
        cfg.client_interval = 0;
        cfg.horizon = deadline;
        cfg.stop_at_first_leader = true;
        let o = simulate_checked(&cfg, seed);
        s.seeds += 1;
        s.candidacies += u64::from(o.candidacies);
        if let Some(t) = o.first_leader_at {
            s.first_leader.push(t);
            s.term_sum += o.first_leader_term;
        }
    }
    s.first_leader.sort_unstable();
    s
}

// ---------------------------------------------------------------------------------------------
// Figure 8 script and mutant hunting
// ---------------------------------------------------------------------------------------------

/// A scripted replay of the paper's Figure 8 on five nodes (`n0..n4` are the paper's `S1..S5`).
///
/// (a) S1 is leader of term 2 and puts index 2 only on S2; S1 crashes. (b) S5 wins term 3 with the
/// votes of S3 and S4 and writes a different entry at index 2 (nobody else sees it); S5 crashes.
/// (c) S1 restarts and wins term 4 with S2 and S3, which brings index 2 (term 2) to a majority,
/// but S1 has no entry of term 4 yet. The correct leader does **not** commit that old entry. (d)
/// S1 crashes, S5 restarts and wins term 5, overwriting index 2 with its own term-3 entry. With
/// [`Mutant::CommitOldTermByCounting`] S1 has already declared the term-2 entry committed, so a
/// committed entry is lost. The script uses forced election timeouts, so no election timer ever
/// fires on its own; message latency is random (seeded, at most 15 ms), so every round trip between two dependent
/// steps (steps are at least 40 ms apart) completes before the next step.
pub fn figure8_config(mutant: Option<Mutant>) -> Config {
    let (s1, s2, s3, s4, s5) = (0, 1, 2, 3, 4);
    let mut cfg = Config::new(5);
    cfg.election_timeout = (1_000_000, 1_000_001);
    cfg.client_interval = 0;
    cfg.horizon = 1600;
    cfg.mutant = mutant;
    cfg.stop_at_violation = false;
    cfg.leader_noop = false;
    cfg.record_trace = true;
    cfg.plan = vec![
        (100, Action::ForceTimeout(s1)),
        (200, Action::Propose(s1, 1)),
        (400, Action::Crash(s1)),
        (410, Action::Restart(s1)),
        (420, Action::ForceTimeout(s1)),
        (500, Action::Partition(vec![vec![s1, s2], vec![s3, s4, s5]])),
        (510, Action::Propose(s1, 2)),
        (560, Action::Crash(s1)),
        (580, Action::ForceTimeout(s5)),
        (
            620,
            Action::Partition(vec![vec![s1, s2], vec![s3, s4], vec![s5]]),
        ),
        (630, Action::Propose(s5, 3)),
        (650, Action::Crash(s5)),
        (700, Action::Restart(s1)),
        (
            700,
            Action::Partition(vec![vec![s1, s2, s3], vec![s4], vec![s5]]),
        ),
        (710, Action::ForceTimeout(s1)),
        (780, Action::ForceTimeout(s1)),
        (900, Action::Crash(s1)),
        (920, Action::Restart(s5)),
        (920, Action::Heal),
        (930, Action::ForceTimeout(s5)),
        (1020, Action::ForceTimeout(s5)),
        (1100, Action::Propose(s5, 5)),
        (1300, Action::Restart(s1)),
    ];
    cfg
}

/// Label of the fault pattern used to hunt `mutant` in [`hunt_config`].
pub fn hunt_label(mutant: Mutant) -> &'static str {
    match mutant {
        Mutant::NoPersistVote => "crash right after voting",
        Mutant::NoUpToDateCheck => "partitions",
        Mutant::MinorityLeader => "no faults",
        Mutant::CommitOldTermByCounting => "Figure 8 script",
        Mutant::NoPrevLogTermCheck => "combined",
    }
}

/// The configuration (mutant switched on) in which `mutant` is hunted for `seed`.
pub fn hunt_config(mutant: Mutant, seed: u64) -> Config {
    let mut cfg = match mutant {
        Mutant::NoPersistVote => {
            let mut c = Config::new(5);
            c.horizon = 1500;
            c.client_interval = 0;
            c.crash_after_vote_permille = 500;
            c.quick_restart = (1, 5);
            c
        }
        Mutant::NoUpToDateCheck => scenario_config(Scenario::Partitions, 5, 4000, seed),
        Mutant::MinorityLeader => scenario_config(Scenario::NoFaults, 5, 4000, seed),
        Mutant::CommitOldTermByCounting => figure8_config(None),
        Mutant::NoPrevLogTermCheck => scenario_config(Scenario::Combined, 5, 4000, seed),
    };
    cfg.mutant = Some(mutant);
    cfg.stop_at_violation = true;
    cfg
}

/// Result of hunting one mutant over a range of seeds.
#[derive(Clone, Debug)]
pub struct MutantReport {
    /// The mutant.
    pub mutant: Mutant,
    /// Fault pattern used (see [`hunt_label`]).
    pub label: &'static str,
    /// Seeds run.
    pub seeds: usize,
    /// Seeds in which the checker reported a violation.
    pub violating: usize,
    /// First violating seed and its first violation.
    pub first: Option<(u64, Violation)>,
    /// Violations of the correct algorithm on the very same seeds and faults (must be 0).
    pub correct_violations: usize,
}

/// Runs the mutant and, for contrast, the correct algorithm on every seed of `seeds`.
pub fn sweep_mutant(mutant: Mutant, seeds: std::ops::Range<u64>) -> MutantReport {
    let mut r = MutantReport {
        mutant,
        label: hunt_label(mutant),
        seeds: 0,
        violating: 0,
        first: None,
        correct_violations: 0,
    };
    for seed in seeds {
        let mut cfg = hunt_config(mutant, seed);
        let o = simulate(&cfg, seed);
        r.seeds += 1;
        if let Some(v) = o.violation {
            r.violating += 1;
            r.first.get_or_insert((seed, v));
        }
        cfg.mutant = None;
        if simulate(&cfg, seed).violation.is_some() {
            r.correct_violations += 1;
        }
    }
    r
}

/// The run behind the lab's election timeline: five nodes with the default timing; the leader is
/// crashed at t = 900 ms and restarted 500 ms later; the next leader is cut off from t = 1500 ms to
/// t = 2000 ms. The trace is recorded.
pub fn timeline(seed: u64) -> Outcome {
    let mut cfg = Config::new(5);
    cfg.horizon = 2600;
    cfg.record_trace = true;
    cfg.plan = vec![
        (900, Action::CrashLeaderFor(500)),
        (1500, Action::IsolateLeader),
        (2000, Action::Heal),
    ];
    simulate_checked(&cfg, seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(term: Term, cmd: u64) -> Entry {
        Entry { term, cmd }
    }

    /// A cluster where nothing happens by itself: no client, election timers never fire.
    fn still_cluster(n: usize, mutant: Option<Mutant>) -> Cluster {
        let mut cfg = Config::new(n);
        cfg.client_interval = 0;
        cfg.election_timeout = (1_000_000, 1_000_001);
        cfg.mutant = mutant;
        Cluster::new(cfg, 1)
    }

    /// Runs `call` and returns the single message it put on the wire.
    fn reply_of(c: &mut Cluster, call: impl FnOnce(&mut Cluster)) -> Msg {
        c.queue.retain(|_, ev| !matches!(ev, Event::Deliver { .. }));
        call(c);
        let mut msgs: Vec<Msg> = c
            .queue
            .values()
            .filter_map(|ev| match ev {
                Event::Deliver { msg, .. } => Some(msg.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(msgs.len(), 1, "a handler sends exactly one reply: {msgs:?}");
        msgs.remove(0)
    }

    fn args(
        term: Term,
        prev_index: usize,
        prev_term: Term,
        entries: &[Entry],
        leader_commit: usize,
    ) -> AppendArgs {
        AppendArgs {
            term,
            prev_index,
            prev_term,
            entries: entries.to_vec(),
            leader_commit,
        }
    }

    fn append(c: &mut Cluster, to: NodeId, a: AppendArgs) -> Msg {
        reply_of(c, |c| c.on_append(to, 0, &a))
    }

    fn granted(m: &Msg) -> bool {
        matches!(m, Msg::VoteReply { granted: true, .. })
    }

    // ----- RequestVote rules --------------------------------------------------------------

    #[test]
    fn majority_sizes_and_quorum_intersection() {
        assert_eq!(
            (
                majority(1),
                majority(3),
                majority(4),
                majority(5),
                majority(7)
            ),
            (1, 2, 3, 3, 4)
        );
        // any two majorities overlap in at least one node: this is what makes Election Safety work
        for n in 1..=MAX_NODES {
            assert!(2 * majority(n) > n, "n={n}");
        }
    }

    #[test]
    fn vote_is_granted_at_most_once_per_term() {
        let mut c = still_cluster(5, None);
        assert!(granted(
            &reply_of(&mut c, |c| c.on_request_vote(1, 0, 1, 0, 0))
        ));
        assert_eq!(c.voted_for(1), Some(0));
        // another candidate of the same term is refused, the first one may ask again (lost reply)
        assert!(!granted(
            &reply_of(&mut c, |c| c.on_request_vote(1, 2, 1, 0, 0))
        ));
        assert!(granted(
            &reply_of(&mut c, |c| c.on_request_vote(1, 0, 1, 0, 0))
        ));
        assert_eq!(c.voted_for(1), Some(0));
        // a stale term is refused and the reply carries our term
        let m = reply_of(&mut c, |c| c.on_request_vote(1, 3, 0, 0, 0));
        assert!(
            matches!(
                m,
                Msg::VoteReply {
                    term: 1,
                    granted: false
                }
            ),
            "{m:?}"
        );
        // a higher term resets votedFor, so the other candidate can now win our vote
        assert!(granted(
            &reply_of(&mut c, |c| c.on_request_vote(1, 2, 2, 0, 0))
        ));
        assert_eq!((c.term(1), c.voted_for(1)), (2, Some(2)));
    }

    #[test]
    fn up_to_date_rule_compares_last_term_first_then_length() {
        // The voter's log is [(index 1, term 1), (index 2, term 2)]: last entry (index 2, term 2).
        // Candidate (lastLogIndex, lastLogTerm) and whether the vote must be granted:
        let cases = [
            (1, 3, true),  // a later last term wins even with a shorter log
            (2, 2, true),  // identical logs
            (3, 2, true),  // same last term, longer
            (1, 2, false), // same last term, shorter
            (9, 1, false), // an older last term loses even with a much longer log
            (0, 0, false), // empty log
        ];
        for (idx, term, expect) in cases {
            for mutant in [None, Some(Mutant::NoUpToDateCheck)] {
                let mut c = still_cluster(5, mutant);
                c.nodes[1].log = vec![e(1, 1), e(2, 2)];
                c.nodes[1].term = 2;
                let m = reply_of(&mut c, |c| c.on_request_vote(1, 0, 5, idx, term));
                // the mutant grants every request: that is the bug
                let want = expect || mutant.is_some();
                assert_eq!(
                    granted(&m),
                    want,
                    "candidate last=({idx},{term}) mutant={mutant:?}"
                );
            }
        }
    }

    #[test]
    fn vote_survives_a_restart_unless_the_mutant_forgets_it() {
        for (mutant, second_candidate_gets_vote) in
            [(None, false), (Some(Mutant::NoPersistVote), true)]
        {
            let mut c = still_cluster(3, mutant);
            c.on_request_vote(1, 0, 1, 0, 0);
            assert_eq!(c.voted_for(1), Some(0));
            c.crash(1);
            c.restart(1);
            assert_eq!(c.term(1), 1, "currentTerm is persistent");
            let m = reply_of(&mut c, |c| c.on_request_vote(1, 2, 1, 0, 0));
            assert_eq!(granted(&m), second_candidate_gets_vote, "mutant={mutant:?}");
        }
    }

    // ----- AppendEntries rules -------------------------------------------------------------

    #[test]
    fn consistency_check_rejects_and_hints_where_to_back_off() {
        let mut c = still_cluster(3, None);
        c.nodes[1].log = vec![e(1, 1), e(1, 2), e(2, 3)];
        c.nodes[1].term = 2;
        // nothing at prevLogIndex 5: back off to len + 1
        let m = append(&mut c, 1, args(2, 5, 2, &[e(2, 9)], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: false,
                    hint: 4,
                    ..
                }
            ),
            "{m:?}"
        );
        // term mismatch at index 3 (follower: term 2): hint = first index of the follower's term 2
        let m = append(&mut c, 1, args(2, 3, 1, &[e(2, 9)], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: false,
                    hint: 3,
                    ..
                }
            ),
            "{m:?}"
        );
        // term mismatch at index 2 (follower: term 1): hint = first index of term 1
        let m = append(&mut c, 1, args(2, 2, 2, &[], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: false,
                    hint: 1,
                    ..
                }
            ),
            "{m:?}"
        );
        assert_eq!(
            c.log(1),
            &[e(1, 1), e(1, 2), e(2, 3)],
            "rejections change nothing"
        );
        // matching prevLogIndex/prevLogTerm and the empty prefix are accepted
        let m = append(&mut c, 1, args(2, 3, 2, &[], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: true,
                    match_index: 3,
                    ..
                }
            ),
            "{m:?}"
        );
        let m = append(&mut c, 1, args(2, 0, 0, &[], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: true,
                    match_index: 0,
                    ..
                }
            ),
            "{m:?}"
        );
    }

    #[test]
    fn conflicting_suffix_is_deleted_but_a_stale_duplicate_deletes_nothing() {
        let mut c = still_cluster(3, None);
        c.nodes[1].log = vec![e(1, 1), e(1, 2), e(1, 3)];
        c.nodes[1].term = 1;
        // A delayed AppendEntries that carries only entry 1 must not shorten the log.
        let m = append(&mut c, 1, args(1, 0, 0, &[e(1, 1)], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: true,
                    match_index: 1,
                    ..
                }
            ),
            "{m:?}"
        );
        assert_eq!((c.log(1).len(), c.overwritten()), (3, 0));
        // A new leader (term 2) whose entry 2 differs: entries 2 and 3 go, its entry is appended.
        let m = append(&mut c, 1, args(2, 1, 1, &[e(2, 9)], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    success: true,
                    match_index: 2,
                    ..
                }
            ),
            "{m:?}"
        );
        assert_eq!(c.log(1), &[e(1, 1), e(2, 9)]);
        assert_eq!(c.overwritten(), 2);
    }

    #[test]
    fn follower_commits_only_up_to_the_last_entry_the_leader_vouched_for() {
        let mut c = still_cluster(3, None);
        // Entries 2-3 come from an old leader and are unknown to the new one (term 3).
        c.nodes[1].log = vec![e(1, 1), e(2, 2), e(2, 3)];
        c.nodes[1].term = 3;
        // leaderCommit = 3 but the last entry this message verified is index 1.
        append(&mut c, 1, args(3, 1, 1, &[], 3));
        assert_eq!(
            c.commit_index(1),
            1,
            "min(leaderCommit, index of last NEW entry), not the log length"
        );
        // A stale heartbeat never moves commitIndex backwards.
        append(&mut c, 1, args(3, 0, 0, &[], 0));
        assert_eq!(c.commit_index(1), 1);
        // Once the leader replicates its own entry 2 (overwriting the old one) the follower commits it.
        append(&mut c, 1, args(3, 1, 1, &[e(3, 4)], 2));
        assert_eq!(c.log(1), &[e(1, 1), e(3, 4)]);
        assert_eq!((c.commit_index(1), c.last_applied(1)), (2, 2));
    }

    #[test]
    fn stale_terms_are_rejected_and_a_higher_term_deposes_a_leader_or_candidate() {
        let mut c = still_cluster(3, None);
        c.nodes[1].term = 5;
        let m = append(&mut c, 1, args(4, 0, 0, &[e(4, 1)], 0));
        assert!(
            matches!(
                m,
                Msg::AppendReply {
                    term: 5,
                    success: false,
                    ..
                }
            ),
            "{m:?}"
        );
        assert!(c.log(1).is_empty());
        // A leader of term 5 learns about term 7 from a reply and steps down.
        c.nodes[0].role = Role::Leader;
        c.nodes[0].term = 5;
        c.nodes[0].voted_for = Some(0);
        c.on_append_reply(0, 2, 7, false, 0, 0);
        assert_eq!(
            (c.role(0), c.term(0), c.voted_for(0)),
            (Role::Follower, 7, None)
        );
        // A candidate that hears a leader of the same term gives up.
        c.nodes[2].role = Role::Candidate;
        c.nodes[2].term = 3;
        append(&mut c, 2, args(3, 0, 0, &[], 0));
        assert_eq!(c.role(2), Role::Follower);
    }

    #[test]
    fn leader_counts_replicas_only_for_entries_of_its_own_term() {
        for (mutant, commit_of_old_entry) in [(None, 0), (Some(Mutant::CommitOldTermByCounting), 2)]
        {
            let mut c = still_cluster(5, mutant);
            let l = &mut c.nodes[0];
            l.role = Role::Leader;
            l.term = 4;
            l.log = vec![e(1, 1), e(2, 2)];
            l.match_index = vec![2, 2, 2, 0, 0];
            l.next_index = vec![3, 3, 3, 1, 1];
            // The term-2 entry is on 3 of 5 servers, but the leader is in term 4 (Figure 8).
            c.advance_commit(0);
            assert_eq!(c.commit_index(0), commit_of_old_entry, "mutant={mutant:?}");
            // An entry of the current term on a majority commits it and everything before it.
            c.nodes[0].log.push(e(4, 3));
            c.nodes[0].match_index = vec![3, 3, 0, 0, 0];
            c.advance_commit(0);
            assert_eq!(
                c.commit_index(0),
                commit_of_old_entry,
                "2 of 5 is not a majority"
            );
            c.nodes[0].match_index = vec![3, 3, 3, 0, 0];
            c.advance_commit(0);
            assert_eq!(c.commit_index(0), 3);
            assert!(c.violation().is_none());
        }
    }

    // ----- checker unit tests (crafted states) -----------------------------------------------

    fn node(term: Term, role: Role, log: &[Entry]) -> Node {
        let mut n = Node::new(Rng::new(0));
        n.term = term;
        n.role = role;
        n.log = log.to_vec();
        n
    }

    #[test]
    fn checker_election_safety_flags_two_leaders_in_one_term_only() {
        let nodes = vec![
            node(3, Role::Leader, &[]),
            node(3, Role::Leader, &[]),
            node(4, Role::Leader, &[]),
        ];
        let mut ch = Checker::new(3);
        ch.on_become_leader(0, &nodes, 10);
        ch.on_become_leader(2, &nodes, 11); // another term: allowed (stale leaders exist)
        assert!(ch.violations.is_empty());
        ch.on_become_leader(1, &nodes, 12);
        assert_eq!(ch.violations.len(), 1);
        assert_eq!(ch.violations[0].invariant, Invariant::ElectionSafety);
    }

    #[test]
    fn checker_leader_completeness_needs_every_entry_committed_in_an_earlier_term() {
        let committed = |ch: &mut Checker| {
            ch.committed.insert(1, (e(1, 1), 1));
            ch.committed.insert(2, (e(2, 2), 2));
        };
        // leader of term 3 lacking index 2: violation
        let mut ch = Checker::new(1);
        committed(&mut ch);
        ch.on_become_leader(0, &[node(3, Role::Leader, &[e(1, 1)])], 5);
        assert_eq!(ch.violations[0].invariant, Invariant::LeaderCompleteness);
        // a different entry at index 2 is just as bad
        let mut ch = Checker::new(1);
        committed(&mut ch);
        ch.on_become_leader(0, &[node(3, Role::Leader, &[e(1, 1), e(3, 9)])], 5);
        assert_eq!(ch.violations[0].invariant, Invariant::LeaderCompleteness);
        // holding both, or being the leader of the term that committed index 2: fine
        let mut ch = Checker::new(2);
        committed(&mut ch);
        ch.on_become_leader(0, &[node(3, Role::Leader, &[e(1, 1), e(2, 2)])], 5);
        ch.on_become_leader(
            1,
            &[
                node(0, Role::Follower, &[]),
                node(2, Role::Leader, &[e(1, 1)]),
            ],
            6,
        );
        assert!(ch.violations.is_empty(), "{:?}", ch.violations);
    }

    #[test]
    fn checker_state_machine_safety_flags_different_commands_at_one_index() {
        let mut ch = Checker::new(3);
        ch.on_apply(0, 1, e(1, 1), 1);
        ch.on_apply(1, 1, e(1, 1), 2);
        ch.on_apply(0, 1, e(1, 1), 3); // re-applying after a restart is fine
        ch.on_apply(2, 2, e(2, 7), 4);
        assert!(ch.violations.is_empty());
        ch.on_apply(1, 2, e(3, 8), 5);
        assert_eq!(ch.violations[0].invariant, Invariant::StateMachineSafety);
    }

    #[test]
    fn checker_log_matching_flags_same_index_and_term_with_different_history() {
        let check = |a: &[Entry], b: &[Entry]| {
            let mut nodes = vec![node(1, Role::Follower, a), node(1, Role::Follower, b)];
            nodes[0].dirty = true;
            let mut ch = Checker::new(2);
            ch.after_event(&mut nodes, 1);
            ch.violations.first().map(|v| v.invariant)
        };
        let ok = None;
        let bad = Some(Invariant::LogMatching);
        assert_eq!(check(&[e(1, 1), e(2, 2)], &[e(1, 1), e(2, 2), e(2, 3)]), ok);
        assert_eq!(
            check(&[e(1, 1), e(2, 2)], &[e(1, 1), e(3, 3)]),
            ok,
            "different terms at index 2: no promise"
        );
        assert_eq!(
            check(&[e(1, 1), e(2, 2)], &[e(1, 9), e(2, 2)]),
            bad,
            "same (index 2, term 2), different index 1"
        );
        assert_eq!(
            check(&[e(1, 1), e(2, 2)], &[e(1, 1), e(2, 9)]),
            bad,
            "same (index, term), different command"
        );
        assert_eq!(
            check(&[e(1, 1), e(1, 2)], &[e(2, 5), e(1, 2)]),
            bad,
            "same (index 2, term 1), different index 1"
        );
        assert_eq!(check(&[], &[e(1, 1)]), ok);
    }

    #[test]
    fn checker_leader_append_only_flags_rewritten_or_truncated_leader_logs() {
        for (mutate, bad) in [
            (0, false), // append only
            (1, true),  // overwrite an entry
            (2, true),  // truncate
        ] {
            let mut nodes = vec![
                node(2, Role::Leader, &[e(1, 1), e(2, 2)]),
                node(2, Role::Follower, &[]),
            ];
            let mut ch = Checker::new(2);
            ch.on_become_leader(0, &nodes, 1);
            match mutate {
                0 => nodes[0].log.push(e(2, 3)),
                1 => nodes[0].log[1] = e(2, 9),
                _ => nodes[0].log.truncate(1),
            }
            nodes[0].dirty = true;
            ch.after_event(&mut nodes, 2);
            let flagged = ch
                .violations
                .iter()
                .any(|v| v.invariant == Invariant::LeaderAppendOnly);
            assert_eq!(flagged, bad, "mutate={mutate}");
        }
        // a node that has stopped being leader may have its log rewritten (that is how followers are repaired)
        let mut nodes = vec![node(2, Role::Leader, &[e(1, 1), e(2, 2)])];
        let mut ch = Checker::new(1);
        ch.on_become_leader(0, &nodes, 1);
        nodes[0].role = Role::Follower;
        nodes[0].log[1] = e(3, 4);
        nodes[0].dirty = true;
        ch.after_event(&mut nodes, 2);
        assert!(ch.violations.is_empty());
    }

    // ----- whole-cluster behaviour -------------------------------------------------------------

    #[test]
    fn percentile_is_nearest_rank() {
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(
            (percentile(&v, 50), percentile(&v, 95), percentile(&v, 100)),
            (Some(50), Some(95), Some(100))
        );
        assert_eq!(percentile(&[7], 95), Some(7));
        assert_eq!(percentile(&[], 50), None);
    }

    #[test]
    fn fault_free_cluster_elects_one_stable_leader_and_commits() {
        for seed in 0..200 {
            let o = simulate_checked(&scenario_config(Scenario::NoFaults, 5, 4000, seed), seed);
            let first = o.first_leader_at.expect("a leader");
            // earliest possible: the smallest election timeout (150) plus a round trip at minimum latency
            assert!(first >= 150 + 2 * 5, "seed {seed}: leader at {first} ms");
            // heartbeats every 50 ms keep every follower below its >= 150 ms timeout: no re-election
            assert_eq!(o.elections, 1, "seed {seed}");
            assert!(
                o.converged && o.committed >= 20,
                "seed {seed}: committed {}",
                o.committed
            );
        }
    }

    #[test]
    fn all_five_invariants_hold_in_every_scenario_and_the_cluster_recovers() {
        for scenario in Scenario::ALL {
            for (nodes, seeds) in [(5usize, 200u64), (3, 100)] {
                for seed in 0..seeds {
                    let o = simulate_checked(&scenario_config(scenario, nodes, 4000, seed), seed);
                    assert!(
                        o.converged,
                        "{scenario:?} n={nodes} seed {seed}: not converged after healing"
                    );
                    assert!(
                        o.committed > 0,
                        "{scenario:?} n={nodes} seed {seed}: nothing committed"
                    );
                }
            }
        }
    }

    /// The no-op is a liveness aid (paper section 8), not a safety requirement: without it the five
    /// properties must still hold, and entries of old terms simply wait for the next client command.
    #[test]
    fn invariants_hold_without_the_leader_noop_too() {
        for scenario in [Scenario::Partitions, Scenario::Combined] {
            for seed in 0..100 {
                let mut cfg = scenario_config(scenario, 5, 4000, seed);
                cfg.leader_noop = false;
                simulate_checked(&cfg, seed);
            }
        }
    }

    /// A random configuration: 3 to 7 nodes, any scenario, election timeouts even far below the
    /// latency, latencies up to 180 ms, loss up to 40 %, duplication up to 20 %, tiny batches...
    fn fuzz_config(seed: u64, horizon: Millis) -> Config {
        let mut r = Rng::new(seed ^ 0xABCDEF);
        let nodes = *r.choose(&[3usize, 4, 5, 6, 7]).unwrap();
        let scenario = *r.choose(&Scenario::ALL).unwrap();
        let mut cfg = scenario_config(scenario, nodes, horizon, seed);
        let lo = r.range(5, 200);
        cfg.election_timeout = (lo, lo + r.below(250));
        cfg.heartbeat = r.range(10, 120);
        let l0 = r.range(1, 60);
        cfg.latency = (l0, l0 + r.below(120));
        if r.chance(0.5) {
            cfg.drop_permille = r.below(400) as u32;
        }
        if r.chance(0.5) {
            cfg.dup_permille = r.below(200) as u32;
        }
        cfg.max_batch = r.range(1, 17) as usize;
        cfg.client_interval = r.range(10, 200);
        cfg.max_down = r.below(nodes as u64) as usize;
        cfg
    }

    /// Safety must not depend on timing (paper section 5.6): the five properties hold for any timeout,
    /// latency, loss, duplication, batch size and fault plan; only availability suffers.
    #[test]
    fn safety_does_not_depend_on_timing() {
        for seed in 0..250u64 {
            simulate_checked(&fuzz_config(seed, 3000), seed);
        }
    }

    /// Long soak (about a minute in release): `cargo test --release -p lab-distributed -- --ignored raft`.
    #[test]
    #[ignore = "soak test: about a minute in release mode"]
    fn soak_100k_random_configurations_and_20k_seeds_per_scenario() {
        for seed in 0..100_000u64 {
            let o = simulate_checked(&fuzz_config(seed, 4000), seed);
            assert!(
                o.events < 1_000_000,
                "seed {seed}: runaway message storm ({} events)",
                o.events
            );
        }
        for scenario in Scenario::ALL {
            for nodes in [3, 5] {
                let s = sweep(scenario, nodes, 4000, 0..20_000);
                assert_eq!(
                    (s.violations, s.converged),
                    (0, 20_000),
                    "{scenario:?} n={nodes}"
                );
            }
        }
    }

    /// Cross-check of the online checker (which only re-examines logs that changed) against a brute
    /// force check of the whole cluster state every 5 ms of virtual time.
    #[test]
    fn online_checks_agree_with_a_brute_force_scan_of_the_whole_state() {
        let log_matching_broken = |c: &Cluster| {
            let n = c.nodes.len();
            (0..n).any(|i| {
                (i + 1..n)
                    .any(|j| log_matching_violation(&c.nodes[i].log, &c.nodes[j].log).is_some())
            })
        };
        let leader_completeness_broken = |c: &Cluster| {
            c.nodes
                .iter()
                .filter(|n| n.alive && n.role == Role::Leader)
                .any(|l| {
                    c.checker
                        .committed
                        .iter()
                        .any(|(&idx, &(e, cterm))| cterm < l.term && l.log.get(idx - 1) != Some(&e))
                })
        };
        // correct algorithm: both properties hold at every checkpoint, not only at elections
        for seed in 0..40 {
            let mut c = Cluster::new(scenario_config(Scenario::Combined, 5, 4000, seed), seed);
            for t in (5..=4000).step_by(5) {
                c.run_until(t);
                assert!(
                    !log_matching_broken(&c) && !leader_completeness_broken(&c),
                    "seed {seed} t={t}"
                );
            }
            assert!(c.violation().is_none(), "seed {seed}");
        }
        // broken algorithms: whenever the brute force sees the damage, the online checker already has
        let mut seen = 0;
        for (m, seeds) in [
            (Mutant::NoPrevLogTermCheck, 0..30),
            (Mutant::NoUpToDateCheck, 0..30),
        ] {
            for seed in seeds {
                let mut cfg = hunt_config(m, seed);
                cfg.stop_at_violation = false;
                let mut c = Cluster::new(cfg, seed);
                for t in (5..=4000).step_by(5) {
                    c.run_until(t);
                    let kinds: Vec<Invariant> =
                        c.outcome().violations.iter().map(|v| v.invariant).collect();
                    if log_matching_broken(&c) {
                        assert!(
                            kinds.contains(&Invariant::LogMatching),
                            "{m:?} seed {seed} t={t}: {kinds:?}"
                        );
                        seen += 1;
                    }
                    if leader_completeness_broken(&c) {
                        assert!(
                            kinds.contains(&Invariant::LeaderCompleteness),
                            "{m:?} seed {seed} t={t}: {kinds:?}"
                        );
                        seen += 1;
                    }
                }
            }
        }
        assert!(
            seen > 0,
            "the brute force scan must have seen some damage for this to mean anything"
        );
    }

    #[test]
    fn same_seed_same_trace_and_instrumentation_does_not_perturb_it() {
        let cfg = scenario_config(Scenario::Combined, 5, 4000, 17);
        let a = simulate(&cfg, 17);
        let b = simulate(&cfg, 17);
        assert_eq!(
            (a.trace_hash, a.events, a.committed, a.max_term),
            (b.trace_hash, b.events, b.committed, b.max_term)
        );
        let mut traced = cfg.clone();
        traced.record_trace = true;
        let c = simulate(&traced, 17);
        assert_eq!(
            a.trace_hash, c.trace_hash,
            "recording a trace must not change the run"
        );
        assert!(!c.trace.is_empty());
        let hashes: std::collections::BTreeSet<u64> = (0..30)
            .map(|s| simulate(&scenario_config(Scenario::Combined, 5, 4000, s), s).trace_hash)
            .collect();
        assert_eq!(hashes.len(), 30, "different seeds give different runs");
    }

    #[test]
    fn progress_needs_a_majority_up() {
        let cases = [
            (1, 0, true),
            (2, 0, true),
            (2, 1, false),
            (3, 1, true),
            (3, 2, false),
        ];
        for (n, down, live) in
            cases
                .into_iter()
                .chain([(5, 2, true), (5, 3, false), (7, 3, true), (7, 4, false)])
        {
            let mut cfg = Config::new(n);
            cfg.horizon = 3000;
            cfg.plan = (0..down).map(|i| (0, Action::Crash(i))).collect();
            let mut c = Cluster::new(cfg, 7);
            c.run_until(3000);
            assert!(c.violation().is_none());
            if live {
                assert!(
                    c.leader().is_some() && c.committed_len() > 10,
                    "n={n} down={down}"
                );
            } else {
                // survivors keep campaigning in ever higher terms but can never win
                assert!(
                    c.leader().is_none() && c.committed_len() == 0,
                    "n={n} down={down}"
                );
                assert!(
                    (down..n).all(|i| c.term(i) >= 5),
                    "n={n}: terms {:?}",
                    (down..n).map(|i| c.term(i)).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn leader_cut_off_keeps_accepting_commands_that_are_overwritten_after_the_heal() {
        for seed in 0..60 {
            let mut cfg = Config::new(5);
            cfg.horizon = 3000;
            cfg.plan = vec![
                (1000, Action::IsolateLeader),
                (1700, Action::Heal),
                (2300, Action::StopClient),
            ];
            let mut c = Cluster::new(cfg, seed);
            c.run_until(999);
            let old = c.leader().expect("a leader long before the cut");
            let committed_before = c.committed_len();
            c.run_until(1690);
            // Raft has no way for the cut-off leader to know: it still believes it leads and stores
            // client commands that can never reach a majority.
            assert_eq!(c.role(old), Role::Leader, "seed {seed}");
            assert!(
                c.log(old).len() > c.commit_index(old),
                "seed {seed}: old leader holds uncommitted entries"
            );
            let doomed: Vec<Entry> = c.log(old)[c.commit_index(old)..].to_vec();
            let new = c.leader().expect("the majority side elected a leader");
            assert!(new != old && c.term(new) > c.term(old), "seed {seed}");
            c.run_until(3000);
            assert!(c.violation().is_none() && c.converged(), "seed {seed}");
            // The doomed entries are gone from every log; what was committed before the cut survives.
            assert!(
                doomed.iter().all(|d| !c.log(old).contains(d)),
                "seed {seed}"
            );
            assert!(c.overwritten() >= doomed.len(), "seed {seed}");
            assert!(c.log(old).len() >= committed_before && c.committed_len() >= committed_before);
        }
    }

    #[test]
    fn no_election_completes_sooner_than_the_timeout_bound_after_a_leader_is_lost() {
        // Followers last heard the leader at most one heartbeat (50 ms) before it was lost, then wait
        // >= 150 ms, then need a round trip of >= 2 * 5 ms: no new leader before 150 + 10 - 50 ms.
        // (Needs a loss-free network: a dropped heartbeat lets the followers' timers run longer.)
        let mut n = 0;
        for scenario in [Scenario::Crashes, Scenario::Partitions] {
            let s = sweep(scenario, 5, 4000, 0..100);
            n += s.recoveries.len();
            assert_eq!(s.violations, 0);
            assert!(
                s.recoveries.iter().all(|&r| r >= 110),
                "{scenario:?}: min recovery {:?}",
                s.recoveries.first()
            );
        }
        assert!(n > 100, "the scenarios did lose leaders: {n}");
    }

    /// A sweep with zero violations only means something if the interesting paths were taken.
    #[test]
    fn sweeps_really_exercise_elections_overwrites_and_split_votes() {
        let quiet = sweep(Scenario::NoFaults, 5, 4000, 0..100);
        let crashes = sweep(Scenario::Crashes, 5, 4000, 0..100);
        let parts = sweep(Scenario::Partitions, 5, 4000, 0..100);
        let combined = sweep(Scenario::Combined, 5, 4000, 0..100);
        assert_eq!(quiet.elections, 100, "one election per fault-free run");
        assert!(
            quiet.candidacies > quiet.elections,
            "split votes / simultaneous candidates do happen"
        );
        assert!(
            crashes.elections > 300,
            "leaders crash and are replaced: {}",
            crashes.elections
        );
        assert!(
            parts.overwritten >= 300,
            "cut-off leaders' uncommitted entries get overwritten: {}",
            parts.overwritten
        );
        assert!(
            parts.max_term >= 8 && combined.max_term >= 8,
            "terms climb during partitions"
        );
        assert!(combined.overwritten > 0 && combined.recoveries.len() > 100);
    }

    // ----- liveness: randomized vs identical timeouts -------------------------------------------

    #[test]
    fn identical_timeouts_split_the_vote_forever_randomized_ones_elect() {
        let fixed = liveness(5, 0, 0..50, 2000);
        assert_eq!(
            fixed.elected(),
            0,
            "all five nodes time out together, vote for themselves, repeat"
        );
        // exactly floor(2000 / 150) = 13 rounds of 5 simultaneous candidates in every seed
        assert_eq!(fixed.candidacies, 50 * 13 * 5);
        let random = liveness(5, 150, 0..50, 2000);
        assert_eq!(random.elected(), 50);
        assert!(percentile(&random.first_leader, 95).unwrap() < 400);
    }

    #[test]
    fn more_timeout_randomness_never_hurts_the_chance_of_electing_a_leader() {
        let elected: Vec<usize> = [0, 5, 10, 25, 50, 150]
            .iter()
            .map(|&w| liveness(5, w, 0..100, 2000).elected())
            .collect();
        assert!(elected.windows(2).all(|w| w[0] <= w[1]), "{elected:?}");
        assert_eq!((elected[0], elected[5]), (0, 100));
        assert!(
            elected[1] < 100,
            "5 ms of randomness is below the 10 ms latency: still split votes"
        );
    }

    // ----- mutants: the checker bites, the correct algorithm is clean on the same seeds --------

    /// `seed` must be the *first* seed (counting from 0) at which `m` is caught, with violation `expect`.
    fn assert_caught(m: Mutant, seed: u64, expect: Invariant) {
        for earlier in 0..seed {
            let cfg = hunt_config(m, earlier);
            assert!(
                simulate(&cfg, earlier).violation.is_none(),
                "{m:?}: seed {earlier} is caught earlier than {seed}"
            );
        }
        let cfg = hunt_config(m, seed);
        let o = simulate(&cfg, seed);
        let v = o
            .violation
            .unwrap_or_else(|| panic!("{m:?} not caught at seed {seed}"));
        assert_eq!(v.invariant, expect, "{m:?} seed {seed}: {v}");
        let mut correct = cfg;
        correct.mutant = None;
        assert!(
            simulate(&correct, seed).violation.is_none(),
            "{m:?} seed {seed}: correct algorithm must be clean"
        );
    }

    #[test]
    fn no_persist_vote_gives_two_leaders_in_one_term() {
        assert_caught(Mutant::NoPersistVote, 5, Invariant::ElectionSafety);
    }

    #[test]
    fn no_up_to_date_check_elects_a_leader_that_lacks_committed_entries() {
        assert_caught(Mutant::NoUpToDateCheck, 1, Invariant::LeaderCompleteness);
    }

    #[test]
    fn minority_leader_gives_two_leaders_in_one_term() {
        assert_caught(Mutant::MinorityLeader, 7, Invariant::ElectionSafety);
    }

    #[test]
    fn skipping_the_prev_log_term_check_lets_divergent_followers_apply_wrong_entries() {
        assert_caught(Mutant::NoPrevLogTermCheck, 1, Invariant::StateMachineSafety);
        // Let the run go on: the divergent logs also break Log Matching (the wrong entry is applied
        // first because commitIndex follows leaderCommit, the mismatch shows up once entries are appended).
        let mut cfg = hunt_config(Mutant::NoPrevLogTermCheck, 1);
        cfg.stop_at_violation = false;
        let kinds: Vec<Invariant> = simulate(&cfg, 1)
            .violations
            .iter()
            .map(|v| v.invariant)
            .collect();
        assert!(
            kinds.contains(&Invariant::StateMachineSafety)
                && kinds.contains(&Invariant::LogMatching),
            "{kinds:?}"
        );
    }

    #[test]
    fn mutants_are_caught_by_sweeps_and_the_correct_algorithm_never_is() {
        for m in Mutant::ALL {
            let r = sweep_mutant(m, 0..120);
            assert!(
                r.violating >= 3,
                "{m:?}: only {} of {} seeds caught",
                r.violating,
                r.seeds
            );
            assert_eq!(r.correct_violations, 0, "{m:?}");
        }
        // Two bugs that random faults do not find: a crash must fall between two votes of one term
        // (NoPersistVote), or a very specific sequence of leaders is needed (Figure 8).
        for m in [Mutant::NoPersistVote, Mutant::CommitOldTermByCounting] {
            for scenario in [Scenario::Crashes, Scenario::Combined] {
                for seed in 0..120 {
                    let mut cfg = scenario_config(scenario, 5, 4000, seed);
                    cfg.mutant = Some(m);
                    assert!(
                        simulate(&cfg, seed).violation.is_none(),
                        "{m:?} {scenario:?} seed {seed}"
                    );
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "(seed 7, 5 nodes, mutant Some(MinorityLeader)): Election Safety")]
    fn checked_mode_panics_naming_the_invariant_and_the_seed() {
        simulate_checked(&hunt_config(Mutant::MinorityLeader, 7), 7);
    }

    // ----- Figure 8 -----------------------------------------------------------------------------

    fn final_logs(c: &Cluster) -> Vec<Vec<(Term, u64)>> {
        (0..5)
            .map(|i| c.log(i).iter().map(|x| (x.term, x.cmd)).collect())
            .collect()
    }

    #[test]
    fn figure_8_old_term_entry_is_overwritten_unless_the_mutant_committed_it() {
        for seed in 0..40 {
            // correct leader: index 2 (term 2) is on a majority at term 4 but is NOT committed
            let mut c = Cluster::new(figure8_config(None), seed);
            c.run_until(880);
            assert_eq!(
                c.role(0),
                Role::Leader,
                "seed {seed}: S1 is leader of term 4 ..."
            );
            assert_eq!(c.term(0), 4);
            assert!(
                c.log(1).len() >= 2 && c.log(2).len() >= 2,
                "... index 2 reached S2 and S3"
            );
            assert_eq!(
                c.committed_len(),
                1,
                "seed {seed}: the term-2 entry is not committed by counting"
            );
            c.run_until(1600);
            assert!(c.violation().is_none(), "seed {seed}");
            // S5's term-3 entry replaced it everywhere, then S5's term-5 entry committed both
            let tail = vec![(1, 1), (3, 3), (5, 5)];
            assert!(
                final_logs(&c).iter().all(|l| *l == tail),
                "seed {seed}: {:?}",
                final_logs(&c)
            );
            assert!(c.converged());
            assert_eq!(
                c.overwritten(),
                3,
                "S2, S3 and the restarted S1 each lose the term-2 entry"
            );

            // the mutant commits the old-term entry at term 4: committed data is lost
            let mut m = Cluster::new(figure8_config(Some(Mutant::CommitOldTermByCounting)), seed);
            m.run_until(1600);
            let kinds: Vec<Invariant> =
                m.outcome().violations.iter().map(|v| v.invariant).collect();
            assert_eq!(
                kinds,
                [Invariant::LeaderCompleteness, Invariant::StateMachineSafety],
                "seed {seed}"
            );
            let first = &m.outcome().violations[0];
            assert!(
                first
                    .detail
                    .contains("lacks index 2 (term 2, cmd 2) committed in term 4"),
                "{first}"
            );
        }
    }

    #[test]
    fn figure_8_leader_commits_the_old_entry_only_together_with_one_of_its_own_term() {
        // S1 (n0), leader of term 4: log [(1,1),(2,2)] on S1, S2, S3. Add a term-4 entry replicated on
        // a majority and the old entry is committed *indirectly*.
        let mut cfg = figure8_config(None);
        cfg.plan.retain(|(t, _)| *t < 900); // S1 does not crash in this variant
        let mut c = Cluster::new(cfg, 3);
        c.run_until(880);
        assert_eq!(
            (c.role(0), c.term(0), c.commit_index(0)),
            (Role::Leader, 4, 0)
        );
        c.schedule_action(c.now(), Action::Propose(0, 4));
        c.run_until(c.now() + 100);
        assert_eq!(c.log(0), &[e(1, 1), e(2, 2), e(4, 4)]);
        assert_eq!(
            c.commit_index(0),
            3,
            "index 3 (term 4) on a majority commits index 2 as well"
        );
        assert!(c.violation().is_none());
    }
}
