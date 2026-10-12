//! Lab (c2): the same ideas through `std::process::Command` and `Stdio::piped`.
//! Run under `strace -f -e trace=pipe2,clone,clone3,vfork,fork,execve,wait4,dup2,dup3` to see
//! which system calls std uses to start a child.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::process::{CommandExt, ExitStatusExt};
    use std::process::{Command, Stdio};

    // 1. Capture a child's stdout through a pipe.
    let mut child = Command::new("echo").arg("hello via Command").stdout(Stdio::piped()).spawn()?;
    let mut out = String::new();
    child.stdout.take().expect("stdout was piped").read_to_string(&mut out)?;
    let status = child.wait()?;
    print!("[spawn] echo said: {out}");
    println!("[spawn] echo pid {} -> {status}", child.id());

    // 2. Pipes in both directions: feed stdin, read stdout.
    let mut child = Command::new("tr")
        .args(["a-z", "A-Z"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    // Dropping the ChildStdin closes our write end, so `tr` reads EOF and exits.
    child.stdin.take().expect("stdin was piped").write_all(b"shout this\n")?;
    let output = child.wait_with_output()?;
    print!("[spawn] tr said: {}", String::from_utf8_lossy(&output.stdout));

    // 3. Exit codes and signals as std reports them.
    let exited = Command::new("sh").args(["-c", "exit 3"]).status()?;
    println!("[spawn] sh exit 3 -> code() = {:?}, signal() = {:?}", exited.code(), exited.signal());
    let killed = Command::new("sh").args(["-c", "kill -TERM $$"]).status()?;
    println!("[spawn] sh kill -TERM $$ -> code() = {:?}, signal() = {:?}", killed.code(), killed.signal());

    // 4. A pre_exec closure must run in the child between fork and exec, so std cannot use
    //    posix_spawn for this command (compare the clone flags in strace).
    let mut cmd = Command::new("true");
    // SAFETY: the closure does nothing, so it is trivially async-signal-safe.
    unsafe { cmd.pre_exec(|| Ok(())) };
    println!("[spawn] true with pre_exec -> {}", cmd.status()?);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_linux::not_linux("spawn");
}
