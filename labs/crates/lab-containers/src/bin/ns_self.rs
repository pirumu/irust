//! Lab 8.1: which namespaces does this process live in? Read /proc/self/ns and compare with PID 1.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    let mut names: Vec<String> = fs::read_dir("/proc/self/ns")?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    names.sort();

    println!("[ns_self] pid = {}", std::process::id());
    println!("{:<20} {:<24} {:>12} {:>9}  same as PID 1?", "namespace", "readlink", "st_ino", "st_dev");
    for name in &names {
        let own = format!("/proc/self/ns/{name}");
        let link = fs::read_link(&own).map(|p| p.display().to_string()).unwrap_or_else(|e| format!("<{e}>"));
        // `fs::metadata` follows the magic symlink, so st_dev/st_ino identify the namespace itself.
        let meta = fs::metadata(&own)?;
        let verdict = match fs::metadata(format!("/proc/1/ns/{name}")) {
            Ok(init) if (init.dev(), init.ino()) == (meta.dev(), meta.ino()) => "yes",
            Ok(_) => "NO",
            Err(_) => "? (cannot stat)",
        };
        println!("{name:<20} {link:<24} {:>12} {:>9}  {verdict}", meta.ino(), meta.dev());
    }

    // The two files that decide how much of a container's identity is "virtual".
    for file in ["uid_map", "gid_map"] {
        let content = fs::read_to_string(format!("/proc/self/{file}"))?;
        println!("[ns_self] /proc/self/{file}: {}", content.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_containers::not_linux("ns_self");
}
