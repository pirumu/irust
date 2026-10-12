//! Lab (b): a mini LSM-tree: memtable, SSTables, merge on read, tombstones, compaction policies,
//! Bloom filters, write amplification (also against an in-place page store), crash recovery.
//!
//! Run: cargo run -q --release -p lab-data --bin lsm [scratch-dir]

#[cfg(not(unix))]
fn main() {
    lab_data::need_unix("lsm");
}

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    app::run()
}

#[cfg(unix)]
mod app {
    use std::fs::{self, OpenOptions};
    use std::io;
    use std::path::Path;

    use lab_data::lsm::{Answer, Lsm, Options, Origin, Policy, TombstonePolicy};
    use lab_data::pagestore::{self, PageStore};
    use lab_data::rng::Rng;
    use lab_data::util::{scratch_dir, thousands};

    const MEMTABLE: usize = 256 << 10;
    const PUTS: u64 = 200_000;
    const KEYS: u64 = 50_000;

    pub fn run() -> io::Result<()> {
        let root = scratch_dir(std::env::args().nth(1), "lsm")?;
        println!("scratch dir: {}", root.display());
        walk_through(&root.join("walk"))?;
        tombstone_resurrection(&root.join("resurrect"))?;
        bloom_filters(&root.join("bloom"))?;
        compaction_policies(&root.join("policies"))?;
        page_store(&root.join("pages"))?;
        crash_recovery(&root.join("crash"))?;
        fs::remove_dir_all(&root)
    }

    fn key(i: u64) -> Vec<u8> {
        format!("user{i:08}").into_bytes() // 12 bytes
    }

    fn value(i: u64, tag: u8) -> Vec<u8> {
        let mut v = format!("{}:{i}:", tag as char).into_bytes();
        v.resize(100, b'.');
        v
    }

    fn origin(o: Origin) -> String {
        match o {
            Origin::Memtable => "memtable".to_string(),
            Origin::Run(id) => format!("run {id:06}"),
        }
    }

    fn describe(db: &Lsm, i: u64) -> io::Result<String> {
        Ok(match db.lookup(&key(i))? {
            Answer::Value(v, o) => format!("value {:?} from {}", String::from_utf8_lossy(&v[..8]), origin(o)),
            Answer::Deleted(o) => format!("tombstone in {} (deleted)", origin(o)),
            Answer::Absent => "absent everywhere".to_string(),
        })
    }

    fn print_runs(db: &Lsm) {
        println!("  memtable: {} entries", db.memtable_len());
        for r in db.runs() {
            println!("  run {:06}  level {}  {:>5} entries  {:>7} B", r.id, r.level, r.entries, thousands(r.file_bytes));
        }
    }

    fn walk_through(dir: &Path) -> io::Result<()> {
        println!("\n== 1. write path, flush, merge on read, tombstones, major compaction ==");
        let opts = Options { memtable_bytes: 1 << 30, durable: false, policy: Policy::None, ..Options::default() };
        let (mut db, _) = Lsm::open(dir, opts)?;
        for i in 0..50 {
            db.put(&key(i), &value(i, b'a'))?;
        }
        db.flush()?;
        for i in 25..75 {
            db.put(&key(i), &value(i, b'b'))?;
        }
        db.flush()?;
        for i in 30..40 {
            db.delete(&key(i))?;
        }
        db.flush()?;
        for i in 70..80 {
            db.put(&key(i), &value(i, b'c'))?;
        }
        println!("after 3 flushes (puts 0..50, puts 25..75, deletes 30..40) and 10 puts left in the memtable:");
        print_runs(&db);
        for i in [10, 28, 35, 60, 72, 99] {
            println!("  lookup user{i:08}: {}", describe(&db, i)?);
        }
        let live = db.scan(&key(0), &key(1000))?;
        println!("  scan of the whole key space: {} live entries (merged from {} sources)", live.len(), db.runs().len() + 1);

        let before = db.table_bytes();
        db.compact_all()?;
        println!("after compact_all (merge every run, drop tombstones and shadowed versions):");
        print_runs(&db);
        println!("  table bytes {} -> {}", thousands(before), thousands(db.table_bytes()));
        println!("  lookup user{:08}: {}", 35, describe(&db, 35)?);
        Ok(())
    }

