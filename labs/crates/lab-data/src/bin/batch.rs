//! Lab (f): batch jobs on files: external merge sort, word count in the MapReduce shape (with and
//! without a combiner), a reduce-side sort-merge join vs a hash join, skew, and a chain of jobs.
//!
//! Run: cargo run -q --release -p lab-data --bin batch [scratch-dir]

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use lab_data::batch::{self, ReduceStrategy};
use lab_data::rng::Rng;
use lab_data::util::{scratch_dir, thousands};

fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    Ok(fs::read_to_string(path)?.lines().map(str::to_string).collect())
}

fn external_sort_section(root: &Path) -> io::Result<()> {
    println!("== 1. external merge sort: more lines than memory ==");
    let input = root.join("lines");
    let mut rng = Rng::new(1);
    let lines: Vec<String> = (0..300_000).map(|_| format!("{:010}-{:010}", rng.below(10_000_000_000), rng.below(10_000_000_000))).collect();
    fs::write(&input, lines.join("\n") + "\n")?;
    let mut want = lines;
    want.sort_unstable();
    println!("input: {} lines of 21 bytes ({:.1} MB); at most 10,000 lines held in memory at a time", thousands(want.len() as u64), want.len() as f64 * 22.0 / 1e6);
    println!("{:>8} {:>6} {:>13} {:>14}  output == in-memory sort", "fan-in", "runs", "merge passes", "spilled MB");
    for fan_in in [2, 4, 32] {
        let out = root.join("sorted");
        let s = batch::external_sort(&input, &out, &root.join("tmp"), 10_000, fan_in)?;
        println!(
            "{fan_in:>8} {:>6} {:>13} {:>14.1}  {}",
            s.runs,
            s.merge_passes,
            s.bytes_spilled as f64 / 1e6,
            read_lines(&out)? == want
        );
    }
    println!("(\"spilled\" counts runs and intermediate merge files; the final output is not included)");
    Ok(())
}

fn word_count_section(root: &Path) -> io::Result<()> {
    println!("\n== 2. word count: map -> partition -> shuffle -> sort -> reduce ==");
    let input = root.join("text");
    let bytes = batch::generate_text(&input, 100_000, 10, 5_000, 1.0, 7)?;
    let baseline = batch::baseline_word_count(&input)?;
    println!(
        "input: 100,000 lines x 10 words = 1,000,000 words ({:.1} MB), Zipf weights 1/rank over 5,000 distinct words; M = 8 map tasks, R = 4 reduce tasks",
        bytes as f64 / 1e6
    );
    println!(
        "{:<9} {:>14} {:>12} {:>10}  {:<34} {:>9} == in-memory count",
        "combiner", "shuffled recs", "shuffle MB", "sort runs", "records per reduce partition", "distinct"
    );
    let mut last = BTreeMap::new();
    for combiner in [false, true] {
        let work = root.join(format!("wc-{combiner}"));
        let (s, files) = batch::word_count(&input, &work, 8, 4, combiner, 50_000)?;
        let counts = batch::read_counts(&files)?;
        println!(
            "{:<9} {:>14} {:>12.1} {:>10}  {:<34} {:>9} {}",
            if combiner { "on" } else { "off" },
            thousands(s.shuffled_records),
            s.shuffle_bytes as f64 / 1e6,
            s.sort.runs,
            format!("{:?}", s.partition_records),
            thousands(s.distinct_words),
            counts == baseline
        );
        last = counts;
    }
    let mut top: Vec<(&String, &u64)> = last.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!(
        "most frequent: {}",
        top.iter().take(5).map(|(w, c)| format!("{w}={}", thousands(**c))).collect::<Vec<_>>().join("  ")
    );
    println!(
        "w00000 is {:.1}% of all words; the partition function sends it (and nothing can split it) to reduce partition {}",
        100.0 * *top[0].1 as f64 / 1e6,
        batch::partition("w00000", 4)
    );
    Ok(())
}

fn join_section(root: &Path) -> io::Result<()> {
    println!("\n== 3. join: clicks x users ==");
    let (users, clicks) = (root.join("users"), root.join("clicks"));
    batch::generate_users(&users, 20_000, 40, 1)?;
    batch::generate_clicks(&clicks, 400_000, 20_000, 0.25, 2)?;
    println!("users: 20,000 rows (id, country); clicks: 400,000 rows (id, url); 25% of the clicks belong to user 0, 1% have an id with no user row");
    println!("reduce-side sort-merge join, R = 8 reducers, records tagged (0 = user, 1 = click), sorted by (id, tag):");
    println!(
        "{:<18} {:>8} {:>8} {:>13} {:>11}  records per reduce partition",
        "reduce function", "joined", "orphans", "peak records", "shuffle MB"
    );
    let mut result = Vec::new();
    for (name, strategy) in [("secondary sort", ReduceStrategy::SecondarySort), ("buffer whole key", ReduceStrategy::BufferWholeKey)] {
        let (s, files) = batch::sort_merge_join(&users, &clicks, &root.join(format!("join-{strategy:?}")), 8, 50_000, strategy)?;
        println!(
            "{name:<18} {:>8} {:>8} {:>13} {:>11.1}  {:?}",
            thousands(s.joined),
            thousands(s.orphan_clicks),
            thousands(s.peak_buffered as u64),
            s.shuffle_bytes as f64 / 1e6,
            s.partition_records
        );
        if strategy == ReduceStrategy::SecondarySort {
            let mean = s.partition_records.iter().sum::<u64>() as f64 / s.partition_records.len() as f64;
            let max = *s.partition_records.iter().max().unwrap_or(&0) as f64;
            println!(
                "  (clicks of the hottest key: {}; biggest partition / mean partition = {:.2}; \"peak records\" = most records the reduce function kept in memory)",
                thousands(s.max_clicks_per_key),
                max / mean
            );
            result = files.iter().map(|f| read_lines(f)).collect::<io::Result<Vec<_>>>()?.concat();
        }
    }
    let (hashed, held) = batch::hash_join(&users, &clicks)?;
    let mut a = result.clone();
    let mut b = hashed;
    a.sort();
    b.sort();
    println!(
        "hash join (map side, no shuffle): {} users held in memory, {} rows joined, same rows as the sort-merge join: {}",
        thousands(held as u64),
        thousands(b.len() as u64),
        a == b
    );

    println!("\n== 4. a chain of three jobs: clicks per country ==");
    let all = root.join("joined-all");
    fs::write(&all, result.join("\n") + "\n")?;
    let countries = root.join("countries");
    let n = batch::project_column(&all, &countries, 1)?;
    let (s, files) = batch::word_count(&countries, &root.join("wc-countries"), 4, 2, true, 50_000)?;
    let counts = batch::read_counts(&files)?;
    let mut top: Vec<(&String, &u64)> = counts.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("job 1 (join) -> {} rows -> job 2 (map-only: keep column `country`) -> {} lines -> job 3 (word count with a combiner)", thousands(result.len() as u64), thousands(n));
    println!("job 3 shuffled {} records for {} distinct countries", thousands(s.shuffled_records), s.distinct_words);
    println!("top countries by clicks: {}", top.iter().take(4).map(|(c, n)| format!("{c}={}", thousands(**n))).collect::<Vec<_>>().join("  "));
    Ok(())
}

fn main() -> io::Result<()> {
    let root = scratch_dir(std::env::args().nth(1), "batch")?;
    println!("scratch dir: {}\n", root.display());
    external_sort_section(&root)?;
    word_count_section(&root)?;
    join_section(&root)?;
    fs::remove_dir_all(&root)
}
