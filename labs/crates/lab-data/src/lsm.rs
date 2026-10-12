//! Lab (b): a mini LSM-tree.
//!
//! * **Write path**: append the record to the write-ahead log, insert into the memtable (a sorted
//!   `BTreeMap`); when the memtable reaches `memtable_bytes` it is written out as an immutable
//!   [`SsTable`] (a *flush*) and the log is replaced by a fresh one.
//! * **Read path**: look in the memtable, then in the tables from newest to oldest; the first table
//!   that has the key (a value *or* a tombstone) answers. Scans merge all sources.
//! * **Deletes** are tombstone records. A tombstone may only be dropped when no older table can
//!   still hold the key, otherwise the old value comes back to life ([`TombstonePolicy`]).
//! * **Compaction** merges several sorted runs into one ([`Policy`]); the output replaces its
//!   inputs through an atomically replaced `MANIFEST`, so a crash at any point leaves either the old
//!   or the new set of files.
//! * **Recovery**: read the manifest, delete files it does not list, replay the log into the
//!   memtable (a torn log tail is cut as in [`crate::kvlog`]).
//!
//! Simplification: every compaction output is a single file (one *sorted run*). A level of
//! RocksDB-style leveled compaction is a run split into many files that are compacted one at a
//! time; here a level is one run that is merged as a whole, which is the shape of the classic
//! LSM-tree of the original paper.

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::record::{self, Recovery, Stop, MAX_KEY_LEN, MAX_VALUE_LEN};
use crate::sstable::{Builder, Entry, IoCounters, Lookup, SsTable};
use crate::util::sync_dir;

const MANIFEST: &str = "MANIFEST";

/// How runs are chosen for compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Never compact: every flush adds a run, so reads get slower and space is never reclaimed.
    None,
    /// Size-tiered: runs carry a *tier*; when `fanout` neighbouring runs share a tier they are
    /// merged into one run of the next tier. Each byte is rewritten about once per tier.
    Tiered { fanout: usize },
    /// One sorted run per level; level `i` may hold `base_bytes * fanout^i`. A run that outgrows its
    /// level moves down; if the next level is occupied the two runs are merged.
    Leveled { fanout: usize, base_bytes: u64 },
}

/// When a compaction may discard tombstones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TombstonePolicy {
    /// Only when the merge includes the oldest run, so nothing older can hold the key. Correct.
    DropAtBottom,
    /// Always. **Wrong on purpose**: it exists so a lab can show deleted keys reappearing.
    AlwaysDrop,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Flush the memtable once its keys and values total this many bytes.
    pub memtable_bytes: usize,
    pub block_bytes: usize,
    /// 0 builds tables without a Bloom filter.
    pub bloom_bits_per_key: usize,
    /// `fsync` the log after every write.
    pub sync_wal: bool,
    /// `false` skips every other `fsync` (tables, manifest, directory): faster tests and
    /// experiments; the crash-safety argument then only covers a *process* crash.
    pub durable: bool,
    pub policy: Policy,
    pub tombstones: TombstonePolicy,
    pub recovery: Recovery,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            memtable_bytes: 4 << 20,
            block_bytes: 4096,
            bloom_bits_per_key: 10,
            sync_wal: false,
            durable: true,
            policy: Policy::Tiered { fanout: 4 },
            tombstones: TombstonePolicy::DropAtBottom,
            recovery: Recovery::TolerateTail,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    /// Key and value bytes passed to `put`, key bytes passed to `delete`.
    pub user_bytes: u64,
    pub wal_bytes: u64,
    pub flush_bytes: u64,
    pub compaction_bytes: u64,
    pub flushes: u64,
    pub compactions: u64,
    /// Runs pushed down a level without rewriting (leveled policy, next level empty).
    pub trivial_moves: u64,
    pub gets: u64,
    pub block_reads: u64,
    pub bloom_negatives: u64,
    pub bloom_false_positives: u64,
    pub range_skips: u64,
}

impl Stats {
    /// Bytes written to storage per byte given to the database (log + flushes + compactions).
    pub fn write_amplification(&self) -> f64 {
        if self.user_bytes == 0 {
            0.0
        } else {
            (self.wal_bytes + self.flush_bytes + self.compaction_bytes) as f64 / self.user_bytes as f64
        }
    }

    /// Data blocks read per `get` (the I/O cost of a point lookup).
    pub fn blocks_per_get(&self) -> f64 {
        if self.gets == 0 { 0.0 } else { self.block_reads as f64 / self.gets as f64 }
    }
}

/// Where the answer to a lookup came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Memtable,
    /// The SSTable with this id.
    Run(u64),
}

/// The result of [`Lsm::lookup`]: a value, a tombstone (the key was deleted), or no trace at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Value(Vec<u8>, Origin),
    Deleted(Origin),
    Absent,
}