    fn tombstone_resurrection(dir: &Path) -> io::Result<()> {
        println!("\n== 2. dropping a tombstone too early ==");
        println!("size-tiered, fanout 3: flush k1=\"the-value\", k100, k101 -> merged into the oldest run;");
        println!("then flush the delete of k1, k200, k201 -> these three merge, but the oldest run is not part of that merge");
        for (name, dirname, tombstones) in [
            ("DropAtBottom (correct)", "correct", TombstonePolicy::DropAtBottom),
            ("AlwaysDrop   (the bug)", "bug", TombstonePolicy::AlwaysDrop),
        ] {
            let opts = Options {
                memtable_bytes: 1 << 30,
                durable: false,
                policy: Policy::Tiered { fanout: 3 },
                tombstones,
                ..Options::default()
            };
            let (mut db, _) = Lsm::open(dir.join(dirname), opts)?;
            for i in [1, 100, 101] {
                db.put(&key(i), if i == 1 { b"the-value" } else { b"x" })?;
                db.flush()?;
            }
            db.delete(&key(1))?;
            db.flush()?;
            for i in [200, 201] {
                db.put(&key(i), b"y")?;
                db.flush()?;
            }
            let runs: Vec<String> = db.runs().iter().map(|r| format!("tier {} / {} entries", r.level, r.entries)).collect();
            let answer = db.get(&key(1))?.map(|v| String::from_utf8_lossy(&v).into_owned());
            println!("{name}: runs (newest first) = [{}]  get(user00000001) -> {answer:?}", runs.join(", "));
        }
        Ok(())
    }

    fn bloom_filters(dir: &Path) -> io::Result<()> {
        println!("\n== 3. Bloom filter: reads per lookup of keys that do not exist ==");
        println!("8 runs of 4000 keys each (no compaction); 40000 lookups of keys that were never written");
        println!("{:>13} {:>10} {:>14} {:>16} {:>14} {:>9}", "bits per key", "filter B", "blocks/lookup", "bloom negatives", "false positives", "FP rate");
        for bits in [0usize, 5, 10, 15] {
            let opts = Options {
                memtable_bytes: 1 << 30,
                bloom_bits_per_key: bits,
                durable: false,
                policy: Policy::None,
                ..Options::default()
            };
            let (mut db, _) = Lsm::open(dir.join(format!("bits{bits}")), opts)?;
            for round in 0..8u64 {
                for i in 0..4000 {
                    db.put(&key(i * 8 + round), &value(i, b'v'))?;
                }
                db.flush()?;
            }
            let probes = 40_000u64;
            for i in 0..probes {
                // user0000xxxxx.: sorts between existing keys, so range checks cannot skip it
                let mut k = key(i % 31_990);
                k.push(b'.');
                assert!(db.get(&k)?.is_none());
            }
            let s = db.stats();
            let considered = s.block_reads + s.bloom_negatives; // runs whose range contained the key
            let fp_rate = if bits == 0 { f64::NAN } else { s.bloom_false_positives as f64 / considered as f64 };
            let filter_bytes = if bits == 0 { 0 } else { 8 * 4000 * bits / 8 };
            println!(
                "{bits:>13} {:>10} {:>14.3} {:>16} {:>14} {:>9}",
                thousands(filter_bytes as u64),
                s.blocks_per_get(),
                thousands(s.bloom_negatives),
                thousands(s.bloom_false_positives),
                if bits == 0 { "-".to_string() } else { format!("{:.2}%", fp_rate * 100.0) }
            );
        }
        println!("(filter B = approximate bytes of filter per run times 8 runs; FP rate = false positives / lookups that passed the range check)");
        Ok(())
    }

