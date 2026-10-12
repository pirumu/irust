//! A *model* of three documented Redis behaviours, in virtual microseconds. It is not Redis and not
//! a protocol implementation: every cost below is an invented model parameter.
//!
//! 1. **Head-of-line blocking** — a single-threaded server runs one request at a time, so one slow
//!    command (`KEYS *`, a big `DEL`) delays every small `GET` queued behind it; `SCAN` in batches and
//!    `UNLINK` keep the loop short ([`scenario1`]).
//! 2. **Where the atomic boundary is** — `GET` + `SET` as two requests loses updates; `INCR`,
//!    `WATCH`/`MULTI`/`EXEC` + retry and a one-command script do not ([`scenario2`]).
//! 3. **Asynchronous replication + `WAIT`** — acknowledged writes missing on the promoted replica
//!    after a master crash, for each `WAIT` level and replica-selection policy ([`failover_sweep`]).
//!
//! Everything is a pure function of the seed (`crate::rng::Rng`), with integer virtual time.

use crate::rng::Rng;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};
use std::fmt::Write as _;

// ---------------------------------------------------------------------------------------------
// Cost table (model parameters, µs of virtual time spent on the single server thread)
// ---------------------------------------------------------------------------------------------

/// `GET` of one key.
pub const COST_GET_US: u64 = 2;
/// `SET` of one key.
pub const COST_SET_US: u64 = 2;
/// `INCR` of one key.
pub const COST_INCR_US: u64 = 2;
/// `MULTI`, `WATCH` or a command that is only queued inside `MULTI`.
pub const COST_QUEUE_US: u64 = 1;
/// Fixed part of `EXEC` (the queued commands add their own cost).
pub const COST_EXEC_BASE_US: u64 = 1;
/// A short script (read + compute + write in one request).
pub const COST_EVAL_US: u64 = 4;
/// Fixed part of `DEL`/`UNLINK`.
pub const COST_DEL_BASE_US: u64 = 2;
/// `DEL` of a collection: µs per 1000 elements (100 ns per element).
pub const COST_DEL_PER_1000_ELEMS_US: u64 = 100;
/// `KEYS *`: µs per 1000 keys in the database (40 ns per key; the `KEYS` doc says a 1 M key
/// database is scanned in 40 ms on an entry-level laptop, which is what this constant reproduces).
pub const COST_KEYS_PER_1000_KEYS_US: u64 = 40;
/// Fixed part of one `SCAN` call.
pub const COST_SCAN_BASE_US: u64 = 5;
/// `SCAN`: µs per 1000 keys walked (same per-key cost as `KEYS`).
pub const COST_SCAN_PER_1000_KEYS_US: u64 = 40;
/// `UNLINK` only reclaims in the background when the value has more than this many elements;
/// smaller values are freed synchronously (Redis 7.2.5 `LAZYFREE_THRESHOLD`).
pub const LAZYFREE_THRESHOLD: u64 = 64;
/// Network round trip between a client's reply and its next request, drawn uniformly (µs).
pub const NET_RTT_US: (u64, u64) = (100, 300);

// ---------------------------------------------------------------------------------------------
// Store, commands, server
// ---------------------------------------------------------------------------------------------

/// A stored value. A collection is only a size: that is all the cost model needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A string holding an integer.
    Int(i64),
    /// A list/set/hash with this many elements.
    Collection(u64),
}

/// One primitive operation applied to the store (the "trace" used by tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Request sequence number: every command of one `EXEC` or script shares the same unit.
    pub unit: u64,
    /// Client connection that caused the operation.
    pub client: usize,
    /// Operation name (`GET`, `SET`, `INCR`, `DEL`, `UNLINK`, `KEYS`, `SCAN`).
    pub op: &'static str,
}

/// The keyspace. `filler` keys exist only as a count (they are costed and scanned but never read),
/// so a 1 M key database does not need 1 M allocations.
#[derive(Clone, Debug, Default)]
pub struct Store {
    map: BTreeMap<String, Value>,
    versions: BTreeMap<String, u64>,
    filler: u64,
    cur_unit: u64,
    cur_client: usize,
    /// Every primitive operation applied, in order.
    pub log: Vec<LogEntry>,
}

impl Store {
    /// Total number of keys (real + filler).
    pub fn dbsize(&self) -> u64 {
        self.map.len() as u64 + self.filler
    }

    /// Modification counter of `key` (what `WATCH` compares).
    pub fn version(&self, key: &str) -> u64 {
        self.versions.get(key).copied().unwrap_or(0)
    }

    fn touch(&mut self, key: &str) {
        *self.versions.entry(key.to_string()).or_insert(0) += 1;
    }

    fn log_op(&mut self, op: &'static str) {
        self.log.push(LogEntry {
            unit: self.cur_unit,
            client: self.cur_client,
            op,
        });
    }

    /// `GET`: the integer, `Nil` if absent, or a type error for a collection.
    pub fn get(&mut self, key: &str) -> Reply {
        self.log_op("GET");
        match self.map.get(key) {
            None => Reply::Nil,
            Some(Value::Int(v)) => Reply::Int(*v),
            Some(Value::Collection(_)) => Reply::Err("WRONGTYPE"),
        }
    }

    /// `SET`: overwrites whatever was there.
    pub fn set(&mut self, key: &str, v: i64) -> Reply {
        self.log_op("SET");
        self.map.insert(key.to_string(), Value::Int(v));
        self.touch(key);
        Reply::Ok
    }

    /// `INCR`: a missing key counts as 0.
    pub fn incr(&mut self, key: &str) -> Reply {
        self.log_op("INCR");
        let cur = match self.map.get(key) {
            None => 0,
            Some(Value::Int(v)) => *v,
            Some(Value::Collection(_)) => return Reply::Err("WRONGTYPE"),
        };
        self.map.insert(key.to_string(), Value::Int(cur + 1));
        self.touch(key);
        Reply::Int(cur + 1)
    }

    fn remove(&mut self, op: &'static str, key: &str) -> Option<Value> {
        self.log_op(op);
        let old = self.map.remove(key);
        if old.is_some() {
            self.touch(key);
        }
        old
    }
}

/// A server command. `Multi`/`Watch`/`Exec` are per-connection state commands.
#[derive(Clone, Debug)]
pub enum Command {
    /// `GET key`.
    Get(String),
    /// `SET key value`.
    Set(String, i64),
    /// `INCR key`.
    Incr(String),
    /// `DEL key` (frees the value in the event loop).
    Del(String),
    /// `UNLINK key` (frees large values off the loop).
    Unlink(String),
    /// `KEYS *`: one O(N) pass over the whole keyspace.
    Keys,
    /// `SCAN cursor COUNT count`.
    Scan {
        /// Position to continue from (0 = start).
        cursor: u64,
        /// Work hint: how many keys to walk in this call.
        count: u64,
    },
    /// `WATCH key`.
    Watch(String),
    /// `MULTI`.
    Multi,
    /// `EXEC`.
    Exec,
    /// `EVAL`: a script that runs to completion as one request.
    Eval {
        /// The key the script touches (Redis requires keys to be declared).
        key: String,
        /// The script body, run with exclusive access to the store.
        script: Script,
    },
}

