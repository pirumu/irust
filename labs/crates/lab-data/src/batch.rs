//! Lab (f): batch processing by hand, on files.
//!
//! * [`external_sort`]: sort more lines than fit in memory with sorted runs on disk and a k-way merge.
//! * [`word_count`]: the MapReduce shape: map, partition into R regions, shuffle, sort, reduce; with
//!   and without a combiner.
//! * [`sort_merge_join`]: a reduce-side join. Records of both inputs are tagged and keyed by the join
//!   key, partitioned, sorted so that the (small) user record of a key comes first, and joined in one
//!   pass in which the reducer buffers a single record.
//! * [`hash_join`]: the map-side alternative that holds the small input in memory.
//!
//! Single process, sequential "tasks": the point is the data flow and what it costs in records and bytes,
//! not parallelism.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::bloom::fnv1a;
use crate::rng::Rng;

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn lines_of(path: &Path) -> io::Result<impl Iterator<Item = io::Result<String>>> {
    Ok(BufReader::new(File::open(path)?).lines())
}

/// Partition function `hash(key) mod R`. FNV-1a has no seed, so the same key lands in the same
/// partition on every run and every machine; the standard library's `DefaultHasher` makes no such promise.
pub fn partition(key: &str, regions: usize) -> usize {
    (fnv1a(key.as_bytes()) % regions as u64) as usize
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SortStats {
    pub records: u64,
    /// Sorted runs written in the first phase.
    pub runs: usize,
    /// Rounds of merging, the last one writing the output.
    pub merge_passes: usize,
    /// Bytes written to temporary files (runs and intermediate merges), not counting the output.
    pub bytes_spilled: u64,
}

fn write_lines(path: &Path, lines: &[String]) -> io::Result<u64> {
    let mut w = BufWriter::new(File::create(path)?);
    let mut bytes = 0;
    for l in lines {
        w.write_all(l.as_bytes())?;
        w.write_all(b"\n")?;
        bytes += l.len() as u64 + 1;
    }
    w.flush()?;
    Ok(bytes)
}

/// K-way merge of sorted line files into `out`; returns (records, bytes).
fn merge_files(inputs: &[PathBuf], out: &Path) -> io::Result<(u64, u64)> {
    let mut readers: Vec<_> = inputs.iter().map(|p| lines_of(p)).collect::<io::Result<_>>()?;
    let mut heap = BinaryHeap::new();
    for (i, r) in readers.iter_mut().enumerate() {
        if let Some(line) = r.next() {
            heap.push(Reverse((line?, i)));
        }
    }
    let mut w = BufWriter::new(File::create(out)?);
    let (mut records, mut bytes) = (0, 0);
    while let Some(Reverse((line, i))) = heap.pop() {
        w.write_all(line.as_bytes())?;
        w.write_all(b"\n")?;
        records += 1;
        bytes += line.len() as u64 + 1;
        if let Some(next) = readers[i].next() {
            heap.push(Reverse((next?, i)));
        }
    }
    w.flush()?;
    Ok((records, bytes))
}

/// Sorts the lines of `input` into `output` while holding at most `mem_records` lines in memory
/// (plus one line per open run during a merge). At most `fan_in` runs are merged at once; more runs
/// are merged in several rounds, as GNU `sort` does.
pub fn external_sort(input: &Path, output: &Path, tmp: &Path, mem_records: usize, fan_in: usize) -> io::Result<SortStats> {
    assert!(mem_records >= 1 && fan_in >= 2);
    fs::create_dir_all(tmp)?;
    let mut stats = SortStats::default();
    let mut runs: Vec<PathBuf> = Vec::new();
    let mut counter = 0;
    let mut new_path = |kind: &str| {
        counter += 1;
        tmp.join(format!("{kind}-{counter:05}"))
    };

    let mut chunk: Vec<String> = Vec::with_capacity(mem_records.min(1 << 20));
    let mut flush_chunk = |chunk: &mut Vec<String>, runs: &mut Vec<PathBuf>, stats: &mut SortStats| -> io::Result<()> {
        if chunk.is_empty() {
            return Ok(());
        }
        chunk.sort_unstable();
        let path = new_path("run");
        stats.bytes_spilled += write_lines(&path, chunk)?;
        stats.records += chunk.len() as u64;
        runs.push(path);
        chunk.clear();
        Ok(())
    };
    for line in lines_of(input)? {
        chunk.push(line?);
        if chunk.len() == mem_records {
            flush_chunk(&mut chunk, &mut runs, &mut stats)?;
        }
    }
    flush_chunk(&mut chunk, &mut runs, &mut stats)?;
    stats.runs = runs.len();

    let mut merge_counter = 0;
    while runs.len() > fan_in {
        let mut next = Vec::new();
        for group in runs.chunks(fan_in) {
            if group.len() == 1 {
                next.push(group[0].clone());
                continue;
            }
            merge_counter += 1;
            let out = tmp.join(format!("merge-{merge_counter:05}"));
            let (_, bytes) = merge_files(group, &out)?;
            stats.bytes_spilled += bytes;
            for p in group {
                fs::remove_file(p)?;
            }
            next.push(out);
        }
        runs = next;
        stats.merge_passes += 1;
    }
    if runs.is_empty() {
        File::create(output)?;
    } else {
        merge_files(&runs, output)?;
        stats.merge_passes += 1;
        for p in &runs {
            fs::remove_file(p)?;
        }
    }
    Ok(stats)
}

/// Words of a line: ASCII letters and digits, lowercased.
pub fn words(line: &str) -> impl Iterator<Item = String> + '_ {
    line.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_ascii_lowercase)
}

