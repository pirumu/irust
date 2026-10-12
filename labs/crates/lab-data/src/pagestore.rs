//! A tiny in-place-update store, to put numbers next to the LSM-tree: sorted, fixed-size *pages*
//! updated where they are, with a write-ahead log for durability.
//!
//! * Keys live in sorted pages of `page_bytes` bytes; a one-level directory (lower bound -> page)
//!   finds the page for a key. A full page splits in two. (A real B-tree has more levels; one level
//!   is enough to count page writes.)
//! * `put` appends a redo record to the WAL and changes the page in memory.
//! * A *checkpoint* writes every dirty page to its place in `pages.db`, then empties the WAL.
//! * Recovery loads the pages and replays the WAL. A page whose checksum fails (a write that was
//!   interrupted half way through a page) is reported: logical WAL records cannot repair a page that
//!   is half old and half new, which is what PostgreSQL's full-page writes exist for. This store has no
//!   such protection.
//!
//! Everything is cached in memory, so reads cost no I/O; the point of the module is the *write*
//! side: `Stats::write_amplification` = (WAL bytes + page bytes) / user bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::ops::Bound;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use crate::crc32c::crc32c;
use crate::record::{self, Recovery};

const PAGE_HEADER: usize = 4 + 2 + 2; // crc32c, entry count, lower-bound length
const ENTRY_OVERHEAD: usize = 4; // key length and value length, u16 each
pub const MAX_KEY_LEN: usize = 255;

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub page_bytes: usize,
    /// Checkpoint after this many puts; 0 means only on request.
    pub checkpoint_every: usize,
    /// `fsync` the WAL on every put.
    pub sync_wal: bool,
    /// `false` skips the `fsync` calls of checkpoints (tests and experiments).
    pub durable: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options { page_bytes: 4096, checkpoint_every: 0, sync_wal: false, durable: true }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub user_bytes: u64,
    pub wal_bytes: u64,
    pub page_bytes: u64,
    pub page_writes: u64,
    pub checkpoints: u64,
    pub splits: u64,
    pub pages: usize,
}