/// A script body: runs with exclusive access to the store.
pub type Script = fn(&mut Store, &str) -> Reply;

/// Read-compute-write in one request: the read-modify-write that the client cannot make atomic
/// with two commands.
pub fn script_incr(store: &mut Store, key: &str) -> Reply {
    let v = match store.get(key) {
        Reply::Int(v) => v,
        Reply::Nil => 0,
        err => return err,
    };
    store.set(key, v + 1);
    Reply::Int(v + 1)
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Command::Get(_) => "GET",
            Command::Set(..) => "SET",
            Command::Incr(_) => "INCR",
            Command::Del(_) => "DEL",
            Command::Unlink(_) => "UNLINK",
            Command::Keys => "KEYS",
            Command::Scan { .. } => "SCAN",
            Command::Watch(_) => "WATCH",
            Command::Multi => "MULTI",
            Command::Exec => "EXEC",
            Command::Eval { .. } => "EVAL",
        }
    }
}

/// A server reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// Absent key.
    Nil,
    /// Integer value.
    Int(i64),
    /// Plain `OK`.
    Ok,
    /// Command accepted into the `MULTI` queue.
    Queued,
    /// A count (keys matched, keys removed).
    Count(u64),
    /// One `SCAN` page: the next cursor (0 = done) and how many keys it covered.
    Scan {
        /// Cursor to pass to the next call; 0 when the walk is complete.
        next: u64,
        /// Keys covered by this call.
        returned: u64,
    },
    /// `EXEC` ran: one reply per queued command.
    Exec(Vec<Reply>),
    /// `EXEC` aborted because a watched key changed (a null array in real Redis).
    ExecAborted,
    /// An error.
    Err(&'static str),
}

#[derive(Debug, Default)]
struct Conn {
    queue: Option<Vec<Command>>,
    watched: BTreeMap<String, u64>,
}

/// The single-threaded server: executes one command at a time, each atomically.
#[derive(Debug)]
pub struct Server {
    /// The data.
    pub store: Store,
    conns: BTreeMap<usize, Conn>,
    next_unit: u64,
    /// Elements reclaimed by `UNLINK` on a background thread (not charged to the loop).
    pub background_freed: u64,
    /// `EXEC`s aborted by `WATCH`.
    pub watch_aborts: u64,
}

impl Server {
    /// A server over `store`.
    pub fn new(store: Store) -> Self {
        Self {
            store,
            conns: BTreeMap::new(),
            next_unit: 0,
            background_freed: 0,
            watch_aborts: 0,
        }
    }

    /// Executes one request from `client` and returns the reply and its cost in µs.
    pub fn handle(&mut self, client: usize, cmd: Command) -> (Reply, u64) {
        self.next_unit += 1;
        self.store.cur_unit = self.next_unit;
        self.store.cur_client = client;
        let conn = self.conns.entry(client).or_default();
        match cmd {
            Command::Multi => {
                if conn.queue.is_some() {
                    return (
                        Reply::Err("ERR MULTI calls can not be nested"),
                        COST_QUEUE_US,
                    );
                }
                conn.queue = Some(Vec::new());
                (Reply::Ok, COST_QUEUE_US)
            }
            Command::Watch(key) => {
                if conn.queue.is_some() {
                    return (
                        Reply::Err("ERR WATCH inside MULTI is not allowed"),
                        COST_QUEUE_US,
                    );
                }
                let v = self.store.version(&key);
                conn.watched.insert(key, v);
                (Reply::Ok, COST_QUEUE_US)
            }
            Command::Exec => {
                let Some(queue) = conn.queue.take() else {
                    return (Reply::Err("ERR EXEC without MULTI"), COST_QUEUE_US);
                };
                let watched = std::mem::take(&mut conn.watched);
                if watched.iter().any(|(k, v)| self.store.version(k) != *v) {
                    self.watch_aborts += 1;
                    return (Reply::ExecAborted, COST_EXEC_BASE_US);
                }
                let mut cost = COST_EXEC_BASE_US;
                let mut replies = Vec::with_capacity(queue.len());
                for c in queue {
                    let (r, cc) = self.apply(c);
                    cost += cc;
                    replies.push(r);
                }
                (Reply::Exec(replies), cost)
            }
            other => {
                if let Some(queue) = conn.queue.as_mut() {
                    queue.push(other);
                    return (Reply::Queued, COST_QUEUE_US);
                }
                self.apply(other)
            }
        }
    }

