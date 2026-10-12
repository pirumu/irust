//! Lab (a): a Bitcask-style key-value store: an append-only data file plus a hash index in RAM.
//!
//! * `put`/`delete` append one record ([`crate::record`]) to the single data file; nothing is ever
//!   overwritten in place. A delete appends a tombstone.
//! * The index (Bitcask calls it the *keydir*) maps every live key to the offset and size of its
//!   newest record, so `get` is one `pread`.
//! * Opening the store scans the file, rebuilds the index, and repairs a torn tail according to the
//!   chosen [`Recovery`] policy. The data file *is* the log, so there is nothing else to replay.
//! * `compact` rewrites only the live records into a new file and swaps it in with the safe-write
//!   pattern (temp file, `fsync`, `rename`, `fsync` of the directory).
//!
//! Simplifications against real Bitcask (on purpose): one data file instead of an active file plus
//! immutable segments, no hint files, one writer, and recovery reads the whole file into memory.

use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use crate::record::{self, Corrupt, Parse, Recovery, Stop, MAX_KEY_LEN, MAX_VALUE_LEN};
use crate::util::sync_dir;

pub const DATA_FILE: &str = "data.log";
const COMPACT_FILE: &str = "data.log.compact";

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// Recovery refused to open the log (see [`Recovery`]).
    Corrupt(Corrupt),
    /// A record read back for `get` or `compact` failed its checksum (bit rot after the open scan).
    ChecksumOnRead { offset: u64 },
    TooLarge,
    /// An earlier write failed; the in-memory state may not match the file. Reopen to recover.
    Poisoned,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Corrupt(c) => write!(f, "recovery refused: {c}"),
            Error::ChecksumOnRead { offset } => write!(f, "checksum mismatch reading offset {offset}"),
            Error::TooLarge => write!(f, "key or value too large"),
            Error::Poisoned => write!(f, "an earlier write failed; reopen the store"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncPolicy {
    /// `write` only: the record sits in the page cache when `put` returns.
    #[default]
    Never,
    /// `sync_data` after every append: `put` returns only after the record is on stable storage
    /// (as far as the OS and device report it).
    EveryWrite,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    pub sync: SyncPolicy,
    pub recovery: Recovery,
}

/// Where the newest record of a key lives.
#[derive(Debug, Clone, Copy)]
struct Loc {
    offset: u64,
    len: u32,
}

/// What `open` found in the file.
#[derive(Debug, Clone, Default)]
pub struct RecoveryReport {
    /// Intact records scanned (puts and tombstones, live or superseded).
    pub records: u64,
    pub tombstones: u64,
    pub valid_bytes: u64,
    /// Bytes cut from the end of the file.
    pub truncated_bytes: u64,
    pub stop: Stop,
    pub live_keys: usize,
    /// A leftover `data.log.compact` from an interrupted compaction was deleted.
    pub removed_stale_compaction: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Stats {
    pub live_keys: usize,
    pub file_bytes: u64,
    pub live_bytes: u64,
    /// Superseded records and tombstones still occupying the file.
    pub garbage_bytes: u64,
    /// `pread` calls made by `get` since open.
    pub reads: u64,
    pub appended_bytes: u64,
    pub fsyncs: u64,
}

impl Stats {
    /// File size divided by live bytes ("space amplification"); 1.0 when nothing is wasted.
    pub fn space_amplification(&self) -> f64 {
        if self.live_bytes == 0 { f64::INFINITY } else { self.file_bytes as f64 / self.live_bytes as f64 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CompactionReport {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub live_keys: usize,
}

pub struct KvLog {
    dir: PathBuf,
    path: PathBuf,
    file: File,
    index: HashMap<Vec<u8>, Loc>,
    /// Offset at which the next record is appended (= logical file length).
    end: u64,
    live_bytes: u64,
    opts: Options,
    poisoned: bool,
    reads: Cell<u64>,
    appended: u64,
    fsyncs: u64,
}

impl KvLog {
    /// Opens (creating if needed) the store in `dir`, rebuilding the index from the data file.
    pub fn open(dir: impl AsRef<Path>, opts: Options) -> Result<(KvLog, RecoveryReport)> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let path = dir.join(DATA_FILE);
        let removed_stale = match fs::remove_file(dir.join(COMPACT_FILE)) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        let file = OpenOptions::new().read(true).append(true).create(true).open(&path)?;
        let bytes = fs::read(&path)?;

        let mut index: HashMap<Vec<u8>, Loc> = HashMap::new();
        let mut live_bytes = 0u64;
        let mut tombstones = 0u64;
        let scan = record::scan(&bytes, |offset, rec| {
            let len = rec.len as u32;
            match rec.value {
                Some(_) => {
                    if let Some(old) = index.insert(rec.key.to_vec(), Loc { offset: offset as u64, len }) {
                        live_bytes -= u64::from(old.len);
                    }
                    live_bytes += u64::from(len);
                }
                None => {
                    tombstones += 1;
                    if let Some(old) = index.remove(rec.key) {
                        live_bytes -= u64::from(old.len);
                    }
                }
            }
        });
        let cut = record::bytes_to_cut(opts.recovery, &scan, bytes.len() as u64).map_err(Error::Corrupt)?;
        if cut > 0 {
            // Cut the damaged tail *before* appending anything: a new record written after garbage
            // would sit behind a record the next scan cannot get past.
            file.set_len(scan.valid_len as u64)?;
            file.sync_all()?;
        }
        let report = RecoveryReport {
            records: scan.records,
            tombstones,
            valid_bytes: scan.valid_len as u64,
            truncated_bytes: cut,
            stop: scan.stop,
            live_keys: index.len(),
            removed_stale_compaction: removed_stale,
        };
        let store = KvLog {
            dir,
            path,
            file,
            index,
            end: scan.valid_len as u64,
            live_bytes,
            opts,
            poisoned: false,
            reads: Cell::new(0),
            appended: 0,
            fsyncs: 0,
        };
        Ok((store, report))
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> Result<()> {
        self.append(key, Some(value))
    }

    /// Appends a tombstone if the key exists. Returns whether it did.
    pub fn delete(&mut self, key: &[u8]) -> Result<bool> {
        if !self.index.contains_key(key) {
            return Ok(false);
        }
        self.append(key, None)?;
        Ok(true)
    }

    fn append(&mut self, key: &[u8], value: Option<&[u8]>) -> Result<()> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        if key.len() > MAX_KEY_LEN || value.is_some_and(|v| v.len() > MAX_VALUE_LEN) {
            return Err(Error::TooLarge);
        }
        let mut buf = Vec::with_capacity(record::encoded_len(key.len(), value.map(<[u8]>::len)));
        record::encode_into(&mut buf, key, value);
        let io_result = self.file.write_all(&buf).and_then(|()| match self.opts.sync {
            SyncPolicy::Never => Ok(()),
            SyncPolicy::EveryWrite => {
                self.fsyncs += 1;
                self.file.sync_data()
            }
        });
        if let Err(e) = io_result {
            self.poisoned = true;
            return Err(e.into());
        }
        let len = buf.len() as u32;
        let loc = Loc { offset: self.end, len };
        self.end += u64::from(len);
        self.appended += u64::from(len);
        match value {
            Some(_) => {
                if let Some(old) = self.index.insert(key.to_vec(), loc) {
                    self.live_bytes -= u64::from(old.len);
                }
                self.live_bytes += u64::from(len);
            }
            None => {
                if let Some(old) = self.index.remove(key) {
                    self.live_bytes -= u64::from(old.len);
                }
            }
        }
        Ok(())
    }

    /// One index lookup and one `pread` of the whole record, whose checksum is verified.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let Some(&loc) = self.index.get(key) else {
            return Ok(None);
        };
        let mut buf = vec![0u8; loc.len as usize];
        self.file.read_exact_at(&mut buf, loc.offset)?;
        self.reads.set(self.reads.get() + 1);
        match record::parse(&buf) {
            Parse::Record(r) if r.key == key && r.value.is_some() => Ok(r.value.map(<[u8]>::to_vec)),
            _ => Err(Error::ChecksumOnRead { offset: loc.offset }),
        }
    }

    pub fn contains_key(&self, key: &[u8]) -> bool {
        self.index.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Live keys in unspecified order.
    pub fn keys(&self) -> impl Iterator<Item = &[u8]> {
        self.index.keys().map(Vec::as_slice)
    }

    /// Forces appended records to stable storage (use with `SyncPolicy::Never`).
    pub fn sync(&mut self) -> Result<()> {
        self.fsyncs += 1;
        self.file.sync_data()?;
        Ok(())
    }

    pub fn stats(&self) -> Stats {
        Stats {
            live_keys: self.index.len(),
            file_bytes: self.end,
            live_bytes: self.live_bytes,
            garbage_bytes: self.end - self.live_bytes,
            reads: self.reads.get(),
            appended_bytes: self.appended,
            fsyncs: self.fsyncs,
        }
    }

    /// Rewrites the live records into a new file (original order kept) and swaps it in atomically.
    /// A crash before the `rename` leaves the old file intact; `open` deletes the leftover temp file.
    pub fn compact(&mut self) -> Result<CompactionReport> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        let tmp = self.dir.join(COMPACT_FILE);
        let result = self.write_compacted(&tmp);
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        let (new_index, new_len) = result?;
        fs::rename(&tmp, &self.path)?;
        sync_dir(&self.dir)?;
        let before = self.end;
        self.file = OpenOptions::new().read(true).append(true).open(&self.path)?;
        self.index = new_index;
        self.end = new_len;
        self.live_bytes = new_len;
        Ok(CompactionReport { before_bytes: before, after_bytes: new_len, live_keys: self.index.len() })
    }

    fn write_compacted(&self, tmp: &Path) -> Result<(HashMap<Vec<u8>, Loc>, u64)> {
        let mut live: Vec<(&Vec<u8>, Loc)> = self.index.iter().map(|(k, &l)| (k, l)).collect();
        live.sort_by_key(|(_, l)| l.offset);
        let mut out = BufWriter::new(File::create(tmp)?);
        let mut new_index = HashMap::with_capacity(live.len());
        let mut pos = 0u64;
        let mut buf = Vec::new();
        for (key, loc) in live {
            buf.resize(loc.len as usize, 0);
            self.file.read_exact_at(&mut buf, loc.offset)?;
            // Verify before copying: compaction must not carry rot into the new file.
            match record::parse(&buf) {
                Parse::Record(r) if r.key == key.as_slice() => {}
                _ => return Err(Error::ChecksumOnRead { offset: loc.offset }),
            }
            out.write_all(&buf)?;
            new_index.insert(key.clone(), Loc { offset: pos, len: loc.len });
            pos += u64::from(loc.len);
        }
        let file = out.into_inner().map_err(io::IntoInnerError::into_error)?;
        file.sync_all()?;
        Ok((new_index, pos))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::util::TempDir;
    use std::collections::BTreeMap;

    fn open(dir: &Path) -> (KvLog, RecoveryReport) {
        KvLog::open(dir, Options::default()).unwrap()
    }

    fn data_path(dir: &Path) -> PathBuf {
        dir.join(DATA_FILE)
    }

    /// A log of five records, one per key `k0..k4`, with values of different sizes. Returns the
    /// byte offset where each record starts, and the file length.
    fn five_records(dir: &Path) -> (Vec<u64>, u64) {
        let (mut kv, _) = open(dir);
        let mut starts = Vec::new();
        for i in 0..5u8 {
            starts.push(kv.stats().file_bytes);
            kv.put(format!("k{i}").as_bytes(), &vec![b'a' + i; 10 + 7 * usize::from(i)]).unwrap();
        }
        (starts, kv.stats().file_bytes)
    }

    #[test]
    fn put_get_overwrite_delete_and_reopen() {
        let t = TempDir::new("kvlog-basic").unwrap();
        {
            let (mut kv, report) = open(t.path());
            assert_eq!((report.records, report.live_keys, report.truncated_bytes), (0, 0, 0));
            kv.put(b"a", b"1").unwrap();
            kv.put(b"b", b"2").unwrap();
            kv.put(b"a", b"3").unwrap(); // overwrite: appended, not updated in place
            assert!(kv.delete(b"b").unwrap());
            assert!(!kv.delete(b"never-existed").unwrap());
            assert_eq!(kv.get(b"a").unwrap().as_deref(), Some(&b"3"[..]));
            assert_eq!(kv.get(b"b").unwrap(), None);
            assert_eq!(kv.len(), 1);
        }
        let (kv, report) = open(t.path());
        // put a, put b, put a, delete b: the tombstone is a record too; the missing key wrote nothing.
        assert_eq!((report.records, report.tombstones, report.live_keys), (4, 1, 1));
        assert_eq!(report.stop, Stop::Clean);
        assert_eq!(kv.get(b"a").unwrap().as_deref(), Some(&b"3"[..]));
        assert_eq!(kv.get(b"b").unwrap(), None);
    }

    #[test]
    fn a_get_is_exactly_one_pread() {
        let t = TempDir::new("kvlog-reads").unwrap();
        let (mut kv, _) = open(t.path());
        for i in 0..100u32 {
            kv.put(&i.to_le_bytes(), &[7; 50]).unwrap();
        }
        let before = kv.stats().reads;
        for i in 0..100u32 {
            assert!(kv.get(&i.to_le_bytes()).unwrap().is_some());
        }
        assert_eq!(kv.stats().reads - before, 100);
        // A key that was never written costs no I/O at all: the index answers.
        assert_eq!(kv.get(b"absent").unwrap(), None);
        assert_eq!(kv.stats().reads - before, 100);
    }

    #[test]
    fn a_torn_tail_is_cut_at_every_possible_length() {
        let t = TempDir::new("kvlog-torn").unwrap();
        let (starts, full) = five_records(t.path());
        let original = fs::read(data_path(t.path())).unwrap();
        let last = starts[4];

        for cut in 0..(full - last) {
            let d = TempDir::new("kvlog-torn-case").unwrap();
            fs::write(data_path(d.path()), &original[..(last + cut) as usize]).unwrap();
            let (mut kv, report) = open(d.path());
            assert_eq!(report.truncated_bytes, cut, "cut {cut}");
            assert!(report.stop == Stop::Clean || report.stop.is_torn_tail(), "cut {cut}: {:?}", report.stop);
            assert_eq!(report.live_keys, 4, "cut {cut}");
            assert_eq!(fs::metadata(data_path(d.path())).unwrap().len(), last, "file cut back, cut {cut}");
            for i in 0..4u8 {
                let want = vec![b'a' + i; 10 + 7 * usize::from(i)];
                assert_eq!(kv.get(format!("k{i}").as_bytes()).unwrap(), Some(want));
            }
            assert_eq!(kv.get(b"k4").unwrap(), None);
            // The log keeps working after the repair, and the new record survives a reopen.
            kv.put(b"after", b"crash").unwrap();
            drop(kv);
            let (kv2, report2) = open(d.path());
            assert_eq!((report2.stop, report2.live_keys), (Stop::Clean, 5));
            assert_eq!(kv2.get(b"after").unwrap().as_deref(), Some(&b"crash"[..]));
        }

        // The complete file is not damaged: nothing is cut and k4 is there.
        let d = TempDir::new("kvlog-torn-full").unwrap();
        fs::write(data_path(d.path()), &original).unwrap();
        let (kv, report) = open(d.path());
        assert_eq!((report.truncated_bytes, report.live_keys), (0, 5));
        assert!(kv.get(b"k4").unwrap().is_some());
    }

    #[test]
    fn a_zero_filled_tail_is_a_torn_tail() {
        let t = TempDir::new("kvlog-zero").unwrap();
        let (_, full) = five_records(t.path());
        let mut f = OpenOptions::new().append(true).open(data_path(t.path())).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        drop(f);
        let (kv, report) = open(t.path());
        assert_eq!((report.stop, report.truncated_bytes, report.live_keys), (Stop::ZeroTail, 100, 5));
        assert_eq!(kv.stats().file_bytes, full);
    }

    #[test]
    fn a_garbled_last_record_is_torn_but_a_garbled_middle_record_is_corruption() {
        let t = TempDir::new("kvlog-garble").unwrap();
        let (starts, full) = five_records(t.path());
        let original = fs::read(data_path(t.path())).unwrap();

        // Last record: flip the final byte (complete extent, bad checksum, nothing after it).
        let mut tail = original.clone();
        *tail.last_mut().unwrap() ^= 0x55;
        let d = TempDir::new("kvlog-garble-tail").unwrap();
        fs::write(data_path(d.path()), &tail).unwrap();
        let (kv, report) = open(d.path());
        assert_eq!((report.stop, report.live_keys), (Stop::BadChecksumAtTail, 4));
        assert_eq!(report.truncated_bytes, full - starts[4]);
        assert_eq!(kv.get(b"k4").unwrap(), None);

        // Strict refuses even that.
        let d = TempDir::new("kvlog-garble-strict").unwrap();
        fs::write(data_path(d.path()), &tail).unwrap();
        let strict = Options { recovery: Recovery::Strict, ..Options::default() };
        match KvLog::open(d.path(), strict) {
            Err(Error::Corrupt(c)) => assert_eq!((c.offset, c.stop), (starts[4], Stop::BadChecksumAtTail)),
            other => panic!("expected refusal, got {:?}", other.map(|(_, r)| r)),
        }
        assert_eq!(fs::metadata(data_path(d.path())).unwrap().len(), full, "a refusal must not modify the file");

        // Middle record: flip a payload byte of record 2. Default policy refuses and leaves the file alone.
        let mut mid = original.clone();
        mid[starts[2] as usize + record::HEADER_LEN + 3] ^= 0x01;
        let d = TempDir::new("kvlog-garble-mid").unwrap();
        fs::write(data_path(d.path()), &mid).unwrap();
        match KvLog::open(d.path(), Options::default()) {
            Err(Error::Corrupt(c)) => assert_eq!((c.offset, c.stop), (starts[2], Stop::BadChecksumInMiddle)),
            other => panic!("expected refusal, got {:?}", other.map(|(_, r)| r)),
        }
        assert_eq!(fs::metadata(data_path(d.path())).unwrap().len(), full);

        // Point-in-time recovery instead stops at the damage and drops the three records after it,
        // although they are intact: that is the price of not refusing.
        let pit = Options { recovery: Recovery::PointInTime, ..Options::default() };
        let (kv, report) = KvLog::open(d.path(), pit).unwrap();
        assert_eq!((report.stop, report.live_keys, report.records), (Stop::BadChecksumInMiddle, 2, 2));
        assert_eq!(report.truncated_bytes, full - starts[2]);
        assert!(kv.contains_key(b"k0") && kv.contains_key(b"k1"));
        assert!(!kv.contains_key(b"k3") && !kv.contains_key(b"k4"));
    }

    #[test]
    fn appending_after_an_unrepaired_torn_tail_buries_the_new_record() {
        // This is why recovery truncates. Simulate a store that notices the torn tail but just keeps
        // appending after it, and then check what a later open can see.
        let t = TempDir::new("kvlog-bury").unwrap();
        let (starts, _) = five_records(t.path());
        let original = fs::read(data_path(t.path())).unwrap();
        let torn_at = starts[4] as usize + record::HEADER_LEN + 1; // header complete, body cut
        let mut damaged = original[..torn_at].to_vec();
        record::encode_into(&mut damaged, b"new", Some(b"acknowledged-write"));
        fs::write(data_path(t.path()), &damaged).unwrap();

        let (kv, report) = open(t.path()); // default policy: damaged tail is cut
        assert!(report.stop.is_torn_tail());
        assert_eq!(report.live_keys, 4);
        assert_eq!(kv.get(b"new").unwrap(), None, "the record written after the torn tail is gone");
    }

    #[test]
    fn an_impossible_header_is_corruption_and_nothing_is_allocated_for_it() {
        let t = TempDir::new("kvlog-badheader").unwrap();
        let (starts, full) = five_records(t.path());
        let mut bytes = fs::read(data_path(t.path())).unwrap();
        let s = starts[4] as usize;
        bytes[s + 8..s + 12].copy_from_slice(&(u32::MAX - 1).to_le_bytes());
        fs::write(data_path(t.path()), &bytes).unwrap();
        match KvLog::open(t.path(), Options::default()) {
            Err(Error::Corrupt(c)) => assert_eq!((c.offset, c.stop), (starts[4], Stop::BadHeader)),
            other => panic!("expected refusal, got {:?}", other.map(|(_, r)| r)),
        }
        assert_eq!(fs::metadata(data_path(t.path())).unwrap().len(), full);
    }

    #[test]
    fn bit_rot_after_open_is_caught_on_read_and_compaction_refuses_to_copy_it() {
        let t = TempDir::new("kvlog-rot").unwrap();
        let (starts, _) = five_records(t.path());
        let (mut kv, _) = open(t.path());
        // Flip one byte of k2's value behind the store's back.
        let f = OpenOptions::new().write(true).open(data_path(t.path())).unwrap();
        f.write_all_at(&[0xEE], starts[2] + record::HEADER_LEN as u64 + 2 + 1).unwrap();
        assert!(matches!(kv.get(b"k2"), Err(Error::ChecksumOnRead { offset }) if offset == starts[2]));
        assert!(kv.get(b"k1").unwrap().is_some(), "other keys are unaffected");
        assert!(matches!(kv.compact(), Err(Error::ChecksumOnRead { .. })));
        assert!(!t.path().join(COMPACT_FILE).exists(), "a failed compaction cleans up its temp file");
        assert!(kv.get(b"k1").unwrap().is_some(), "and leaves the store usable");
    }

    #[test]
    fn compaction_reclaims_garbage_and_keeps_every_live_key() {
        let t = TempDir::new("kvlog-compact").unwrap();
        let (mut kv, _) = open(t.path());
        let mut model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        for i in 0..100u32 {
            let k = format!("key{i:03}").into_bytes();
            let v = format!("v{i}").repeat(8).into_bytes();
            kv.put(&k, &v).unwrap();
            model.insert(k, v);
        }
        for round in 0..9u32 {
            for i in 0..50u32 {
                let k = format!("key{i:03}").into_bytes();
                let v = format!("r{round}-{i}").repeat(8).into_bytes();
                kv.put(&k, &v).unwrap();
                model.insert(k, v);
            }
        }
        for i in 80..100u32 {
            let k = format!("key{i:03}").into_bytes();
            assert!(kv.delete(&k).unwrap());
            model.remove(&k);
        }
        let before = kv.stats();
        assert!(before.garbage_bytes > 0 && before.space_amplification() > 3.0, "{before:?}");

        let report = kv.compact().unwrap();
        let after = kv.stats();
        assert_eq!(report.before_bytes, before.file_bytes);
        assert_eq!(report.after_bytes, after.file_bytes);
        assert!(after.file_bytes < before.file_bytes / 3);
        assert_eq!((after.garbage_bytes, after.live_keys), (0, model.len()));
        assert!((after.space_amplification() - 1.0).abs() < f64::EPSILON);
        for (k, v) in &model {
            assert_eq!(kv.get(k).unwrap().as_deref(), Some(v.as_slice()));
        }
        assert_eq!(kv.get(b"key090").unwrap(), None, "deleted keys stay deleted");

        // Appends continue after compaction and everything survives a reopen.
        kv.put(b"post", b"compaction").unwrap();
        model.insert(b"post".to_vec(), b"compaction".to_vec());
        drop(kv);
        let (kv, report) = open(t.path());
        assert_eq!((report.live_keys, report.stop), (model.len(), Stop::Clean));
        for (k, v) in &model {
            assert_eq!(kv.get(k).unwrap().as_deref(), Some(v.as_slice()));
        }
    }

    #[test]
    fn a_leftover_temp_file_from_an_interrupted_compaction_is_ignored() {
        let t = TempDir::new("kvlog-stale").unwrap();
        {
            let (mut kv, _) = open(t.path());
            kv.put(b"a", b"1").unwrap();
        }
        // A crash before the rename leaves a half-written temp file next to the intact data file.
        fs::write(t.path().join(COMPACT_FILE), b"half a compacted file").unwrap();
        let (kv, report) = open(t.path());
        assert!(report.removed_stale_compaction);
        assert!(!t.path().join(COMPACT_FILE).exists());
        assert_eq!(kv.get(b"a").unwrap().as_deref(), Some(&b"1"[..]));
    }

    #[test]
    fn size_limits_and_fsync_accounting() {
        let t = TempDir::new("kvlog-limits").unwrap();
        let opts = Options { sync: SyncPolicy::EveryWrite, ..Options::default() };
        let (mut kv, _) = KvLog::open(t.path(), opts).unwrap();
        assert!(matches!(kv.put(&vec![0; MAX_KEY_LEN + 1], b"v"), Err(Error::TooLarge)));
        kv.put(b"", b"").unwrap(); // empty key and value are legal
        kv.put(b"k", b"v").unwrap();
        assert_eq!(kv.stats().fsyncs, 2);
        assert_eq!(kv.get(b"").unwrap(), Some(Vec::new()));
        kv.sync().unwrap();
        assert_eq!(kv.stats().fsyncs, 3);
    }

    #[test]
    fn random_operations_match_a_model_across_reopens_and_compactions() {
        let t = TempDir::new("kvlog-model").unwrap();
        let (mut kv, _) = open(t.path());
        let mut model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let mut rng = Rng::new(0xC0FFEE);
        for step in 0..3000 {
            let key = format!("k{}", rng.below(40)).into_bytes();
            match rng.below(100) {
                0..=54 => {
                    let mut v = vec![0u8; rng.index(60)];
                    rng.fill(&mut v);
                    kv.put(&key, &v).unwrap();
                    model.insert(key, v);
                }
                55..=74 => assert_eq!(kv.delete(&key).unwrap(), model.remove(&key).is_some()),
                75..=94 => assert_eq!(kv.get(&key).unwrap(), model.get(&key).cloned(), "step {step}"),
                95..=97 => {
                    kv.compact().unwrap();
                }
                _ => {
                    drop(kv);
                    let (k2, report) = open(t.path());
                    assert_eq!(report.stop, Stop::Clean);
                    kv = k2;
                }
            }
            assert_eq!(kv.len(), model.len(), "step {step}");
        }
        let mut keys: Vec<Vec<u8>> = kv.keys().map(<[u8]>::to_vec).collect();
        keys.sort();
        assert_eq!(keys, model.keys().cloned().collect::<Vec<_>>());
    }
}
