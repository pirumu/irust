//! Lab 8.4: a miniature container. unshare(NEWNS|NEWPID|NEWUTS) + fork, then in the child
//! pivot_root (the runc sequence) or chroot into a small rootfs, mount /proc, and exec a command.
//!
//! usage: mini_container <rootfs> [--chroot] [--leak-fd] [-- cmd args...]
//!
//! Needs CAP_SYS_ADMIN (run in `docker run --privileged` or as root on a Linux VM).
//! Without a command it prints a short report from inside the new root and exits.

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CStr, CString};
    use std::io::{Error, Result};
    use std::os::raw::{c_char, c_int, c_ulong};
    use std::path::Path;
    use std::{env, fs, process};

    fn check(ret: c_int, what: &str) -> Result<c_int> {
        if ret == -1 {
            let e = Error::last_os_error();
            Err(Error::new(e.kind(), format!("{what}: {e}")))
        } else {
            Ok(ret)
        }
    }

    fn mount(source: Option<&CStr>, target: &CStr, fstype: Option<&CStr>, flags: c_ulong, what: &str) -> Result<()> {
        let ptr = |s: Option<&CStr>| s.map_or(std::ptr::null(), CStr::as_ptr);
        check(unsafe { libc::mount(ptr(source), target.as_ptr(), ptr(fstype), flags, std::ptr::null()) }, what).map(|_| ())
    }

    fn open_dir(path: &CStr) -> Result<c_int> {
        check(unsafe { libc::open(path.as_ptr(), libc::O_DIRECTORY | libc::O_RDONLY) }, "open directory")
    }

    /// chroot(2): only changes where "/" points for this process; the mount table is untouched.
    fn enter_chroot(rootfs: &CStr) -> Result<()> {
        check(unsafe { libc::chroot(rootfs.as_ptr()) }, "chroot")?;
        check(unsafe { libc::chdir(c"/".as_ptr()) }, "chdir(/)")?;
        Ok(())
    }

    /// The runc sequence (docs/08-containers/04-filesystem-cua-container.md, section 1).
    fn enter_pivot_root(rootfs: &CStr) -> Result<()> {
        // 1. Stop mount events from propagating to the host, then make rootfs a mount point.
        mount(None, c"/", None, libc::MS_SLAVE | libc::MS_REC, "mount(/, MS_SLAVE|MS_REC)")?;
        mount(Some(rootfs), rootfs, None, libc::MS_BIND | libc::MS_REC, "bind rootfs onto itself")?;
        // 2. pivot_root(".", ".") stacks the old root on top of the new one.
        let old_root = open_dir(c"/")?;
        let new_root = open_dir(rootfs)?;
        check(unsafe { libc::fchdir(new_root) }, "fchdir(new_root)")?;
        check(unsafe { libc::syscall(libc::SYS_pivot_root, c".".as_ptr(), c".".as_ptr()) } as c_int, "pivot_root(., .)")?;
        // 3. Detach the old root so the host tree is no longer in this namespace's mount table.
        check(unsafe { libc::fchdir(old_root) }, "fchdir(old_root)")?;
        mount(None, c".", None, libc::MS_SLAVE | libc::MS_REC, "mount(., MS_SLAVE|MS_REC)")?;
        check(unsafe { libc::umount2(c".".as_ptr(), libc::MNT_DETACH) }, "umount2(., MNT_DETACH)")?;
        check(unsafe { libc::chdir(c"/".as_ptr()) }, "chdir(/)")?;
        unsafe {
            libc::close(old_root);
            libc::close(new_root);
        }
        Ok(())
    }

    fn list(dir: &str) -> String {
        match fs::read_dir(dir) {
            Ok(rd) => {
                let mut names: Vec<String> = rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                names.sort();
                names.join(" ")
            }
            Err(e) => format!("<{e}>"),
        }
    }

    fn child(rootfs: &CString, use_chroot: bool, leak_fd: Option<c_int>, command: &[String]) -> Result<()> {
        check(unsafe { libc::sethostname(c"mini".as_ptr(), 4) }, "sethostname")?;
        if use_chroot { enter_chroot(rootfs)? } else { enter_pivot_root(rootfs)? }

        // A fresh procfs, mounted from inside the new PID namespace, shows only this namespace's PIDs.
        if Path::new("/proc").is_dir() {
            mount(Some(c"proc"), c"/proc", Some(c"proc"), libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC, "mount proc")?;
        }

        if command.is_empty() {
            println!("[mini_container] pid = {}, ppid = {}", process::id(), unsafe { libc::getppid() });
            println!("[mini_container] ls / = {}", list("/"));
            if let Ok(mi) = fs::read_to_string("/proc/self/mountinfo") {
                println!("[mini_container] /proc/self/mountinfo has {} line(s):", mi.lines().count());
                for line in mi.lines().take(6) {
                    println!("[mini_container]   {line}");
                }
            }
            if let Some(fd) = leak_fd {
                // An fd opened before the root change still points outside it.
                check(unsafe { libc::fchdir(fd) }, "fchdir(leaked fd)")?;
                println!("[mini_container] after fchdir(leaked fd), ls . = {}", list("."));
            }
            return Ok(());
        }

        let args: Vec<CString> = command.iter().map(|a| CString::new(a.as_str()).expect("argument contains NUL")).collect();
        let mut argv: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        unsafe { libc::execvp(argv[0], argv.as_ptr()) };
        Err(Error::new(Error::last_os_error().kind(), format!("execvp {}: {}", command[0], Error::last_os_error())))
    }

    pub fn run() -> Result<i32> {
        let mut args = env::args().skip(1);
        let Some(rootfs) = args.next() else {
            eprintln!("usage: mini_container <rootfs> [--chroot] [--leak-fd] [-- cmd args...]");
            return Ok(2);
        };
        let rest: Vec<String> = args.collect();
        let (opts, command) = match rest.iter().position(|a| a == "--") {
            Some(i) => (&rest[..i], rest[i + 1..].to_vec()),
            None => (&rest[..], Vec::new()),
        };
        let use_chroot = opts.iter().any(|a| a == "--chroot");
        let want_leak = opts.iter().any(|a| a == "--leak-fd");

        let rootfs = fs::canonicalize(&rootfs)?;
        let rootfs = CString::new(rootfs.to_string_lossy().as_bytes()).expect("path contains NUL");
        let leak_fd = if want_leak { Some(open_dir(c"/")?) } else { None };

        println!("[mini_container] parent pid = {}, rootfs = {:?}, method = {}", process::id(), rootfs, if use_chroot { "chroot" } else { "pivot_root" });
        // Same flag set as `unshare --mount --pid --uts --fork`.
        if let Err(e) = check(unsafe { libc::unshare(libc::CLONE_NEWNS | libc::CLONE_NEWPID | libc::CLONE_NEWUTS) }, "unshare") {
            eprintln!("[mini_container] {e}\n[mini_container] needs CAP_SYS_ADMIN (docker run --privileged, or root on a Linux VM)");
            return Ok(1);
        }
        let pid = check(unsafe { libc::fork() }, "fork")?;
        if pid == 0 {
            // The first child after unshare(CLONE_NEWPID) is PID 1 of the new namespace.
            let code = match child(&rootfs, use_chroot, leak_fd, &command) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("[mini_container] child failed: {e}");
                    125
                }
            };
            process::exit(code);
        }
        let mut status = 0;
        check(unsafe { libc::waitpid(pid, &mut status, 0) }, "waitpid")?;
        Ok(if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else if libc::WIFSIGNALED(status) {
            128 + libc::WTERMSIG(status)
        } else {
            1
        })
    }
}

#[cfg(target_os = "linux")]
fn main() {
    match linux::run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("[mini_container] error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_containers::not_linux("mini_container");
}
