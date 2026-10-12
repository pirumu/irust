//! Lab (a): append-only log + hash index (Bitcask style), CRC per record, torn-tail recovery, compaction.
//!
//! Run: cargo run -q -p lab-data --bin kvlog [scratch-dir]

#[cfg(not(unix))]
fn main() {
    lab_data::need_unix("kvlog");
}

#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}

#[cfg(unix)]
mod app {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::Path;
    use std::time::Instant;

    use lab_data::kvlog::{CompactionReport, KvLog, Options, SyncPolicy, DATA_FILE};
    use lab_data::record::{self, Recovery};
    use lab_data::rng::Rng;
    use lab_data::util::{scratch_dir, thousands};

    type Res = Result<(), Box<dyn std::error::Error>>;

    pub fn run() -> Res {
        let root = scratch_dir(std::env::args().nth(1), "kvlog")?;
        println!("scratch dir: {}", root.display());
        load_and_read(&root.join("load"))?;
        torn_tail(&root.join("torn"))?;
        damage_in_the_middle(&root.join("middle"))?;
        compaction(&root.join("compact"))?;
        durability_knob(&root.join("sync"))?;
        fs::remove_dir_all(&root)?;
        Ok(())
    }

    fn key(i: u32) -> Vec<u8> {
        format!("user{i:05}").into_bytes() // 9 bytes
    }

    fn value(i: u32, version: u32) -> Vec<u8> {
        let mut v = format!("v{version}:{i}:").into_bytes();
        v.resize(100, b'.');
        v
    }

    fn load_and_read(dir: &Path) -> Res {
        println!("\n== 1. append-only log + keydir ==");
        let (mut kv, report) = KvLog::open(dir, Options::default())?;
        println!("open empty dir: records={} live_keys={}", report.records, report.live_keys);

        const N: u32 = 10_000;
        for i in 0..N {
            kv.put(&key(i), &value(i, 0))?;
        }
        let s = kv.stats();
        println!(
            "after {N} puts (new keys)         : file={:>9} B  live_keys={:>5}  garbage={:>9} B  space_amp={:.2}",
            thousands(s.file_bytes),
            s.live_keys,
            thousands(s.garbage_bytes),
            s.space_amplification()
        );

        let mut rng = Rng::new(1);
        for version in 1..=3 {
            for _ in 0..5_000 {
                let i = rng.below(u64::from(N)) as u32;
                kv.put(&key(i), &value(i, version))?;
            }
        }
        let mut deleted = 0;
        for i in (0..N).step_by(10) {
            deleted += u32::from(kv.delete(&key(i))?);
        }
        let s = kv.stats();
        println!(
            "after 15000 overwrites, {deleted} deletes: file={:>9} B  live_keys={:>5}  garbage={:>9} B  space_amp={:.2}",
            thousands(s.file_bytes),
            s.live_keys,
            thousands(s.garbage_bytes),
            s.space_amplification()
        );

        let before = kv.stats().reads;
        let mut hits = 0;
        for _ in 0..1000 {
            let i = rng.below(u64::from(N)) as u32;
            if kv.get(&key(i))?.is_some() {
                hits += 1;
            }
        }
        let after_hits = kv.stats().reads;
        for i in N..N + 1000 {
            assert!(kv.get(&key(i))?.is_none());
        }
        println!(
            "1000 random get(): hits={hits} preads={}   1000 get() of absent keys: preads={}",
            after_hits - before,
            kv.stats().reads - after_hits
        );

        drop(kv);
        let t = Instant::now();
        let (kv, report) = KvLog::open(dir, Options::default())?;
        println!(
            "reopen: scanned {} records ({} tombstones), rebuilt keydir with {} keys, stop={:?}  [{:.1} ms, machine-dependent]",
            thousands(report.records),
            report.tombstones,
            kv.len(),
            report.stop,
            t.elapsed().as_secs_f64() * 1e3
        );
        Ok(())
    }

    /// Builds a 5-record log in `dir`; returns (data file bytes, offset where the last record starts).
    fn five_records(dir: &Path) -> Result<(Vec<u8>, u64), Box<dyn std::error::Error>> {
        let (mut kv, _) = KvLog::open(dir, Options::default())?;
        let mut last = 0;
        for i in 0..5u32 {
            last = kv.stats().file_bytes;
            kv.put(format!("k{i}").as_bytes(), &[b'a' + i as u8; 30])?;
        }
        drop(kv);
        Ok((fs::read(dir.join(DATA_FILE))?, last))
    }