    struct Row {
        name: String,
        wa: f64,
        wal: u64,
        flush: u64,
        compact: u64,
        runs: usize,
        space_amp: f64,
        hit_blocks: f64,
        miss_blocks: f64,
    }

    fn lsm_row(dir: &Path, name: &str, policy: Policy) -> io::Result<Row> {
        let opts = Options { memtable_bytes: MEMTABLE, durable: false, policy, ..Options::default() };
        let (mut db, _) = Lsm::open(dir, opts)?;
        let mut rng = Rng::new(42);
        for _ in 0..PUTS {
            let i = rng.below(KEYS);
            db.put(&key(i), &value(i, b'v'))?;
        }
        let s = db.stats();
        // Space amplification: bytes of table files over the encoded size of the live entries (key,
        // value and the 12-byte record header), so a perfectly compacted store scores about 1.0.
        let (live_keys, live_bytes) = db.live_data()?;
        let live_encoded = live_bytes + live_keys * lab_data::record::HEADER_LEN as u64;
        let space_amp = db.table_bytes() as f64 / live_encoded as f64;

        let probes = 20_000u64;
        let (g0, b0) = (db.stats().gets, db.stats().block_reads);
        for _ in 0..probes {
            db.get(&key(rng.below(KEYS)))?;
        }
        let (g1, b1) = (db.stats().gets, db.stats().block_reads);
        for i in 0..probes {
            let mut k = key(i % KEYS);
            k.push(b'.');
            db.get(&k)?;
        }
        let (g2, b2) = (db.stats().gets, db.stats().block_reads);
        Ok(Row {
            name: name.to_string(),
            wa: s.write_amplification(),
            wal: s.wal_bytes,
            flush: s.flush_bytes,
            compact: s.compaction_bytes,
            runs: db.runs().len(),
            space_amp,
            hit_blocks: (b1 - b0) as f64 / (g1 - g0) as f64,
            miss_blocks: (b2 - b1) as f64 / (g2 - g1) as f64,
        })
    }

    fn print_rows(rows: &[Row], user_bytes: u64) {
        println!(
            "{:<26} {:>6} {:>10} {:>10} {:>10} {:>5} {:>9} {:>10} {:>11}",
            "engine", "WA", "log MB", "flush MB", "compact MB", "runs", "space amp", "blk/get hit", "blk/get miss"
        );
        for r in rows {
            println!(
                "{:<26} {:>6.2} {:>10.1} {:>10.1} {:>10.1} {:>5} {:>9.2} {:>10.2} {:>11.2}",
                r.name,
                r.wa,
                r.wal as f64 / 1e6,
                r.flush as f64 / 1e6,
                r.compact as f64 / 1e6,
                r.runs,
                r.space_amp,
                r.hit_blocks,
                r.miss_blocks
            );
        }
        println!("(user data written: {:.1} MB)", user_bytes as f64 / 1e6);
    }

    fn compaction_policies(dir: &Path) -> io::Result<()> {
        println!("\n== 4. compaction policies: write, read and space amplification ==");
        println!(
            "{} puts of 112-byte entries over {} distinct keys (uniform random), memtable {} KiB, 10 bits/key Bloom filter",
            thousands(PUTS),
            thousands(KEYS),
            MEMTABLE >> 10
        );
        let mut rows = Vec::new();
        for (name, policy) in [
            ("no compaction", Policy::None),
            ("tiered, fanout 4", Policy::Tiered { fanout: 4 }),
            ("tiered, fanout 10", Policy::Tiered { fanout: 10 }),
            ("leveled, fanout 10", Policy::Leveled { fanout: 10, base_bytes: 1 << 20 }),
            ("leveled, fanout 4", Policy::Leveled { fanout: 4, base_bytes: 1 << 20 }),
        ] {
            rows.push(lsm_row(&dir.join(name.replace([' ', ','], "_")), name, policy)?);
        }
        print_rows(&rows, PUTS * 112);
        Ok(())
    }