/// `lines` lines of `words_per_line` words drawn from `vocab` words (`w00000` ...) with Zipf weights
/// `1 / (rank + 1)^s`: a few words are very frequent, most are rare.
pub fn generate_text(path: &Path, lines: usize, words_per_line: usize, vocab: usize, s: f64, seed: u64) -> io::Result<u64> {
    let mut cumulative = Vec::with_capacity(vocab);
    let mut total = 0.0;
    for rank in 0..vocab {
        total += 1.0 / ((rank + 1) as f64).powf(s);
        cumulative.push(total);
    }
    let mut rng = Rng::new(seed);
    let mut w = BufWriter::new(File::create(path)?);
    let mut bytes = 0;
    for _ in 0..lines {
        let mut line = String::new();
        for i in 0..words_per_line {
            let x = rng.f64() * total;
            let rank = cumulative.partition_point(|&c| c < x).min(vocab - 1);
            if i > 0 {
                line.push(' ');
            }
            line.push_str(&format!("w{rank:05}"));
        }
        line.push('\n');
        w.write_all(line.as_bytes())?;
        bytes += line.len() as u64;
    }
    w.flush()?;
    Ok(bytes)
}

/// Word counts computed in memory, the reference for [`word_count`].
pub fn baseline_word_count(input: &Path) -> io::Result<BTreeMap<String, u64>> {
    let mut counts = BTreeMap::new();
    for line in lines_of(input)? {
        for w in words(&line?) {
            *counts.entry(w).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

#[derive(Debug, Clone, Default)]
pub struct WordCountStats {
    pub input_lines: u64,
    /// Words seen by the map tasks.
    pub words: u64,
    /// Records the map tasks wrote for the shuffle (equal to `words` without a combiner).
    pub shuffled_records: u64,
    pub shuffle_bytes: u64,
    /// Records per reduce partition.
    pub partition_records: Vec<u64>,
    pub distinct_words: u64,
    pub sort: SortStats,
}

/// `word\tcount` lines of the reduce outputs, as a map.
pub fn read_counts(files: &[PathBuf]) -> io::Result<BTreeMap<String, u64>> {
    let mut counts = BTreeMap::new();
    for f in files {
        for line in lines_of(f)? {
            let line = line?;
            let (w, c) = line.split_once('\t').ok_or_else(|| invalid("record without a tab"))?;
            let c: u64 = c.parse().map_err(|_| invalid("bad count"))?;
            if counts.insert(w.to_string(), c).is_some() {
                return Err(invalid(format!("word {w:?} appears in two reduce outputs")));
            }
        }
    }
    Ok(counts)
}

/// MapReduce word count over `input`, split into `splits` map tasks and `reducers` reduce tasks.
/// Returns the statistics and one output file per reducer (each sorted by word; a word is in exactly one).
pub fn word_count(
    input: &Path,
    work: &Path,
    splits: usize,
    reducers: usize,
    combiner: bool,
    mem_records: usize,
) -> io::Result<(WordCountStats, Vec<PathBuf>)> {
    assert!(splits >= 1 && reducers >= 1);
    fs::create_dir_all(work)?;
    let total_lines = lines_of(input)?.try_fold(0u64, |n, l| l.map(|_| n + 1))?;
    let mut stats = WordCountStats { input_lines: total_lines, partition_records: vec![0; reducers], ..WordCountStats::default() };

    // Map phase: each task reads its range of lines and writes R region files ("local disk of the map worker").
    let mut lines = lines_of(input)?;
    for task in 0..splits {
        let lo = total_lines * task as u64 / splits as u64;
        let hi = total_lines * (task as u64 + 1) / splits as u64;
        let mut regions: Vec<BufWriter<File>> = (0..reducers)
            .map(|r| File::create(work.join(format!("map{task:03}-region{r}"))).map(BufWriter::new))
            .collect::<io::Result<_>>()?;
        let mut local: HashMap<String, u64> = HashMap::new();
        for _ in lo..hi {
            let line = lines.next().ok_or_else(|| invalid("input shrank"))??;
            for w in words(&line) {
                stats.words += 1;
                if combiner {
                    *local.entry(w).or_insert(0) += 1;
                } else {
                    writeln!(regions[partition(&w, reducers)], "{w}\t1")?;
                    stats.shuffled_records += 1;
                }
            }
        }
        if combiner {
            let mut entries: Vec<_> = local.into_iter().collect();
            entries.sort();
            for (w, c) in entries {
                writeln!(regions[partition(&w, reducers)], "{w}\t{c}")?;
                stats.shuffled_records += 1;
            }
        }
        for mut r in regions {
            r.flush()?;
        }
    }

    // Shuffle: reducer r fetches region r of every map task. Sort: external sort of what it fetched.
    // Reduce: add up adjacent records with the same word.
    let mut outputs = Vec::new();
    for r in 0..reducers {
        let fetched = work.join(format!("reduce{r}-input"));
        {
            let mut w = BufWriter::new(File::create(&fetched)?);
            for task in 0..splits {
                let region = work.join(format!("map{task:03}-region{r}"));
                let bytes = io::copy(&mut File::open(&region)?, &mut w)?;
                stats.shuffle_bytes += bytes;
                for _ in lines_of(&region)? {
                    stats.partition_records[r] += 1;
                }
                fs::remove_file(region)?;
            }
            w.flush()?;
        }
        let sorted = work.join(format!("reduce{r}-sorted"));
        let s = external_sort(&fetched, &sorted, &work.join(format!("tmp{r}")), mem_records, 16)?;
        stats.sort.records += s.records;
        stats.sort.runs += s.runs;
        stats.sort.merge_passes = stats.sort.merge_passes.max(s.merge_passes);
        stats.sort.bytes_spilled += s.bytes_spilled;
        fs::remove_file(&fetched)?;

        let out = work.join(format!("part-{r}"));
        let mut w = BufWriter::new(File::create(&out)?);
        let mut current: Option<(String, u64)> = None;
        for line in lines_of(&sorted)? {
            let line = line?;
            let (word, c) = line.split_once('\t').ok_or_else(|| invalid("record without a tab"))?;
            let c: u64 = c.parse().map_err(|_| invalid("bad count"))?;
            match &mut current {
                Some((cw, total)) if cw == word => *total += c,
                _ => {
                    if let Some((cw, total)) = current.take() {
                        writeln!(w, "{cw}\t{total}")?;
                        stats.distinct_words += 1;
                    }
                    current = Some((word.to_string(), c));
                }
            }
        }
        if let Some((cw, total)) = current {
            writeln!(w, "{cw}\t{total}")?;
            stats.distinct_words += 1;
        }
        w.flush()?;
        fs::remove_file(sorted)?;
        outputs.push(out);
    }
    Ok((stats, outputs))
}

/// `user_id\tcountry` lines; countries are `c00` ... .
pub fn generate_users(path: &Path, users: u64, countries: u64, seed: u64) -> io::Result<()> {
    let mut rng = Rng::new(seed);
    let mut w = BufWriter::new(File::create(path)?);
    for id in 0..users {
        writeln!(w, "{id}\tc{:02}", rng.below(countries))?;
    }
    w.flush()
}

/// `user_id\turl` lines. A fraction `hot` of the clicks belongs to user 0 (a skewed key), and 1% of
/// the clicks carry a user id that has no user record.
pub fn generate_clicks(path: &Path, clicks: u64, users: u64, hot: f64, seed: u64) -> io::Result<()> {
    let mut rng = Rng::new(seed);
    let mut w = BufWriter::new(File::create(path)?);
    for n in 0..clicks {
        let id = if rng.chance(hot) {
            0
        } else if rng.chance(0.01) {
            users + rng.below(1000)
        } else {
            rng.below(users)
        };
        writeln!(w, "{id}\t/page/{}", n % 997)?;
    }
    w.flush()
}

#[derive(Debug, Clone, Default)]
pub struct JoinStats {
    pub map_records: u64,
    /// Bytes of the map output that the shuffle moves to the reducers.
    pub shuffle_bytes: u64,
    pub partition_records: Vec<u64>,
    pub joined: u64,
    /// Clicks whose user id has no user record.
    pub orphan_clicks: u64,
    /// Most click records any single key has.
    pub max_clicks_per_key: u64,
    /// Most records the reduce function kept in memory at once (counted where it stores them).
    pub peak_buffered: usize,
    pub sort: SortStats,
}

/// How the reduce function of the join treats the records of one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReduceStrategy {
    /// Records arrive sorted by (key, tag), so the user record comes first: remember it, and stream
    /// the clicks that follow. Memory: one record.
    SecondarySort,
    /// Do not rely on the order within a key: collect every record of the key, then combine them.
    /// Memory: the whole key, however skewed.
    BufferWholeKey,
}

fn parse_pair(line: &str) -> io::Result<(u64, &str)> {
    let (id, rest) = line.split_once('\t').ok_or_else(|| invalid("record without a tab"))?;
    Ok((id.parse().map_err(|_| invalid("bad id"))?, rest))
}

/// `id\ttag\tpayload` of a shuffled record; the tag is `0` for users and `1` for clicks.
fn parse_shuffled(line: &str) -> io::Result<(u64, bool, &str)> {
    let mut parts = line.splitn(3, '\t');
    let (Some(id), Some(tag), Some(payload)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(invalid("malformed shuffled record"));
    };
    Ok((id.parse().map_err(|_| invalid("bad id"))?, tag == "1", payload))
}

/// The reduce function: reads the sorted records of one partition, writes joined lines.
fn reduce_join(sorted: &Path, out: &Path, strategy: ReduceStrategy, stats: &mut JoinStats) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(out)?);
    let mut key: Option<u64> = None;
    let mut country: Option<String> = None; // SecondarySort: the one buffered record
    let mut group_users: Vec<String> = Vec::new(); // BufferWholeKey: everything of the current key
    let mut group_clicks: Vec<String> = Vec::new();
    let mut clicks_in_key = 0u64;

    let finish_group = |id: u64, users: &mut Vec<String>, clicks: &mut Vec<String>, w: &mut BufWriter<File>, stats: &mut JoinStats| -> io::Result<()> {
        for click in clicks.iter() {
            if users.is_empty() {
                stats.orphan_clicks += 1;
            }
            for user in users.iter() {
                writeln!(w, "{id}\t{user}\t{click}")?;
                stats.joined += 1;
            }
        }
        users.clear();
        clicks.clear();
        Ok(())
    };

    for line in lines_of(sorted)? {
        let line = line?;
        let (id, is_click, payload) = parse_shuffled(&line)?;
        if key != Some(id) {
            if let Some(prev) = key {
                finish_group(prev, &mut group_users, &mut group_clicks, &mut w, stats)?;
            }
            key = Some(id);
            country = None;
            clicks_in_key = 0;
        }
        clicks_in_key += u64::from(is_click);
        stats.max_clicks_per_key = stats.max_clicks_per_key.max(clicks_in_key);
        match (strategy, is_click) {
            (ReduceStrategy::SecondarySort, false) => {
                country = Some(payload.to_string());
                stats.peak_buffered = stats.peak_buffered.max(1);
            }
            (ReduceStrategy::SecondarySort, true) => match &country {
                Some(c) => {
                    writeln!(w, "{id}\t{c}\t{payload}")?;
                    stats.joined += 1;
                }
                None => stats.orphan_clicks += 1,
            },
            (ReduceStrategy::BufferWholeKey, false) => group_users.push(payload.to_string()),
            (ReduceStrategy::BufferWholeKey, true) => group_clicks.push(payload.to_string()),
        }
        stats.peak_buffered = stats.peak_buffered.max(group_users.len() + group_clicks.len());
    }
    if let Some(prev) = key {
        finish_group(prev, &mut group_users, &mut group_clicks, &mut w, stats)?;
    }
    w.flush()
}

