//! Lab 4: O_DIRECT reads: alignment rules, and no page-cache population.
//!
//! Run on Linux: ./labs/linux.sh cargo run -q -p lab-storage --bin direct_io [dir] [size_mib]
//! If `open` fails with EINVAL the filesystem under `dir` does not support O_DIRECT: pick another dir.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use lab_storage::{AlignedBuf, fadvise, ms, resident_pages, scratch_dir};
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};

    const CHUNK: usize = 1 << 20;
    const ALIGN: usize = 4096;

    fn read_all(f: &mut File, buf: &mut [u8]) -> std::io::Result<(u64, Duration)> {
        f.seek(SeekFrom::Start(0))?;
        let mut total = 0u64;
        let start = Instant::now();
        loop {
            let n = f.read(buf)?;
            if n == 0 {
                break;
            }
            total += n as u64;
        }
        Ok((total, start.elapsed()))
    }

    fn show(label: &str, f: &File) -> std::io::Result<()> {
        let (res, total) = resident_pages(f)?;
        println!("{label:<46} resident pages: {res:>7} / {total}");
        Ok(())
    }

    let mut args = std::env::args().skip(1);
    let dir = scratch_dir(args.next())?;
    let size_mib: usize = args.next().map_or(128, |s| s.parse().expect("size_mib"));
    let path = dir.join("direct_io.dat");

    // Setup: buffered write + sync_all, then drop the cache so we start from "nothing resident".
    {
        let mut f = File::create(&path)?;
        let chunk = vec![0x5Au8; CHUNK];
        for _ in 0..size_mib {
            f.write_all(&chunk)?;
        }
        f.sync_all()?;
        fadvise(&f, libc::POSIX_FADV_DONTNEED)?;
    }
    let probe = File::open(&path)?;
    show("0. start (cache dropped)", &probe)?;

    let mut direct = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECT)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "open(O_DIRECT) failed: {e} (raw os error {:?})",
                e.raw_os_error()
            );
            return Err(e);
        }
    };
    let mut buf = AlignedBuf::new(CHUNK, ALIGN);

    let (n1, t1) = read_all(&mut direct, buf.as_mut_slice())?;
    show("1. after O_DIRECT read #1", &probe)?;
    let (n2, t2) = read_all(&mut direct, buf.as_mut_slice())?;
    show("2. after O_DIRECT read #2", &probe)?;
    println!(
        "   O_DIRECT #1: {n1} B in {}; #2: {n2} B in {}",
        ms(t1),
        ms(t2)
    );

    // Same file through the page cache, for comparison.
    let mut buffered = File::open(&path)?;
    let (_, b1) = read_all(&mut buffered, buf.as_mut_slice())?;
    show("3. after buffered read #1", &probe)?;
    let (_, b2) = read_all(&mut buffered, buf.as_mut_slice())?;
    println!("   buffered #1: {}; #2: {}", ms(b1), ms(b2));

    // Alignment experiments: each prints Ok(n) or the OS error.
    println!("alignment experiments (O_DIRECT read):");
    let mut try_read =
        |label: &str, off: u64, range: std::ops::Range<usize>, buf: &mut AlignedBuf| {
            let r = direct
                .seek(SeekFrom::Start(off))
                .and_then(|_| direct.read(&mut buf.as_mut_slice()[range]));
            match r {
                Ok(n) => println!("  {label:<44} -> Ok({n})"),
                Err(e) => println!("  {label:<44} -> Err({e}) raw={:?}", e.raw_os_error()),
            }
        };
    try_read("aligned addr, len 4096, offset 0", 0, 0..4096, &mut buf);
    try_read("aligned addr, len 4096, offset 1", 1, 0..4096, &mut buf);
    try_read("aligned addr, len 100, offset 0", 0, 0..100, &mut buf);
    try_read("addr + 1, len 4096, offset 0", 0, 1..4097, &mut buf);

    std::fs::remove_file(&path)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_storage::need_linux("direct_io");
}
