//! Lab 3: page cache residency (mincore) and posix_fadvise(DONTNEED / WILLNEED).
//!
//! Run on Linux: ./labs/linux.sh cargo run -q -p lab-storage --bin page_cache [dir] [size_mib]

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use lab_storage::{fadvise, ms, resident_pages, scratch_dir};
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    const CHUNK: usize = 1 << 20;

    fn read_all(f: &mut File) -> std::io::Result<Duration> {
        use std::io::{Seek, SeekFrom};
        f.seek(SeekFrom::Start(0))?;
        let mut buf = vec![0u8; CHUNK];
        let start = Instant::now();
        while f.read(&mut buf)? != 0 {}
        Ok(start.elapsed())
    }

    fn show(label: &str, f: &File) -> std::io::Result<()> {
        let (res, total) = resident_pages(f)?;
        println!("{label:<46} resident pages: {res:>7} / {total}");
        Ok(())
    }

    let mut args = std::env::args().skip(1);
    let dir = scratch_dir(args.next())?;
    let size_mib: usize = args.next().map_or(128, |s| s.parse().expect("size_mib"));
    let path = dir.join("page_cache.dat");
    println!(
        "file = {}, {size_mib} MiB, page = {} B",
        path.display(),
        lab_storage::page_size()
    );

    // Setup: write the file, then sync_all so no page is dirty (DONTNEED does not free dirty pages).
    let mut f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    let chunk = vec![0x5Au8; CHUNK];
    for _ in 0..size_mib {
        f.write_all(&chunk)?;
    }
    f.sync_all()?;
    show("1. after write + sync_all", &f)?;

    fadvise(&f, libc::POSIX_FADV_DONTNEED)?;
    show("2. after posix_fadvise(DONTNEED)", &f)?;

    let cold = read_all(&mut f)?;
    show("3. after first full read", &f)?;
    println!("   first read : {}", ms(cold));

    let warm = read_all(&mut f)?;
    println!("   second read: {}", ms(warm));

    fadvise(&f, libc::POSIX_FADV_DONTNEED)?;
    show("4. DONTNEED again", &f)?;
    fadvise(&f, libc::POSIX_FADV_WILLNEED)?;
    show("5. right after posix_fadvise(WILLNEED)", &f)?;
    std::thread::sleep(Duration::from_secs(2));
    show("6. 2 s later", &f)?;

    std::fs::remove_file(&path)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_storage::need_linux("page_cache");
}