    fn torn_tail(dir: &Path) -> Res {
        println!("\n== 2. torn tail: cut the file inside the last record ==");
        let (bytes, last) = five_records(dir)?;
        let full = bytes.len() as u64;
        let last_len = full - last;
        println!(
            "5 records of {last_len} B each (header {} B = crc32c + key_len + val_len); last record starts at offset {last}",
            record::HEADER_LEN
        );
        println!("{:>5}  {:<20} {:>9} {:>5} {:>12}  k4", "cut", "stop", "truncated", "keys", "file after");
        for cut in [0, 1, 11, 12, 13, 20, last_len - 1] {
            let case = dir.join(format!("cut{cut}"));
            fs::create_dir_all(&case)?;
            fs::write(case.join(DATA_FILE), &bytes[..(last + cut) as usize])?;
            let (kv, report) = KvLog::open(&case, Options::default())?;
            println!(
                "{cut:>5}  {:<20} {:>9} {:>5} {:>12}  {}",
                format!("{:?}", report.stop),
                report.truncated_bytes,
                kv.len(),
                fs::metadata(case.join(DATA_FILE))?.len(),
                if kv.contains_key(b"k4") { "present" } else { "absent" }
            );
        }
        println!("(cut = bytes of the last record that reached the disk; every cut < {last_len} is repaired by truncation)");

        // Zero-filled tail: the file got longer but the data never arrived.
        let case = dir.join("zeros");
        fs::create_dir_all(&case)?;
        let mut zeros = bytes.clone();
        zeros.extend_from_slice(&[0; 4096]);
        fs::write(case.join(DATA_FILE), &zeros)?;
        let (kv, report) = KvLog::open(&case, Options::default())?;
        println!(
            "4096 zero bytes appended: stop={:?} truncated={} keys={}",
            report.stop,
            report.truncated_bytes,
            kv.len()
        );

        // What if the engine had kept appending after the torn tail instead of cutting it? The torn
        // bytes are the first 13 bytes of a 44-byte record; whatever is appended next is parsed as
        // the rest of that record.
        println!("appending after an unrepaired torn tail, then reopening:");
        for (label, payload) in [
            ("short record (17 B)", &b"ok"[..]),
            ("long record (51 B) ", &b"acknowledged-write-0123456789abcdef-0123456789"[..]),
        ] {
            let case = dir.join(format!("buried-{}", payload.len()));
            fs::create_dir_all(&case)?;
            let mut buried = bytes[..(last + 13) as usize].to_vec();
            record::encode_into(&mut buried, b"new", Some(payload));
            fs::write(case.join(DATA_FILE), &buried)?;
            match KvLog::open(&case, Options::default()) {
                Ok((kv, report)) => println!(
                    "  {label}: opened, stop={:?}, truncated={} B, get(new)={:?}",
                    report.stop,
                    report.truncated_bytes,
                    kv.get(b"new")?
                ),
                Err(e) => println!("  {label}: {e}"),
            }
        }
        Ok(())
    }

    fn damage_in_the_middle(dir: &Path) -> Res {
        println!("\n== 3. damage in the middle: three recovery policies ==");
        let (bytes, _) = five_records(dir)?;
        let rec_len = bytes.len() as u64 / 5;
        let mut rotten = bytes.clone();
        rotten[(2 * rec_len) as usize + record::HEADER_LEN + 5] ^= 0x01; // one bit in record #3's value
        for (name, recovery) in [
            ("Strict", Recovery::Strict),
            ("TolerateTail", Recovery::TolerateTail),
            ("PointInTime", Recovery::PointInTime),
        ] {
            let case = dir.join(name);
            fs::create_dir_all(&case)?;
            fs::write(case.join(DATA_FILE), &rotten)?;
            let opts = Options { recovery, ..Options::default() };
            match KvLog::open(&case, opts) {
                Ok((kv, report)) => println!(
                    "{name:<13} opened: kept {} of 5 records, dropped {} B = {} records ({} of them intact); keys={}",
                    report.records,
                    report.truncated_bytes,
                    5 - report.records,
                    5 - report.records - 1,
                    kv.len()
                ),
                Err(e) => println!(
                    "{name:<13} {e}; file untouched ({} B)",
                    fs::metadata(case.join(DATA_FILE))?.len()
                ),
            }
        }
        Ok(())
    }

    fn compaction(dir: &Path) -> Res {
        println!("\n== 4. compaction ==");
        let (mut kv, _) = KvLog::open(dir, Options::default())?;
        for i in 0..2000 {
            kv.put(&key(i), &value(i, 0))?;
        }
        for round in 1..=4 {
            for i in 0..1000 {
                kv.put(&key(i), &value(i, round))?;
            }
        }
        for i in 1500..2000 {
            kv.delete(&key(i))?;
        }
        let s = kv.stats();
        println!(
            "before: file={} B live={} B garbage={} B  space_amp={:.2}  live_keys={}",
            thousands(s.file_bytes),
            thousands(s.live_bytes),
            thousands(s.garbage_bytes),
            s.space_amplification(),
            s.live_keys
        );
        let CompactionReport { before_bytes, after_bytes, live_keys } = kv.compact()?;
        let s = kv.stats();
        println!(
            "after : file={} B live={} B garbage={} B  space_amp={:.2}  live_keys={}  (reclaimed {} B)",
            thousands(s.file_bytes),
            thousands(s.live_bytes),
            thousands(s.garbage_bytes),
            s.space_amplification(),
            live_keys,
            thousands(before_bytes - after_bytes)
        );
        assert!(kv.get(&key(1700))?.is_none(), "tombstoned keys stay deleted");
        println!("deleted key user01700 after compaction: {:?}", kv.get(&key(1700))?);
        drop(kv);

        // A crash before the rename leaves a half-written temp file; open deletes it.
        let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(dir.join("data.log.compact"))?;
        f.write_all(b"half a compacted file")?;
        drop(f);
        let (kv, report) = KvLog::open(dir, Options::default())?;
        println!(
            "leftover data.log.compact from an interrupted compaction: removed={} live_keys={}",
            report.removed_stale_compaction,
            kv.len()
        );
        Ok(())
    }

    fn durability_knob(dir: &Path) -> Res {
        println!("\n== 5. durability knob: SyncPolicy ==");
        const N: u32 = 200;
        for (name, sync) in [("Never", SyncPolicy::Never), ("EveryWrite", SyncPolicy::EveryWrite)] {
            let case = dir.join(name);
            let (mut kv, _) = KvLog::open(&case, Options { sync, ..Options::default() })?;
            let t = Instant::now();
            for i in 0..N {
                kv.put(&key(i), &value(i, 0))?;
            }
            let el = t.elapsed();
            println!(
                "{name:<11} {N} puts: fsyncs={:>3}  {:>9.3} ms/put  [machine-dependent]",
                kv.stats().fsyncs,
                el.as_secs_f64() * 1e3 / f64::from(N)
            );
        }
        Ok(())
    }
}