/// Reduce-side (repartition) join of `users` (`id\tcountry`) and `clicks` (`id\turl`), inner join.
/// Output lines are `id\tcountry\turl`, one file per reducer.
pub fn sort_merge_join(
    users: &Path,
    clicks: &Path,
    work: &Path,
    reducers: usize,
    mem_records: usize,
    strategy: ReduceStrategy,
) -> io::Result<(JoinStats, Vec<PathBuf>)> {
    fs::create_dir_all(work)?;
    let mut stats = JoinStats { partition_records: vec![0; reducers], ..JoinStats::default() };

    // Map: tag every record (0 = user, 1 = click) and key it by the zero-padded id so that plain
    // string order is (id, tag) order; partition by id only, so both inputs meet in one reducer.
    let mut regions: Vec<BufWriter<File>> =
        (0..reducers).map(|r| File::create(work.join(format!("region{r}"))).map(BufWriter::new)).collect::<io::Result<_>>()?;
    for (tag, path) in [(0, users), (1, clicks)] {
        for line in lines_of(path)? {
            let line = line?;
            let (id, payload) = parse_pair(&line)?;
            writeln!(regions[partition(&id.to_string(), reducers)], "{id:010}\t{tag}\t{payload}")?;
            stats.map_records += 1;
        }
    }
    for mut r in regions {
        r.flush()?;
    }

    let mut outputs = Vec::new();
    for r in 0..reducers {
        let region = work.join(format!("region{r}"));
        let sorted = work.join(format!("sorted{r}"));
        let s = external_sort(&region, &sorted, &work.join(format!("tmp{r}")), mem_records, 16)?;
        stats.partition_records[r] = s.records;
        stats.sort.records += s.records;
        stats.sort.runs += s.runs;
        stats.sort.merge_passes = stats.sort.merge_passes.max(s.merge_passes);
        stats.sort.bytes_spilled += s.bytes_spilled;
        stats.shuffle_bytes += fs::metadata(&region)?.len();
        fs::remove_file(region)?;

        let out = work.join(format!("joined{r}"));
        reduce_join(&sorted, &out, strategy, &mut stats)?;
        fs::remove_file(sorted)?;
        outputs.push(out);
    }
    Ok((stats, outputs))
}

