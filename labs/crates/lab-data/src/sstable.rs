//! An SSTable: an immutable file of records sorted by key.
//!
//! ```text
//! file   := block* | index | bloom | footer
//! block  := record*                       (records of crate::record, ~block_bytes long, never split)
//! index  := count: u32 | (key_len: u32, first_key, offset: u64, len: u32)* | min_key | max_key | crc32c
//! bloom  := Bloom::to_bytes | crc32c      (absent when built without a filter)
//! footer := index_off, index_len, bloom_off, bloom_len, entries, MAGIC (6 x u64) | crc32c
//! ```
//!
//! Opening a table loads the index (one entry per block) and the filter into RAM. A point lookup
//! then costs: a key-range check, a filter probe, a binary search in the index and a read of one
//! block. Deleted keys are stored as tombstone records, so a table can say "deleted" as well as
//! "has value" and "no information".

use std::cell::Cell;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::bloom::{hash_pair, Bloom};
use crate::crc32c::crc32c;
use crate::record::{self, Parse};

const MAGIC: u64 = 0x4C41_425F_4C53_4D31; // "LAB_LSM1"
const FOOTER_LEN: usize = 6 * 8 + 4;

/// One key with its value, or `None` for a tombstone.
pub type Entry = (Vec<u8>, Option<Vec<u8>>);

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Counters shared by all tables of one database, so a lab can report read amplification.
#[derive(Debug, Default)]
pub struct IoCounters {
    /// Data blocks read from disk (point lookups and scans).
    pub block_reads: Cell<u64>,
    /// Lookups answered "absent" by the Bloom filter.
    pub bloom_negatives: Cell<u64>,
    /// The filter said "maybe" but the block did not contain the key.
    pub bloom_false_positives: Cell<u64>,
    /// Lookups answered "absent" because the key is outside [min_key, max_key].
    pub range_skips: Cell<u64>,
}

fn bump(c: &Cell<u64>) {
    c.set(c.get() + 1);
}

/// What one table knows about a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(Vec<u8>),
    /// A tombstone: the key was deleted here, so older tables must not be consulted.
    Deleted,
    Absent,
}

#[derive(Debug, Clone)]
struct IndexEntry {
    first_key: Vec<u8>,
    offset: u64,
    len: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct TableInfo {
    pub file_bytes: u64,
    pub entries: u64,
}

pub struct Builder {
    out: BufWriter<File>,
    block: Vec<u8>,
    block_first_key: Vec<u8>,
    block_bytes: usize,
    offset: u64,
    index: Vec<IndexEntry>,
    hashes: Vec<(u64, u64)>,
    bits_per_key: usize,
    entries: u64,
    min_key: Option<Vec<u8>>,
    last_key: Option<Vec<u8>>,
    sync: bool,
}

impl Builder {
    /// `bits_per_key == 0` builds a table without a Bloom filter.
    pub fn create(path: &Path, block_bytes: usize, bits_per_key: usize) -> io::Result<Builder> {
        Ok(Builder {
            out: BufWriter::new(File::create(path)?),
            block: Vec::new(),
            block_first_key: Vec::new(),
            block_bytes: block_bytes.max(1),
            offset: 0,
            index: Vec::new(),
            hashes: Vec::new(),
            bits_per_key,
            entries: 0,
            min_key: None,
            last_key: None,
            sync: true,
        })
    }

    /// `false` skips the final `fsync` (tests and benchmarks that only care about a process crash).
    pub fn with_sync(mut self, sync: bool) -> Builder {
        self.sync = sync;
        self
    }

    /// Keys must arrive in strictly increasing order.
    pub fn add(&mut self, key: &[u8], value: Option<&[u8]>) -> io::Result<()> {
        if self.last_key.as_deref().is_some_and(|last| key <= last) {
            return Err(invalid("keys must be added in strictly increasing order"));
        }
        if self.block.is_empty() {
            self.block_first_key = key.to_vec();
        }
        record::encode_into(&mut self.block, key, value);
        if self.bits_per_key > 0 {
            self.hashes.push(hash_pair(key));
        }
        self.entries += 1;
        if self.min_key.is_none() {
            self.min_key = Some(key.to_vec());
        }
        self.last_key = Some(key.to_vec());
        if self.block.len() >= self.block_bytes {
            self.flush_block()?;
        }
        Ok(())
    }

