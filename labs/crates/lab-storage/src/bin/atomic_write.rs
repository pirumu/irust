//! Lab 2: naive overwrite vs the safe-write pattern (temp + fsync + rename + fsync dir),
//! with a simulated "crash" (stop) after step N inside the same process.
//!
//! Run: cargo run -q -p lab-storage --bin atomic_write [dir] [naive|safe] [stop_after]
//! Note: stopping the *process* never loses the page cache, so this lab shows what other readers can
//! observe between steps (atomic visibility). It does NOT simulate power loss.

#[cfg(unix)]
mod imp {
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Write};
    use std::path::Path;

    pub const OLD: &[u8] = b"OLD-CONFIG v1\n";
    pub const NEW: &[u8] = b"NEW-CONFIG v2 with a longer body so a half write is visible\n";

    /// `Ok(true)` = ran to the end, `Ok(false)` = stopped early after `stop_after` steps.
    pub fn naive(target: &Path, data: &[u8], stop_after: usize) -> io::Result<bool> {
        let mut step = 0;
        let mut tick = |what: &str| {
            step += 1;
            println!("  step {step}: {what}");
            step >= stop_after && stop_after != 0
        };
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(target)?;
        if tick("open(O_TRUNC): target is now empty") {
            return Ok(false);
        }
        f.write_all(&data[..data.len() / 2])?;
        if tick("write first half") {
            return Ok(false);
        }
        f.write_all(&data[data.len() / 2..])?;
        if tick("write second half") {
            return Ok(false);
        }
        f.sync_all()?;
        tick("sync_all");
        Ok(true)
    }

    pub fn safe(target: &Path, data: &[u8], stop_after: usize) -> io::Result<bool> {
        let mut step = 0;
        let mut tick = |what: &str| {
            step += 1;
            println!("  step {step}: {what}");
            step >= stop_after && stop_after != 0
        };
        let dir = target.parent().expect("target has a parent");
        let tmp = dir.join(format!(
            "{}.tmp.{}",
            target.file_name().expect("file name").to_string_lossy(),
            std::process::id()
        ));
        // create_new: fail if the temp name already exists instead of clobbering it.
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        if tick("create temp file (create_new)") {
            return Ok(false);
        }
        f.write_all(data)?;
        if tick("write all data into temp") {
            return Ok(false);
        }
        f.sync_all()?;
        if tick("sync_all on temp file") {
            return Ok(false);
        }
        drop(f);
        fs::rename(&tmp, target)?;
        if tick("rename(temp -> target)") {
            return Ok(false);
        }
        File::open(dir)?.sync_all()?;
        tick("sync_all on the parent directory");
        Ok(true)
    }

    pub fn report(dir: &Path, target: &Path) -> io::Result<()> {
        let body = fs::read(target)?;
        println!(
            "  target = {} bytes: {:?}",
            body.len(),
            String::from_utf8_lossy(&body)
        );
        let mut names: Vec<String> = fs::read_dir(dir)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<io::Result<_>>()?;
        names.sort();
        println!("  directory entries: {names:?}");
        Ok(())
    }
}

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    use imp::{NEW, OLD, naive, report, safe};
    let mut args = std::env::args().skip(1);
    let dir = lab_storage::scratch_dir(args.next())?;
    let which = args.next().unwrap_or_else(|| "safe".to_owned());
    let stop_after: usize = args.next().map_or(0, |s| s.parse().expect("stop_after"));
    let target = dir.join("config.txt");

    // Setup (not under test): start every run from a known, complete OLD version.
    std::fs::write(&target, OLD)?;
    println!("before: ({which}, stop_after={stop_after})");
    report(&dir, &target)?;

    println!("update with `{which}`:");
    let finished = match which.as_str() {
        "naive" => naive(&target, NEW, stop_after)?,
        "safe" => safe(&target, NEW, stop_after)?,
        other => panic!("unknown mode {other:?}, expected naive|safe"),
    };
    println!(
        "{}",
        if finished {
            "finished all steps"
        } else {
            "STOPPED here (simulated crash)"
        }
    );
    println!("state a reader would see now:");
    report(&dir, &target)?;
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    lab_storage::need_unix("atomic_write");
}
