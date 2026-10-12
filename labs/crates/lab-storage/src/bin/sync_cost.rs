//! Lab 1: cost and syscalls of write / sync_data / sync_all.
//!
//! Run: cargo run -q -p lab-storage --bin sync_cost [dir] [records]
//! Linux + strace: ./labs/linux.sh bash -c 'cargo build -q -p lab-storage --bin sync_cost && \
//!   strace -f -e trace=write,fsync,fdatasync ./target-linux/debug/sync_cost'

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const RECORD: usize = 4096;

#[derive(Clone, Copy)]
enum Mode {
    /// write only; leave the data in the page cache.
    NoSync,
    /// write + `sync_data()` after every record.
    DataEach,
    /// write + `sync_all()` after every record.
    AllEach,
    /// write every record, then a single `sync_all()` at the end.
    AllOnce,
}

fn run(path: &Path, mode: Mode, records: usize) -> io::Result<Duration> {
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    let rec = [0xABu8; RECORD];
    let start = Instant::now();
    for _ in 0..records {
        f.write_all(&rec)?;
        match mode {
            Mode::NoSync | Mode::AllOnce => {}
            Mode::DataEach => f.sync_data()?,
            Mode::AllEach => f.sync_all()?,
        }
    }
    if matches!(mode, Mode::AllOnce) {
        f.sync_all()?;
    }
    Ok(start.elapsed())
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = lab_storage::scratch_dir(args.next())?;
    let records: usize = args.next().map_or(200, |s| s.parse().expect("records"));
    let path = dir.join("sync_cost.dat");
    println!("dir = {}, {records} records x {RECORD} B", dir.display());
    println!("{:<34} {:>14} {:>14}", "mode", "total", "per record");

    let modes = [
        ("write only (no sync)", Mode::NoSync),
        ("write + sync_data each record", Mode::DataEach),
        ("write + sync_all each record", Mode::AllEach),
        ("write all, one sync_all at end", Mode::AllOnce),
    ];
    for (label, mode) in modes {
        let total = run(&path, mode, records)?;
        let per = total / u32::try_from(records).expect("records fits u32");
        println!(
            "{label:<34} {:>14} {:>14}",
            lab_storage::ms(total),
            lab_storage::ms(per)
        );
    }
    std::fs::remove_file(&path)?;
    Ok(())
}