    fn flush_block(&mut self) -> io::Result<()> {
        if self.block.is_empty() {
            return Ok(());
        }
        self.out.write_all(&self.block)?;
        self.index.push(IndexEntry {
            first_key: std::mem::take(&mut self.block_first_key),
            offset: self.offset,
            len: self.block.len() as u32,
        });
        self.offset += self.block.len() as u64;
        self.block.clear();
        Ok(())
    }

    /// Writes index, filter and footer, then `fsync`s the file (unless `with_sync(false)`).
    pub fn finish(mut self) -> io::Result<TableInfo> {
        self.flush_block()?;

        let mut index = Vec::new();
        index.extend_from_slice(&(self.index.len() as u32).to_le_bytes());
        for e in &self.index {
            index.extend_from_slice(&(e.first_key.len() as u32).to_le_bytes());
            index.extend_from_slice(&e.first_key);
            index.extend_from_slice(&e.offset.to_le_bytes());
            index.extend_from_slice(&e.len.to_le_bytes());
        }
        for key in [self.min_key.as_deref().unwrap_or(&[]), self.last_key.as_deref().unwrap_or(&[])] {
            index.extend_from_slice(&(key.len() as u32).to_le_bytes());
            index.extend_from_slice(key);
        }
        let crc = crc32c(&index);
        index.extend_from_slice(&crc.to_le_bytes());

        let mut bloom_bytes = Vec::new();
        if self.bits_per_key > 0 && self.entries > 0 {
            let mut bloom = Bloom::with_capacity(self.entries as usize, self.bits_per_key);
            for h in &self.hashes {
                bloom.insert_hashed(*h);
            }
            bloom_bytes = bloom.to_bytes();
            let crc = crc32c(&bloom_bytes);
            bloom_bytes.extend_from_slice(&crc.to_le_bytes());
        }

        let index_off = self.offset;
        let bloom_off = index_off + index.len() as u64;
        let mut footer = Vec::with_capacity(FOOTER_LEN);
        for v in [index_off, index.len() as u64, bloom_off, bloom_bytes.len() as u64, self.entries, MAGIC] {
            footer.extend_from_slice(&v.to_le_bytes());
        }
        let crc = crc32c(&footer);
        footer.extend_from_slice(&crc.to_le_bytes());

        self.out.write_all(&index)?;
        self.out.write_all(&bloom_bytes)?;
        self.out.write_all(&footer)?;
        self.out.flush()?;
        if self.sync {
            self.out.get_ref().sync_all()?;
        }
        Ok(TableInfo { file_bytes: bloom_off + bloom_bytes.len() as u64 + footer.len() as u64, entries: self.entries })
    }
}

/// Cursor over a byte slice that reports truncation as `InvalidData` instead of panicking.
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len()).ok_or_else(|| invalid("section truncated"))?;
        let s = &self.buf[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }

    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
}

/// Splits `payload | crc32c` and checks the checksum.
fn checked_payload<'a>(section: &'a [u8], what: &str) -> io::Result<&'a [u8]> {
    let (payload, crc) = section.split_at(section.len().checked_sub(4).ok_or_else(|| invalid(format!("{what} too short")))?);
    if crc32c(payload).to_le_bytes() != crc {
        return Err(invalid(format!("{what} checksum mismatch")));
    }
    Ok(payload)
}

fn parse_block(buf: &[u8]) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < buf.len() {
        match record::parse(&buf[pos..]) {
            Parse::Record(r) => {
                out.push((r.key.to_vec(), r.value.map(<[u8]>::to_vec)));
                pos += r.len;
            }
            _ => return Err(invalid("damaged record in data block")),
        }
    }
    Ok(out)
}

pub struct SsTable {
    file: File,
    path: PathBuf,
    index: Vec<IndexEntry>,
    bloom: Option<Bloom>,
    min_key: Vec<u8>,
    max_key: Vec<u8>,
    entries: u64,
    file_bytes: u64,
    counters: Rc<IoCounters>,
}

