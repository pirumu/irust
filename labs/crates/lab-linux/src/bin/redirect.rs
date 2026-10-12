//! Extra lab: what a shell does for `cmd > file` — dup2 a file onto fd 1, then put fd 1 back.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::fs::{self, File};
    use std::io::{Error, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let path = std::env::temp_dir().join("p01-redirect.txt");
    let file = File::create(&path)?;

    // Keep a copy of the real stdout so it can be restored afterwards.
    let saved = unsafe { libc::dup(1) };
    if saved == -1 {
        return Err(Error::last_os_error());
    }
    // SAFETY: `saved` was just returned by dup and nothing else owns it.
    let saved = unsafe { OwnedFd::from_raw_fd(saved) };
    println!("[redirect] file fd = {}, saved copy of stdout = fd {}", file.as_raw_fd(), saved.as_raw_fd());

    // fd 1 now refers to the file's open file description.
    if unsafe { libc::dup2(file.as_raw_fd(), 1) } == -1 {
        return Err(Error::last_os_error());
    }
    println!("this line was written to fd 1, which is now the file");
    std::io::stdout().flush()?;

    // Put the terminal back on fd 1; `saved` is closed when it is dropped.
    if unsafe { libc::dup2(saved.as_raw_fd(), 1) } == -1 {
        return Err(Error::last_os_error());
    }
    drop(saved);
    drop(file);

    print!("[redirect] file contains: {}", fs::read_to_string(&path)?);
    fs::remove_file(&path)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_linux::not_linux("redirect");
}