#[derive(Debug, Clone)]
pub struct RunInfo {
    pub id: u64,
    pub level: u32,
    pub file_bytes: u64,
    pub entries: u64,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenReport {
    pub runs: usize,
    pub wal_records: u64,
    pub wal_truncated_bytes: u64,
    pub wal_stop: Stop,
    /// Files in the directory that the manifest does not list (leftovers of a crash) and were deleted.
    pub orphans_removed: usize,
}

struct Run {
    id: u64,
    level: u32,
    table: SsTable,
}

struct Manifest {
    wal: u64,
    next: u64,
    /// `(id, level)`, newest first.
    runs: Vec<(u64, u32)>,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn sst_path(dir: &Path, id: u64) -> PathBuf {
    dir.join(format!("{id:06}.sst"))
}

fn wal_name(id: u64) -> String {
    format!("wal-{id:06}.log")
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

fn entry_size(key: &[u8], value: Option<&[u8]>) -> usize {
    key.len() + value.map_or(0, <[u8]>::len)
}

fn read_manifest(dir: &Path) -> io::Result<Option<Manifest>> {
    let text = match fs::read_to_string(dir.join(MANIFEST)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut lines = text.lines();
    if lines.next() != Some("lab-lsm 1") {
        return Err(invalid("MANIFEST: bad header"));
    }
    let (mut wal, mut next, mut runs) = (None, None, Vec::new());
    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let num = |s: &str| s.parse::<u64>().map_err(|_| invalid(format!("MANIFEST: bad number in {line:?}")));
        match parts.as_slice() {
            ["wal", n] => wal = Some(num(n)?),
            ["next", n] => next = Some(num(n)?),
            ["run", id, level] => runs.push((num(id)?, num(level)? as u32)),
            _ => return Err(invalid(format!("MANIFEST: unrecognised line {line:?}"))),
        }
    }
    Ok(Some(Manifest {
        wal: wal.ok_or_else(|| invalid("MANIFEST: no wal line"))?,
        next: next.ok_or_else(|| invalid("MANIFEST: no next line"))?,
        runs,
    }))
}

fn level_limit(base: u64, fanout: usize, level: u32) -> u64 {
    base.saturating_mul((fanout as u64).saturating_pow(level))
}

struct HeapItem {
    key: Vec<u8>,
    src: usize,
    value: Option<Vec<u8>>,
}

impl PartialEq for HeapItem {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for HeapItem {}

impl PartialOrd for HeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapItem {
    /// Reversed, so that `BinaryHeap` (a max-heap) pops the smallest key, and for equal keys the
    /// lowest source index, which is the newest source.
    fn cmp(&self, other: &Self) -> Ordering {
        (&other.key, other.src).cmp(&(&self.key, self.src))
    }
}

type Entries<'a> = Box<dyn Iterator<Item = io::Result<Entry>> + 'a>;

/// K-way merge of key-ordered sources. `sources[0]` is the newest: when several sources hold the same
/// key only the newest entry is returned, the older ones are skipped.
struct MergeIter<'a> {
    sources: Vec<Entries<'a>>,
    heap: BinaryHeap<HeapItem>,
    failed: bool,
}

impl<'a> MergeIter<'a> {
    fn new(sources: Vec<Entries<'a>>) -> io::Result<MergeIter<'a>> {
        let mut m = MergeIter { sources, heap: BinaryHeap::new(), failed: false };
        for src in 0..m.sources.len() {
            m.refill(src)?;
        }
        Ok(m)
    }

    fn refill(&mut self, src: usize) -> io::Result<()> {
        if let Some(item) = self.sources[src].next() {
            let (key, value) = item?;
            self.heap.push(HeapItem { key, src, value });
        }
        Ok(())
    }

    fn advance(&mut self) -> io::Result<Option<Entry>> {
        let Some(top) = self.heap.pop() else {
            return Ok(None);
        };
        self.refill(top.src)?;
        while self.heap.peek().is_some_and(|p| p.key == top.key) {
            let older = self.heap.pop().expect("peeked");
            self.refill(older.src)?;
        }
        Ok(Some((top.key, top.value)))
    }
}

impl Iterator for MergeIter<'_> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.advance() {
            Ok(entry) => entry.map(Ok),
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}

pub struct Lsm {
    dir: PathBuf,
    opts: Options,
    mem: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    mem_bytes: usize,
    wal: File,
    wal_id: u64,
    /// Newest first.
    runs: Vec<Run>,
    next_id: u64,
    counters: Rc<IoCounters>,
    stats: Stats,
    gets: Cell<u64>,
}

impl Lsm {
    pub fn open(dir: impl AsRef<Path>, opts: Options) -> io::Result<(Lsm, OpenReport)> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let counters = Rc::new(IoCounters::default());

        let manifest = match read_manifest(&dir)? {
            Some(m) => m,
            None => {
                let m = Manifest { wal: 1, next: 2, runs: Vec::new() };
                Self::write_manifest_file(&dir, &m, opts.durable)?;
                m
            }
        };

        // Anything the manifest does not list is a leftover of a flush or compaction that crashed
        // before (or after) its commit point.
        let live: HashSet<String> = manifest
            .runs
            .iter()
            .map(|&(id, _)| format!("{id:06}.sst"))
            .chain([wal_name(manifest.wal), MANIFEST.to_string()])
            .collect();
        let mut orphans_removed = 0;
        for entry in fs::read_dir(&dir)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            let ours = name.ends_with(".sst") || name.ends_with(".tmp") || name.starts_with("wal-");
            if ours && !live.contains(&name) {
                fs::remove_file(dir.join(&name))?;
                orphans_removed += 1;
            }
        }

        let mut runs = Vec::new();
        for &(id, level) in &manifest.runs {
            runs.push(Run { id, level, table: SsTable::open(&sst_path(&dir, id), counters.clone())? });
        }

        let wal_path = dir.join(wal_name(manifest.wal));
        let bytes = match fs::read(&wal_path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut mem = BTreeMap::new();
        let mut mem_bytes = 0;
        let scan = record::scan(&bytes, |_, rec| {
            Self::mem_apply(&mut mem, &mut mem_bytes, rec.key, rec.value);
        });
        let cut = record::bytes_to_cut(opts.recovery, &scan, bytes.len() as u64)
            .map_err(|c| invalid(format!("write-ahead log: {c}")))?;
        let wal = OpenOptions::new().create(true).append(true).open(&wal_path)?;
        if cut > 0 {
            wal.set_len(scan.valid_len as u64)?;
            wal.sync_all()?;
        }

        let report = OpenReport {
            runs: runs.len(),
            wal_records: scan.records,
            wal_truncated_bytes: cut,
            wal_stop: scan.stop,
            orphans_removed,
        };
        let lsm = Lsm {
            dir,
            opts,
            mem,
            mem_bytes,
            wal,
            wal_id: manifest.wal,
            runs,
            next_id: manifest.next,
            counters,
            stats: Stats::default(),
            gets: Cell::new(0),
        };
        Ok((lsm, report))
    }

    fn mem_apply(mem: &mut BTreeMap<Vec<u8>, Option<Vec<u8>>>, mem_bytes: &mut usize, key: &[u8], value: Option<&[u8]>) {
        let new = entry_size(key, value);
        match mem.insert(key.to_vec(), value.map(<[u8]>::to_vec)) {
            Some(old) => *mem_bytes = *mem_bytes - entry_size(key, old.as_deref()) + new,
            None => *mem_bytes += new,
        }
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> io::Result<()> {
        self.write(key, Some(value))
    }

    pub fn delete(&mut self, key: &[u8]) -> io::Result<()> {
        self.write(key, None)
    }

    fn write(&mut self, key: &[u8], value: Option<&[u8]>) -> io::Result<()> {
        if key.len() > MAX_KEY_LEN || value.is_some_and(|v| v.len() > MAX_VALUE_LEN) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "key or value too large"));
        }
        let mut buf = Vec::with_capacity(record::encoded_len(key.len(), value.map(<[u8]>::len)));
        record::encode_into(&mut buf, key, value);
        // The log first: if we crash after this line the record is replayed; before it, it never happened.
        self.wal.write_all(&buf)?;
        if self.opts.sync_wal {
            self.wal.sync_data()?;
        }
        self.stats.wal_bytes += buf.len() as u64;
        self.stats.user_bytes += entry_size(key, value) as u64;
        Self::mem_apply(&mut self.mem, &mut self.mem_bytes, key, value);
        if self.mem_bytes >= self.opts.memtable_bytes {
            self.flush()?;
        }
        Ok(())
    }