impl SsTable {
    pub fn open(path: &Path, counters: Rc<IoCounters>) -> io::Result<SsTable> {
        let file = File::open(path)?;
        let file_bytes = file.metadata()?.len();
        if file_bytes < FOOTER_LEN as u64 {
            return Err(invalid("file shorter than a footer"));
        }
        let mut footer = [0u8; FOOTER_LEN];
        file.read_exact_at(&mut footer, file_bytes - FOOTER_LEN as u64)?;
        let footer = checked_payload(&footer, "footer")?;
        let mut c = Cursor { buf: footer, pos: 0 };
        let (index_off, index_len, bloom_off, bloom_len, entries, magic) =
            (c.u64()?, c.u64()?, c.u64()?, c.u64()?, c.u64()?, c.u64()?);
        if magic != MAGIC {
            return Err(invalid("bad magic"));
        }
        let data_end = file_bytes - FOOTER_LEN as u64;
        let sections_ok = index_off.checked_add(index_len) == Some(bloom_off)
            && bloom_off.checked_add(bloom_len) == Some(data_end);
        if !sections_ok {
            return Err(invalid("footer offsets are inconsistent"));
        }

        let mut raw = vec![0u8; index_len as usize];
        file.read_exact_at(&mut raw, index_off)?;
        let payload = checked_payload(&raw, "index")?;
        let mut c = Cursor { buf: payload, pos: 0 };
        let count = c.u32()? as usize;
        let mut index = Vec::with_capacity(count);
        for _ in 0..count {
            let klen = c.u32()? as usize;
            let first_key = c.take(klen)?.to_vec();
            index.push(IndexEntry { first_key, offset: c.u64()?, len: c.u32()? });
        }
        let min_len = c.u32()? as usize;
        let min_key = c.take(min_len)?.to_vec();
        let max_len = c.u32()? as usize;
        let max_key = c.take(max_len)?.to_vec();

        let bloom = if bloom_len == 0 {
            None
        } else {
            let mut raw = vec![0u8; bloom_len as usize];
            file.read_exact_at(&mut raw, bloom_off)?;
            let payload = checked_payload(&raw, "bloom filter")?;
            Some(Bloom::from_bytes(payload).ok_or_else(|| invalid("bad bloom filter"))?)
        };
        Ok(SsTable { file, path: path.to_path_buf(), index, bloom, min_key, max_key, entries, file_bytes, counters })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_bytes(&self) -> u64 {
        self.file_bytes
    }

    pub fn entries(&self) -> u64 {
        self.entries
    }

    pub fn block_count(&self) -> usize {
        self.index.len()
    }

    pub fn has_bloom(&self) -> bool {
        self.bloom.is_some()
    }

    pub fn min_key(&self) -> &[u8] {
        &self.min_key
    }

    pub fn max_key(&self) -> &[u8] {
        &self.max_key
    }

    fn read_block(&self, e: &IndexEntry) -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; e.len as usize];
        self.file.read_exact_at(&mut buf, e.offset)?;
        bump(&self.counters.block_reads);
        Ok(buf)
    }

    /// Point lookup: range check, filter probe, index search, one block read.
    pub fn get(&self, key: &[u8]) -> io::Result<Lookup> {
        if self.entries == 0 || key < self.min_key.as_slice() || key > self.max_key.as_slice() {
            bump(&self.counters.range_skips);
            return Ok(Lookup::Absent);
        }
        if let Some(bloom) = &self.bloom
            && !bloom.may_contain(key)
        {
            bump(&self.counters.bloom_negatives);
            return Ok(Lookup::Absent);
        }
        let n = self.index.partition_point(|e| e.first_key.as_slice() <= key);
        let entry = &self.index[n - 1]; // key >= min_key == first block's first key, so n >= 1
        let block = self.read_block(entry)?;
        let mut pos = 0;
        while pos < block.len() {
            let Parse::Record(r) = record::parse(&block[pos..]) else {
                return Err(invalid("damaged record in data block"));
            };
            match r.key.cmp(key) {
                std::cmp::Ordering::Equal => {
                    return Ok(r.value.map_or(Lookup::Deleted, |v| Lookup::Found(v.to_vec())));
                }
                std::cmp::Ordering::Greater => break,
                std::cmp::Ordering::Less => pos += r.len,
            }
        }
        if self.bloom.is_some() {
            bump(&self.counters.bloom_false_positives);
        }
        Ok(Lookup::Absent)
    }

    /// All entries in key order, reading one block at a time.
    pub fn iter(&self) -> Iter<'_> {
        self.iter_from(&[])
    }

    /// Entries with key >= `from`.
    pub fn iter_from(&self, from: &[u8]) -> Iter<'_> {
        let first = self.index.partition_point(|e| e.first_key.as_slice() <= from).saturating_sub(1);
        Iter { table: self, next_block: first, pending: Vec::new().into_iter(), from: from.to_vec() }
    }
}

