//! Shared helpers for the P3 memory labs.

/// One line of `/proc/self/maps`, with its parsed address range.
#[cfg(target_os = "linux")]
pub struct MapsEntry {
    pub start: usize,
    pub end: usize,
    pub line: String,
}

/// Reads `/proc/self/maps` (format in proc(5): `address perms offset dev inode pathname`).
#[cfg(target_os = "linux")]
pub fn read_maps() -> std::io::Result<Vec<MapsEntry>> {
    let text = std::fs::read_to_string("/proc/self/maps")?;
    let mut entries = Vec::new();
    for line in text.lines() {
        let range = line.split_whitespace().next().unwrap_or_default();
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start, 16),
            usize::from_str_radix(end, 16),
        ) else {
            continue;
        };
        entries.push(MapsEntry {
            start,
            end,
            line: line.to_owned(),
        });
    }
    Ok(entries)
}

/// The mapping that contains `addr`, if any.
#[cfg(target_os = "linux")]
pub fn region_of(maps: &[MapsEntry], addr: usize) -> Option<&MapsEntry> {
    maps.iter().find(|e| e.start <= addr && addr < e.end)
}

/// Field 10 (`minflt`) of `/proc/self/stat`, see proc(5).
#[cfg(target_os = "linux")]
pub fn minflt_from_proc() -> std::io::Result<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    // Field 2 (comm) is in parentheses and may contain spaces, so count fields after the last ')'.
    let after_comm = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest)
        .ok_or_else(|| std::io::Error::other("unexpected /proc/self/stat format"))?;
    // after_comm starts at field 3 (state); minflt is field 10.
    after_comm
        .split_whitespace()
        .nth(10 - 3)
        .ok_or_else(|| std::io::Error::other("minflt field missing"))?
        .parse()
        .map_err(std::io::Error::other)
}

/// `ru_minflt` and `ru_majflt` of the calling process from getrusage(2).
#[cfg(unix)]
pub fn rusage_faults() -> std::io::Result<(libc::c_long, libc::c_long)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `usage` points to writable memory of the right type; getrusage fills it on success.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: getrusage returned 0, so the struct is initialised.
    let usage = unsafe { usage.assume_init() };
    Ok((usage.ru_minflt, usage.ru_majflt))
}

/// Message printed by the non-Linux branch of Linux-only labs.
pub fn linux_only(bin: &str) {
    eprintln!(
        "Lab `{bin}` cần Linux (/proc, mmap trên Linux). Chạy trong container:\n  ./labs/linux.sh cargo run -p lab-memory --bin {bin}"
    );
}