    /// Memtable first, then runs from newest to oldest; the first run that knows the key decides.
    pub fn get(&self, key: &[u8]) -> io::Result<Option<Vec<u8>>> {
        Ok(match self.lookup(key)? {
            Answer::Value(v, _) => Some(v),
            Answer::Deleted(_) | Answer::Absent => None,
        })
    }

    /// Like [`Lsm::get`] but also says which source answered, so a lab can show merge-on-read at work.
    pub fn lookup(&self, key: &[u8]) -> io::Result<Answer> {
        self.gets.set(self.gets.get() + 1);
        if let Some(v) = self.mem.get(key) {
            return Ok(match v {
                Some(v) => Answer::Value(v.clone(), Origin::Memtable),
                None => Answer::Deleted(Origin::Memtable),
            });
        }
        for run in &self.runs {
            match run.table.get(key)? {
                Lookup::Found(v) => return Ok(Answer::Value(v, Origin::Run(run.id))),
                Lookup::Deleted => return Ok(Answer::Deleted(Origin::Run(run.id))),
                Lookup::Absent => {}
            }
        }
        Ok(Answer::Absent)
    }

    /// Live entries with `from <= key < to`, in key order (a merge of the memtable and every run).
    pub fn scan(&self, from: &[u8], to: &[u8]) -> io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let mut out = Vec::new();
        for item in self.merged_from(from)? {
            let (k, v) = item?;
            if k.as_slice() >= to {
                break;
            }
            if let Some(v) = v {
                out.push((k, v));
            }
        }
        Ok(out)
    }

    fn merged_from(&self, from: &[u8]) -> io::Result<MergeIter<'_>> {
        let mut sources: Vec<Entries<'_>> = Vec::with_capacity(self.runs.len() + 1);
        let mem = self.mem.range::<[u8], _>((Bound::Included(from), Bound::Unbounded));
        sources.push(Box::new(mem.map(|(k, v)| Ok((k.clone(), v.clone())))));
        for run in &self.runs {
            sources.push(Box::new(run.table.iter_from(from)));
        }
        MergeIter::new(sources)
    }

    /// Number and total size (keys + values) of live entries; reads everything.
    pub fn live_data(&self) -> io::Result<(u64, u64)> {
        let (mut keys, mut bytes) = (0, 0);
        for item in self.merged_from(&[])? {
            let (k, v) = item?;
            if let Some(v) = v {
                keys += 1;
                bytes += (k.len() + v.len()) as u64;
            }
        }
        Ok((keys, bytes))
    }

    pub fn runs(&self) -> Vec<RunInfo> {
        self.runs
            .iter()
            .map(|r| RunInfo { id: r.id, level: r.level, file_bytes: r.table.file_bytes(), entries: r.table.entries() })
            .collect()
    }

    pub fn memtable_len(&self) -> usize {
        self.mem.len()
    }

    /// Bytes of all table files.
    pub fn table_bytes(&self) -> u64 {
        self.runs.iter().map(|r| r.table.file_bytes()).sum()
    }

    pub fn stats(&self) -> Stats {
        let c = &self.counters;
        Stats {
            gets: self.gets.get(),
            block_reads: c.block_reads.get(),
            bloom_negatives: c.bloom_negatives.get(),
            bloom_false_positives: c.bloom_false_positives.get(),
            range_skips: c.range_skips.get(),
            ..self.stats
        }
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn write_manifest_file(dir: &Path, m: &Manifest, durable: bool) -> io::Result<()> {
        let mut text = format!("lab-lsm 1\nwal {}\nnext {}\n", m.wal, m.next);
        for (id, level) in &m.runs {
            text.push_str(&format!("run {id} {level}\n"));
        }
        let path = dir.join(MANIFEST);
        let tmp = tmp_path(&path);
        let mut f = File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        if durable {
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        if durable {
            sync_dir(dir)?;
        }
        Ok(())
    }

    /// The commit point of a flush or compaction: replace the manifest atomically.
    fn commit_manifest(&self, wal: u64) -> io::Result<()> {
        let m = Manifest { wal, next: self.next_id, runs: self.runs.iter().map(|r| (r.id, r.level)).collect() };
        Self::write_manifest_file(&self.dir, &m, self.opts.durable)
    }

    /// Writes the memtable out as a new level-0 run, starts a fresh log, then runs the compaction policy.
    pub fn flush(&mut self) -> io::Result<()> {
        if self.mem.is_empty() {
            return Ok(());
        }
        let id = self.alloc_id();
        let path = sst_path(&self.dir, id);
        let tmp = tmp_path(&path);
        let mut b = Builder::create(&tmp, self.opts.block_bytes, self.opts.bloom_bits_per_key)?.with_sync(self.opts.durable);
        for (k, v) in &self.mem {
            b.add(k, v.as_deref())?;
        }
        let info = b.finish()?;
        fs::rename(&tmp, &path)?;
        let table = SsTable::open(&path, self.counters.clone())?;

        let new_wal_id = self.alloc_id();
        let new_wal = OpenOptions::new().create_new(true).append(true).open(self.dir.join(wal_name(new_wal_id)))?;
        self.runs.insert(0, Run { id, level: 0, table });
        self.commit_manifest(new_wal_id)?;

        let old_wal = self.dir.join(wal_name(self.wal_id));
        self.wal = new_wal;
        self.wal_id = new_wal_id;
        fs::remove_file(old_wal)?;
        self.mem.clear();
        self.mem_bytes = 0;
        self.stats.flushes += 1;
        self.stats.flush_bytes += info.file_bytes;
        self.maybe_compact()
    }

    fn maybe_compact(&mut self) -> io::Result<()> {
        match self.opts.policy {
            Policy::None => Ok(()),
            Policy::Tiered { fanout } => self.compact_tiered(fanout.max(2)),
            Policy::Leveled { fanout, base_bytes } => self.compact_leveled(fanout.max(2), base_bytes),
        }
    }

    fn drops_tombstones(&self, last_position: usize) -> bool {
        match self.opts.tombstones {
            TombstonePolicy::AlwaysDrop => true,
            TombstonePolicy::DropAtBottom => last_position + 1 == self.runs.len(),
        }
    }

    fn compact_tiered(&mut self, fanout: usize) -> io::Result<()> {
        while let Some((start, tier)) = self.find_tier_group(fanout) {
            let group: Vec<usize> = (start..start + fanout).collect();
            let drop = self.drops_tombstones(start + fanout - 1);
            self.merge_runs(&group, tier + 1, drop)?;
        }
        Ok(())
    }

    /// Lowest tier that has `fanout` neighbouring runs; returns the position of the newest of them.
    fn find_tier_group(&self, fanout: usize) -> Option<(usize, u32)> {
        let mut best: Option<(usize, u32)> = None;
        let mut i = 0;
        while i < self.runs.len() {
            let tier = self.runs[i].level;
            let mut j = i;
            while j < self.runs.len() && self.runs[j].level == tier {
                j += 1;
            }
            if j - i >= fanout && best.is_none_or(|(_, t)| tier < t) {
                best = Some((i, tier));
            }
            i = j;
        }
        best
    }

    fn compact_leveled(&mut self, fanout: usize, base_bytes: u64) -> io::Result<()> {
        loop {
            // Invariant: runs are ordered by level, at most one per level. A fresh flush at level 0
            // collides with the existing level-0 run, and a run pushed down may collide too: merge.
            if let Some(i) = (0..self.runs.len().saturating_sub(1)).find(|&i| self.runs[i].level == self.runs[i + 1].level) {
                let level = self.runs[i].level;
                let drop = self.drops_tombstones(i + 1);
                self.merge_runs(&[i, i + 1], level, drop)?;
                continue;
            }
            // A run bigger than its level's capacity moves down one level.
            let Some(i) = self.runs.iter().position(|r| r.table.file_bytes() > level_limit(base_bytes, fanout, r.level)) else {
                return Ok(());
            };
            self.runs[i].level += 1;
            self.commit_manifest(self.wal_id)?;
            self.stats.trivial_moves += 1;
        }
    }

    /// Major compaction: merge everything into one run and drop all tombstones.
    pub fn compact_all(&mut self) -> io::Result<()> {
        self.flush()?;
        if self.runs.is_empty() {
            return Ok(());
        }
        let positions: Vec<usize> = (0..self.runs.len()).collect();
        let level = self.runs.last().map_or(0, |r| r.level);
        self.merge_runs(&positions, level, true)
    }

    /// Merges the runs at `positions` (ascending, i.e. newest first) into one run placed where the
    /// newest input was. Commit point: the manifest replacement.
    fn merge_runs(&mut self, positions: &[usize], out_level: u32, drop_tombstones: bool) -> io::Result<()> {
        let id = self.alloc_id();
        let path = sst_path(&self.dir, id);
        let tmp = tmp_path(&path);
        let info = {
            let sources: Vec<Entries<'_>> = positions.iter().map(|&p| Box::new(self.runs[p].table.iter()) as Entries<'_>).collect();
            let mut b = Builder::create(&tmp, self.opts.block_bytes, self.opts.bloom_bits_per_key)?.with_sync(self.opts.durable);
            for item in MergeIter::new(sources)? {
                let (k, v) = item?;
                if v.is_none() && drop_tombstones {
                    continue;
                }
                b.add(&k, v.as_deref())?;
            }
            b.finish()?
        };
        let output = if info.entries == 0 {
            fs::remove_file(&tmp)?;
            None
        } else {
            fs::rename(&tmp, &path)?;
            Some(Run { id, level: out_level, table: SsTable::open(&path, self.counters.clone())? })
        };
        let first = positions[0];
        let mut removed = Vec::with_capacity(positions.len());
        for &p in positions.iter().rev() {
            removed.push(self.runs.remove(p));
        }
        if let Some(run) = output {
            self.runs.insert(first, run);
        }
        self.commit_manifest(self.wal_id)?;
        for run in removed {
            let path = run.table.path().to_path_buf();
            drop(run);
            fs::remove_file(path)?;
        }
        self.stats.compactions += 1;
        self.stats.compaction_bytes += info.file_bytes;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::util::TempDir;

    fn small(policy: Policy) -> Options {
        Options { memtable_bytes: 1 << 30, block_bytes: 256, durable: false, policy, ..Options::default() }
    }

    fn put_all(db: &mut Lsm, keys: impl IntoIterator<Item = u32>, tag: &str) {
        for i in keys {
            db.put(format!("k{i:04}").as_bytes(), format!("{tag}{i}").as_bytes()).unwrap();
        }
    }

    fn get(db: &Lsm, i: u32) -> Option<String> {
        db.get(format!("k{i:04}").as_bytes()).unwrap().map(|v| String::from_utf8(v).unwrap())
    }

    #[test]
    fn memtable_flush_and_merge_on_read_newest_wins() {
        let t = TempDir::new("lsm-read").unwrap();
        let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
        put_all(&mut db, 0..10, "old");
        db.flush().unwrap();
        put_all(&mut db, 5..15, "mid");
        db.flush().unwrap();
        put_all(&mut db, 10..20, "new"); // stays in the memtable
        assert_eq!(db.runs().len(), 2);
        assert_eq!(db.memtable_len(), 10);
        assert_eq!(get(&db, 0).as_deref(), Some("old0"));
        assert_eq!(get(&db, 7).as_deref(), Some("mid7"), "second run shadows the first");
        assert_eq!(get(&db, 12).as_deref(), Some("new12"), "memtable shadows both runs");
        assert_eq!(get(&db, 99), None);
        // A scan sees one entry per key, the newest one.
        let all = db.scan(b"", b"\xff").unwrap();
        assert_eq!(all.len(), 20);
        assert_eq!(all[7], (b"k0007".to_vec(), b"mid7".to_vec()));
        assert!(all.windows(2).all(|w| w[0].0 < w[1].0));
        let part = db.scan(b"k0008", b"k0012").unwrap();
        assert_eq!(part.iter().map(|(k, _)| String::from_utf8_lossy(k).into_owned()).collect::<Vec<_>>(), ["k0008", "k0009", "k0010", "k0011"]);
    }

    #[test]
    fn a_tombstone_hides_older_values_in_every_older_run() {
        let t = TempDir::new("lsm-tomb").unwrap();
        let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
        put_all(&mut db, 0..5, "v");
        db.flush().unwrap();
        db.delete(b"k0002").unwrap();
        db.flush().unwrap();
        assert_eq!(get(&db, 2), None, "deleted in the newer run");
        assert_eq!(get(&db, 3).as_deref(), Some("v3"));
        assert_eq!(db.scan(b"", b"\xff").unwrap().len(), 4);
        db.put(b"k0002", b"back").unwrap();
        assert_eq!(get(&db, 2).as_deref(), Some("back"), "a later put in the memtable wins over the tombstone");
        // Deleting a key that was never written is harmless.
        db.delete(b"never").unwrap();
        assert_eq!(db.get(b"never").unwrap(), None);
    }

    /// Six flushes under size-tiered compaction with fanout 3: the first three runs merge into the
    /// oldest run (which holds `k0001 = v1`), then the tombstone for `k0001` is flushed together
    /// with two other runs, and *that* merge does not include the oldest run.
    /// Returns the answer for `k0001` and the entry count of the newest run.
    fn deleted_key_scenario(tombstones: TombstonePolicy) -> (Option<String>, u64) {
        let t = TempDir::new("lsm-resurrect").unwrap();
        let opts = Options { tombstones, ..small(Policy::Tiered { fanout: 3 }) };
        let (mut db, _) = Lsm::open(t.path(), opts).unwrap();
        for i in [1, 100, 101] {
            put_all(&mut db, [i], "v");
            db.flush().unwrap();
        }
        db.delete(b"k0001").unwrap();
        db.flush().unwrap();
        for i in [200, 201] {
            put_all(&mut db, [i], "w");
            db.flush().unwrap();
        }
        assert_eq!(db.runs().iter().map(|r| r.level).collect::<Vec<_>>(), [1, 1]);
        (get(&db, 1), db.runs()[0].entries)
    }

    #[test]
    fn dropping_a_tombstone_too_early_resurrects_the_deleted_key() {
        // Correct: the newer run keeps the tombstone (and the two puts): 3 entries; the key stays deleted.
        assert_eq!(deleted_key_scenario(TombstonePolicy::DropAtBottom), (None, 3));
        // Wrong: the merge drops the tombstone (2 entries left) and the old value shows through again.
        assert_eq!(deleted_key_scenario(TombstonePolicy::AlwaysDrop), (Some("v1".to_string()), 2));
    }

    #[test]
    fn major_compaction_purges_tombstones_and_shadowed_values() {
        let t = TempDir::new("lsm-major").unwrap();
        let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
        put_all(&mut db, 0..100, "a");
        db.flush().unwrap();
        put_all(&mut db, 0..100, "b");
        for i in 0..50 {
            db.delete(format!("k{i:04}").as_bytes()).unwrap();
        }
        db.flush().unwrap();
        let before = db.table_bytes();
        assert_eq!(db.runs().len(), 2);
        db.compact_all().unwrap();
        assert_eq!(db.runs().len(), 1);
        assert_eq!(db.runs()[0].entries, 50, "no tombstones, no shadowed versions");
        assert!(db.table_bytes() < before / 2 + 200);
        assert_eq!(get(&db, 10), None);
        assert_eq!(get(&db, 60).as_deref(), Some("b60"));
        // Everything deleted: the merge output is empty and the run disappears.
        for i in 50..100 {
            db.delete(format!("k{i:04}").as_bytes()).unwrap();
        }
        db.compact_all().unwrap();
        assert!(db.runs().is_empty());
        assert_eq!(db.scan(b"", b"\xff").unwrap().len(), 0);
    }

    #[test]
    fn tiered_compaction_keeps_the_run_count_logarithmic() {
        let t = TempDir::new("lsm-tiered").unwrap();
        let (mut db, _) = Lsm::open(t.path(), small(Policy::Tiered { fanout: 4 })).unwrap();
        for round in 0..64 {
            put_all(&mut db, round * 10..round * 10 + 10, "v");
            db.flush().unwrap();
            let levels: Vec<u32> = db.runs().iter().map(|r| r.level).collect();
            assert!(levels.windows(2).all(|w| w[0] <= w[1]), "newest runs have the lowest tier: {levels:?}");
            assert!(levels.iter().filter(|&&l| l == 0).count() < 4);
        }
        // 64 flushes with fanout 4 leave exactly 4^3 = 64 -> a single run of tier 3.
        let runs = db.runs();
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0].level, 3);
        assert_eq!(db.stats().flushes, 64);
        assert!(db.stats().compactions >= 21);
        assert_eq!(db.scan(b"", b"\xff").unwrap().len(), 640);
    }

    #[test]
    fn leveled_compaction_keeps_one_run_per_level_within_capacity() {
        let t = TempDir::new("lsm-leveled").unwrap();
        let opts = small(Policy::Leveled { fanout: 3, base_bytes: 600 });
        let (mut db, _) = Lsm::open(t.path(), opts).unwrap();
        let mut rng = Rng::new(5);
        for _ in 0..80 {
            for _ in 0..8 {
                put_all(&mut db, [rng.below(300) as u32], "v");
            }
            db.flush().unwrap();
            let runs = db.runs();
            assert!(runs.windows(2).all(|w| w[0].level < w[1].level), "one run per level, ordered: {runs:?}");
            for r in &runs {
                assert!(r.file_bytes <= level_limit(600, 3, r.level), "level {} over capacity: {r:?}", r.level);
            }
        }
        assert!(db.stats().trivial_moves > 0 && db.stats().compactions > 0);
    }

    #[test]
    fn the_filter_cuts_read_amplification_for_missing_keys() {
        let t = TempDir::new("lsm-bloom").unwrap();
        let reads = |bits: usize| {
            let dir = t.path().join(format!("b{bits}"));
            let opts = Options { bloom_bits_per_key: bits, ..small(Policy::None) };
            let (mut db, _) = Lsm::open(&dir, opts).unwrap();
            for round in 0..8 {
                put_all(&mut db, (0..500).map(|i| i * 8 + round), "v"); // 8 interleaved runs
                db.flush().unwrap();
            }
            for i in 0..4000u32 {
                assert_eq!(db.get(format!("k{i:04}x").as_bytes()).unwrap(), None); // never written
            }
            db.stats().blocks_per_get()
        };
        let without = reads(0);
        let with = reads(10);
        assert!(without > 7.0, "without a filter every run reads a block: {without}");
        assert!(with < 0.5, "with 10 bits per key almost none do: {with}");
    }

    #[test]
    fn unflushed_writes_survive_a_crash_through_the_log() {
        let t = TempDir::new("lsm-wal").unwrap();
        {
            let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
            put_all(&mut db, 0..5, "flushed");
            db.flush().unwrap();
            put_all(&mut db, 5..10, "logged");
            db.delete(b"k0001").unwrap();
            // Dropped without a flush: only the log has the last six operations.
        }
        let (db, report) = Lsm::open(t.path(), small(Policy::None)).unwrap();
        assert_eq!((report.runs, report.wal_records, report.wal_truncated_bytes), (1, 6, 0));
        assert_eq!(get(&db, 3).as_deref(), Some("flushed3"));
        assert_eq!(get(&db, 7).as_deref(), Some("logged7"));
        assert_eq!(get(&db, 1), None, "the logged delete is replayed too");
    }

    #[test]
    fn a_torn_log_tail_loses_only_the_unfinished_record() {
        let t = TempDir::new("lsm-torn").unwrap();
        let wal = {
            let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
            put_all(&mut db, 0..10, "v");
            db.dir.join(wal_name(db.wal_id))
        };
        let full = fs::metadata(&wal).unwrap().len();
        let last_len = record::encoded_len(5, Some(format!("v{}", 9).len())) as u64;
        for cut in [1, 5, 12, last_len - 1] {
            let d = TempDir::new("lsm-torn-case").unwrap();
            fs::copy(&wal, d.path().join(wal.file_name().unwrap())).unwrap();
            fs::copy(t.path().join(MANIFEST), d.path().join(MANIFEST)).unwrap();
            let copy = d.path().join(wal.file_name().unwrap());
            OpenOptions::new().write(true).open(&copy).unwrap().set_len(full - cut).unwrap();
            let (db, report) = Lsm::open(d.path(), small(Policy::None)).unwrap();
            assert_eq!((report.wal_records, report.wal_truncated_bytes), (9, last_len - cut), "cut {cut}");
            assert!(report.wal_stop.is_torn_tail());
            assert_eq!(get(&db, 8).as_deref(), Some("v8"));
            assert_eq!(get(&db, 9), None);
        }
    }

    #[test]
    fn files_the_manifest_does_not_list_are_leftovers_of_a_crash_and_get_deleted() {
        let t = TempDir::new("lsm-orphans").unwrap();
        {
            let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
            put_all(&mut db, 0..5, "v");
            db.flush().unwrap();
        }
        // A crash after writing a table but before the manifest commit; a half-written table; an old log.
        let existing = fs::read_dir(t.path()).unwrap().map(|e| e.unwrap().path()).find(|p| p.extension().is_some_and(|e| e == "sst")).unwrap();
        fs::copy(&existing, sst_path(t.path(), 99)).unwrap();
        fs::write(t.path().join("000100.sst.tmp"), b"junk").unwrap();
        fs::write(t.path().join(wal_name(77)), b"old log").unwrap();
        let (db, report) = Lsm::open(t.path(), small(Policy::None)).unwrap();
        assert_eq!(report.orphans_removed, 3);
        assert!(!sst_path(t.path(), 99).exists());
        assert_eq!(db.scan(b"", b"\xff").unwrap().len(), 5, "the orphan table was not mistaken for data");
    }

    #[test]
    fn a_damaged_manifest_is_an_error_not_a_silent_empty_database() {
        let t = TempDir::new("lsm-manifest").unwrap();
        {
            let (mut db, _) = Lsm::open(t.path(), small(Policy::None)).unwrap();
            put_all(&mut db, 0..3, "v");
            db.flush().unwrap();
        }
        fs::write(t.path().join(MANIFEST), "garbage\n").unwrap();
        assert!(Lsm::open(t.path(), small(Policy::None)).is_err());
    }

    #[test]
    fn write_amplification_counts_log_flush_and_compaction_bytes() {
        let t = TempDir::new("lsm-wa").unwrap();
        let opts = Options { memtable_bytes: 4000, ..small(Policy::Tiered { fanout: 4 }) };
        let (mut db, _) = Lsm::open(t.path(), opts).unwrap();
        let mut rng = Rng::new(11);
        for _ in 0..4000 {
            let k = rng.below(2000) as u32;
            put_all(&mut db, [k], "value-");
        }
        let s = db.stats();
        assert!(s.flushes > 10 && s.compactions > 0);
        assert!(s.wal_bytes > s.user_bytes, "the log adds a 12-byte header to every record");
        assert!(s.write_amplification() > 2.0, "{s:?}");
        let expect = (s.wal_bytes + s.flush_bytes + s.compaction_bytes) as f64 / s.user_bytes as f64;
        assert!((s.write_amplification() - expect).abs() < 1e-12);
    }

    /// Random operations against a `BTreeMap`, with flushes, compactions and reopens, for every policy.
    fn model_check(policy: Policy, seed: u64) {
        let t = TempDir::new("lsm-model").unwrap();
        let opts = Options { memtable_bytes: 700, ..small(policy) };
        let (mut db, _) = Lsm::open(t.path(), opts).unwrap();
        let mut model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        let mut rng = Rng::new(seed);
        for step in 0..2500 {
            let key = format!("k{:03}", rng.below(120)).into_bytes();
            match rng.below(100) {
                0..=49 => {
                    let mut v = vec![0u8; rng.index(40)];
                    rng.fill(&mut v);
                    db.put(&key, &v).unwrap();
                    model.insert(key, v);
                }
                50..=64 => {
                    db.delete(&key).unwrap();
                    model.remove(&key);
                }
                65..=89 => assert_eq!(db.get(&key).unwrap(), model.get(&key).cloned(), "{policy:?} step {step}"),
                90..=93 => {
                    let other = format!("k{:03}", rng.below(120)).into_bytes();
                    let (from, to) = if key <= other { (key, other) } else { (other, key) };
                    let want: Vec<_> = model.range(from.clone()..to.clone()).map(|(k, v)| (k.clone(), v.clone())).collect();
                    assert_eq!(db.scan(&from, &to).unwrap(), want, "{policy:?} step {step}");
                }
                94..=95 => db.flush().unwrap(),
                96 => db.compact_all().unwrap(),
                _ => {
                    drop(db);
                    db = Lsm::open(t.path(), opts).unwrap().0;
                }
            }
        }
        let all: Vec<_> = model.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(db.scan(b"", b"\xff").unwrap(), all, "{policy:?}");
    }

    #[test]
    fn random_operations_match_a_model_under_every_compaction_policy() {
        model_check(Policy::None, 1);
        model_check(Policy::Tiered { fanout: 2 }, 2);
        model_check(Policy::Tiered { fanout: 4 }, 3);
        model_check(Policy::Leveled { fanout: 2, base_bytes: 300 }, 4);
        model_check(Policy::Leveled { fanout: 4, base_bytes: 900 }, 5);
    }
}