    fn page_row(dir: &Path, name: &str, checkpoint_every: usize, ascending: bool) -> io::Result<Row> {
        let opts = pagestore::Options { checkpoint_every, durable: false, ..pagestore::Options::default() };
        let (mut store, _) = PageStore::open(dir, opts)?;
        let mut rng = Rng::new(42);
        for n in 0..PUTS {
            let i = if ascending { n } else { rng.below(KEYS) };
            store.put(&key(i), &value(i, b'v'))?;
        }
        store.checkpoint()?;
        let s = store.stats();
        Ok(Row {
            name: name.to_string(),
            wa: s.write_amplification(),
            wal: s.wal_bytes,
            flush: 0,
            compact: s.page_bytes,
            runs: s.pages,
            space_amp: f64::NAN,
            hit_blocks: 1.0,
            miss_blocks: 1.0,
        })
    }

    fn page_store(dir: &Path) -> io::Result<()> {
        println!("\n== 5. the same workload on an in-place page store (4 KiB pages + WAL + checkpoints) ==");
        let mut rows = Vec::new();
        for (name, every, asc) in [
            ("pages, checkpoint/1000", 1_000, false),
            ("pages, checkpoint/10000", 10_000, false),
            ("pages, checkpoint/100000", 100_000, false),
            ("pages, ascending keys", 10_000, true),
        ] {
            rows.push(page_row(&dir.join(name.replace([' ', ','], "_").replace('/', "-")), name, every, asc)?);
        }
        println!(
            "{:<26} {:>6} {:>10} {:>12} {:>7}",
            "engine", "WA", "WAL MB", "pages MB", "pages"
        );
        for r in &rows {
            println!("{:<26} {:>6.2} {:>10.1} {:>12.1} {:>7}", r.name, r.wa, r.wal as f64 / 1e6, r.compact as f64 / 1e6, r.runs);
        }
        println!("(\"pages MB\" = bytes written in place at checkpoints; every row writes {:.1} MB of user data)", (PUTS * 112) as f64 / 1e6);
        Ok(())
    }

    fn crash_recovery(dir: &Path) -> io::Result<()> {
        println!("\n== 6. crash recovery ==");
        let opts = Options { memtable_bytes: 1 << 30, durable: false, policy: Policy::None, ..Options::default() };
        let wal_file;
        {
            let (mut db, _) = Lsm::open(dir, opts)?;
            for i in 0..1000 {
                db.put(&key(i), &value(i, b'a'))?;
            }
            db.flush()?;
            for i in 1000..1500 {
                db.put(&key(i), &value(i, b'b'))?;
            }
            db.delete(&key(5))?;
            wal_file = fs::read_dir(dir)?
                .map(|e| e.map(|e| e.path()))
                .collect::<io::Result<Vec<_>>>()?
                .into_iter()
                .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("wal-")))
                .expect("a log file");
            // The process "crashes" here: no flush, the memtable is lost with it.
        }
        let wal_len = fs::metadata(&wal_file)?.len();
        let (db, report) = Lsm::open(dir, opts)?;
        println!(
            "reopen after a crash: {} run, replayed {} log records, memtable has {} entries, orphans removed {}",
            report.runs,
            report.wal_records,
            db.memtable_len(),
            report.orphans_removed
        );
        println!("  user00001400 -> {}   user00000005 (deleted in the log) -> {}", describe(&db, 1400)?, describe(&db, 5)?);
        drop(db);

        OpenOptions::new().write(true).open(&wal_file)?.set_len(wal_len - 20)?;
        let (db, report) = Lsm::open(dir, opts)?;
        println!(
            "log cut 20 bytes short (torn last record): replayed {} records, cut {} B, stop={:?}",
            report.wal_records, report.wal_truncated_bytes, report.wal_stop
        );
        println!("  the delete of user00000005 was the last record: {}", describe(&db, 5)?);
        Ok(())
    }
}
