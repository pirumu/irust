//! Lab (e): an in-memory multi-version store with the same visibility rules as PostgreSQL's
//! (`xmin`, `xmax`, snapshots as `xmin:xmax:xip`), three isolation levels, and a serializability check.
//!
//! Everything is single-threaded and deterministic: a test or a lab decides in which order the
//! operations of several open transactions run, which makes each anomaly an exact interleaving
//! instead of a race. Where PostgreSQL would *wait* for a row lock, [`Db`] returns
//! [`Error::WouldBlock`] so the caller can show the situation and resume the other transaction.
//!
//! * [`Isolation::ReadCommitted`]: a new snapshot for every statement.
//! * [`Isolation::SnapshotIsolation`]: one snapshot, taken at the first statement, for the whole
//!   transaction (PostgreSQL's Repeatable Read); the first transaction to update a row wins and a
//!   later updater of the same row fails with [`Error::ConcurrentUpdate`].
//! * [`Isolation::Serializable`]: snapshot isolation plus detection of *dangerous structures*: a
//!   transaction with both an incoming and an outgoing read-write antidependency is aborted (the
//!   basic algorithm of Cahill et al.; PostgreSQL refines it to abort less often).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;

pub type Txid = u64;

/// Rows loaded with [`Db::load`] carry this transaction id; every snapshot sees it (like a frozen xid).
pub const FROZEN_XID: Txid = 2;
/// The first transaction id handed out.
pub const FIRST_XID: Txid = 100;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    ReadCommitted,
    SnapshotIsolation,
    Serializable,
}

impl fmt::Display for Isolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Isolation::ReadCommitted => "read committed",
            Isolation::SnapshotIsolation => "snapshot isolation",
            Isolation::Serializable => "serializable (SSI)",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Another open transaction holds the row; PostgreSQL would wait for it to finish.
    WouldBlock { key: String, holder: Txid },
    /// First-updater-wins: "could not serialize access due to concurrent update".
    ConcurrentUpdate { key: String },
    /// "could not serialize access due to read/write dependencies among transactions".
    ReadWriteDependency,
    /// The transaction already committed or aborted.
    NotActive,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::WouldBlock { key, holder } => write!(f, "would block: row {key:?} is being modified by transaction {holder}"),
            Error::ConcurrentUpdate { key } => write!(f, "could not serialize access due to concurrent update (row {key:?})"),
            Error::ReadWriteDependency => write!(f, "could not serialize access due to read/write dependencies among transactions"),
            Error::NotActive => write!(f, "transaction is not active"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    /// Errors after which the whole transaction must be rolled back and retried.
    pub fn is_serialization_failure(&self) -> bool {
        matches!(self, Error::ConcurrentUpdate { .. } | Error::ReadWriteDependency)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    InProgress,
    Committed,
    Aborted,
}

/// One version of a row. `xmax == 0` means nobody has deleted or replaced it (yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub xmin: Txid,
    pub xmax: Txid,
    pub value: i64,
}

/// What a transaction can see: transactions below `xmin` are finished; those at or above `xmax`
/// started later; `xip` were running when the snapshot was taken. Displayed as `xmin:xmax:xip,...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub xmin: Txid,
    pub xmax: Txid,
    pub xip: Vec<Txid>,
}

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let xip: Vec<String> = self.xip.iter().map(Txid::to_string).collect();
        write!(f, "{}:{}:{}", self.xmin, self.xmax, xip.join(","))
    }
}

struct Txn {
    iso: Isolation,
    status: Status,
    snapshot: Option<Snapshot>,
    /// Logical time of the snapshot (concurrency test for SSI) and of the commit.
    snap_seq: u64,
    commit_seq: u64,
    reads: BTreeSet<String>,
    pred_reads: Vec<String>,
    doomed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VacuumReport {
    /// Versions removed from the store.
    pub removed: usize,
    /// Versions that remain.
    pub remain: usize,
    /// Deleted or replaced versions that an open snapshot may still need.
    pub dead_not_yet_removable: usize,
}

pub struct Db {
    next_txid: Txid,
    clock: u64,
    txns: HashMap<Txid, Txn>,
    /// Versions of each key, oldest first.
    rows: BTreeMap<String, Vec<Version>>,
    /// Read-write antidependencies `(reader, writer)`: the reader read a version the writer replaced.
    edges: BTreeSet<(Txid, Txid)>,
}

impl Default for Db {
    fn default() -> Db {
        Db::new()
    }
}

fn status_of(txns: &HashMap<Txid, Txn>, x: Txid) -> Status {
    // Ids we have no record of (FROZEN_XID) belong to rows loaded before any transaction: committed.
    txns.get(&x).map_or(Status::Committed, |t| t.status)
}

fn committed_in(txns: &HashMap<Txid, Txn>, x: Txid, snap: &Snapshot) -> bool {
    x < snap.xmax && !snap.xip.contains(&x) && status_of(txns, x) == Status::Committed
}

/// PostgreSQL's rule: the inserter must be me or committed in my snapshot, and the deleter must not be.
fn visible(txns: &HashMap<Txid, Txn>, v: &Version, snap: &Snapshot, me: Txid) -> bool {
    let inserted = v.xmin == me || committed_in(txns, v.xmin, snap);
    let deleted = v.xmax != 0 && (v.xmax == me || committed_in(txns, v.xmax, snap));
    inserted && !deleted
}

impl Db {
    pub fn new() -> Db {
        Db { next_txid: FIRST_XID, clock: 0, txns: HashMap::new(), rows: BTreeMap::new(), edges: BTreeSet::new() }
    }

    /// Inserts initial data visible to everyone.
    pub fn load(&mut self, key: &str, value: i64) {
        self.rows.entry(key.to_string()).or_default().push(Version { xmin: FROZEN_XID, xmax: 0, value });
    }

