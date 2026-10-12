//! Lab 8.5: be PID 1 on purpose. Run it as the main process of a container and watch what
//! `docker stop` does with and without a SIGTERM handler.
//!
//! usage: pid1 [graceful|ignore|zombie]
//!   graceful (default) install SIGTERM/SIGINT handlers, drain, exit 0
//!   ignore             install no handler at all and just sleep
//!   zombie             fork a child that exits at once and never wait() for it
//! Each mode prints /proc/self/status Pid/NSpid and the SigCgt/SigIgn masks first.

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{Error, Result};
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::time::{Duration, Instant};
    use std::{env, fs, process, thread};

    /// 0 = nothing yet; otherwise the signal number last caught. Only an atomic store happens
    /// inside the handler, which is async-signal-safe.
    static CAUGHT: AtomicI32 = AtomicI32::new(0);

    extern "C" fn on_signal(sig: libc::c_int) {
        CAUGHT.store(sig, Ordering::SeqCst);
    }

    fn install(sig: libc::c_int) -> Result<()> {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        unsafe { libc::sigemptyset(&mut action.sa_mask) };
        if unsafe { libc::sigaction(sig, &action, std::ptr::null_mut()) } == -1 {
            return Err(Error::last_os_error());
        }
        Ok(())
    }

    fn status_fields() -> String {
        let status = fs::read_to_string("/proc/self/status").unwrap_or_default();
        status
            .lines()
            .filter(|l| ["Pid:", "NSpid:", "PPid:", "SigIgn:", "SigCgt:"].iter().any(|p| l.starts_with(p)))
            .map(|l| format!("[pid1]   {}", l.split_whitespace().collect::<Vec<_>>().join(" ")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn graceful() -> Result<()> {
        install(libc::SIGTERM)?;
        install(libc::SIGINT)?;
        println!("[pid1] handlers installed for SIGTERM and SIGINT; waiting");
        // `pause`-style wait without a race: poll the flag with a short sleep (a lab, not a server).
        let sig = loop {
            let s = CAUGHT.load(Ordering::SeqCst);
            if s != 0 {
                break s;
            }
            thread::sleep(Duration::from_millis(20));
        };
        println!("[pid1] caught signal {sig}; draining for 200ms");
        let started = Instant::now();
        thread::sleep(Duration::from_millis(200));
        println!("[pid1] drained after {:?}; exit(0)", started.elapsed());
        Ok(())
    }

    fn ignore() -> ! {
        println!("[pid1] no handlers installed; sleeping forever");
        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    }

    fn zombie() -> Result<()> {
        let pid = unsafe { libc::fork() };
        if pid == -1 {
            return Err(Error::last_os_error());
        }
        if pid == 0 {
            unsafe { libc::_exit(0) };
        }
        thread::sleep(Duration::from_millis(300));
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_else(|e| format!("<{e}>"));
        // Field 3 of /proc/<pid>/stat is the state letter, after the "(comm)" field.
        let state = stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().next()).unwrap_or("?");
        println!("[pid1] child {pid} exited and was never waited for; its state in /proc/{pid}/stat = {state}");
        println!("[pid1] now reaping it with waitpid(-1)");
        let mut status = 0;
        let reaped = unsafe { libc::waitpid(-1, &mut status, 0) };
        println!("[pid1] waitpid returned {reaped}; /proc/{pid}/stat exists afterwards: {}", fs::metadata(format!("/proc/{pid}/stat")).is_ok());
        Ok(())
    }

    pub fn run() -> Result<()> {
        let mode = env::args().nth(1).unwrap_or_else(|| "graceful".into());
        println!("[pid1] mode = {mode}, std::process::id() = {}", process::id());
        println!("{}", status_fields());
        match mode.as_str() {
            "graceful" => graceful(),
            "ignore" => ignore(),
            "zombie" => zombie(),
            other => {
                eprintln!("unknown mode {other}; use graceful | ignore | zombie");
                process::exit(2);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_containers::not_linux("pid1");
}
