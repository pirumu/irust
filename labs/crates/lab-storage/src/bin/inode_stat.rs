//! Lab 5a: inode number, link count, hard link vs symlink, rename, unlink-while-open.
//!
//! Run: cargo run -q -p lab-storage --bin inode_stat [dir] [other_fs_dir]
//! Linux: ./labs/linux.sh cargo run -q -p lab-storage --bin inode_stat /work/labs/target-linux/inode_lab /dev/shm
//! `other_fs_dir` (optional) should live on a *different* filesystem, to try a cross-device hard link.

#[cfg(unix)]
fn main() -> std::io::Result<()> {
    use std::fs::{self, File, Metadata};
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, symlink};
    use std::path::Path;

    fn show(label: &str, m: &Metadata) {
        println!(
            "  {label:<34} dev={} ino={} nlink={} len={} blocks512={} symlink={}",
            m.dev(),
            m.ino(),
            m.nlink(),
            m.len(),
            m.blocks(),
            m.file_type().is_symlink()
        );
    }
    fn stat(label: &str, p: &Path) -> std::io::Result<()> {
        show(label, &fs::metadata(p)?);
        Ok(())
    }

    let mut args = std::env::args().skip(1);
    let dir = lab_storage::scratch_dir(args.next())?;
    let other = args.next();
    let a = dir.join("a.txt");
    let b = dir.join("b.txt");
    let c = dir.join("c.txt");
    let s = dir.join("s.lnk");
    for p in [&a, &b, &c, &s] {
        let _ = fs::remove_file(p);
    }

    println!("1. create a.txt");
    fs::write(&a, b"hello inode\n")?;
    stat("a.txt", &a)?;

    println!("2. hard_link(a.txt, b.txt)");
    fs::hard_link(&a, &b)?;
    stat("a.txt", &a)?;
    stat("b.txt", &b)?;

    println!("3. symlink(a.txt, s.lnk): metadata (follows) vs symlink_metadata (lstat)");
    symlink(&a, &s)?;
    stat("fs::metadata(s.lnk)", &s)?;
    show("fs::symlink_metadata(s.lnk)", &fs::symlink_metadata(&s)?);
    println!("  a.txt path length = {} bytes", a.as_os_str().len());

    println!("4. rename(b.txt, c.txt)");
    fs::rename(&b, &c)?;
    stat("a.txt", &a)?;
    stat("c.txt", &c)?;

    println!("5. remove_file(a.txt): the symlink now dangles, c.txt keeps the data");
    fs::remove_file(&a)?;
    stat("c.txt", &c)?;
    println!(
        "  fs::metadata(s.lnk) = {:?}",
        fs::metadata(&s).map(|_| ()).map_err(|e| e.kind())
    );

    println!("6. open c.txt, then remove_file(c.txt) while the fd is open");
    let mut f = File::open(&c)?;
    fs::remove_file(&c)?;
    show("File::metadata (via fd)", &f.metadata()?);
    let mut text = String::new();
    f.read_to_string(&mut text)?;
    println!("  still readable through the fd: {text:?}");
    println!("  path exists? {}", c.exists());

    println!("7. hard_link to a directory");
    match fs::hard_link(&dir, dir.join("dirlink")) {
        Ok(()) => println!("  unexpectedly succeeded"),
        Err(e) => println!("  Err({e}) raw={:?}", e.raw_os_error()),
    }

    if let Some(other) = other {
        println!("8. hard_link across filesystems: {other}");
        fs::write(&a, b"x")?;
        let target = Path::new(&other).join("lab-storage-xdev.lnk");
        match fs::hard_link(&a, &target) {
            Ok(()) => {
                println!("  succeeded (same filesystem?)");
                fs::remove_file(&target)?;
            }
            Err(e) => println!("  Err({e}) raw={:?}", e.raw_os_error()),
        }
        fs::remove_file(&a)?;
    }
    let _ = fs::remove_file(&s);
    Ok(())
}

#[cfg(not(unix))]
fn main() {
    lab_storage::need_unix("inode_stat");
}