    pub fn begin(&mut self, iso: Isolation) -> Txid {
        let id = self.next_txid;
        self.next_txid += 1;
        self.txns.insert(
            id,
            Txn {
                iso,
                status: Status::InProgress,
                snapshot: None,
                snap_seq: 0,
                commit_seq: u64::MAX,
                reads: BTreeSet::new(),
                pred_reads: Vec::new(),
                doomed: false,
            },
        );
        id
    }

    fn take_snapshot(&self, me: Txid) -> Snapshot {
        let latest_completed = self.txns.iter().filter(|(_, t)| t.status != Status::InProgress).map(|(&id, _)| id).max();
        let xmax = latest_completed.map_or(FIRST_XID, |x| x + 1);
        let mut xip: Vec<Txid> = self
            .txns
            .iter()
            .filter(|&(&id, t)| t.status == Status::InProgress && id != me && id < xmax)
            .map(|(&id, _)| id)
            .collect();
        xip.sort_unstable();
        let xmin = xip.iter().copied().chain([xmax]).chain((me < xmax).then_some(me)).min().expect("non-empty");
        Snapshot { xmin, xmax, xip }
    }

    /// The snapshot for the statement about to run: fresh in Read Committed, otherwise taken once,
    /// at the first statement of the transaction (not at `BEGIN`).
    fn statement_snapshot(&mut self, id: Txid) -> Result<Snapshot> {
        let txn = self.txns.get(&id).ok_or(Error::NotActive)?;
        if txn.status != Status::InProgress {
            return Err(Error::NotActive);
        }
        if self.is_doomed(id) {
            self.abort(id);
            return Err(Error::ReadWriteDependency);
        }
        let fresh = txn.iso == Isolation::ReadCommitted || txn.snapshot.is_none();
        if fresh {
            let snap = self.take_snapshot(id);
            self.clock += 1;
            let seq = self.clock;
            let txn = self.txns.get_mut(&id).expect("exists");
            if txn.snapshot.is_none() {
                txn.snap_seq = seq;
            }
            txn.snapshot = Some(snap);
        }
        Ok(self.txns[&id].snapshot.clone().expect("just set"))
    }

    pub fn snapshot_of(&self, id: Txid) -> Option<&Snapshot> {
        self.txns.get(&id)?.snapshot.as_ref()
    }

    /// Value of `key` in the transaction's snapshot.
    pub fn read(&mut self, id: Txid, key: &str) -> Result<Option<i64>> {
        let snap = self.statement_snapshot(id)?;
        let value = self.rows.get(key).and_then(|vs| vs.iter().rev().find(|v| visible(&self.txns, v, &snap, id))).map(|v| v.value);
        self.track_read(id, key, &snap)?;
        Ok(value)
    }

    /// All visible rows whose key starts with `prefix`, in key order. For a serializable
    /// transaction this is a *predicate read*: a later insert under the prefix by a concurrent
    /// transaction is a read-write conflict even though no row existed to lock.
    pub fn scan_prefix(&mut self, id: Txid, prefix: &str) -> Result<Vec<(String, i64)>> {
        let snap = self.statement_snapshot(id)?;
        let mut out = Vec::new();
        let mut keys = Vec::new();
        for (k, versions) in self.rows.range(prefix.to_string()..) {
            if !k.starts_with(prefix) {
                break;
            }
            keys.push(k.clone());
            if let Some(v) = versions.iter().rev().find(|v| visible(&self.txns, v, &snap, id)) {
                out.push((k.clone(), v.value));
            }
        }
        if self.txns[&id].iso == Isolation::Serializable {
            self.txns.get_mut(&id).expect("exists").pred_reads.push(prefix.to_string());
            for k in keys {
                self.track_read(id, &k, &snap)?;
            }
        }
        Ok(out)
    }

    /// Serializable only: remember the read, and record an antidependency `me -> W` for every
    /// concurrent writer `W` whose version of `key` is not in my snapshot.
    fn track_read(&mut self, id: Txid, key: &str, snap: &Snapshot) -> Result<()> {
        if self.txns[&id].iso != Isolation::Serializable {
            return Ok(());
        }
        self.txns.get_mut(&id).expect("exists").reads.insert(key.to_string());
        let mut writers = Vec::new();
        if let Some(versions) = self.rows.get(key) {
            for v in versions {
                for w in [v.xmin, v.xmax] {
                    let hidden = w != 0 && w != id && w != FROZEN_XID && !committed_in(&self.txns, w, snap);
                    if hidden && status_of(&self.txns, w) != Status::Aborted {
                        writers.push(w);
                    }
                }
            }
        }
        for w in writers {
            self.add_edge(id, w, id)?;
        }
        Ok(())
    }