/// Map-side hash join: the whole `users` input is loaded into a hash table, `clicks` is streamed.
/// Returns the output lines (`id\tcountry\turl`) and how many users were held in memory.
pub fn hash_join(users: &Path, clicks: &Path) -> io::Result<(Vec<String>, usize)> {
    let mut table: HashMap<u64, String> = HashMap::new();
    for line in lines_of(users)? {
        let line = line?;
        let (id, country) = parse_pair(&line)?;
        table.insert(id, country.to_string());
    }
    let mut out = Vec::new();
    for line in lines_of(clicks)? {
        let line = line?;
        let (id, url) = parse_pair(&line)?;
        if let Some(c) = table.get(&id) {
            out.push(format!("{id}\t{c}\t{url}"));
        }
    }
    Ok((out, table.len()))
}

/// A map-only job: keep column `col` (0-based, tab separated) of every line.
pub fn project_column(input: &Path, output: &Path, col: usize) -> io::Result<u64> {
    let mut w = BufWriter::new(File::create(output)?);
    let mut n = 0;
    for line in lines_of(input)? {
        let line = line?;
        let field = line.split('\t').nth(col).ok_or_else(|| invalid("missing column"))?;
        writeln!(w, "{field}")?;
        n += 1;
    }
    w.flush()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::TempDir;

    fn write(path: &Path, lines: &[String]) {
        write_lines(path, lines).unwrap();
    }

    fn read(path: &Path) -> Vec<String> {
        lines_of(path).unwrap().map(Result::unwrap).collect()
    }

    fn random_lines(n: usize, seed: u64) -> Vec<String> {
        let mut rng = Rng::new(seed);
        (0..n).map(|_| format!("{:06}-{}", rng.below(5000), rng.below(100))).collect()
    }

    #[test]
    fn external_sort_matches_an_in_memory_sort_for_every_memory_and_fan_in() {
        let t = TempDir::new("batch-sort").unwrap();
        let input = t.path().join("in");
        for n in [0usize, 1, 2, 7, 1000] {
            let lines = random_lines(n, n as u64);
            write(&input, &lines);
            let mut want = lines.clone();
            want.sort();
            for (mem, fan_in) in [(1, 2), (3, 2), (7, 3), (50, 4), (1000, 2), (5000, 16)] {
                let out = t.path().join("out");
                let s = external_sort(&input, &out, &t.path().join("tmp"), mem, fan_in).unwrap();
                assert_eq!(read(&out), want, "n={n} mem={mem} fan_in={fan_in}");
                assert_eq!(s.records, n as u64);
                assert_eq!(s.runs, n.div_ceil(mem));
                if n > 0 {
                    // The number of merge rounds is the number of times the run count must be divided by fan_in.
                    let (mut runs, mut rounds) = (s.runs, 1);
                    while runs > fan_in {
                        runs = runs.div_ceil(fan_in);
                        rounds += 1;
                    }
                    assert_eq!(s.merge_passes, rounds, "n={n} mem={mem} fan_in={fan_in}");
                }
                assert_eq!(fs::read_dir(t.path().join("tmp")).unwrap().count(), 0, "temporary runs are removed");
            }
        }
    }

    #[test]
    fn a_wider_fan_in_needs_fewer_passes_and_spills_less() {
        let t = TempDir::new("batch-fanin").unwrap();
        let input = t.path().join("in");
        write(&input, &random_lines(2000, 5));
        let stats = |fan_in| external_sort(&input, &t.path().join("out"), &t.path().join("tmp"), 20, fan_in).unwrap();
        let (narrow, wide) = (stats(2), stats(128));
        assert_eq!((narrow.runs, wide.runs), (100, 100));
        assert!(narrow.merge_passes > 5 && wide.merge_passes == 1);
        assert!(narrow.bytes_spilled > 3 * wide.bytes_spilled, "{narrow:?} vs {wide:?}");
    }

    #[test]
    fn tokenizer_lowercases_and_splits_on_anything_that_is_not_alphanumeric() {
        assert_eq!(words("Hello, WORLD! hello-world 42x").collect::<Vec<_>>(), ["hello", "world", "hello", "world", "42x"]);
        assert_eq!(words("  ,,, ").count(), 0);
    }

    #[test]
    fn partition_is_stable_and_spreads_keys() {
        assert_eq!(partition("hello", 4), partition("hello", 4));
        // Fixed expectation: the function must not change between runs, machines or Rust versions.
        assert_eq!(fnv1a(b"hello"), 0xa430_d846_80aa_bd0b);
        let mut buckets = [0u32; 4];
        for i in 0..4000 {
            buckets[partition(&format!("w{i:05}"), 4)] += 1;
        }
        assert!(buckets.iter().all(|&b| (800..1200).contains(&b)), "{buckets:?}");
    }

    #[test]
    fn word_count_equals_the_in_memory_count_with_and_without_a_combiner() {
        let t = TempDir::new("batch-wc").unwrap();
        let input = t.path().join("text");
        generate_text(&input, 3000, 8, 400, 1.0, 11).unwrap();
        let want = baseline_word_count(&input).unwrap();
        let mut shuffled = Vec::new();
        for (combiner, splits, reducers, mem) in [(false, 1, 1, 100), (false, 4, 3, 500), (true, 1, 1, 100), (true, 5, 4, 50), (true, 7, 2, 10_000)] {
            let (stats, files) = word_count(&input, &t.path().join("work"), splits, reducers, combiner, mem).unwrap();
            let got = read_counts(&files).unwrap();
            assert_eq!(got, want, "combiner={combiner} splits={splits} reducers={reducers}");
            assert_eq!(stats.words, 24_000);
            assert_eq!(stats.input_lines, 3000);
            assert_eq!(stats.distinct_words, want.len() as u64);
            assert_eq!(stats.partition_records.iter().sum::<u64>(), stats.shuffled_records);
            if !combiner {
                assert_eq!(stats.shuffled_records, stats.words, "without a combiner every word is shuffled");
            }
            shuffled.push((combiner, splits, stats.shuffled_records, stats.shuffle_bytes));
        }
        // A combiner shuffles at most one record per (map task, word), usually far fewer than one per word.
        let plain = shuffled.iter().find(|s| !s.0 && s.1 == 4).unwrap();
        let combined = shuffled.iter().find(|s| s.0 && s.1 == 5).unwrap();
        assert!(combined.2 * 4 < plain.2 && combined.3 < plain.3, "{plain:?} vs {combined:?}");
    }

    #[test]
    fn a_word_always_goes_to_the_same_reducer() {
        let t = TempDir::new("batch-wc-part").unwrap();
        let input = t.path().join("text");
        generate_text(&input, 500, 8, 100, 1.0, 3).unwrap();
        let (_, files) = word_count(&input, &t.path().join("work"), 3, 4, true, 100).unwrap();
        for (r, f) in files.iter().enumerate() {
            for line in read(f) {
                let word = line.split('\t').next().unwrap();
                assert_eq!(partition(word, 4), r);
            }
            let lines = read(f);
            assert!(lines.windows(2).all(|w| w[0] < w[1]), "each reduce output is sorted by word");
        }
    }

    #[test]
    fn zipf_text_has_a_heavy_head_and_is_reproducible() {
        let t = TempDir::new("batch-zipf").unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b"));
        generate_text(&a, 2000, 10, 1000, 1.0, 42).unwrap();
        generate_text(&b, 2000, 10, 1000, 1.0, 42).unwrap();
        assert_eq!(read(&a), read(&b));
        let counts = baseline_word_count(&a).unwrap();
        let top = counts["w00000"];
        assert!(top > 5 * counts.get("w00100").copied().unwrap_or(0).max(1), "rank 0 should dwarf rank 100");
        let total: u64 = counts.values().sum();
        assert!(top as f64 > 0.08 * total as f64, "the most frequent word holds a visible share: {top} of {total}");
    }

    fn nested_loop_join(users: &Path, clicks: &Path) -> Vec<String> {
        let users: Vec<(u64, String)> = read(users).iter().map(|l| parse_pair(l).map(|(i, c)| (i, c.to_string())).unwrap()).collect();
        let mut out = Vec::new();
        for l in read(clicks) {
            let (id, url) = parse_pair(&l).unwrap();
            for (uid, c) in &users {
                if *uid == id {
                    out.push(format!("{id}\t{c}\t{url}"));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn sort_merge_join_and_hash_join_equal_a_nested_loop_join() {
        let t = TempDir::new("batch-join").unwrap();
        let (users, clicks) = (t.path().join("users"), t.path().join("clicks"));
        generate_users(&users, 300, 7, 1).unwrap();
        generate_clicks(&clicks, 5000, 300, 0.2, 2).unwrap();
        let want = nested_loop_join(&users, &clicks);
        assert!(!want.is_empty());

        for (reducers, mem) in [(1, 100), (4, 500), (3, 10_000)] {
            for strategy in [ReduceStrategy::SecondarySort, ReduceStrategy::BufferWholeKey] {
                let (stats, files) = sort_merge_join(&users, &clicks, &t.path().join("work"), reducers, mem, strategy).unwrap();
                let mut got: Vec<String> = files.iter().flat_map(|f| read(f)).collect();
                got.sort();
                assert_eq!(got, want, "reducers={reducers} {strategy:?}");
                assert_eq!(stats.joined as usize, want.len());
                assert_eq!(stats.joined + stats.orphan_clicks, 5000, "every click is either joined or an orphan");
                assert!(stats.orphan_clicks > 10, "1% of the clicks have no user: {stats:?}");
                assert_eq!(stats.map_records, 300 + 5000);
                if strategy == ReduceStrategy::SecondarySort {
                    assert_eq!(stats.peak_buffered, 1, "the reducer holds one record: the user of the current key");
                }
            }
        }

        let (mut hashed, held) = hash_join(&users, &clicks).unwrap();
        hashed.sort();
        assert_eq!(hashed, want);
        assert_eq!(held, 300);
    }

    #[test]
    fn a_hot_key_makes_one_partition_huge_and_only_a_buffering_reducer_pays_for_it_in_memory() {
        let t = TempDir::new("batch-skew").unwrap();
        let (users, clicks) = (t.path().join("users"), t.path().join("clicks"));
        generate_users(&users, 1000, 5, 1).unwrap();
        generate_clicks(&clicks, 20_000, 1000, 0.3, 2).unwrap();
        let work = t.path().join("work");
        let (stats, _) = sort_merge_join(&users, &clicks, &work, 8, 1000, ReduceStrategy::SecondarySort).unwrap();
        let hot = stats.max_clicks_per_key;
        assert!(hot > 5500 && hot < 6500, "about 30% of 20000 clicks belong to user 0: {hot}");
        let biggest = *stats.partition_records.iter().max().unwrap();
        let mean = stats.partition_records.iter().sum::<u64>() as f64 / 8.0;
        assert!(biggest as f64 > 2.0 * mean, "the hot key's partition is far above the mean: {:?}", stats.partition_records);
        assert_eq!(stats.peak_buffered, 1);
        let (naive, _) = sort_merge_join(&users, &clicks, &work, 8, 1000, ReduceStrategy::BufferWholeKey).unwrap();
        assert_eq!(naive.peak_buffered as u64, hot + 1, "all clicks of the hot key plus its user record");
        assert_eq!(naive.joined, stats.joined);
    }

    #[test]
    fn jobs_chain_through_files() {
        let t = TempDir::new("batch-chain").unwrap();
        let (users, clicks) = (t.path().join("users"), t.path().join("clicks"));
        generate_users(&users, 200, 6, 1).unwrap();
        generate_clicks(&clicks, 3000, 200, 0.0, 2).unwrap();
        let (_, joined) = sort_merge_join(&users, &clicks, &t.path().join("j"), 2, 500, ReduceStrategy::SecondarySort).unwrap();
        let all = t.path().join("joined-all");
        write(&all, &joined.iter().flat_map(|f| read(f)).collect::<Vec<_>>());
        let countries = t.path().join("countries");
        let n = project_column(&all, &countries, 1).unwrap();
        let (stats, outs) = word_count(&countries, &t.path().join("wc"), 3, 2, true, 100).unwrap();
        let per_country = read_counts(&outs).unwrap();
        assert_eq!(per_country.values().sum::<u64>(), n);
        assert!(per_country.len() <= 6 && stats.distinct_words as usize == per_country.len());
        // Cross-check with the hash join.
        let (hashed, _) = hash_join(&users, &clicks).unwrap();
        let mut direct: BTreeMap<String, u64> = BTreeMap::new();
        for l in hashed {
            *direct.entry(l.split('\t').nth(1).unwrap().to_string()).or_insert(0) += 1;
        }
        assert_eq!(per_country, direct);
    }
}