    /// Runs a plain (non-connection-state) command to completion.
    fn apply(&mut self, cmd: Command) -> (Reply, u64) {
        match cmd {
            Command::Get(k) => (self.store.get(&k), COST_GET_US),
            Command::Set(k, v) => (self.store.set(&k, v), COST_SET_US),
            Command::Incr(k) => (self.store.incr(&k), COST_INCR_US),
            Command::Del(k) => {
                // The reply counts removed KEYS (an `Int` or an empty collection is still a key);
                // only the cost depends on the number of elements freed synchronously.
                let (found, n) = match self.store.remove("DEL", &k) {
                    Some(Value::Collection(n)) => (1, n),
                    Some(Value::Int(_)) => (1, 0),
                    None => (0, 0),
                };
                (
                    Reply::Count(found),
                    COST_DEL_BASE_US + n * COST_DEL_PER_1000_ELEMS_US / 1000,
                )
            }
            Command::Unlink(k) => {
                let removed = self.store.remove("UNLINK", &k);
                match removed {
                    Some(Value::Collection(n)) if n > LAZYFREE_THRESHOLD => {
                        self.background_freed += n;
                        (Reply::Count(1), COST_DEL_BASE_US)
                    }
                    Some(Value::Collection(n)) => (
                        Reply::Count(1),
                        COST_DEL_BASE_US + n * COST_DEL_PER_1000_ELEMS_US / 1000,
                    ),
                    Some(Value::Int(_)) => (Reply::Count(1), COST_DEL_BASE_US),
                    None => (Reply::Count(0), COST_DEL_BASE_US),
                }
            }
            Command::Keys => {
                self.store.log_op("KEYS");
                let n = self.store.dbsize();
                (Reply::Count(n), n * COST_KEYS_PER_1000_KEYS_US / 1000)
            }
            Command::Scan { cursor, count } => {
                self.store.log_op("SCAN");
                let n = self.store.dbsize();
                let returned = count.min(n.saturating_sub(cursor));
                let next = if cursor + returned >= n {
                    0
                } else {
                    cursor + returned
                };
                (
                    Reply::Scan { next, returned },
                    COST_SCAN_BASE_US + returned * COST_SCAN_PER_1000_KEYS_US / 1000,
                )
            }
            Command::Eval { key, script } => (script(&mut self.store, &key), COST_EVAL_US),
            Command::Multi | Command::Watch(_) | Command::Exec => {
                (Reply::Err("ERR not allowed here"), COST_QUEUE_US)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Event-driven simulation: requests with arrival times, one server thread, closed-loop clients
// ---------------------------------------------------------------------------------------------

/// Marks a request from the open-loop load generator: nobody waits for its reply.
pub const ANON: usize = usize::MAX;

/// A client connection driven by the replies it receives.
pub trait Client {
    /// The first request (absolute arrival time in µs and command), if any.
    fn start(&mut self, rng: &mut Rng) -> Option<(u64, Command)>;
    /// Reacts to a reply; `Some(cmd)` is sent after one network round trip.
    fn on_reply(&mut self, reply: &Reply) -> Option<Command>;
    /// How many `WATCH` retries this client needed.
    fn aborts(&self) -> u64 {
        0
    }
}

/// Timing of one executed request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// Connection id ([`ANON`] for the open-loop load).
    pub client: usize,
    /// Command name.
    pub name: &'static str,
    /// When the request reached the server.
    pub arrival: u64,
    /// When the server thread started it (`max(previous finish, arrival)`).
    pub start: u64,
    /// When the server thread finished it.
    pub finish: u64,
}

impl Done {
    /// Time from arrival to completion (queueing + service).
    pub fn latency(&self) -> u64 {
        self.finish - self.arrival
    }
}

struct Pending {
    arrival: u64,
    seq: u64,
    client: usize,
    cmd: Command,
}
impl PartialEq for Pending {
    fn eq(&self, o: &Self) -> bool {
        (self.arrival, self.seq) == (o.arrival, o.seq)
    }
}
impl Eq for Pending {}
impl PartialOrd for Pending {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Pending {
    fn cmp(&self, o: &Self) -> Ordering {
        // BinaryHeap is a max-heap: reverse so the earliest arrival (then lowest seq) pops first.
        (o.arrival, o.seq).cmp(&(self.arrival, self.seq))
    }
}

/// Runs the server until no request is pending. `open_loop` are requests with fixed arrival times
/// whose replies nobody waits for; `clients` are closed-loop. Returns one [`Done`] per request in
/// execution order.
pub fn run(
    server: &mut Server,
    clients: &mut [Box<dyn Client>],
    open_loop: Vec<(u64, Command)>,
    rng: &mut Rng,
) -> Vec<Done> {
    let mut heap = BinaryHeap::new();
    let mut seq = 0u64;
    for (arrival, cmd) in open_loop {
        heap.push(Pending {
            arrival,
            seq,
            client: ANON,
            cmd,
        });
        seq += 1;
    }
    for (i, c) in clients.iter_mut().enumerate() {
        if let Some((arrival, cmd)) = c.start(rng) {
            heap.push(Pending {
                arrival,
                seq,
                client: i,
                cmd,
            });
            seq += 1;
        }
    }
    let mut now = 0u64;
    let mut done = Vec::new();
    while let Some(p) = heap.pop() {
        let start = now.max(p.arrival);
        let name = p.cmd.name();
        let (reply, cost) = server.handle(p.client, p.cmd);
        let finish = start + cost;
        now = finish;
        done.push(Done {
            client: p.client,
            name,
            arrival: p.arrival,
            start,
            finish,
        });
        if p.client != ANON
            && let Some(next) = clients[p.client].on_reply(&reply)
        {
            let rtt = rng.range(NET_RTT_US.0, NET_RTT_US.1 + 1);
            heap.push(Pending {
                arrival: finish + rtt,
                seq,
                client: p.client,
                cmd: next,
            });
            seq += 1;
        }
    }
    done
}

// ---------------------------------------------------------------------------------------------
// Scenario 1: head-of-line blocking
// ---------------------------------------------------------------------------------------------

/// The slow event of scenario 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlowKind {
    /// No slow event (baseline).
    None,
    /// `KEYS *`.
    Keys,
    /// `SCAN` walked in batches of `count` keys.
    Scan {
        /// `COUNT` of each call.
        count: u64,
    },
    /// `DEL big`.
    Del,
    /// `UNLINK big`.
    Unlink,
}

impl SlowKind {
    /// Short label for tables.
    pub fn label(&self) -> String {
        match self {
            SlowKind::None => "no slow event".into(),
            SlowKind::Keys => "KEYS *".into(),
            SlowKind::Scan { count } => format!("SCAN COUNT {count}"),
            SlowKind::Del => "DEL big".into(),
            SlowKind::Unlink => "UNLINK big".into(),
        }
    }
}

/// Parameters of scenario 1.
#[derive(Clone, Copy, Debug)]
pub struct HolConfig {
    /// Total keys in the database.
    pub keys: u64,
    /// Elements of the collection `big`.
    pub big: u64,
    /// Open-loop `GET` arrivals per second.
    pub get_per_sec: u64,
    /// Length of the run (µs).
    pub duration_us: u64,
    /// When the slow event is submitted (µs).
    pub slow_at_us: u64,
}

impl Default for HolConfig {
    fn default() -> Self {
        Self {
            keys: 1_000_000,
            big: 1_000_000,
            get_per_sec: 50_000,
            duration_us: 1_000_000,
            slow_at_us: 300_000,
        }
    }
}

/// Number of distinct hot keys the `GET`s read.
const HOT_KEYS: u64 = 1000;
/// Connection id of the slow command / scanner.
const SLOW_CLIENT: usize = 0;

struct OneShot {
    at: u64,
    cmd: Command,
}
impl Client for OneShot {
    fn start(&mut self, _: &mut Rng) -> Option<(u64, Command)> {
        Some((self.at, self.cmd.clone()))
    }
    fn on_reply(&mut self, _: &Reply) -> Option<Command> {
        None
    }
}

struct Scanner {
    at: u64,
    count: u64,
}
impl Client for Scanner {
    fn start(&mut self, _: &mut Rng) -> Option<(u64, Command)> {
        Some((
            self.at,
            Command::Scan {
                cursor: 0,
                count: self.count,
            },
        ))
    }
    fn on_reply(&mut self, reply: &Reply) -> Option<Command> {
        match reply {
            Reply::Scan { next, .. } if *next != 0 => Some(Command::Scan {
                cursor: *next,
                count: self.count,
            }),
            _ => None,
        }
    }
}

/// Result of one scenario-1 variant.
#[derive(Clone, Debug)]
pub struct HolResult {
    /// Which slow event was injected.
    pub kind: SlowKind,
    /// Every executed request, in execution order.
    pub done: Vec<Done>,
    /// Elements freed off-loop.
    pub background_freed: u64,
}

impl HolResult {
    /// Latencies of the small `GET`s, sorted ascending (µs).
    pub fn get_latencies(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .done
            .iter()
            .filter(|d| d.client == ANON)
            .map(Done::latency)
            .collect();
        v.sort_unstable();
        v
    }

    /// Requests of the slow client (one for `KEYS`/`DEL`/`UNLINK`, many for `SCAN`).
    pub fn slow_requests(&self) -> Vec<&Done> {
        self.done
            .iter()
            .filter(|d| d.client == SLOW_CLIENT)
            .collect()
    }

    /// Longest time the server thread spent on one slow-client request (µs).
    pub fn longest_slow_us(&self) -> u64 {
        self.slow_requests()
            .iter()
            .map(|d| d.finish - d.start)
            .max()
            .unwrap_or(0)
    }
}

/// `q`-th permille of a sorted sample (nearest-rank).
pub fn permille(sorted: &[u64], q: u64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() as u64 * q).div_ceil(1000).max(1);
    sorted[rank as usize - 1]
}

/// Runs one variant of scenario 1. The `GET` arrival times depend only on `seed` and `cfg`, so all
/// variants of one seed face the identical load.
pub fn scenario1(seed: u64, kind: SlowKind, cfg: HolConfig) -> HolResult {
    let mut root = Rng::new(seed);
    let mut load_rng = root.fork(1);
    let mut net_rng = root.fork(2);

    let mut store = Store::default();
    for i in 0..HOT_KEYS {
        store.map.insert(format!("hot:{i}"), Value::Int(1));
    }
    store.map.insert("big".into(), Value::Collection(cfg.big));
    store.filler = cfg.keys.saturating_sub(store.map.len() as u64);

    let n = cfg.get_per_sec * cfg.duration_us / 1_000_000;
    let mut arrivals: Vec<u64> = (0..n).map(|_| load_rng.below(cfg.duration_us)).collect();
    arrivals.sort_unstable();
    let open_loop: Vec<(u64, Command)> = arrivals
        .into_iter()
        .map(|t| (t, Command::Get(format!("hot:{}", load_rng.below(HOT_KEYS)))))
        .collect();

    let slow: Option<Box<dyn Client>> = match kind {
        SlowKind::None => None,
        SlowKind::Keys => Some(Box::new(OneShot {
            at: cfg.slow_at_us,
            cmd: Command::Keys,
        })),
        SlowKind::Scan { count } => Some(Box::new(Scanner {
            at: cfg.slow_at_us,
            count,
        })),
        SlowKind::Del => Some(Box::new(OneShot {
            at: cfg.slow_at_us,
            cmd: Command::Del("big".into()),
        })),
        SlowKind::Unlink => Some(Box::new(OneShot {
            at: cfg.slow_at_us,
            cmd: Command::Unlink("big".into()),
        })),
    };
    // Index 0 must be the slow client; the baseline gets an inert one.
    let slow = slow.unwrap_or_else(|| Box::new(Inert));
    let mut clients = vec![slow];
    let mut server = Server::new(store);
    let done = run(&mut server, &mut clients, open_loop, &mut net_rng);
    HolResult {
        kind,
        done,
        background_freed: server.background_freed,
    }
}

struct Inert;
impl Client for Inert {
    fn start(&mut self, _: &mut Rng) -> Option<(u64, Command)> {
        None
    }
    fn on_reply(&mut self, _: &Reply) -> Option<Command> {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// Scenario 2: where the atomic boundary is
// ---------------------------------------------------------------------------------------------

/// How a client increments the shared counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rmw {
    /// `GET` then `SET v+1`: two requests, other clients can run in between.
    GetSet,
    /// `INCR`: one command.
    Incr,
    /// `WATCH x; GET x; MULTI; SET x v+1; EXEC`, retried when `EXEC` aborts.
    WatchMulti,
    /// One `EVAL` that reads, adds 1 and writes.
    Script,
}

impl Rmw {
    /// Short label for tables.
    pub fn label(&self) -> &'static str {
        match self {
            Rmw::GetSet => "GET + SET (2 requests)",
            Rmw::Incr => "INCR",
            Rmw::WatchMulti => "WATCH/MULTI/EXEC + retry",
            Rmw::Script => "EVAL script (1 request)",
        }
    }
}

const COUNTER: &str = "counter";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Watch,
    Get,
    Multi,
    QueueSet,
    Exec,
    Set,
    Single,
}

struct RmwClient {
    variant: Rmw,
    remaining: u64,
    phase: Phase,
    seen: i64,
    aborts: u64,
}

impl RmwClient {
    fn new(variant: Rmw, incs: u64) -> Self {
        let phase = match variant {
            Rmw::GetSet => Phase::Get,
            Rmw::WatchMulti => Phase::Watch,
            Rmw::Incr | Rmw::Script => Phase::Single,
        };
        Self {
            variant,
            remaining: incs,
            phase,
            seen: 0,
            aborts: 0,
        }
    }

