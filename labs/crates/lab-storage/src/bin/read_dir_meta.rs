//! Lab 5b: `read_dir` order, d_ino, `DirEntry::file_type` vs `metadata`, and symlinks.
//!
//! Run: cargo run -q -p lab-storage --bin read_dir_meta [dir]
//! Linux: ./labs/linux.sh cargo run -q -p lab-storage --bin read_dir_meta

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{DirEntryExt, MetadataExt, symlink};

    let dir = lab_storage::scratch_dir(std::env::args().nth(1))?;
    let work = dir.join("read_dir_lab");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir(&work)?;

    // Creation order is deliberately not alphabetical.
    let created = [
        "zeta.txt",
        "alpha.txt",
        "mid.txt",
        "beta.txt",
        "omega.txt",
        "a1.txt",
        "k.txt",
    ];
    for name in created {
        fs::write(work.join(name), name.as_bytes())?;
    }
    fs::create_dir(work.join("subdir"))?;
    symlink(work.join("alpha.txt"), work.join("link-to-alpha"))?;
    symlink(work.join("does-not-exist"), work.join("dangling"))?;
    println!("created (in this order): {created:?} + subdir + link-to-alpha + dangling");

    for pass in 1..=2 {
        let names: Vec<String> = fs::read_dir(&work)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<io::Result<_>>()?;
        println!("read_dir pass {pass}: {names:?}");
    }

    println!(
        "\n{:<14} {:>8} {:<22} {:<22}",
        "name", "d_ino", "entry.file_type()", "entry.metadata().len"
    );
    let mut entries: Vec<_> = fs::read_dir(&work)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in &entries {
        let ft = e.file_type()?;
        let kind = if ft.is_symlink() {
            "symlink"
        } else if ft.is_dir() {
            "dir"
        } else {
            "file"
        };
        let md = e.metadata()?;
        println!(
            "{:<14} {:>8} {:<22} {:<22}",
            e.file_name().to_string_lossy(),
            e.ino(),
            kind,
            format!("{} (md.ino={})", md.len(), md.ino())
        );
    }

    println!("\nfs::metadata (follows symlinks) vs entry.metadata (does not):");
    for name in ["link-to-alpha", "dangling"] {
        let p = work.join(name);
        let followed = fs::metadata(&p)
            .map(|m| (m.ino(), m.len()))
            .map_err(|e| e.kind());
        println!("  fs::metadata({name}) = {followed:?}");
    }
    println!(
        "  contains \".\" or \"..\"? {}",
        entries
            .iter()
            .any(|e| e.file_name() == "." || e.file_name() == "..")
    );

    fs::remove_dir_all(&work)?;
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    lab_storage::need_unix("read_dir_meta");
}