impl Stats {
    pub fn write_amplification(&self) -> f64 {
        if self.user_bytes == 0 { 0.0 } else { (self.wal_bytes + self.page_bytes) as f64 / self.user_bytes as f64 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct OpenReport {
    pub pages: usize,
    pub wal_records: u64,
    pub wal_truncated_bytes: u64,
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

struct Page {
    lower_bound: Vec<u8>,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Page {
    fn used(&self) -> usize {
        PAGE_HEADER
            + self.lower_bound.len()
            + self.entries.iter().map(|(k, v)| ENTRY_OVERHEAD + k.len() + v.len()).sum::<usize>()
    }

    fn encode(&self, page_bytes: usize) -> Vec<u8> {
        let mut buf = vec![0u8; page_bytes];
        buf[4..6].copy_from_slice(&(self.entries.len() as u16).to_le_bytes());
        buf[6..8].copy_from_slice(&(self.lower_bound.len() as u16).to_le_bytes());
        let mut pos = PAGE_HEADER;
        buf[pos..pos + self.lower_bound.len()].copy_from_slice(&self.lower_bound);
        pos += self.lower_bound.len();
        for (k, v) in &self.entries {
            buf[pos..pos + 2].copy_from_slice(&(k.len() as u16).to_le_bytes());
            buf[pos + 2..pos + 4].copy_from_slice(&(v.len() as u16).to_le_bytes());
            pos += ENTRY_OVERHEAD;
            buf[pos..pos + k.len()].copy_from_slice(k);
            pos += k.len();
            buf[pos..pos + v.len()].copy_from_slice(v);
            pos += v.len();
        }
        let crc = crc32c(&buf[4..]);
        buf[0..4].copy_from_slice(&crc.to_le_bytes());
        buf
    }

    fn decode(buf: &[u8]) -> io::Result<Page> {
        let stored = u32::from_le_bytes(buf[0..4].try_into().expect("4 bytes"));
        if crc32c(&buf[4..]) != stored {
            return Err(invalid("page checksum mismatch"));
        }
        let n = u16::from_le_bytes(buf[4..6].try_into().expect("2 bytes")) as usize;
        let lb = u16::from_le_bytes(buf[6..8].try_into().expect("2 bytes")) as usize;
        let take = |pos: &mut usize, len: usize| -> io::Result<Vec<u8>> {
            let end = pos.checked_add(len).filter(|&e| e <= buf.len()).ok_or_else(|| invalid("page overruns"))?;
            let out = buf[*pos..end].to_vec();
            *pos = end;
            Ok(out)
        };
        let mut pos = PAGE_HEADER;
        let lower_bound = take(&mut pos, lb)?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let header = take(&mut pos, ENTRY_OVERHEAD)?;
            let klen = u16::from_le_bytes([header[0], header[1]]) as usize;
            let vlen = u16::from_le_bytes([header[2], header[3]]) as usize;
            let k = take(&mut pos, klen)?;
            let v = take(&mut pos, vlen)?;
            entries.push((k, v));
        }
        Ok(Page { lower_bound, entries })
    }
}

pub struct PageStore {
    dir: PathBuf,
    opts: Options,
    file: File,
    wal: File,
    pages: Vec<Page>,
    /// Lower bound of each page -> page id.
    directory: BTreeMap<Vec<u8>, usize>,
    dirty: BTreeSet<usize>,
    puts_since_checkpoint: usize,
    stats: Stats,
}

impl PageStore {
    pub fn open(dir: impl AsRef<Path>, opts: Options) -> io::Result<(PageStore, OpenReport)> {
        assert!(opts.page_bytes >= 512 && opts.page_bytes <= 1 << 16, "page size out of range");
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("pages.db"))?;
        let len = file.metadata()?.len();
        if len % opts.page_bytes as u64 != 0 {
            return Err(invalid("pages.db is not a whole number of pages"));
        }
        let mut pages = Vec::new();
        for id in 0..len / opts.page_bytes as u64 {
            let mut buf = vec![0u8; opts.page_bytes];
            file.read_exact_at(&mut buf, id * opts.page_bytes as u64)?;
            pages.push(Page::decode(&buf).map_err(|e| invalid(format!("page {id}: {e} (torn page write?)")))?);
        }
        let mut dirty = BTreeSet::new();
        if pages.is_empty() {
            pages.push(Page { lower_bound: Vec::new(), entries: Vec::new() });
            dirty.insert(0);
        }
        let directory = pages.iter().enumerate().map(|(id, p)| (p.lower_bound.clone(), id)).collect();

        let wal_path = dir.join("pages.wal");
        let bytes = match fs::read(&wal_path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let wal = OpenOptions::new().create(true).append(true).open(&wal_path)?;
        let mut store = PageStore {
            dir,
            opts,
            file,
            wal,
            pages,
            directory,
            dirty,
            puts_since_checkpoint: 0,
            stats: Stats::default(),
        };
        let mut replayed = Vec::new();
        let scan = record::scan(&bytes, |_, rec| {
            if let Some(v) = rec.value {
                replayed.push((rec.key.to_vec(), v.to_vec()));
            }
        });
        let cut = record::bytes_to_cut(Recovery::TolerateTail, &scan, bytes.len() as u64)
            .map_err(|c| invalid(format!("write-ahead log: {c}")))?;
        if cut > 0 {
            store.wal.set_len(scan.valid_len as u64)?;
            store.wal.sync_all()?;
        }
        for (k, v) in replayed {
            store.insert(&k, &v);
        }
        store.stats.splits = 0; // splits during replay are not part of this session's statistics
        let report = OpenReport { pages: store.pages.len(), wal_records: scan.records, wal_truncated_bytes: cut };
        store.stats.pages = store.pages.len();
        Ok((store, report))
    }

    fn page_for(&self, key: &[u8]) -> usize {
        let (_, &id) = self
            .directory
            .range::<[u8], _>((Bound::Unbounded, Bound::Included(key)))
            .next_back()
            .expect("the first page has the empty lower bound");
        id
    }

    pub fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let page = &self.pages[self.page_for(key)];
        page.entries.binary_search_by(|(k, _)| k.as_slice().cmp(key)).ok().map(|i| page.entries[i].1.as_slice())
    }

    /// Insert or replace in memory; split the page if it no longer fits. No logging.
    fn insert(&mut self, key: &[u8], value: &[u8]) {
        let id = self.page_for(key);
        let page = &mut self.pages[id];
        let mut appended = false;
        match page.entries.binary_search_by(|(k, _)| k.as_slice().cmp(key)) {
            Ok(i) => page.entries[i].1 = value.to_vec(),
            Err(i) => {
                appended = i == page.entries.len();
                page.entries.insert(i, (key.to_vec(), value.to_vec()));
            }
        }
        self.dirty.insert(id);
        if self.pages[id].used() > self.opts.page_bytes {
            // A new largest key landing in the last page means keys arrive in ascending order: split
            // off only that key, so the full page stays full instead of ending up half empty.
            let last_page = self.directory.range::<[u8], _>((Bound::Excluded(self.pages[id].lower_bound.as_slice()), Bound::Unbounded)).next().is_none();
            self.split(id, appended && last_page);
        }
    }

    fn split(&mut self, id: usize, ascending_append: bool) {
        let page = &mut self.pages[id];
        let mut mid = page.entries.len() - 1;
        if !ascending_append {
            let total = page.used() - PAGE_HEADER - page.lower_bound.len();
            let mut acc = 0;
            for (i, (k, v)) in page.entries.iter().enumerate() {
                acc += ENTRY_OVERHEAD + k.len() + v.len();
                if acc * 2 >= total {
                    mid = i.clamp(1, page.entries.len() - 1);
                    break;
                }
            }
        }
        let upper = page.entries.split_off(mid);
        let new_page = Page { lower_bound: upper[0].0.clone(), entries: upper };
        let new_id = self.pages.len();
        self.directory.insert(new_page.lower_bound.clone(), new_id);
        self.pages.push(new_page);
        self.dirty.insert(new_id);
        self.stats.splits += 1;
        self.stats.pages = self.pages.len();
    }

    pub fn put(&mut self, key: &[u8], value: &[u8]) -> io::Result<()> {
        let entry = ENTRY_OVERHEAD + key.len() + value.len();
        if key.len() > MAX_KEY_LEN || entry > self.opts.page_bytes / 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "entry too large for the page size"));
        }
        let mut buf = Vec::with_capacity(record::encoded_len(key.len(), Some(value.len())));
        record::encode_into(&mut buf, key, Some(value));
        self.wal.write_all(&buf)?;
        if self.opts.sync_wal {
            self.wal.sync_data()?;
        }
        self.stats.wal_bytes += buf.len() as u64;
        self.stats.user_bytes += (key.len() + value.len()) as u64;
        self.insert(key, value);
        self.puts_since_checkpoint += 1;
        if self.opts.checkpoint_every > 0 && self.puts_since_checkpoint >= self.opts.checkpoint_every {
            self.checkpoint()?;
        }
        Ok(())
    }

