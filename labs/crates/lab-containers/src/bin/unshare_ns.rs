//! Lab 8.2: `unshare(2)` by hand with libc — UTS, PID or mount namespace.
//!
//! usage: unshare_ns [uts|pid|mount] [--userns] [--hold SECONDS]
//!
//! Creating non-user namespaces needs CAP_SYS_ADMIN. `--userns` adds CLONE_NEWUSER to the same
//! call so an unprivileged caller can still do it (docs/labs/08-containers-lab.md, lab 8.2).

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CString;
    use std::io::{Error, ErrorKind, Result};
    use std::{env, fs, process, thread, time::Duration};

    /// C convention "-1 means look at errno" -> io::Result, keeping the call name in the message.
    fn check(ret: libc::c_int, what: &str) -> Result<libc::c_int> {
        if ret == -1 {
            let e = Error::last_os_error();
            Err(Error::new(e.kind(), format!("{what}: {e}")))
        } else {
            Ok(ret)
        }
    }

    fn ns_link(name: &str) -> String {
        fs::read_link(format!("/proc/self/ns/{name}")).map(|p| p.display().to_string()).unwrap_or_else(|e| format!("<{e}>"))
    }

    fn hostname() -> Result<String> {
        let mut buf = [0u8; 256];
        check(unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) }, "gethostname")?;
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Ok(String::from_utf8_lossy(&buf[..len]).into_owned())
    }

    /// Give the fresh user namespace a uid/gid map so ids are not the overflow id 65534.
    /// An unprivileged process may write exactly one line mapping its own id, and must write
    /// "deny" to setgroups before gid_map.
    fn write_id_maps(uid: libc::uid_t, gid: libc::gid_t) -> Result<()> {
        fs::write("/proc/self/setgroups", "deny")?;
        fs::write("/proc/self/uid_map", format!("0 {uid} 1"))?;
        fs::write("/proc/self/gid_map", format!("0 {gid} 1"))?;
        Ok(())
    }

    fn hold(secs: u64) {
        if secs > 0 {
            println!("[unshare_ns] holding the namespace for {secs}s; pid {} (look at it from another shell)", process::id());
            thread::sleep(Duration::from_secs(secs));
        }
    }

    fn demo_uts(hold_secs: u64) -> Result<()> {
        println!("[unshare_ns] uts namespace now: {}, hostname = {:?}", ns_link("uts"), hostname()?);
        let new_name = "lab-ns-demo";
        let c = CString::new(new_name).expect("no NUL in literal");
        check(unsafe { libc::sethostname(c.as_ptr(), new_name.len()) }, "sethostname")?;
        println!("[unshare_ns] after sethostname: hostname = {:?}", hostname()?);
        hold(hold_secs);
        Ok(())
    }

    fn demo_pid(hold_secs: u64) -> Result<()> {
        // unshare(CLONE_NEWPID) does NOT move the caller; only its future children.
        println!("[unshare_ns] caller after unshare: getpid() = {}, pid ns = {}, pid_for_children = {}", process::id(), ns_link("pid"), ns_link("pid_for_children"));
        let pid = check(unsafe { libc::fork() }, "fork")?;
        if pid == 0 {
            // Child: first process created in the new PID namespace.
            println!("[unshare_ns] child: getpid() = {}, getppid() = {}, pid ns = {}", process::id(), unsafe { libc::getppid() }, ns_link("pid"));
            // /proc is still the procfs mounted in the OLD namespace, so `self` resolves in that view.
            println!("[unshare_ns] child: readlink /proc/self = {:?}", fs::read_link("/proc/self").map(|p| p.display().to_string()));
            hold(hold_secs);
            process::exit(0);
        }
        println!("[unshare_ns] parent: forked child pid {pid} (as seen from the old namespace)");
        let mut status = 0;
        check(unsafe { libc::waitpid(pid, &mut status, 0) }, "waitpid")?;
        println!("[unshare_ns] parent: child exited, WIFEXITED = {}, WEXITSTATUS = {}", libc::WIFEXITED(status), libc::WEXITSTATUS(status));
        Ok(())
    }

    fn demo_mount(hold_secs: u64) -> Result<()> {
        // The new namespace starts as a copy of our mount list. Make "/" private first so the
        // tmpfs below cannot propagate back to peers in the original namespace.
        check(unsafe { libc::mount(std::ptr::null(), c"/".as_ptr(), std::ptr::null(), libc::MS_REC | libc::MS_PRIVATE, std::ptr::null()) }, "mount(/, MS_REC|MS_PRIVATE)")?;
        let dir = "/tmp/lab-8-2-mnt";
        fs::create_dir_all(dir)?;
        let target = CString::new(dir).expect("no NUL in literal");
        check(unsafe { libc::mount(c"tmpfs".as_ptr(), target.as_ptr(), c"tmpfs".as_ptr(), 0, std::ptr::null()) }, "mount(tmpfs)")?;
        fs::write(format!("{dir}/inside.txt"), "visible only in this mount namespace\n")?;
        println!("[unshare_ns] mnt namespace now: {}", ns_link("mnt"));
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        for line in mountinfo.lines().filter(|l| l.contains(dir)) {
            println!("[unshare_ns] mountinfo: {line}");
        }
        hold(hold_secs);
        Ok(())
    }

    pub fn run() -> Result<()> {
        let args: Vec<String> = env::args().skip(1).collect();
        let userns = args.iter().any(|a| a == "--userns");
        let hold_secs = args.iter().position(|a| a == "--hold").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(0);
        let mode = args.iter().find(|a| matches!(a.as_str(), "uts" | "pid" | "mount")).map_or("uts", String::as_str);
        let mut flags = match mode {
            "pid" => libc::CLONE_NEWPID,
            "mount" => libc::CLONE_NEWNS,
            _ => libc::CLONE_NEWUTS,
        };
        if userns {
            flags |= libc::CLONE_NEWUSER;
        }
        let (uid, gid) = unsafe { (libc::geteuid(), libc::getegid()) };

        println!("[unshare_ns] mode = {mode}, userns = {userns}, euid = {uid}");
        println!("[unshare_ns] before: uts = {}, pid = {}, mnt = {}, user = {}", ns_link("uts"), ns_link("pid"), ns_link("mnt"), ns_link("user"));
        if let Err(e) = check(unsafe { libc::unshare(flags) }, "unshare") {
            if e.kind() == ErrorKind::PermissionDenied {
                println!("[unshare_ns] {e}");
                println!("[unshare_ns] this namespace needs CAP_SYS_ADMIN; rerun with --userns or in a container with that capability");
                process::exit(1);
            }
            return Err(e);
        }
        if userns {
            write_id_maps(uid, gid)?;
            println!("[unshare_ns] user namespace created: user = {}, getuid() = {}", ns_link("user"), unsafe { libc::getuid() });
        }
        match mode {
            "pid" => demo_pid(hold_secs),
            "mount" => demo_mount(hold_secs),
            _ => demo_uts(hold_secs),
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_containers::not_linux("unshare_ns");
}
