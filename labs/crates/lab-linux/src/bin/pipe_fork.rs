//! Lab (c1): pipe + fork + waitpid straight through libc, and the exit status a parent sees.
//! Run under `strace -f` to see pipe2, clone, write, read, wait4 and kill.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::io::Error;

    /// Turn the C convention "-1 means look at errno" into io::Result.
    fn check(ret: libc::c_int) -> std::io::Result<libc::c_int> {
        if ret == -1 { Err(Error::last_os_error()) } else { Ok(ret) }
    }

    /// Wait for `pid` and describe how it ended, decoding the status with the wait(2) macros.
    fn reap(pid: libc::pid_t) -> std::io::Result<String> {
        let mut status: libc::c_int = 0;
        check(unsafe { libc::waitpid(pid, &mut status, 0) })?;
        Ok(if libc::WIFEXITED(status) {
            format!("exited, WEXITSTATUS = {}", libc::WEXITSTATUS(status))
        } else if libc::WIFSIGNALED(status) {
            format!("killed by signal, WTERMSIG = {}", libc::WTERMSIG(status))
        } else {
            format!("other state change, raw status = {status:#x}")
        })
    }

    // 1. Child writes into the pipe and exits with status 7; parent reads until EOF.
    let mut fds = [0 as libc::c_int; 2];
    check(unsafe { libc::pipe(fds.as_mut_ptr()) })?;
    let (read_end, write_end) = (fds[0], fds[1]);
    println!("[pipe_fork] parent pid {} | pipe: read end fd {read_end}, write end fd {write_end}", std::process::id());

    let pid = check(unsafe { libc::fork() })?;
    if pid == 0 {
        // Child: only async-signal-safe calls between fork and _exit (see fork(2)).
        unsafe {
            libc::close(read_end);
            let msg = b"hello parent, this is the child\n";
            libc::write(write_end, msg.as_ptr().cast(), msg.len());
            libc::close(write_end);
            libc::_exit(7);
        }
    }
    // Parent: close its copy of the write end, otherwise read() never sees EOF.
    unsafe { libc::close(write_end) };
    let mut received = Vec::new();
    let mut buf = [0u8; 64];
    loop {
        let n = unsafe { libc::read(read_end, buf.as_mut_ptr().cast(), buf.len()) };
        match n {
            -1 => return Err(Error::last_os_error()),
            0 => break, // all write ends closed -> end of file
            n => received.extend_from_slice(&buf[..n as usize]),
        }
    }
    unsafe { libc::close(read_end) };
    print!("[pipe_fork] parent read {} bytes: {}", received.len(), String::from_utf8_lossy(&received));
    println!("[pipe_fork] child {pid}: {}", reap(pid)?);

    // 2. Child is ended by a signal instead of calling exit.
    let pid = check(unsafe { libc::fork() })?;
    if pid == 0 {
        unsafe {
            libc::kill(libc::getpid(), libc::SIGTERM);
            libc::_exit(0); // not reached: SIGTERM's default action terminates the process
        }
    }
    println!("[pipe_fork] child {pid}: {}", reap(pid)?);

    // 3. Writing to a pipe whose read end is closed. The Rust runtime set SIGPIPE to SIG_IGN at
    //    startup, so instead of being killed we get -1 with errno = EPIPE.
    let mut fds = [0 as libc::c_int; 2];
    check(unsafe { libc::pipe(fds.as_mut_ptr()) })?;
    unsafe { libc::close(fds[0]) };
    let ret = unsafe { libc::write(fds[1], b"x".as_ptr().cast(), 1) };
    let err = Error::last_os_error();
    println!("[pipe_fork] write to pipe without reader -> {ret}, errno -> {err}");
    unsafe { libc::close(fds[1]) };
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_linux::not_linux("pipe_fork");
}