    /// `UPDATE`/`INSERT`/`DELETE` of one row: `f` gets the value visible in the statement's snapshot
    /// and returns the new value (`None` deletes). Returns the old value.
    pub fn modify(&mut self, id: Txid, key: &str, f: impl FnOnce(Option<i64>) -> Option<i64>) -> Result<Option<i64>> {
        let snap = self.statement_snapshot(id)?;
        let versions = self.rows.entry(key.to_string()).or_default();
        let visible_idx = versions.iter().rposition(|v| visible(&self.txns, v, &snap, id));
        let old = visible_idx.map(|i| versions[i].value);
        let new = f(old);
        if old.is_none() && new.is_none() {
            if self.rows.get(key).is_some_and(Vec::is_empty) {
                self.rows.remove(key);
            }
            return Ok(None);
        }
        match visible_idx {
            Some(i) => {
                let xmax = versions[i].xmax;
                if xmax != 0 && xmax != id {
                    match status_of(&self.txns, xmax) {
                        Status::InProgress => return Err(Error::WouldBlock { key: key.to_string(), holder: xmax }),
                        Status::Committed => return Err(Error::ConcurrentUpdate { key: key.to_string() }),
                        Status::Aborted => {}
                    }
                }
                versions[i].xmax = id;
            }
            None => {
                // Inserting: any version by someone else that I cannot see is a conflicting insert.
                for v in versions.iter() {
                    if v.xmin == id {
                        continue;
                    }
                    match status_of(&self.txns, v.xmin) {
                        Status::InProgress => return Err(Error::WouldBlock { key: key.to_string(), holder: v.xmin }),
                        Status::Committed if !committed_in(&self.txns, v.xmin, &snap) && (v.xmax == 0 || !committed_in(&self.txns, v.xmax, &snap)) => {
                            return Err(Error::ConcurrentUpdate { key: key.to_string() });
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(value) = new {
            versions.push(Version { xmin: id, xmax: 0, value });
        }
        self.track_write(id, key)?;
        Ok(old)
    }

    pub fn write(&mut self, id: Txid, key: &str, value: i64) -> Result<()> {
        self.modify(id, key, |_| Some(value)).map(drop)
    }

    /// Returns whether a row was deleted.
    pub fn delete(&mut self, id: Txid, key: &str) -> Result<bool> {
        self.modify(id, key, |_| None).map(|old| old.is_some())
    }

    /// `UPDATE ... SET v = f(v)` as one statement.
    pub fn update(&mut self, id: Txid, key: &str, f: impl FnOnce(i64) -> i64) -> Result<Option<i64>> {
        self.modify(id, key, |old| old.map(f))
    }

    /// `SELECT ... FOR UPDATE`: PostgreSQL's docs say the row must actually be updated, even if no
    /// value changes; so lock = write the value back.
    pub fn lock_for_update(&mut self, id: Txid, key: &str) -> Result<Option<i64>> {
        self.modify(id, key, |old| old)
    }

    /// Serializable only: `W = me` wrote `key`; every concurrent serializable reader of it gets an
    /// antidependency `reader -> me`.
    fn track_write(&mut self, id: Txid, key: &str) -> Result<()> {
        if self.txns[&id].iso != Isolation::Serializable {
            return Ok(());
        }
        let my_snap_seq = self.txns[&id].snap_seq;
        let readers: Vec<Txid> = self
            .txns
            .iter()
            .filter(|&(&r, t)| {
                r != id
                    && t.iso == Isolation::Serializable
                    && t.status != Status::Aborted
                    && t.snapshot.is_some()
                    && t.commit_seq > my_snap_seq // not committed before my snapshot: concurrent with me
                    && (t.reads.contains(key) || t.pred_reads.iter().any(|p| key.starts_with(p.as_str())))
            })
            .map(|(&r, _)| r)
            .collect();
        for r in readers {
            self.add_edge(r, id, id)?;
        }
        Ok(())
    }

    fn live(&self, t: Txid) -> bool {
        status_of(&self.txns, t) != Status::Aborted
    }

    fn has_in(&self, t: Txid) -> bool {
        self.edges.iter().any(|&(r, w)| w == t && self.live(r))
    }

    fn has_out(&self, t: Txid) -> bool {
        self.edges.iter().any(|&(r, w)| r == t && self.live(w))
    }

    /// Still marked for failure *and* still a pivot: a doom is lifted if the transaction on the other
    /// end of one of its edges has meanwhile rolled back.
    fn is_doomed(&self, id: Txid) -> bool {
        self.txns.get(&id).is_some_and(|t| t.doomed) && self.has_in(id) && self.has_out(id)
    }

    /// Records `reader -> writer`. If that creates a dangerous structure (a transaction with an
    /// edge in and an edge out, the *pivot*), one transaction in it must fail: the operating
    /// transaction right away if it is the pivot, otherwise the pivot if it is still open (marked
    /// to fail at its next step), otherwise the operating transaction.
    fn add_edge(&mut self, reader: Txid, writer: Txid, operating: Txid) -> Result<()> {
        if reader == writer || !self.edges.insert((reader, writer)) {
            return Ok(());
        }
        let other = if reader == operating { writer } else { reader };
        for pivot in [operating, other] {
            if self.has_in(pivot) && self.has_out(pivot) {
                let pivot_open = self.txns.get(&pivot).is_some_and(|t| t.status == Status::InProgress);
                if pivot == operating || !pivot_open {
                    self.abort(operating);
                    return Err(Error::ReadWriteDependency);
                }
                self.txns.get_mut(&pivot).expect("exists").doomed = true;
            }
        }
        Ok(())
    }

    pub fn commit(&mut self, id: Txid) -> Result<()> {
        let txn = self.txns.get(&id).ok_or(Error::NotActive)?;
        if txn.status != Status::InProgress {
            return Err(Error::NotActive);
        }
        if self.is_doomed(id) {
            self.abort(id);
            return Err(Error::ReadWriteDependency);
        }
        self.clock += 1;
        let seq = self.clock;
        let txn = self.txns.get_mut(&id).expect("exists");
        txn.status = Status::Committed;
        txn.commit_seq = seq;
        Ok(())
    }

    /// Rolls back: the transaction's versions become invisible garbage and its row claims are released.
    pub fn abort(&mut self, id: Txid) {
        if let Some(t) = self.txns.get_mut(&id)
            && t.status == Status::InProgress
        {
            t.status = Status::Aborted;
        }
    }

    pub fn is_active(&self, id: Txid) -> bool {
        self.txns.get(&id).is_some_and(|t| t.status == Status::InProgress)
    }

    /// Every stored version of `key`, oldest first, as `(xmin, xmax, value, state of xmin)`: what
    /// `SELECT xmin, xmax, * FROM t` plus the commit log would tell you.
    pub fn versions(&self, key: &str) -> Vec<Version> {
        self.rows.get(key).cloned().unwrap_or_default()
    }

    pub fn state_of(&self, x: Txid) -> &'static str {
        if x == 0 {
            return "-";
        }
        match status_of(&self.txns, x) {
            Status::InProgress => "in progress",
            Status::Committed => "committed",
            Status::Aborted => "aborted",
        }
    }

    pub fn version_count(&self) -> usize {
        self.rows.values().map(Vec::len).sum()
    }

    /// Latest committed value of every key, via a throwaway read-committed transaction.
    pub fn committed_state(&mut self) -> BTreeMap<String, i64> {
        let t = self.begin(Isolation::ReadCommitted);
        let out = self.scan_prefix(t, "").expect("fresh transaction").into_iter().collect();
        self.abort(t);
        out
    }

    /// Removes versions no snapshot can see any more: versions written by aborted transactions, and
    /// versions deleted or replaced by a transaction that committed before the oldest snapshot still
    /// open (the *horizon*). Like `VACUUM`, it cannot remove what an open snapshot may still read.
    pub fn vacuum(&mut self) -> VacuumReport {
        let horizon = self
            .txns
            .values()
            .filter(|t| t.status == Status::InProgress)
            .filter_map(|t| t.snapshot.as_ref().filter(|_| t.iso != Isolation::ReadCommitted))
            .map(|s| s.xmin)
            .min()
            .unwrap_or(Txid::MAX);
        let mut report = VacuumReport::default();
        let txns = &self.txns;
        for versions in self.rows.values_mut() {
            versions.retain(|v| {
                let aborted_insert = status_of(txns, v.xmin) == Status::Aborted;
                let deleted = v.xmax != 0 && status_of(txns, v.xmax) == Status::Committed;
                if aborted_insert || (deleted && v.xmax < horizon) {
                    report.removed += 1;
                    return false;
                }
                if deleted {
                    report.dead_not_yet_removable += 1;
                }
                true
            });
        }
        self.rows.retain(|_, v| !v.is_empty());
        report.remain = self.version_count();
        report
    }
}

/// Anomaly scenarios: each runs one exact interleaving under a given isolation level and reports
/// whether the anomaly happened. The unit tests assert the table; `bin/mvcc.rs` prints it.
pub mod scenarios {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Outcome {
        pub anomaly: bool,
        pub detail: String,
    }

    fn outcome(anomaly: bool, detail: impl Into<String>) -> Outcome {
        Outcome { anomaly, detail: detail.into() }
    }

    /// T1 writes x without committing; T2 reads x. A dirty read would show T2 the uncommitted value.
    pub fn dirty_read(iso: Isolation) -> Outcome {
        let mut db = Db::new();
        db.load("x", 100);
        let (t1, t2) = (db.begin(iso), db.begin(iso));
        db.write(t1, "x", 999).unwrap();
        let seen = db.read(t2, "x").unwrap();
        db.abort(t1);
        outcome(seen != Some(100), format!("T2 reads x = {seen:?} while T1's write of 999 is uncommitted"))
    }

    /// T1 writes x without committing; T2 tries to overwrite it.
    pub fn dirty_write(iso: Isolation) -> Outcome {
        let mut db = Db::new();
        db.load("x", 100);
        let (t1, t2) = (db.begin(iso), db.begin(iso));
        db.write(t1, "x", 1).unwrap();
        match db.write(t2, "x", 2) {
            Err(e @ Error::WouldBlock { .. }) => outcome(false, format!("T2's write: {e}")),
            other => outcome(true, format!("T2 overwrote an uncommitted write: {other:?}")),
        }
    }

    /// x = y = 50. T1 reads x; T2 moves 40 from x to y and commits; T1 reads y. Invariant: x + y = 100.
    pub fn read_skew(iso: Isolation) -> Outcome {
        let mut db = Db::new();
        db.load("x", 50);
        db.load("y", 50);
        let t1 = db.begin(iso);
        let x = db.read(t1, "x").unwrap().unwrap();
        let t2 = db.begin(iso);
        db.update(t2, "x", |v| v - 40).unwrap();
        db.update(t2, "y", |v| v + 40).unwrap();
        db.commit(t2).unwrap();
        let y = db.read(t1, "y").unwrap().unwrap();
        db.commit(t1).unwrap();
        outcome(x + y != 100, format!("T1 read x = {x}, then y = {y}: sum {}", x + y))
    }

    /// A counter c = 0. Two transactions each *read* it, add 1 in application code, and write the
    /// result back. A failed transaction is retried from the start, as the PostgreSQL docs prescribe.
    pub fn lost_update(iso: Isolation) -> Outcome {
        let mut db = Db::new();
        db.load("c", 0);
        let (t1, t2) = (db.begin(iso), db.begin(iso));
        let r1 = db.read(t1, "c").unwrap().unwrap();
        let r2 = db.read(t2, "c").unwrap().unwrap();
        db.write(t1, "c", r1 + 1).unwrap();
        db.commit(t1).unwrap();
        let mut note = String::new();
        match db.write(t2, "c", r2 + 1) {
            Ok(()) => db.commit(t2).unwrap(),
            Err(e) => {
                db.abort(t2);
                let t3 = db.begin(iso); // the retry
                let r3 = db.read(t3, "c").unwrap().unwrap();
                db.write(t3, "c", r3 + 1).unwrap();
                db.commit(t3).unwrap();
                note = format!("T2 failed ({e}); retried as T3 -> ");
            }
        }
        let c = db.committed_state()["c"];
        outcome(c != 2, format!("{note}two increments leave c = {c}"))
    }

    /// The same counter, but each increment is one statement `UPDATE c SET v = v + 1`.
    pub fn atomic_update(iso: Isolation) -> Outcome {
        let mut db = Db::new();
        db.load("c", 0);
        let (t1, t2) = (db.begin(iso), db.begin(iso));
        db.update(t1, "c", |v| v + 1).unwrap();
        let mut note = match db.update(t2, "c", |v| v + 1) {
            Err(Error::WouldBlock { holder, .. }) => format!("T2 waits for T{holder}; "),
            other => panic!("expected T2 to wait: {other:?}"),
        };
        db.commit(t1).unwrap();
        let mut t2 = t2;
        if let Err(e) = db.update(t2, "c", |v| v + 1) {
            note.push_str(&format!("after T1 commits T2's retry of the statement fails ({e}); T2 restarts; "));
            db.abort(t2);
            t2 = db.begin(iso);
            db.update(t2, "c", |v| v + 1).unwrap();
        }
        db.commit(t2).unwrap();
        let c = db.committed_state()["c"];
        outcome(c != 2, format!("{note}c = {c}"))
    }

    /// The on-call example: Alice and Bob are on call; the rule is that at least one must stay.
    /// Each transaction checks "are there at least two on call?" and, if so, takes its doctor off call.
    /// `fix` adds a way to avoid the anomaly.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OnCallFix {
        None,
        /// Both transactions first update a shared `shift` row, so their writes conflict.
        MaterializedConflict,
        /// Each transaction `SELECT ... FOR UPDATE`s the on-call rows it reads.
        ForUpdate,
    }

    fn on_call(db: &mut Db, t: Txid) -> Result<usize> {
        Ok(db.scan_prefix(t, "oncall/")?.iter().filter(|(_, v)| *v == 1).count())
    }

    /// One doctor's transaction. `Ok(true)` if the doctor went off call, `Ok(false)` if the check refused.
    fn go_off_call(db: &mut Db, t: Txid, doctor: &str, fix: OnCallFix) -> Result<bool> {
        match fix {
            OnCallFix::None => {}
            OnCallFix::MaterializedConflict => db.update(t, "shift", |v| v + 1).map(drop)?,
            OnCallFix::ForUpdate => {
                for k in ["oncall/alice", "oncall/bob"] {
                    db.lock_for_update(t, k)?;
                }
            }
        }
        if on_call(db, t)? >= 2 {
            db.write(t, &format!("oncall/{doctor}"), 0)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn write_skew(iso: Isolation, fix: OnCallFix) -> Outcome {
        let mut db = Db::new();
        db.load("oncall/alice", 1);
        db.load("oncall/bob", 1);
        db.load("shift", 0);
        let mut log = Vec::new();
        let (ta, tb) = (db.begin(iso), db.begin(iso));
        let step = |db: &mut Db, t, who: &str, log: &mut Vec<String>| match go_off_call(db, t, who, fix) {
            Ok(went) => {
                log.push(format!("{who}: {}", if went { "saw 2 on call, went off" } else { "saw 1 on call, stayed" }));
                true
            }
            Err(e) => {
                log.push(format!("{who}: {e}"));
                db.abort(t);
                false
            }
        };
        // Interleaved: both transactions run their statements before either commits.
        let (ok_a, ok_b) = (step(&mut db, ta, "alice", &mut log), step(&mut db, tb, "bob", &mut log));
        let commit = |db: &mut Db, t, ok: bool, who: &str, log: &mut Vec<String>| {
            if ok {
                if let Err(e) = db.commit(t) {
                    log.push(format!("{who} commit: {e}"));
                    return false;
                }
                return true;
            }
            false
        };
        let ca = commit(&mut db, ta, ok_a, "alice", &mut log);
        let cb = commit(&mut db, tb, ok_b, "bob", &mut log);
        // Any transaction that failed is retried once, alone.
        for (who, committed) in [("alice", ca), ("bob", cb)] {
            if !committed {
                let t = db.begin(iso);
                match go_off_call(&mut db, t, who, fix).and_then(|went| db.commit(t).map(|()| went)) {
                    Ok(went) => log.push(format!("retry of {who}: {}", if went { "went off call" } else { "saw 1 on call, stayed" })),
                    Err(e) => log.push(format!("retry of {who}: {e}")),
                }
            }
        }
        let state = db.committed_state();
        let on = state.iter().filter(|(k, v)| k.starts_with("oncall/") && **v == 1).count();
        outcome(on == 0, format!("{}; doctors on call at the end: {on}", log.join("; ")))
    }

    /// Booking a room: "if nobody has booked 9am, book it". The rows that would conflict do not exist yet.
    /// With `lock_slot` both transactions first update a pre-created `slot` row (a materialized conflict).
    pub fn double_booking(iso: Isolation, lock_slot: bool) -> Outcome {
        let mut db = Db::new();
        db.load("slot/r1/9am", 0);
        let (ta, tb) = (db.begin(iso), db.begin(iso));
        let mut log = Vec::new();
        let books = |db: &mut Db, t: Txid, who: &str, log: &mut Vec<String>| -> Result<()> {
            if lock_slot {
                db.update(t, "slot/r1/9am", |v| v + 1)?;
            }
            let taken = db.scan_prefix(t, "booking/r1/9am/")?;
            if taken.is_empty() {
                db.write(t, &format!("booking/r1/9am/{who}"), 1)?;
                log.push(format!("{who}: room free, booked"));
            } else {
                log.push(format!("{who}: room taken, did not book"));
            }
            Ok(())
        };
        let ra = books(&mut db, ta, "alice", &mut log);
        let rb = books(&mut db, tb, "bob", &mut log);
        for (who, t, r) in [("alice", ta, ra), ("bob", tb, rb)] {
            match r.and_then(|()| db.commit(t)) {
                Ok(()) => {}
                Err(e) => {
                    log.push(format!("{who}: {e}"));
                    db.abort(t);
                }
            }
        }
        let bookings = db.committed_state().keys().filter(|k| k.starts_with("booking/")).count();
        outcome(bookings > 1, format!("{}; committed bookings for 9am: {bookings}", log.join("; ")))
    }
}

/// Random transactions on random schedules, and a brute-force check that the committed outcome
/// equals *some* serial execution.
pub mod fuzz {
    use super::*;
    use crate::rng::Rng;

    pub const KEYS: [&str; 4] = ["k0", "k1", "k2", "k3"];

    #[derive(Debug, Clone)]
    pub enum Op {
        Read(&'static str),
        Scan,
        /// Write `sum of everything this transaction has read so far + the constant`.
        Write(&'static str, i64),
    }

    /// What a transaction observed, in order: the value of each read, the rows of each scan.
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct Observations {
        pub reads: Vec<Option<i64>>,
        pub scans: Vec<Vec<(String, i64)>>,
    }

    pub fn random_program(rng: &mut Rng) -> Vec<Op> {
        (0..2 + rng.index(3))
            .map(|_| match rng.below(10) {
                0..=3 => Op::Read(KEYS[rng.index(4)]),
                4..=5 => Op::Scan,
                _ => Op::Write(KEYS[rng.index(4)], rng.below(5) as i64),
            })
            .collect()
    }

    fn fresh_db() -> Db {
        let mut db = Db::new();
        db.load("k0", 10);
        db.load("k1", 20);
        db
    }

    fn run_op(db: &mut Db, t: Txid, op: &Op, seen: &mut Observations) -> Result<()> {
        match op {
            Op::Read(k) => seen.reads.push(db.read(t, k)?),
            Op::Scan => seen.scans.push(db.scan_prefix(t, "k")?),
            Op::Write(k, c) => {
                let sum: i64 = seen.reads.iter().flatten().sum::<i64>() + seen.scans.iter().flatten().map(|(_, v)| *v).sum::<i64>();
                db.write(t, k, sum + c)?;
            }
        }
        Ok(())
    }

    #[derive(Debug, Clone, Default)]
    pub struct Trial {
        /// Committed transactions in commit order, with what each observed.
        pub committed: Vec<(usize, Observations)>,
        pub aborted: usize,
        pub final_state: BTreeMap<String, i64>,
        pub serializable: bool,
        /// Which transaction took each step (an operation, or its commit), in order.
        pub schedule: Vec<usize>,
    }

    /// Runs `programs` interleaved at random under `iso`. A transaction that gets any error is rolled
    /// back (a blocked one counts as a deadlock victim); nothing is retried.
    pub fn trial(programs: &[Vec<Op>], iso: Isolation, rng: &mut Rng) -> Trial {
        let mut db = fresh_db();
        let ids: Vec<Txid> = programs.iter().map(|_| db.begin(iso)).collect();
        let mut next = vec![0usize; programs.len()];
        let mut seen = vec![Observations::default(); programs.len()];
        let mut done = vec![false; programs.len()];
        let mut committed_order = Vec::new();
        let mut schedule = Vec::new();
        let mut aborted = 0;
        while done.iter().any(|d| !d) {
            let open: Vec<usize> = (0..programs.len()).filter(|&i| !done[i]).collect();
            let i = open[rng.index(open.len())];
            schedule.push(i);
            let result = if next[i] < programs[i].len() {
                let r = run_op(&mut db, ids[i], &programs[i][next[i]], &mut seen[i]);
                next[i] += 1;
                r
            } else {
                done[i] = true;
                db.commit(ids[i]).inspect(|()| committed_order.push(i))
            };
            if result.is_err() {
                db.abort(ids[i]);
                done[i] = true;
                aborted += 1;
            }
        }
        let committed: Vec<(usize, Observations)> = committed_order.iter().map(|&i| (i, seen[i].clone())).collect();
        let final_state = db.committed_state();
        let serializable = is_serializable(programs, &committed, &final_state);
        Trial { committed, aborted, final_state, serializable, schedule }
    }

    fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut out = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let head = rest.remove(i);
            for mut p in permutations(&rest) {
                p.insert(0, head);
                out.push(p);
            }
        }
        out
    }

    /// Is there an order of the committed transactions in which running them one after another
    /// reproduces every value they read and the final state?
    pub fn is_serializable(programs: &[Vec<Op>], committed: &[(usize, Observations)], final_state: &BTreeMap<String, i64>) -> bool {
        let ids: Vec<usize> = committed.iter().map(|(i, _)| *i).collect();
        permutations(&ids).into_iter().any(|order| {
            let mut db = fresh_db();
            for &i in &order {
                let t = db.begin(Isolation::SnapshotIsolation);
                let mut seen = Observations::default();
                for op in &programs[i] {
                    run_op(&mut db, t, op, &mut seen).expect("serial execution cannot conflict");
                }
                db.commit(t).expect("serial commit");
                let want = &committed.iter().find(|(j, _)| *j == i).expect("committed").1;
                if &seen != want {
                    return false;
                }
            }
            &db.committed_state() == final_state
        })
    }
}

#[cfg(test)]
mod tests {
    use super::scenarios::*;
    use super::*;
    use crate::rng::Rng;

    const RC: Isolation = Isolation::ReadCommitted;
    const SI: Isolation = Isolation::SnapshotIsolation;
    const SER: Isolation = Isolation::Serializable;

    #[test]
    fn versions_carry_xmin_and_xmax_like_postgresql() {
        let mut db = Db::new();
        db.load("x", 1);
        let t = db.begin(RC);
        assert_eq!(t, FIRST_XID);
        db.write(t, "x", 2).unwrap();
        assert_eq!(
            db.versions("x"),
            vec![Version { xmin: FROZEN_XID, xmax: t, value: 1 }, Version { xmin: t, xmax: 0, value: 2 }]
        );
        assert_eq!((db.state_of(t), db.state_of(FROZEN_XID)), ("in progress", "committed"));
        db.commit(t).unwrap();
        // A later reader sees the new version only.
        let r = db.begin(RC);
        assert_eq!(db.read(r, "x").unwrap(), Some(2));
        // A rolled-back update leaves the old version in charge: xmax is non-zero but harmless.
        let t2 = db.begin(RC);
        db.write(t2, "x", 3).unwrap();
        db.abort(t2);
        assert_eq!(db.state_of(t2), "aborted");
        assert_eq!(db.read(r, "x").unwrap(), Some(2));
        let t3 = db.begin(RC);
        db.write(t3, "x", 4).unwrap(); // the claim of the aborted updater does not block
        db.commit(t3).unwrap();
        assert_eq!(db.committed_state()["x"], 4);
    }

    #[test]
    fn snapshot_text_follows_the_postgresql_format_and_is_taken_at_the_first_statement() {
        let mut db = Db::new();
        db.load("x", 1);
        db.load("z", 0);
        let (a, b, c, d) = (db.begin(SI), db.begin(SI), db.begin(SI), db.begin(RC));
        assert!(db.snapshot_of(b).is_none(), "BEGIN alone does not take a snapshot");
        db.write(a, "x", 2).unwrap(); // a is still running when b's snapshot is taken
        db.write(d, "z", 1).unwrap();
        db.commit(d).unwrap(); // d finishes before b's first statement; ids above d are not yet "completed"
        assert_eq!(db.read(b, "x").unwrap(), Some(1));
        let snap = db.snapshot_of(b).unwrap();
        // xmin: lowest running transaction; xmax: one past the highest completed one (d); xip: running ones in between.
        assert_eq!(snap.to_string(), format!("{a}:{}:{a},{c}", d + 1));
        assert_eq!(snap.xip, vec![a, c]);
        assert_eq!(db.read(b, "z").unwrap(), Some(1), "d finished before the snapshot");
        db.commit(a).unwrap();
        assert_eq!(db.read(b, "x").unwrap(), Some(1), "a committed after the snapshot: still invisible, it is in xip");
        let later = db.begin(SI);
        assert_eq!(db.read(later, "x").unwrap(), Some(2), "a new snapshot sees it");
    }

    #[test]
    fn dirty_reads_and_dirty_writes_are_prevented_at_every_level() {
        for iso in [RC, SI, SER] {
            let r = dirty_read(iso);
            assert!(!r.anomaly, "{iso}: {r:?}");
            let w = dirty_write(iso);
            assert!(!w.anomaly && w.detail.contains("would block"), "{iso}: {w:?}");
        }
    }

    #[test]
    fn read_skew_happens_in_read_committed_not_in_snapshot_isolation() {
        let rc = read_skew(RC);
        assert!(rc.anomaly && rc.detail.contains("sum 140"), "{rc:?}"); // x = 50 then y = 90
        assert!(!read_skew(SI).anomaly);
        assert!(!read_skew(SER).anomaly);
    }

    #[test]
    fn lost_update_happens_in_read_committed_and_is_an_error_to_retry_in_snapshot_isolation() {
        let rc = lost_update(RC);
        assert!(rc.anomaly && rc.detail.contains("c = 1"), "{rc:?}");
        for iso in [SI, SER] {
            let r = lost_update(iso);
            assert!(!r.anomaly && r.detail.contains("concurrent update") && r.detail.contains("c = 2"), "{iso}: {r:?}");
        }
    }

    #[test]
    fn one_statement_updates_do_not_lose_updates_at_any_level() {
        for iso in [RC, SI, SER] {
            let r = atomic_update(iso);
            assert!(!r.anomaly && r.detail.contains("c = 2") && r.detail.contains("waits"), "{iso}: {r:?}");
        }
    }

    #[test]
    fn write_skew_is_allowed_by_snapshot_isolation_and_stopped_by_ssi() {
        for iso in [RC, SI] {
            let r = write_skew(iso, OnCallFix::None);
            assert!(r.anomaly && r.detail.contains("on call at the end: 0"), "{iso}: {r:?}");
        }
        let r = write_skew(SER, OnCallFix::None);
        assert!(!r.anomaly && r.detail.contains("read/write dependencies") && r.detail.contains("stayed"), "{r:?}");
        assert!(r.detail.ends_with("on call at the end: 1"), "{r:?}");
    }

    #[test]
    fn write_skew_is_fixed_by_materializing_the_conflict_or_by_locking_the_rows_read() {
        for fix in [OnCallFix::MaterializedConflict, OnCallFix::ForUpdate] {
            for iso in [RC, SI, SER] {
                let r = write_skew(iso, fix);
                assert!(!r.anomaly, "{fix:?} under {iso}: {r:?}");
                assert!(r.detail.ends_with("on call at the end: 1"), "{fix:?} under {iso}: {r:?}");
            }
        }
    }

    #[test]
    fn phantom_double_booking_needs_a_predicate_read_or_a_materialized_conflict() {
        for iso in [RC, SI] {
            let r = double_booking(iso, false);
            assert!(r.anomaly, "{iso}: {r:?}");
        }
        let ser = double_booking(SER, false);
        assert!(!ser.anomaly && ser.detail.contains("read/write dependencies"), "{ser:?}");
        // The slot row exists, so updating it first makes the two transactions conflict at every level.
        for iso in [RC, SI, SER] {
            let r = double_booking(iso, true);
            assert!(!r.anomaly, "{iso}: {r:?}");
        }
    }

    #[test]
    fn select_for_update_cannot_lock_rows_that_do_not_exist_yet() {
        // Locking the rows a predicate returns locks nothing when it returns no rows: both
        // transactions still insert, in snapshot isolation.
        let mut db = Db::new();
        let (a, b) = (db.begin(SI), db.begin(SI));
        for t in [a, b] {
            let rows = db.scan_prefix(t, "booking/").unwrap();
            assert!(rows.is_empty());
            for (k, _) in rows {
                db.lock_for_update(t, &k).unwrap();
            }
        }
        db.write(a, "booking/alice", 1).unwrap();
        db.write(b, "booking/bob", 1).unwrap();
        db.commit(a).unwrap();
        db.commit(b).unwrap();
        assert_eq!(db.committed_state().len(), 2);
    }

    #[test]
    fn ssi_aborts_some_transactions_that_would_have_been_serializable() {
        // T1 reads x; T2 writes x and reads y; T3 writes y. The serial order T1, T2, T3 explains
        // every read, yet T1 -rw-> T2 -rw-> T3 makes T2 a pivot, so the basic algorithm aborts T2.
        let mut db = Db::new();
        db.load("x", 0);
        db.load("y", 0);
        let (t1, t2, t3) = (db.begin(SER), db.begin(SER), db.begin(SER));
        assert_eq!(db.read(t1, "x").unwrap(), Some(0));
        db.write(t2, "x", 1).unwrap(); // edge T1 -> T2
        assert_eq!(db.read(t2, "y").unwrap(), Some(0));
        db.write(t3, "y", 1).unwrap(); // edge T2 -> T3: T2 is now a pivot and is doomed
        db.commit(t3).unwrap();
        db.commit(t1).unwrap();
        assert_eq!(db.commit(t2), Err(Error::ReadWriteDependency));
    }

    #[test]
    fn a_read_only_transaction_and_disjoint_writers_are_never_aborted_by_ssi() {
        let mut db = Db::new();
        db.load("a", 1);
        db.load("b", 2);
        let (t1, t2, t3) = (db.begin(SER), db.begin(SER), db.begin(SER));
        db.write(t1, "a", 10).unwrap();
        db.write(t2, "b", 20).unwrap();
        assert_eq!(db.read(t3, "a").unwrap(), Some(1)); // old version: edge T3 -> T1, nothing else
        db.commit(t1).unwrap();
        db.commit(t2).unwrap();
        db.commit(t3).unwrap();
    }

    #[test]
    fn operations_on_finished_transactions_fail() {
        let mut db = Db::new();
        let t = db.begin(SI);
        db.commit(t).unwrap();
        assert_eq!(db.read(t, "x"), Err(Error::NotActive));
        assert_eq!(db.commit(t), Err(Error::NotActive));
        assert_eq!(db.read(12345, "x"), Err(Error::NotActive));
    }

    #[test]
    fn vacuum_removes_only_what_no_open_snapshot_can_see() {
        let mut db = Db::new();
        db.load("x", 0);
        let old = db.begin(SI);
        assert_eq!(db.read(old, "x").unwrap(), Some(0)); // takes the snapshot that holds the horizon back
        for i in 1..=5 {
            let t = db.begin(RC);
            db.write(t, "x", i).unwrap();
            db.commit(t).unwrap();
        }
        let aborted = db.begin(RC);
        db.write(aborted, "x", 99).unwrap();
        db.abort(aborted);
        assert_eq!(db.version_count(), 7);
        // The old snapshot still needs the first version; the others were replaced after it started too.
        let held = db.vacuum();
        assert_eq!(held, VacuumReport { removed: 1, remain: 6, dead_not_yet_removable: 5 });
        assert_eq!(db.read(old, "x").unwrap(), Some(0), "the snapshot still reads its version");
        db.commit(old).unwrap();
        let freed = db.vacuum();
        assert_eq!(freed, VacuumReport { removed: 5, remain: 1, dead_not_yet_removable: 0 });
        assert_eq!(db.committed_state()["x"], 5);
    }

    #[test]
    fn a_read_committed_transaction_does_not_hold_back_vacuum_between_statements() {
        let mut db = Db::new();
        db.load("x", 0);
        let rc = db.begin(RC);
        db.read(rc, "x").unwrap();
        let t = db.begin(RC);
        db.write(t, "x", 1).unwrap();
        db.commit(t).unwrap();
        assert_eq!(db.vacuum().removed, 1);
        assert_eq!(db.read(rc, "x").unwrap(), Some(1));
    }

    fn fuzz_counts(iso: Isolation, trials: u64) -> (u64, u64) {
        let mut rng = Rng::new(2024);
        let (mut not_serializable, mut aborted) = (0, 0);
        for _ in 0..trials {
            let programs: Vec<_> = (0..3 + rng.index(2)).map(|_| fuzz::random_program(&mut rng)).collect();
            let t = fuzz::trial(&programs, iso, &mut rng);
            not_serializable += u64::from(!t.serializable);
            aborted += t.aborted as u64;
        }
        (not_serializable, aborted)
    }

    #[test]
    fn random_schedules_are_not_always_serializable_under_snapshot_isolation_but_always_are_under_ssi() {
        let (si_bad, _) = fuzz_counts(SI, 3000);
        assert!(si_bad > 20, "snapshot isolation should produce non-serializable outcomes, got {si_bad}");
        let (ser_bad, ser_aborted) = fuzz_counts(SER, 3000);
        assert_eq!(ser_bad, 0, "SSI let a non-serializable outcome through");
        assert!(ser_aborted > 0, "SSI pays with aborts");
        let (rc_bad, _) = fuzz_counts(RC, 3000);
        assert!(rc_bad >= si_bad, "read committed is weaker than snapshot isolation here: {rc_bad} vs {si_bad}");
    }

    #[test]
    fn the_serializability_checker_rejects_a_known_write_skew() {
        use fuzz::{is_serializable, Observations, Op};
        // Two transactions that each read both rows and write a different one: a classic cycle.
        let programs = vec![vec![Op::Read("k0"), Op::Read("k1"), Op::Write("k0", 1)], vec![Op::Read("k0"), Op::Read("k1"), Op::Write("k1", 1)]];
        let seen = Observations { reads: vec![Some(10), Some(20)], scans: vec![] };
        let committed = vec![(0, seen.clone()), (1, seen)];
        // Both read the initial state and wrote 31 and 31; no serial order has the second one read 10 and 20.
        let final_state = BTreeMap::from([("k0".to_string(), 31), ("k1".to_string(), 31)]);
        assert!(!is_serializable(&programs, &committed, &final_state));
        // A single committed transaction is trivially serializable.
        let only_first = vec![(0, Observations { reads: vec![Some(10), Some(20)], scans: vec![] })];
        let fs = BTreeMap::from([("k0".to_string(), 31), ("k1".to_string(), 20)]);
        assert!(is_serializable(&programs, &only_first, &fs));
    }
}
