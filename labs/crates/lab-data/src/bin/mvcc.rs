//! Lab (e): MVCC versions (`xmin`/`xmax`), snapshots, which anomalies each isolation level allows,
//! the fixes for write skew, a VACUUM analogue, and a random-schedule serializability check.
//!
//! Run: cargo run -q --release -p lab-data --bin mvcc [trials]

use lab_data::mvcc::fuzz::{self, Op};
use lab_data::mvcc::scenarios::{self, OnCallFix, Outcome};
use lab_data::mvcc::{Db, Isolation, Version};
use lab_data::rng::Rng;

const RC: Isolation = Isolation::ReadCommitted;
const SI: Isolation = Isolation::SnapshotIsolation;
const SER: Isolation = Isolation::Serializable;

fn print_versions(db: &Db, key: &str) {
    println!("  {:>6} {:>6} {:>6}   xmin state     xmax state", "xmin", "xmax", "value");
    for Version { xmin, xmax, value } in db.versions(key) {
        let xmax_s = if xmax == 0 { "0".to_string() } else { xmax.to_string() };
        println!("  {xmin:>6} {xmax_s:>6} {value:>6}   {:<14} {}", db.state_of(xmin), db.state_of(xmax));
    }
}

fn versions_and_snapshots() {
    println!("== 1. one row, several versions: xmin, xmax and snapshots ==");
    let mut db = Db::new();
    db.load("acct", 100);
    println!("row loaded before any transaction (xmin = 2 plays the role of a frozen xid):");
    print_versions(&db, "acct");

    let writer = db.begin(RC);
    db.write(writer, "acct", 80).unwrap();
    println!("\ntransaction {writer} UPDATEs acct to 80 and has not committed:");
    print_versions(&db, "acct");

    let reader = db.begin(SI);
    let seen = db.read(reader, "acct").unwrap();
    println!(
        "\ntransaction {reader} (snapshot isolation) reads acct = {seen:?}; its snapshot is {} (xmin:xmax:xip)",
        db.snapshot_of(reader).unwrap()
    );
    db.commit(writer).unwrap();
    println!("\ntransaction {writer} commits. The reader's snapshot is fixed, so it still reads {:?};", db.read(reader, "acct").unwrap());
    let newcomer = db.begin(SI);
    println!("a transaction that starts now reads {:?}, snapshot {}", db.read(newcomer, "acct").unwrap(), db.snapshot_of(newcomer).unwrap());

    let rolled_back = db.begin(RC);
    db.write(rolled_back, "acct", 60).unwrap();
    db.abort(rolled_back);
    println!("\ntransaction {rolled_back} updates acct to 60 and rolls back; its version stays in the table, invisible:");
    print_versions(&db, "acct");
    db.commit(reader).unwrap();
    db.commit(newcomer).unwrap();
    println!("({} versions of one logical row are stored; vacuum is what removes the dead ones, section 3)", db.version_count());
}

type Scenario = Box<dyn Fn(Isolation) -> Outcome>;

fn matrix() {
    println!("\n== 2. which anomalies does each isolation level allow? ==");
    let table: Vec<(&str, Scenario)> = vec![
        ("dirty read", Box::new(scenarios::dirty_read)),
        ("dirty write", Box::new(scenarios::dirty_write)),
        ("read skew", Box::new(scenarios::read_skew)),
        ("lost update (read in app, then write)", Box::new(scenarios::lost_update)),
        ("lost update with UPDATE SET v = v + 1", Box::new(scenarios::atomic_update)),
        ("write skew: doctors on call", Box::new(|i| scenarios::write_skew(i, OnCallFix::None))),
        ("  fix 1: both update a shared row first", Box::new(|i| scenarios::write_skew(i, OnCallFix::MaterializedConflict))),
        ("  fix 2: SELECT ... FOR UPDATE the rows read", Box::new(|i| scenarios::write_skew(i, OnCallFix::ForUpdate))),
        ("phantom write skew: double booking", Box::new(|i| scenarios::double_booking(i, false))),
        ("  fix: both update a shared slot row first", Box::new(|i| scenarios::double_booking(i, true))),
    ];
    println!("{:<46} {:<16} {:<20} {:<20}", "scenario", "read committed", "snapshot isolation", "serializable (SSI)");
    let results: Vec<[Outcome; 3]> = table.iter().map(|(_, f)| [f(RC), f(SI), f(SER)]).collect();
    for ((name, _), r) in table.iter().zip(&results) {
        let cell = |o: &Outcome| if o.anomaly { "ANOMALY" } else { "prevented" };
        println!("{name:<46} {:<16} {:<20} {:<20}", cell(&r[0]), cell(&r[1]), cell(&r[2]));
    }
    println!("\ndetails (what happened in each run):");
    for ((name, _), r) in table.iter().zip(&results) {
        println!("{}", name.trim_start());
        for (iso, o) in [RC, SI, SER].into_iter().zip(r) {
            println!("  [{iso}] {}", o.detail);
        }
    }
}