    /// First command of the next increment.
    fn begin(&mut self) -> Command {
        match self.variant {
            Rmw::GetSet => {
                self.phase = Phase::Get;
                Command::Get(COUNTER.into())
            }
            Rmw::Incr => Command::Incr(COUNTER.into()),
            Rmw::Script => Command::Eval {
                key: COUNTER.into(),
                script: script_incr,
            },
            Rmw::WatchMulti => {
                self.phase = Phase::Watch;
                Command::Watch(COUNTER.into())
            }
        }
    }

    fn finish_one(&mut self) -> Option<Command> {
        self.remaining -= 1;
        if self.remaining == 0 {
            None
        } else {
            Some(self.begin())
        }
    }
}

impl Client for RmwClient {
    fn start(&mut self, rng: &mut Rng) -> Option<(u64, Command)> {
        if self.remaining == 0 {
            return None;
        }
        Some((rng.below(NET_RTT_US.1), self.begin()))
    }

    fn on_reply(&mut self, reply: &Reply) -> Option<Command> {
        match (self.variant, self.phase, reply) {
            (Rmw::GetSet, Phase::Get, Reply::Int(_) | Reply::Nil) => {
                self.seen = if let Reply::Int(v) = reply { *v } else { 0 };
                self.phase = Phase::Set;
                Some(Command::Set(COUNTER.into(), self.seen + 1))
            }
            (Rmw::GetSet, Phase::Set, Reply::Ok) => self.finish_one(),
            (Rmw::Incr | Rmw::Script, Phase::Single, Reply::Int(_)) => self.finish_one(),
            (Rmw::WatchMulti, Phase::Watch, Reply::Ok) => {
                self.phase = Phase::Get;
                Some(Command::Get(COUNTER.into()))
            }
            (Rmw::WatchMulti, Phase::Get, Reply::Int(_) | Reply::Nil) => {
                self.seen = if let Reply::Int(v) = reply { *v } else { 0 };
                self.phase = Phase::Multi;
                Some(Command::Multi)
            }
            (Rmw::WatchMulti, Phase::Multi, Reply::Ok) => {
                self.phase = Phase::QueueSet;
                Some(Command::Set(COUNTER.into(), self.seen + 1))
            }
            (Rmw::WatchMulti, Phase::QueueSet, Reply::Queued) => {
                self.phase = Phase::Exec;
                Some(Command::Exec)
            }
            (Rmw::WatchMulti, Phase::Exec, Reply::Exec(_)) => self.finish_one(),
            (Rmw::WatchMulti, Phase::Exec, Reply::ExecAborted) => {
                self.aborts += 1;
                Some(self.begin())
            }
            (v, p, r) => unreachable!("client state machine: {v:?} {} {r:?}", p as u8),
        }
    }

