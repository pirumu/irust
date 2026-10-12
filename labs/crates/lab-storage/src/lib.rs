//! Shared helpers for the P5 storage labs.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

/// Directory for a lab's scratch files: `arg` if given, else `<tmp>/lab-storage-<pid>`. Created if missing.
pub fn scratch_dir(arg: Option<String>) -> io::Result<PathBuf> {
    let dir = match arg {
        Some(p) => PathBuf::from(p),
        None => std::env::temp_dir().join(format!("lab-storage-{}", std::process::id())),
    };
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Message printed by the Linux-only bins when built for another OS.
pub fn need_linux(bin: &str) -> ! {
    eprintln!(
        "`{bin}` dùng syscall chỉ có trên Linux (O_DIRECT / posix_fadvise / mincore / /proc)."
    );
    eprintln!("Chạy trong container Linux của repo:");
    eprintln!("  ./labs/linux.sh cargo run -q -p lab-storage --bin {bin}");
    std::process::exit(2);
}

/// Message printed by the Unix-only bins when built for a non-Unix OS.
pub fn need_unix(bin: &str) -> ! {
    eprintln!("`{bin}` dùng std::os::unix (MetadataExt/DirEntryExt) nên chỉ build trên Unix.");
    std::process::exit(2);
}

/// Human-readable duration in milliseconds with microsecond resolution.
pub fn ms(d: Duration) -> String {
    format!("{:.3} ms", d.as_secs_f64() * 1e3)
}

/// Value (kB) of a `/proc/meminfo` key such as `Dirty` or `Writeback`; see proc(5).
#[cfg(target_os = "linux")]
pub fn meminfo_kib(key: &str) -> io::Result<u64> {
    let text = fs::read_to_string("/proc/meminfo")?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) else {
            continue;
        };
        return rest
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .map_err(io::Error::other);
    }
    Err(io::Error::other(format!(
        "{key} not found in /proc/meminfo"
    )))
}

/// Raw content (trimmed) of `/proc/sys/vm/<name>`.
#[cfg(target_os = "linux")]
pub fn vm_sysctl(name: &str) -> io::Result<String> {
    Ok(fs::read_to_string(format!("/proc/sys/vm/{name}"))?
        .trim()
        .to_owned())
}

/// Page size in bytes (`sysconf(_SC_PAGESIZE)`).
#[cfg(target_os = "linux")]
pub fn page_size() -> usize {
    // SAFETY: sysconf has no preconditions.
    let n = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(n).expect("sysconf(_SC_PAGESIZE) failed")
}

/// How many pages of `file` are resident in the page cache, via `mmap(MAP_SHARED)` + `mincore(2)`.
/// Returns `(resident_pages, total_pages)`. The mapping itself does not read the file.
#[cfg(target_os = "linux")]
pub fn resident_pages(file: &fs::File) -> io::Result<(usize, usize)> {
    use std::os::fd::AsRawFd;

    let len = usize::try_from(file.metadata()?.len()).map_err(io::Error::other)?;
    if len == 0 {
        return Ok((0, 0));
    }
    let page = page_size();
    let pages = len.div_ceil(page);
    // SAFETY: a fresh read-only shared mapping of an open fd; no address hint.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        return Err(io::Error::last_os_error());
    }
    let mut vec = vec![0u8; pages];
    // SAFETY: `ptr` maps `len` bytes; `vec` has one byte per page as mincore(2) requires.
    let rc = unsafe { libc::mincore(ptr, len, vec.as_mut_ptr()) };
    let mincore_err = (rc != 0).then(io::Error::last_os_error);
    // SAFETY: unmapping exactly the mapping created above; nothing references it afterwards.
    unsafe { libc::munmap(ptr, len) };
    if let Some(e) = mincore_err {
        return Err(e);
    }
    Ok((vec.iter().filter(|b| **b & 1 == 1).count(), pages))
}

/// `posix_fadvise(2)` on the whole file. Unlike most syscalls it returns the error number directly.
#[cfg(target_os = "linux")]
pub fn fadvise(file: &fs::File, advice: libc::c_int) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: valid fd; offset 0 / len 0 means "to the end of the file".
    let rc = unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, advice) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(rc))
    }
}

/// Heap buffer whose address and length are aligned to `align` bytes (needed for `O_DIRECT`).
#[cfg(target_os = "linux")]
pub struct AlignedBuf {
    ptr: std::ptr::NonNull<u8>,
    layout: std::alloc::Layout,
}

#[cfg(target_os = "linux")]
impl AlignedBuf {
    pub fn new(len: usize, align: usize) -> Self {
        let layout = std::alloc::Layout::from_size_align(len, align).expect("bad layout");
        assert!(layout.size() > 0);
        // SAFETY: layout has non-zero size.
        let raw = unsafe { std::alloc::alloc_zeroed(layout) };
        let ptr =
            std::ptr::NonNull::new(raw).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        Self { ptr, layout }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: `ptr` points to `layout.size()` initialised (zeroed) bytes owned by `self`.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.layout.size()) }
    }
}

#[cfg(target_os = "linux")]
impl Drop for AlignedBuf {
    fn drop(&mut self) {
        // SAFETY: allocated in `new` with exactly this layout.
        unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) }
    }
}