fn vacuum() {
    println!("\n== 3. VACUUM analogue: an open snapshot holds dead versions back ==");
    let mut db = Db::new();
    db.load("x", 0);
    let old = db.begin(SI);
    db.read(old, "x").unwrap();
    println!("transaction {old} (snapshot isolation) reads x and stays open; snapshot {}", db.snapshot_of(old).unwrap());
    for i in 1..=5 {
        let t = db.begin(RC);
        db.write(t, "x", i).unwrap();
        db.commit(t).unwrap();
    }
    let aborted = db.begin(RC);
    db.write(aborted, "x", 99).unwrap();
    db.abort(aborted);
    println!("5 committed updates and 1 rolled-back update later: {} versions stored", db.version_count());
    let r = db.vacuum();
    println!("vacuum while {old} is open : {} removed, {} remain, {} are dead but not yet removable", r.removed, r.remain, r.dead_not_yet_removable);
    db.commit(old).unwrap();
    let r = db.vacuum();
    println!("vacuum after {old} commits : {} removed, {} remain, {} are dead but not yet removable", r.removed, r.remain, r.dead_not_yet_removable);
}

fn fmt_op(op: &Op) -> String {
    match op {
        Op::Read(k) => format!("read {k}"),
        Op::Scan => "scan k*".to_string(),
        Op::Write(k, c) => format!("write {k} = (sum of values read) + {c}"),
    }
}

fn fuzz_section(trials: u64) {
    println!("\n== 4. random transactions, random interleavings, brute-force serializability check ==");
    println!("{trials} trials per level; each trial: 3-4 transactions of 2-4 operations on keys k0..k3 (k0, k1 exist initially)");
    println!("a transaction that gets any error is rolled back and not retried; \"serializable\" = some serial order of the");
    println!("committed transactions reproduces everything they read and the final state\n");
    println!("{:<20} {:>10} {:>12} {:>16} {:>22}", "isolation", "trials", "committed", "rolled back", "NOT serializable");
    let mut example: Option<(Vec<Vec<Op>>, fuzz::Trial)> = None;
    for iso in [RC, SI, SER] {
        let mut rng = Rng::new(7);
        let (mut committed, mut aborted, mut bad) = (0u64, 0u64, 0u64);
        for _ in 0..trials {
            let programs: Vec<Vec<Op>> = (0..3 + rng.index(2)).map(|_| fuzz::random_program(&mut rng)).collect();
            let t = fuzz::trial(&programs, iso, &mut rng);
            committed += t.committed.len() as u64;
            aborted += t.aborted as u64;
            if !t.serializable {
                bad += 1;
                let size: usize = programs.iter().map(Vec::len).sum();
                if iso == SI && example.as_ref().is_none_or(|(p, _)| size < p.iter().map(Vec::len).sum()) {
                    example = Some((programs, t));
                }
            }
        }
        println!("{:<20} {trials:>10} {committed:>12} {aborted:>16} {bad:>14} ({:.2}%)", iso.to_string(), 100.0 * bad as f64 / trials as f64);
    }
    if let Some((programs, t)) = example {
        println!("\nthe smallest non-serializable outcome found under snapshot isolation:");
        for (i, p) in programs.iter().enumerate() {
            println!("  T{i}: {}", p.iter().map(fmt_op).collect::<Vec<_>>().join("; "));
        }
        println!("  steps in this order (a transaction's last step is its commit): {}", t.schedule.iter().map(|i| format!("T{i}")).collect::<Vec<_>>().join(" "));
        for (i, o) in &t.committed {
            println!("  T{i} committed having read {:?} and scanned {:?}", o.reads, o.scans);
        }
        println!("  final state {:?}", t.final_state);
    }
}

fn main() {
    let trials: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(20_000);
    versions_and_snapshots();
    matrix();
    vacuum();
    fuzz_section(trials);
}