    /// Writes every dirty page in place, then empties the WAL.
    pub fn checkpoint(&mut self) -> io::Result<()> {
        for &id in &self.dirty {
            let buf = self.pages[id].encode(self.opts.page_bytes);
            self.file.write_all_at(&buf, id as u64 * self.opts.page_bytes as u64)?;
            self.stats.page_writes += 1;
            self.stats.page_bytes += self.opts.page_bytes as u64;
        }
        if self.opts.durable {
            self.file.sync_data()?;
        }
        self.wal.set_len(0)?;
        if self.opts.durable {
            self.wal.sync_data()?;
        }
        self.dirty.clear();
        self.puts_since_checkpoint = 0;
        self.stats.checkpoints += 1;
        Ok(())
    }

    pub fn stats(&self) -> Stats {
        Stats { pages: self.pages.len(), ..self.stats }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Live entries in key order.
    pub fn entries(&self) -> Vec<(&[u8], &[u8])> {
        self.directory
            .values()
            .flat_map(|&id| self.pages[id].entries.iter().map(|(k, v)| (k.as_slice(), v.as_slice())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;
    use crate::util::TempDir;

    fn opts(checkpoint_every: usize) -> Options {
        Options { checkpoint_every, durable: false, ..Options::default() }
    }

    fn key(i: u64) -> Vec<u8> {
        format!("user{i:08}").into_bytes()
    }

    fn check_structure(store: &PageStore) {
        let bounds: Vec<&Vec<u8>> = store.directory.keys().collect();
        assert_eq!(bounds[0], &Vec::<u8>::new(), "the first page covers everything below the second");
        let mut prev: Option<Vec<u8>> = None;
        for (i, (lb, &id)) in store.directory.iter().enumerate() {
            let page = &store.pages[id];
            assert_eq!(&page.lower_bound, lb);
            assert!(page.used() <= store.opts.page_bytes, "page {id} overfull");
            let upper = bounds.get(i + 1).map(|b| b.as_slice());
            for (k, _) in &page.entries {
                assert!(k >= lb && upper.is_none_or(|u| k.as_slice() < u), "key outside its page's range");
                if let Some(p) = &prev {
                    assert!(k > p, "keys are globally sorted");
                }
                prev = Some(k.clone());
            }
        }
    }

    #[test]
    fn random_puts_match_a_model_and_keep_pages_sorted_and_within_their_range() {
        let t = TempDir::new("pages-model").unwrap();
        let (mut store, _) = PageStore::open(t.path(), opts(300)).unwrap();
        let mut model = BTreeMap::new();
        let mut rng = Rng::new(21);
        for step in 0..5000 {
            let k = key(rng.below(2000));
            let mut v = vec![0u8; 20 + rng.index(80)];
            rng.fill(&mut v);
            store.put(&k, &v).unwrap();
            model.insert(k, v);
            if step % 997 == 0 {
                check_structure(&store);
            }
        }
        check_structure(&store);
        assert!(store.stats().splits > 10);
        for (k, v) in &model {
            assert_eq!(store.get(k), Some(v.as_slice()));
        }
        assert_eq!(store.get(b"absent"), None);
        let all: Vec<_> = store.entries().into_iter().map(|(k, v)| (k.to_vec(), v.to_vec())).collect();
        assert_eq!(all, model.into_iter().collect::<Vec<_>>());
    }

    #[test]
    fn the_wal_makes_uncheckpointed_puts_survive_a_crash() {
        let t = TempDir::new("pages-wal").unwrap();
        {
            let (mut store, _) = PageStore::open(t.path(), opts(0)).unwrap();
            for i in 0..300 {
                store.put(&key(i), b"first").unwrap();
            }
            store.checkpoint().unwrap();
            for i in 200..500 {
                store.put(&key(i), b"second").unwrap(); // only in the WAL
            }
            // dropped without a checkpoint
        }
        let (store, report) = PageStore::open(t.path(), opts(0)).unwrap();
        assert_eq!(report.wal_records, 300);
        assert_eq!(store.get(&key(10)), Some(&b"first"[..]));
        assert_eq!(store.get(&key(250)), Some(&b"second"[..]));
        assert_eq!(store.get(&key(499)), Some(&b"second"[..]));
        check_structure(&store);
    }

    #[test]
    fn a_torn_wal_tail_is_cut_like_in_the_other_engines() {
        let t = TempDir::new("pages-torn-wal").unwrap();
        {
            let (mut store, _) = PageStore::open(t.path(), opts(0)).unwrap();
            for i in 0..10 {
                store.put(&key(i), b"v").unwrap();
            }
        }
        let wal = t.path().join("pages.wal");
        let len = fs::metadata(&wal).unwrap().len();
        OpenOptions::new().write(true).open(&wal).unwrap().set_len(len - 4).unwrap();
        let (store, report) = PageStore::open(t.path(), opts(0)).unwrap();
        assert_eq!(report.wal_records, 9);
        assert!(report.wal_truncated_bytes > 0);
        assert_eq!(store.get(&key(8)), Some(&b"v"[..]));
        assert_eq!(store.get(&key(9)), None);
    }

    #[test]
    fn an_interrupted_in_place_page_write_cannot_be_repaired_from_the_wal() {
        let t = TempDir::new("pages-torn-page").unwrap();
        {
            let (mut store, _) = PageStore::open(t.path(), opts(0)).unwrap();
            for i in 0..200 {
                store.put(&key(i), &[b'x'; 60]).unwrap();
            }
            store.checkpoint().unwrap();
        }
        // A torn write leaves a page that is partly new and partly old. Simulate it by replacing one
        // 512-byte sector inside page 1; the page checksum notices, and the WAL (emptied by the
        // checkpoint) holds nothing to rebuild the page from.
        let f = OpenOptions::new().write(true).open(t.path().join("pages.db")).unwrap();
        f.write_all_at(&[0xAB; 512], 4096 + 100).unwrap(); // a sector inside page 1 differs: part new, part old
        let err = PageStore::open(t.path(), opts(0)).err().expect("open must fail");
        assert!(err.to_string().contains("page 1") && err.to_string().contains("torn"), "{err}");
    }

    fn write_amplification(keys: impl Fn(u64, &mut Rng) -> u64, checkpoint_every: usize) -> Stats {
        let t = TempDir::new("pages-wa").unwrap();
        let (mut store, _) = PageStore::open(t.path(), opts(checkpoint_every)).unwrap();
        let mut rng = Rng::new(3);
        for i in 0..20_000 {
            let k = keys(i, &mut rng);
            store.put(&key(k), &[b'v'; 100]).unwrap();
        }
        store.checkpoint().unwrap();
        store.stats()
    }

    #[test]
    fn page_granularity_makes_random_writes_far_more_expensive_than_sequential_ones() {
        let seq = write_amplification(|i, _| i, 1000);
        let rnd = write_amplification(|_, r| r.below(20_000), 1000);
        assert!(seq.write_amplification() < 3.0, "{seq:?}");
        assert!(rnd.write_amplification() > 3.0 * seq.write_amplification(), "seq {seq:?} rnd {rnd:?}");
        assert_eq!(seq.user_bytes, rnd.user_bytes);
    }

    #[test]
    fn rarer_checkpoints_coalesce_page_writes() {
        let often = write_amplification(|_, r| r.below(20_000), 200);
        let rarely = write_amplification(|_, r| r.below(20_000), 5000);
        assert!(rarely.page_bytes * 2 < often.page_bytes, "often {often:?} rarely {rarely:?}");
        assert_eq!(often.wal_bytes, rarely.wal_bytes, "the WAL cost does not depend on the checkpoint interval");
    }
}
