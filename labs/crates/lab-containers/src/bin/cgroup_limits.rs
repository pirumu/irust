//! Lab 8.3: read the cgroup v2 limits this process lives under and compare them with
//! `available_parallelism` and `nproc`-style CPU counts.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::fs;
    use std::path::PathBuf;

    const MOUNT: &str = "/sys/fs/cgroup";

    fn read(path: &std::path::Path) -> String {
        match fs::read_to_string(path) {
            Ok(s) => s.trim().to_string(),
            Err(e) => format!("<{e}>"),
        }
    }

    // cgroup v2 shows as a single line "0::<path>" in /proc/self/cgroup.
    let proc_cgroup = fs::read_to_string("/proc/self/cgroup")?;
    println!("[cgroup_limits] /proc/self/cgroup: {}", proc_cgroup.trim().replace('\n', " | "));
    let rel = proc_cgroup
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .unwrap_or("/");
    let dir = PathBuf::from(MOUNT).join(rel.trim_start_matches('/'));
    println!("[cgroup_limits] cgroup dir = {}", dir.display());

    for file in ["cpu.max", "cpu.stat", "memory.max", "memory.high", "memory.current", "memory.events", "pids.max"] {
        let content = read(&dir.join(file));
        println!("[cgroup_limits] {file}: {}", content.replace('\n', " | "));
    }

    // Derive what the quota means in "cores", the way the docs of Rust's std describe it:
    // integer division, rounded down (std additionally clamps the result to at least 1).
    let cpu_max = read(&dir.join("cpu.max"));
    let mut fields = cpu_max.split_whitespace();
    let quota_cores = match (fields.next(), fields.next()) {
        (Some("max"), _) => None,
        (Some(q), Some(p)) => q.parse::<u64>().ok().zip(p.parse::<u64>().ok()).filter(|&(_, p)| p > 0).map(|(q, p)| q / p),
        _ => None,
    };
    match quota_cores {
        Some(c) => println!("[cgroup_limits] quota/period (integer division) = {c} core(s)"),
        None => println!("[cgroup_limits] no CPU quota parsed from cpu.max"),
    }

    // Parse memory.max too: "max" or a byte count.
    match read(&dir.join("memory.max")).as_str() {
        "max" => println!("[cgroup_limits] memory.max = unlimited"),
        s => match s.parse::<u64>() {
            Ok(b) => println!("[cgroup_limits] memory.max = {b} bytes = {} MiB", b / (1024 * 1024)),
            Err(_) => println!("[cgroup_limits] memory.max not readable: {s}"),
        },
    }

    println!("[cgroup_limits] available_parallelism() = {:?}", std::thread::available_parallelism());
    // The host-wide counts that ignore the quota: sysconf online CPUs.
    let online = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) };
    println!("[cgroup_limits] sysconf(_SC_NPROCESSORS_ONLN) = {online}");
    // The affinity mask (what `nproc` counts).
    let mut set: libc::cpu_set_t = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::sched_getaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &mut set) };
    if rc == 0 {
        let n = (0..libc::CPU_SETSIZE as usize).filter(|&i| unsafe { libc::CPU_ISSET(i, &set) }).count();
        println!("[cgroup_limits] CPUs in sched_getaffinity mask = {n}");
    } else {
        println!("[cgroup_limits] sched_getaffinity failed: {}", std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_containers::not_linux("cgroup_limits");
}
