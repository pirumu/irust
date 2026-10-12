//! Lab (c): anonymous mmap, then touch it page by page and count minor page faults.
//!
//! Run on Linux: ./labs/linux.sh cargo run -p lab-memory --bin page_faults

#[cfg(target_os = "linux")]
fn main() {
    use lab_memory::{minflt_from_proc, rusage_faults};

    const LEN: usize = 64 << 20; // 64 MiB

    // SAFETY: sysconf has no preconditions.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    let pages = LEN / page;
    println!("page size = {page} B, mapping {LEN} B = {pages} pages");

    let report = |label: &str| {
        let (minflt, majflt) = rusage_faults().expect("getrusage");
        let proc_minflt = minflt_from_proc().expect("/proc/self/stat");
        println!("{label:<34} ru_minflt={minflt:>7} ru_majflt={majflt} /proc minflt={proc_minflt:>7}");
        minflt
    };

    let start = report("before mmap");

    // SAFETY: an anonymous private mapping with no address hint touches no existing memory.
    let ptr = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            LEN,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    if ptr == libc::MAP_FAILED {
        eprintln!("mmap failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
    let base = ptr.cast::<u8>();
    let after_mmap = report("after mmap (nothing touched)");

    // SAFETY: base..base+LEN is our readable mapping; reads of anonymous memory see zeroes.
    let first = unsafe { base.read_volatile() };
    let after_read = report("after reading the first byte");
    println!("first byte reads as {first} (anonymous memory is zero-filled)");

    for i in 0..pages {
        // SAFETY: i * page < LEN, inside our writable mapping.
        unsafe { base.add(i * page).write_volatile(1) };
    }
    let after_first_pass = report("after writing 1 byte per page");

    for i in 0..pages {
        // SAFETY: as above.
        unsafe { base.add(i * page).write_volatile(2) };
    }
    let after_second_pass = report("after a second pass");

    println!();
    println!("faults during mmap itself         : {}", after_mmap - start);
    println!("faults during the 1-byte read     : {}", after_read - after_mmap);
    println!(
        "faults during first write pass    : {} (pages = {pages})",
        after_first_pass - after_read
    );
    println!(
        "faults during second write pass   : {}",
        after_second_pass - after_first_pass
    );

    // SAFETY: unmapping exactly the region mmap returned.
    if unsafe { libc::munmap(ptr, LEN) } != 0 {
        eprintln!("munmap failed: {}", std::io::Error::last_os_error());
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_memory::linux_only("page_faults");
}
