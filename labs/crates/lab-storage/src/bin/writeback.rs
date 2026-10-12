//! Lab 6: watch /proc/meminfo `Dirty` and `Writeback` while buffered data is written back.
//!
//! Run on Linux: ./labs/linux.sh cargo run -q -p lab-storage --bin writeback [dir] [size_mib] [seconds]
//! Dirty/Writeback are whole-machine numbers: other activity adds noise.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use lab_storage::{meminfo_kib, scratch_dir, vm_sysctl};
    use std::fs::File;
    use std::io::Write;
    use std::time::{Duration, Instant};

    fn sample(label: &str, t0: Instant) -> std::io::Result<()> {
        println!(
            "t={:>6.1}s {label:<34} Dirty={:>9} kB  Writeback={:>9} kB",
            t0.elapsed().as_secs_f64(),
            meminfo_kib("Dirty")?,
            meminfo_kib("Writeback")?
        );
        Ok(())
    }

    let mut args = std::env::args().skip(1);
    let dir = scratch_dir(args.next())?;
    let size_mib: usize = args.next().map_or(512, |s| s.parse().expect("size_mib"));
    let seconds: u64 = args.next().map_or(40, |s| s.parse().expect("seconds"));
    let path = dir.join("writeback.dat");

    for name in [
        "dirty_background_ratio",
        "dirty_ratio",
        "dirty_background_bytes",
        "dirty_bytes",
        "dirty_expire_centisecs",
        "dirty_writeback_centisecs",
    ] {
        println!("vm.{name} = {}", vm_sysctl(name)?);
    }

    let t0 = Instant::now();
    sample("baseline", t0)?;
    let mut f = File::create(&path)?;
    let chunk = vec![0x5Au8; 1 << 20];
    for _ in 0..size_mib {
        f.write_all(&chunk)?;
    }
    sample(&format!("after write_all x {size_mib} MiB"), t0)?;

    for _ in 0..seconds {
        std::thread::sleep(Duration::from_secs(1));
        sample("waiting (no sync issued)", t0)?;
    }

    let before = Instant::now();
    f.sync_all()?;
    println!("sync_all took {:.3} s", before.elapsed().as_secs_f64());
    sample("after sync_all", t0)?;

    drop(f);
    std::fs::remove_file(&path)?;
    sample("after remove_file", t0)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_storage::need_linux("writeback");
}
