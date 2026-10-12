//! Lab (b): look at this process through /proc/self: its fd table, status and memory maps.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::fs;
    use std::net::TcpListener;

    // Hold a few different kinds of "files" open so the fd table has something to show.
    let (pipe_reader, pipe_writer) = std::io::pipe()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let file = fs::File::open("/etc/hostname")?;

    println!("[procfs] std::process::id() = {}", std::process::id());
    println!("[procfs] /proc/self/fd:");
    // read_dir itself opens /proc/self/fd, so that directory fd shows up in its own listing.
    let mut entries: Vec<(u32, String)> = Vec::new();
    for entry in fs::read_dir("/proc/self/fd")? {
        let entry = entry?;
        let fd: u32 = entry.file_name().to_string_lossy().parse().unwrap_or(u32::MAX);
        let target = match fs::read_link(entry.path()) {
            Ok(t) => t.display().to_string(),
            Err(e) => format!("<{e}>"),
        };
        entries.push((fd, target));
    }
    entries.sort();
    for (fd, target) in &entries {
        println!("  {fd:>3} -> {target}");
    }

    println!("[procfs] /proc/self/status (selected fields):");
    let status = fs::read_to_string("/proc/self/status")?;
    const FIELDS: [&str; 9] = [
        "Name:", "State:", "Pid:", "PPid:", "Uid:", "Gid:", "FDSize:", "VmRSS:", "Threads:",
    ];
    for line in status.lines().filter(|l| FIELDS.iter().any(|f| l.starts_with(f))) {
        println!("  {line}");
    }

    println!("[procfs] /proc/self/maps (binary, [heap], [stack], [vdso]):");
    let exe = fs::read_link("/proc/self/exe")?;
    let exe = exe.to_string_lossy();
    let maps = fs::read_to_string("/proc/self/maps")?;
    let mut shown_exe = false;
    for line in maps.lines() {
        let is_exe = line.ends_with(exe.as_ref());
        let is_special = ["[heap]", "[stack]", "[vdso]"].iter().any(|s| line.ends_with(s));
        if (is_exe && !shown_exe) || is_special {
            println!("  {line}");
            shown_exe |= is_exe;
        }
    }

    drop((pipe_reader, pipe_writer, listener, file));
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_linux::not_linux("procfs");
}
