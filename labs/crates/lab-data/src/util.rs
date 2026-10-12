//! Small helpers shared by the labs: scratch directories, directory fsync, hex dumps.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A scratch directory that is removed again when the value is dropped. Unit tests use it so that
/// parallel tests never share files.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates `<tmp>/lab-data-<tag>-<pid>-<n>` (fresh and empty).
    pub fn new(tag: &str) -> io::Result<TempDir> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lab-data-{tag}-{}-{n}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(TempDir { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Scratch directory for a lab binary: `arg` if given, otherwise `<tmp>/lab-data-<tag>-<pid>`.
/// The directory is created fresh (an existing one is emptied first).
pub fn scratch_dir(arg: Option<String>, tag: &str) -> io::Result<PathBuf> {
    let dir = match arg {
        Some(p) => PathBuf::from(p),
        None => std::env::temp_dir().join(format!("lab-data-{tag}-{}", std::process::id())),
    };
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// `fsync` on a directory, so that a `rename` or a created/removed file inside it is durable
/// (a file's own `fsync` does not cover its directory entry; see P5 chapter 06).
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// `08 96 01`-style hex dump.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 1234567 -> "1,234,567".
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Printed by the bins whose code needs `std::os::unix` when built for another OS.
pub fn need_unix(bin: &str) -> ! {
    eprintln!("`{bin}` dùng std::os::unix::fs::FileExt (read_at/write_at) nên chỉ build trên Unix.");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_thousands() {
        assert_eq!(hex(&[0x08, 0x96, 0x01]), "08 96 01");
        assert_eq!(hex(&[]), "");
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn temp_dirs_are_unique_and_cleaned_up() {
        let (a, b);
        let pa;
        {
            let t1 = TempDir::new("util").unwrap();
            let t2 = TempDir::new("util").unwrap();
            a = t1.path().to_path_buf();
            b = t2.path().to_path_buf();
            assert_ne!(a, b);
            assert!(a.is_dir() && b.is_dir());
            pa = a.clone();
            sync_dir(t1.path()).unwrap();
        }
        assert!(!pa.exists() && !b.exists());
    }
}