pub struct Iter<'a> {
    table: &'a SsTable,
    next_block: usize,
    pending: std::vec::IntoIter<Entry>,
    from: Vec<u8>,
}

impl Iterator for Iter<'_> {
    type Item = io::Result<Entry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(e) = self.pending.next() {
                return Some(Ok(e));
            }
            let entry = self.table.index.get(self.next_block)?;
            self.next_block += 1;
            let entries = match self.table.read_block(entry).and_then(|b| parse_block(&b)) {
                Ok(v) => v,
                Err(e) => {
                    self.next_block = usize::MAX; // stop after reporting the error once
                    return Some(Err(e));
                }
            };
            let from = &self.from;
            self.pending = entries.into_iter().filter(|(k, _)| k >= from).collect::<Vec<_>>().into_iter();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::TempDir;
    use std::fs::OpenOptions;

    fn key(i: u32) -> Vec<u8> {
        format!("key{i:06}").into_bytes()
    }

    /// Keys 0, 2, 4, ... (odd numbers are the "absent" keys inside the table's range); every 7th
    /// entry is a tombstone.
    fn build(dir: &Path, n: u32, block_bytes: usize, bits: usize) -> (SsTable, Rc<IoCounters>) {
        let path = dir.join("t.sst");
        let mut b = Builder::create(&path, block_bytes, bits).unwrap();
        for i in 0..n {
            let k = key(2 * i);
            if i % 7 == 3 {
                b.add(&k, None).unwrap();
            } else {
                b.add(&k, Some(format!("value-{i}").as_bytes())).unwrap();
            }
        }
        let info = b.finish().unwrap();
        assert_eq!(info.entries, u64::from(n));
        assert_eq!(info.file_bytes, std::fs::metadata(&path).unwrap().len());
        let counters = Rc::new(IoCounters::default());
        (SsTable::open(&path, counters.clone()).unwrap(), counters)
    }

    #[test]
    fn every_key_is_found_deleted_or_absent_as_built() {
        let t = TempDir::new("sst-get").unwrap();
        let (table, _) = build(t.path(), 1000, 256, 10);
        assert!(table.block_count() > 50, "small blocks give many index entries");
        for i in 0..1000u32 {
            let want = if i % 7 == 3 { Lookup::Deleted } else { Lookup::Found(format!("value-{i}").into_bytes()) };
            assert_eq!(table.get(&key(2 * i)).unwrap(), want, "key {i}");
            assert_eq!(table.get(&key(2 * i + 1)).unwrap(), Lookup::Absent, "absent key {i}");
        }
        assert_eq!(table.get(b"aaa").unwrap(), Lookup::Absent);
        assert_eq!(table.get(b"zzz").unwrap(), Lookup::Absent);
    }

    #[test]
    fn a_lookup_reads_at_most_one_block_and_the_filter_avoids_most_misses() {
        let t = TempDir::new("sst-io").unwrap();
        let (table, c) = build(t.path(), 2000, 512, 10);
        for i in 0..2000u32 {
            table.get(&key(2 * i)).unwrap();
        }
        assert_eq!(c.block_reads.get(), 2000, "a hit costs exactly one block read");

        let before = c.block_reads.get();
        let misses = 1999u64; // odd keys 1, 3, ..., 3997 lie inside [min, max]; 3999 would be a range skip
        for i in 0..1999u32 {
            assert_eq!(table.get(&key(2 * i + 1)).unwrap(), Lookup::Absent);
        }
        let reads = c.block_reads.get() - before;
        assert_eq!(reads, c.bloom_false_positives.get());
        assert_eq!(c.bloom_negatives.get() + reads, misses);
        assert!(reads < misses / 20, "the filter should stop >95% of misses: {reads} of {misses} read a block");

        // Without a filter every miss inside [min, max] reads a block.
        let t2 = TempDir::new("sst-io2").unwrap();
        let (plain, c2) = build(t2.path(), 2000, 512, 0);
        assert!(!plain.has_bloom());
        for i in 0..1999u32 {
            plain.get(&key(2 * i + 1)).unwrap();
        }
        assert_eq!(c2.block_reads.get(), misses);
        assert_eq!(c2.bloom_negatives.get(), 0);
    }

    #[test]
    fn keys_outside_the_range_never_touch_the_disk() {
        let t = TempDir::new("sst-range").unwrap();
        let (table, c) = build(t.path(), 100, 256, 10);
        assert_eq!(table.get(b"a").unwrap(), Lookup::Absent);
        assert_eq!(table.get(b"zzzz").unwrap(), Lookup::Absent);
        assert_eq!((c.range_skips.get(), c.block_reads.get(), c.bloom_negatives.get()), (2, 0, 0));
        assert_eq!(table.min_key(), key(0).as_slice());
        assert_eq!(table.max_key(), key(198).as_slice());
    }

    #[test]
    fn iteration_returns_everything_in_order_and_iter_from_seeks() {
        let t = TempDir::new("sst-iter").unwrap();
        let (table, c) = build(t.path(), 500, 300, 10);
        let all: Vec<Entry> = table.iter().map(Result::unwrap).collect();
        assert_eq!(all.len(), 500);
        assert!(all.windows(2).all(|w| w[0].0 < w[1].0));
        assert_eq!(all[3].1, None, "entry 3 is a tombstone");

        let before = c.block_reads.get();
        let from: Vec<Entry> = table.iter_from(&key(601)).map(Result::unwrap).collect();
        assert_eq!(from.first().unwrap().0, key(602));
        assert_eq!(from.len(), 500 - 301);
        assert!(c.block_reads.get() - before < table.block_count() as u64, "seeking skips the leading blocks");
        assert!(table.iter_from(b"zzz").next().is_none());
        assert_eq!(table.iter_from(b"").count(), 500);
    }

    #[test]
    fn the_builder_rejects_unsorted_and_duplicate_keys() {
        let t = TempDir::new("sst-order").unwrap();
        let mut b = Builder::create(&t.path().join("x.sst"), 100, 10).unwrap();
        b.add(b"b", Some(b"1")).unwrap();
        assert!(b.add(b"a", Some(b"2")).is_err());
        assert!(b.add(b"b", Some(b"3")).is_err());
        b.add(b"c", None).unwrap();
    }

    #[test]
    fn an_empty_table_is_valid() {
        let t = TempDir::new("sst-empty").unwrap();
        let path = t.path().join("e.sst");
        Builder::create(&path, 100, 10).unwrap().finish().unwrap();
        let table = SsTable::open(&path, Rc::default()).unwrap();
        assert_eq!(table.entries(), 0);
        assert_eq!(table.get(b"k").unwrap(), Lookup::Absent);
        assert_eq!(table.iter().count(), 0);
    }

    fn flip(path: &Path, offset: u64) {
        let f = OpenOptions::new().read(true).write(true).open(path).unwrap();
        let mut b = [0u8];
        f.read_exact_at(&mut b, offset).unwrap();
        f.write_all_at(&[b[0] ^ 0x40], offset).unwrap();
    }

    #[test]
    fn damage_is_detected_where_it_is_read() {
        let t = TempDir::new("sst-damage").unwrap();
        let (table, _) = build(t.path(), 200, 256, 10);
        let path = t.path().join("t.sst");
        let len = table.file_bytes();
        drop(table);

        // A byte inside the first data block: open still works (the index is fine), but reads of that
        // block fail instead of returning wrong data.
        flip(&path, 20);
        let table = SsTable::open(&path, Rc::default()).unwrap();
        assert!(table.get(&key(0)).is_err());
        assert!(table.iter().any(|e| e.is_err()));
        drop(table);
        flip(&path, 20);

        // The index section, the Bloom filter and the footer are covered by their own checksums.
        for target in [len - FOOTER_LEN as u64 - 5, len - 3, len - FOOTER_LEN as u64 + 2] {
            flip(&path, target);
            assert!(SsTable::open(&path, Rc::default()).is_err(), "damage at {target} must be rejected at open");
            flip(&path, target);
        }
        assert!(SsTable::open(&path, Rc::default()).is_ok(), "restored file opens again");

        // A truncated file has no footer.
        let f = OpenOptions::new().write(true).open(&path).unwrap();
        f.set_len(len - 1).unwrap();
        assert!(SsTable::open(&path, Rc::default()).is_err());
        f.set_len(10).unwrap();
        assert!(SsTable::open(&path, Rc::default()).is_err());
    }
}