    fn aborts(&self) -> u64 {
        self.aborts
    }
}

/// Result of one scenario-2 variant.
#[derive(Clone, Debug)]
pub struct RmwResult {
    /// The increment strategy.
    pub variant: Rmw,
    /// Clients × increments each.
    pub expected: i64,
    /// The counter at the end.
    pub final_value: i64,
    /// `EXEC`s aborted by `WATCH` (each costs the client a full retry).
    pub aborts: u64,
    /// Requests the server executed.
    pub requests: u64,
    /// Virtual time at which the last request finished (µs).
    pub end_us: u64,
    /// Primitive operations applied to the store, in order.
    pub log: Vec<LogEntry>,
    /// Timing of every request.
    pub done: Vec<Done>,
}

impl RmwResult {
    /// Increments that were acknowledged but are missing from the final value.
    pub fn lost(&self) -> i64 {
        self.expected - self.final_value
    }
}

/// `clients` clients each add 1 to one counter `incs` times with `variant`.
pub fn scenario2(seed: u64, variant: Rmw, clients: usize, incs: u64) -> RmwResult {
    let mut rng = Rng::new(seed).fork(3);
    let mut server = Server::new(Store::default());
    let mut cs: Vec<Box<dyn Client>> = (0..clients)
        .map(|_| Box::new(RmwClient::new(variant, incs)) as Box<dyn Client>)
        .collect();
    let done = run(&mut server, &mut cs, Vec::new(), &mut rng);
    let final_value = match server.store.map.get(COUNTER) {
        Some(Value::Int(v)) => *v,
        _ => 0,
    };
    RmwResult {
        variant,
        expected: clients as i64 * incs as i64,
        final_value,
        aborts: cs.iter().map(|c| c.aborts()).sum(),
        requests: done.len() as u64,
        end_us: done.last().map_or(0, |d| d.finish),
        log: std::mem::take(&mut server.store.log),
        done,
    }
}

// ---------------------------------------------------------------------------------------------
// Scenario 3: asynchronous replication, WAIT, promotion policy
// ---------------------------------------------------------------------------------------------

/// How the new master is picked among the surviving replicas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// Highest replication offset first.
    ByOffset,
    /// Lowest `replica-priority` number first, then highest offset (Sentinel's documented order).
    ByPriorityThenOffset,
}

impl Policy {
    /// Short label for tables.
    pub fn label(&self) -> &'static str {
        match self {
            Policy::ByOffset => "ByOffset",
            Policy::ByPriorityThenOffset => "ByPriorityThenOffset",
        }
    }
}

/// Replica 0 is "A" (fast link), replica 1 is "B" (lagging link, but the better priority).
pub const REPLICA_PRIORITY: [u32; 2] = [100, 10];
/// Gap between a client's acknowledged write and its next write (µs, uniform).
pub const THINK_US: (u64, u64) = (200, 800);
/// Master → replica A apply delay per write (µs, uniform).
pub const DELAY_A_US: (u64, u64) = (100, 500);
/// Master → replica B apply delay per write (µs, uniform).
pub const DELAY_B_US: (u64, u64) = (1000, 6000);
/// Replica → master acknowledgement delay (µs, uniform).
pub const ACK_US: (u64, u64) = (50, 200);
/// The master crashes at a uniform time in this window (µs).
pub const CRASH_WINDOW_US: (u64, u64) = (20_000, 80_000);
const MAX_WRITES: usize = 512;

/// Outcome of one master crash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failover {
    /// Writes the client was told are done (offsets `1..=acked`).
    pub acked: u64,
    /// Which replica was promoted (0 = A, 1 = B), if any survived.
    pub promoted: Option<usize>,
    /// Writes (offsets) the promoted replica had applied at the crash.
    pub promoted_offset: u64,
    /// Acked writes that the new master does not have.
    pub lost: u64,
}

/// One master crash. `wait` is the `WAIT` level (0 = none: the master's own reply is the ack).
/// `a_alive == false` means replica A died together with the master.
///
/// The per-write random delays are drawn up front from `(seed, run)`, so the same crash can be
/// replayed under every `wait`/`policy`.
pub fn failover_run(seed: u64, run: u64, wait: usize, policy: Policy, a_alive: bool) -> Failover {
    assert!(wait <= 2, "only two replicas");
    let mut r = Rng::new(seed).fork(0x100 + run);
    let crash = r.range(CRASH_WINDOW_US.0, CRASH_WINDOW_US.1);
    let mut draw = |lo_hi: (u64, u64)| -> Vec<u64> {
        (0..MAX_WRITES).map(|_| r.range(lo_hi.0, lo_hi.1)).collect()
    };
    let think = draw(THINK_US);
    let da = draw(DELAY_A_US);
    let db = draw(DELAY_B_US);
    let ack_a = draw(ACK_US);
    let ack_b = draw(ACK_US);

    let mut applies: [Vec<u64>; 2] = [Vec::new(), Vec::new()];
    let mut t = 0u64;
    let mut acked = 0u64;
    for i in 0..MAX_WRITES {
        if t > crash {
            break;
        }
        // The replication stream is offset-ordered: write i cannot be applied before write i-1.
        let prev = [
            applies[0].last().copied().unwrap_or(0),
            applies[1].last().copied().unwrap_or(0),
        ];
        let apply = [(t + da[i]).max(prev[0]), (t + db[i]).max(prev[1])];
        applies[0].push(apply[0]);
        applies[1].push(apply[1]);
        let ack_at = [apply[0] + ack_a[i], apply[1] + ack_b[i]];
        let acked_at = match wait {
            0 => t,
            1 => ack_at[0].min(ack_at[1]),
            _ => ack_at[0].max(ack_at[1]),
        };
        if acked_at <= crash {
            acked = i as u64 + 1;
        }
        t = acked_at + think[i];
    }

    let offset_at_crash =
        |a: &[u64]| -> u64 { a.iter().take_while(|&&x| x <= crash).count() as u64 };
    let offsets = [offset_at_crash(&applies[0]), offset_at_crash(&applies[1])];
    let candidates: Vec<usize> = (0..2).filter(|&i| i == 1 || a_alive).collect();
    let promoted = candidates.into_iter().min_by_key(|&i| match policy {
        Policy::ByOffset => (0, std::cmp::Reverse(offsets[i]), i),
        Policy::ByPriorityThenOffset => (REPLICA_PRIORITY[i], std::cmp::Reverse(offsets[i]), i),
    });
    let promoted_offset = promoted.map_or(0, |i| offsets[i]);
    Failover {
        acked,
        promoted,
        promoted_offset,
        lost: acked.saturating_sub(promoted_offset),
    }
}

/// Aggregate over `runs` crashes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailoverStats {
    /// Crashes simulated.
    pub runs: u64,
    /// Acknowledged writes over all runs.
    pub acked: u64,
    /// Acknowledged writes missing on the new master over all runs.
    pub lost: u64,
    /// Runs that lost at least one acknowledged write.
    pub runs_with_loss: u64,
    /// The first run index that lost something, if any (a pinned counterexample).
    pub first_loss_run: Option<u64>,
}

/// Replays `runs` crashes (run indices `0..runs`) and aggregates.
pub fn failover_sweep(
    seed: u64,
    runs: u64,
    wait: usize,
    policy: Policy,
    a_alive: bool,
) -> FailoverStats {
    let mut s = FailoverStats {
        runs,
        acked: 0,
        lost: 0,
        runs_with_loss: 0,
        first_loss_run: None,
    };
    for run in 0..runs {
        let f = failover_run(seed, run, wait, policy, a_alive);
        s.acked += f.acked;
        s.lost += f.lost;
        if f.lost > 0 {
            s.runs_with_loss += 1;
            s.first_loss_run.get_or_insert(run);
        }
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Report (what the binary prints)
// ---------------------------------------------------------------------------------------------

/// The text the binary prints for `seed` and `runs` crashes per scenario-3 row.
pub fn render_report(seed: u64, runs: u64) -> String {
    let mut o = String::new();
    let cfg = HolConfig::default();
    let _ = writeln!(o, "Lab (i) redis_loop - seed {seed}");
    let _ = writeln!(
        o,
        "== Cost model (invented parameters, virtual us on the one server thread) =="
    );
    let _ = writeln!(
        o,
        "GET/SET/INCR {COST_GET_US}  EVAL {COST_EVAL_US}  MULTI/WATCH/queued {COST_QUEUE_US}  EXEC {COST_EXEC_BASE_US}+queued"
    );
    let _ = writeln!(
        o,
        "KEYS {COST_KEYS_PER_1000_KEYS_US} us/1000 keys  SCAN {COST_SCAN_BASE_US}+{COST_SCAN_PER_1000_KEYS_US} us/1000 keys  DEL {COST_DEL_BASE_US}+{COST_DEL_PER_1000_ELEMS_US} us/1000 elems  UNLINK {COST_DEL_BASE_US} (> {LAZYFREE_THRESHOLD} elems)"
    );

    let _ = writeln!(
        o,
        "\n== 1. Head-of-line blocking: {} GET/s for {} ms, slow event at {} ms, {} keys, big={} ==",
        cfg.get_per_sec,
        cfg.duration_us / 1000,
        cfg.slow_at_us / 1000,
        cfg.keys,
        cfg.big
    );
    let _ = writeln!(
        o,
        "{:<16}{:>11}{:>8}{:>9}{:>10}{:>10}",
        "slow event", "longest us", "p50 us", "p99 us", "max us", "GET>1ms"
    );
    for kind in [
        SlowKind::None,
        SlowKind::Keys,
        SlowKind::Scan { count: 1000 },
        SlowKind::Del,
        SlowKind::Unlink,
    ] {
        let r = scenario1(seed, kind, cfg);
        let lat = r.get_latencies();
        let _ = writeln!(
            o,
            "{:<16}{:>11}{:>8}{:>9}{:>10}{:>10}",
            kind.label(),
            r.longest_slow_us(),
            permille(&lat, 500),
            permille(&lat, 990),
            lat.last().copied().unwrap_or(0),
            lat.iter().filter(|&&l| l > 1000).count()
        );
    }

    let (clients, incs) = (8usize, 1000u64);
    let _ = writeln!(
        o,
        "\n== 2. Atomic boundary: {clients} clients x {incs} increments of one counter, RTT {}-{} us ==",
        NET_RTT_US.0, NET_RTT_US.1
    );
    let _ = writeln!(
        o,
        "{:<26}{:>9}{:>8}{:>7}{:>9}{:>10}",
        "variant", "expected", "final", "lost", "aborts", "requests"
    );
    for v in [Rmw::GetSet, Rmw::Incr, Rmw::WatchMulti, Rmw::Script] {
        let r = scenario2(seed, v, clients, incs);
        let _ = writeln!(
            o,
            "{:<26}{:>9}{:>8}{:>7}{:>9}{:>10}",
            v.label(),
            r.expected,
            r.final_value,
            r.lost(),
            r.aborts,
            r.requests
        );
    }

    let _ = writeln!(
        o,
        "\n== 3. Async replication: acked writes missing on the new master, {runs} crashes per row =="
    );
    let _ = writeln!(
        o,
        "replica priority: A={} (fast link), B={} (lagging link; lower number wins)",
        REPLICA_PRIORITY[0], REPLICA_PRIORITY[1]
    );
    let _ = writeln!(
        o,
        "{:<22}{:<6}{:>8}{:>8}{:>16}",
        "policy", "WAIT", "acked", "lost", "runs with loss"
    );
    for policy in [Policy::ByOffset, Policy::ByPriorityThenOffset] {
        for wait in 0..=2 {
            let s = failover_sweep(seed, runs, wait, policy, true);
            let _ = writeln!(
                o,
                "{:<22}{:<6}{:>8}{:>8}{:>16}",
                policy.label(),
                if wait == 0 {
                    "none".to_string()
                } else {
                    wait.to_string()
                },
                s.acked,
                s.lost,
                s.runs_with_loss
            );
        }
    }
    let _ = writeln!(
        o,
        "same, but replica A dies together with the master (only B can be promoted):"
    );
    for wait in 0..=2 {
        let s = failover_sweep(seed, runs, wait, Policy::ByOffset, false);
        let _ = writeln!(
            o,
            "{:<22}{:<6}{:>8}{:>8}{:>16}",
            "only B",
            if wait == 0 {
                "none".to_string()
            } else {
                wait.to_string()
            },
            s.acked,
            s.lost,
            s.runs_with_loss
        );
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hol(seed: u64, kind: SlowKind) -> HolResult {
        scenario1(seed, kind, HolConfig::default())
    }

    #[test]
    fn permille_is_nearest_rank() {
        let v: Vec<u64> = (1..=100).collect();
        assert_eq!(permille(&v, 500), 50);
        assert_eq!(permille(&v, 990), 99);
        assert_eq!(permille(&v, 1000), 100);
        assert_eq!(permille(&[], 500), 0);
        assert_eq!(permille(&[7], 1), 7);
    }

    #[test]
    fn keys_cost_matches_the_documented_one_million_keys_in_40_ms() {
        let r = hol(1, SlowKind::Keys);
        assert_eq!(r.longest_slow_us(), 40_000);
        let r = hol(1, SlowKind::Del);
        assert_eq!(r.longest_slow_us(), COST_DEL_BASE_US + 100_000);
    }

    #[test]
    fn every_get_arriving_during_a_slow_command_waits_for_the_rest_of_it() {
        for seed in 0..3 {
            for kind in [SlowKind::Keys, SlowKind::Del] {
                let r = hol(seed, kind);
                let slow = r.slow_requests();
                assert_eq!(slow.len(), 1);
                let s = slow[0];
                let mut blocked = 0;
                for d in r.done.iter().filter(|d| d.client == ANON) {
                    if d.arrival >= s.start && d.arrival < s.finish {
                        blocked += 1;
                        assert!(
                            d.latency() >= s.finish - d.arrival,
                            "seed {seed} {kind:?}: GET at {} finished at {} before the slow command ({})",
                            d.arrival,
                            d.finish,
                            s.finish
                        );
                    }
                }
                // 50 000 GET/s over a >= 40 ms window: about 2000 GETs are caught behind it.
                assert!(blocked > 1000, "{kind:?}: only {blocked} blocked GETs");
            }
        }
    }

    #[test]
    fn the_server_never_runs_two_requests_at_once() {
        let r = hol(3, SlowKind::Scan { count: 1000 });
        let mut prev_finish = 0;
        for d in &r.done {
            assert!(d.start >= prev_finish && d.start >= d.arrival && d.finish > d.start);
            prev_finish = d.finish;
        }
    }

    #[test]
    fn keys_p99_is_orders_of_magnitude_above_scan_batches() {
        for seed in 0..3 {
            let keys = hol(seed, SlowKind::Keys).get_latencies();
            let scan = hol(seed, SlowKind::Scan { count: 1000 }).get_latencies();
            let base = hol(seed, SlowKind::None).get_latencies();
            let (kp99, sp99, bp99) = (
                permille(&keys, 990),
                permille(&scan, 990),
                permille(&base, 990),
            );
            assert!(
                kp99 > 100 * sp99,
                "seed {seed}: KEYS p99 {kp99} vs SCAN p99 {sp99}"
            );
            // A SCAN batch is at most 45 us of server time, so a GET waits at most about that long.
            let batch = COST_SCAN_BASE_US + 1000 * COST_SCAN_PER_1000_KEYS_US / 1000;
            assert!(
                *scan.last().unwrap() <= batch + 20 * COST_GET_US,
                "seed {seed}"
            );
            assert!(
                sp99 < 100 && bp99 < 100,
                "seed {seed}: scan p99 {sp99}, base p99 {bp99}"
            );
        }
    }

    #[test]
    fn scan_walk_covers_every_key_exactly_once_and_terminates() {
        let cfg = HolConfig::default();
        let r = hol(2, SlowKind::Scan { count: 1000 });
        let calls = r.slow_requests().len() as u64;
        assert_eq!(calls, cfg.keys.div_ceil(1000));
        // And the cursor arithmetic itself, including a ragged last page.
        let mut s = Server::new(Store {
            filler: 2500,
            ..Store::default()
        });
        let (mut cursor, mut seen, mut pages) = (0, 0, 0);
        loop {
            let (reply, _) = s.handle(
                0,
                Command::Scan {
                    cursor,
                    count: 1000,
                },
            );
            let Reply::Scan { next, returned } = reply else {
                panic!("not a scan reply")
            };
            seen += returned;
            pages += 1;
            if next == 0 {
                break;
            }
            cursor = next;
        }
        assert_eq!((seen, pages), (2500, 3));
    }

    #[test]
    fn unlink_keeps_max_latency_small_but_del_does_not() {
        for seed in 0..3 {
            let unlink = hol(seed, SlowKind::Unlink);
            let del = hol(seed, SlowKind::Del);
            let base = hol(seed, SlowKind::None);
            assert_eq!(unlink.background_freed, 1_000_000);
            assert_eq!(del.background_freed, 0);
            let (um, dm, bm) = (
                *unlink.get_latencies().last().unwrap(),
                *del.get_latencies().last().unwrap(),
                *base.get_latencies().last().unwrap(),
            );
            assert!(
                um <= bm + 2 * COST_DEL_BASE_US + COST_GET_US,
                "seed {seed}: {um} vs base {bm}"
            );
            assert!(dm >= 90_000, "seed {seed}: DEL max {dm}");
        }
    }

    #[test]
    fn del_and_unlink_reply_with_the_number_of_removed_keys() {
        // DEL and UNLINK must agree on what they report: keys removed, whatever the value type.
        for use_unlink in [false, true] {
            let mut store = Store::default();
            store.map.insert("int".into(), Value::Int(7));
            store.map.insert("big".into(), Value::Collection(10));
            let mut s = Server::new(store);
            let mut del = |k: &str| {
                let cmd = if use_unlink {
                    Command::Unlink(k.into())
                } else {
                    Command::Del(k.into())
                };
                s.handle(0, cmd).0
            };
            assert_eq!(del("int"), Reply::Count(1), "unlink = {use_unlink}");
            assert_eq!(del("big"), Reply::Count(1), "unlink = {use_unlink}");
            assert_eq!(del("missing"), Reply::Count(0), "unlink = {use_unlink}");
            assert_eq!(del("int"), Reply::Count(0), "already removed");
        }
    }

    #[test]
    fn unlink_of_a_small_value_is_freed_synchronously() {
        let mut store = Store::default();
        store
            .map
            .insert("small".into(), Value::Collection(LAZYFREE_THRESHOLD));
        store
            .map
            .insert("large".into(), Value::Collection(LAZYFREE_THRESHOLD + 1));
        let mut s = Server::new(store);
        let (_, c_small) = s.handle(0, Command::Unlink("small".into()));
        assert_eq!(
            c_small,
            COST_DEL_BASE_US + LAZYFREE_THRESHOLD * COST_DEL_PER_1000_ELEMS_US / 1000
        );
        assert_eq!(s.background_freed, 0);
        let (_, c_large) = s.handle(0, Command::Unlink("large".into()));
        assert_eq!(c_large, COST_DEL_BASE_US);
        assert_eq!(s.background_freed, LAZYFREE_THRESHOLD + 1);
    }

    #[test]
    fn non_atomic_get_set_loses_updates_and_the_log_shows_the_interleaving() {
        let mut lossy_seeds = 0;
        for seed in 0..20 {
            let r = scenario2(seed, Rmw::GetSet, 8, 1000);
            assert!(r.final_value <= r.expected);
            if r.lost() > 0 {
                lossy_seeds += 1;
            }
            // Lost update = a client's SET lands after a foreign SET that came between its GET and SET.
            let mut open_get: BTreeMap<usize, usize> = BTreeMap::new();
            let mut interleaved = 0;
            for (i, e) in r.log.iter().enumerate() {
                match e.op {
                    "GET" => {
                        open_get.insert(e.client, i);
                    }
                    "SET" => {
                        let g = open_get.remove(&e.client).unwrap();
                        if r.log[g..i]
                            .iter()
                            .any(|x| x.client != e.client && x.op == "SET")
                        {
                            interleaved += 1;
                        }
                    }
                    _ => {}
                }
            }
            // Every lost update corresponds to at least one interleaved GET..SET window.
            assert!(
                interleaved as i64 >= r.lost(),
                "seed {seed}: {interleaved} < {}",
                r.lost()
            );
        }
        assert_eq!(
            lossy_seeds, 20,
            "with 8 clients and RTT 100-300 us every seed loses updates"
        );
    }

    #[test]
    fn incr_script_and_watch_retry_never_lose_an_update() {
        for seed in 0..8 {
            for v in [Rmw::Incr, Rmw::WatchMulti, Rmw::Script] {
                let r = scenario2(seed, v, 8, 1000);
                assert_eq!(r.final_value, 8000, "seed {seed} {v:?}");
                assert_eq!(r.lost(), 0);
            }
        }
    }

    #[test]
    fn watch_aborts_under_contention_and_only_then() {
        for seed in 0..4 {
            let many = scenario2(seed, Rmw::WatchMulti, 8, 300);
            assert!(many.aborts > 0, "seed {seed}: no abort with 8 clients");
            // Each abort costs a full 5-request attempt on top of the 5 requests that commit.
            assert_eq!(many.requests, 2400 * 5 + many.aborts * 5);
            let alone = scenario2(seed, Rmw::WatchMulti, 1, 1000);
            assert_eq!(
                alone.aborts, 0,
                "a single client never conflicts with itself"
            );
            assert_eq!(alone.final_value, 1000);
        }
    }

    #[test]
    fn multi_exec_and_scripts_are_never_interleaved_in_the_trace() {
        for v in [Rmw::WatchMulti, Rmw::Script] {
            let r = scenario2(7, v, 8, 300);
            // Group the primitive operations by unit: each unit's entries must be contiguous.
            let mut seen_units = std::collections::BTreeSet::new();
            let mut prev: Option<u64> = None;
            for e in &r.log {
                if prev != Some(e.unit) {
                    assert!(
                        seen_units.insert(e.unit),
                        "{v:?}: unit {} reappears after another",
                        e.unit
                    );
                    prev = Some(e.unit);
                }
            }
            // And in time: no request overlaps another on the single thread.
            for w in r.done.windows(2) {
                assert!(w[1].start >= w[0].finish);
            }
        }
        // The EXEC unit really contains the queued SET (plus nothing from other clients).
        let r = scenario2(7, Rmw::WatchMulti, 8, 300);
        let exec_sets = r.log.iter().filter(|e| e.op == "SET").count();
        assert_eq!(
            exec_sets as i64,
            8 * 300,
            "only EXEC runs SETs, one per successful attempt"
        );
    }

    #[test]
    fn multi_queues_commands_and_exec_runs_them_back_to_back_without_rollback() {
        let mut s = Server::new(Store::default());
        assert_eq!(s.handle(1, Command::Multi).0, Reply::Ok);
        assert_eq!(s.handle(1, Command::Set("x".into(), 5)).0, Reply::Queued);
        // Another client sees nothing yet: the SET is only queued.
        assert_eq!(s.handle(2, Command::Get("x".into())).0, Reply::Nil);
        assert_eq!(s.handle(1, Command::Incr("x".into())).0, Reply::Queued);
        s.store.map.insert("coll".into(), Value::Collection(3));
        assert_eq!(s.handle(1, Command::Incr("coll".into())).0, Reply::Queued);
        assert_eq!(s.handle(1, Command::Incr("x".into())).0, Reply::Queued);
        let (reply, _) = s.handle(1, Command::Exec);
        // The failing INCR does not undo the others and does not stop the following one.
        assert_eq!(
            reply,
            Reply::Exec(vec![
                Reply::Ok,
                Reply::Int(6),
                Reply::Err("WRONGTYPE"),
                Reply::Int(7)
            ])
        );
        assert_eq!(s.store.get("x"), Reply::Int(7));
        assert_eq!(
            s.handle(1, Command::Exec).0,
            Reply::Err("ERR EXEC without MULTI")
        );
    }

    #[test]
    fn a_get_whose_result_decides_the_set_cannot_be_queued_in_multi() {
        // Inside MULTI the GET only answers QUEUED, so a client cannot compute `v + 1` from it.
        let mut s = Server::new(Store::default());
        s.handle(0, Command::Set("x".into(), 41));
        s.handle(1, Command::Multi);
        assert_eq!(s.handle(1, Command::Get("x".into())).0, Reply::Queued);
    }

    #[test]
    fn watch_aborts_when_another_client_modifies_the_key_and_clears_the_watch() {
        let mut s = Server::new(Store::default());
        s.handle(1, Command::Watch("x".into()));
        s.handle(2, Command::Set("x".into(), 1));
        s.handle(1, Command::Multi);
        s.handle(1, Command::Set("x".into(), 99));
        assert_eq!(s.handle(1, Command::Exec).0, Reply::ExecAborted);
        assert_eq!(
            s.store.get("x"),
            Reply::Int(1),
            "aborted transaction applied nothing"
        );
        assert_eq!(s.watch_aborts, 1);
        // The watch is gone after EXEC: the next MULTI/EXEC goes through.
        s.handle(1, Command::Multi);
        s.handle(1, Command::Set("x".into(), 99));
        assert!(matches!(s.handle(1, Command::Exec).0, Reply::Exec(_)));
        assert_eq!(s.store.get("x"), Reply::Int(99));
    }

    #[test]
    fn wait_two_never_loses_an_acked_write() {
        for policy in [Policy::ByOffset, Policy::ByPriorityThenOffset] {
            for seed in 0..5 {
                let s = failover_sweep(seed, 400, 2, policy, true);
                assert_eq!(s.lost, 0, "seed {seed} {policy:?}");
                assert!(s.acked > 400, "the runs must actually acknowledge writes");
            }
        }
    }

    #[test]
    fn by_offset_with_wait_one_never_loses_while_every_replica_survives() {
        for seed in 0..5 {
            let s = failover_sweep(seed, 400, 1, Policy::ByOffset, true);
            assert_eq!(s.lost, 0, "seed {seed}");
        }
    }

    #[test]
    fn by_priority_with_wait_one_loses_acked_writes() {
        // The better-priority replica B is the lagging one; WAIT 1 is satisfied by A alone.
        for seed in 0..5 {
            let s = failover_sweep(seed, 400, 1, Policy::ByPriorityThenOffset, true);
            assert!(
                s.runs_with_loss > 300,
                "seed {seed}: only {} runs lose",
                s.runs_with_loss
            );
        }
        // A pinned crash: first loss at run 0 of seed 42, promoted replica is B.
        let f = failover_run(42, 0, 1, Policy::ByPriorityThenOffset, true);
        assert_eq!(f.promoted, Some(1));
        assert!(f.lost > 0 && f.promoted_offset < f.acked, "{f:?}");
        // The same crash under ByOffset promotes A and loses nothing.
        let g = failover_run(42, 0, 1, Policy::ByOffset, true);
        assert_eq!((g.promoted, g.lost), (Some(0), 0));
        assert_eq!(
            g.acked, f.acked,
            "same crash, same acked prefix (WAIT level is identical)"
        );
    }

    #[test]
    fn without_wait_acked_writes_are_lost_under_both_policies() {
        for policy in [Policy::ByOffset, Policy::ByPriorityThenOffset] {
            let s = failover_sweep(5, 400, 0, policy, true);
            assert!(s.lost > 0 && s.runs_with_loss > 0, "{policy:?}");
        }
        // The lagging replica makes ByPriority strictly worse than ByOffset.
        let by_off = failover_sweep(5, 400, 0, Policy::ByOffset, true);
        let by_pri = failover_sweep(5, 400, 0, Policy::ByPriorityThenOffset, true);
        assert!(
            by_pri.lost > 10 * by_off.lost,
            "{} vs {}",
            by_pri.lost,
            by_off.lost
        );
    }

    #[test]
    fn wait_one_is_not_enough_when_the_replica_that_acked_dies_with_the_master() {
        // Counterexample to "ByOffset + WAIT 1 never loses": it needs a surviving replica that acked.
        let s = failover_sweep(9, 400, 1, Policy::ByOffset, false);
        assert!(s.lost > 0, "only B survives and B lags");
        let s2 = failover_sweep(9, 400, 2, Policy::ByOffset, false);
        assert_eq!(s2.lost, 0, "WAIT 2 means B also had every acked write");
    }

    #[test]
    fn stricter_wait_never_acks_more_writes_in_the_same_crash() {
        for seed in 0..3 {
            for run in 0..200 {
                let a: Vec<u64> = (0..=2)
                    .map(|w| failover_run(seed, run, w, Policy::ByOffset, true).acked)
                    .collect();
                assert!(a[0] >= a[1] && a[1] >= a[2], "seed {seed} run {run}: {a:?}");
            }
        }
    }

    #[test]
    fn the_report_is_deterministic_and_seed_dependent() {
        let a = render_report(42, 300);
        assert_eq!(a, render_report(42, 300));
        assert_ne!(a, render_report(43, 300));
    }
}
