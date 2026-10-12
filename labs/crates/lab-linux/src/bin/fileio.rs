//! Lab (a): open, write, read and close a file through `std::fs::File`, printing the raw fds.
//! Run it under `strace` to see the `openat`/`write`/`read`/`close` calls behind each step.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};

    let path = std::env::temp_dir().join("p01-fileio.txt");
    {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        println!("[fileio] open {} -> fd {}", path.display(), file.as_raw_fd());

        file.write_all(b"hello from p01\n")?;
        file.seek(SeekFrom::Start(0))?;
        let mut text = String::new();
        file.read_to_string(&mut text)?;
        print!("[fileio] read back: {text}");
        println!("[fileio] as_fd() = {:?}", file.as_fd());

        // A second open gets the lowest free number, i.e. the next one.
        let second = File::open(&path)?;
        println!("[fileio] second open -> fd {}", second.as_raw_fd());
        let owned: OwnedFd = second.into();
        let raw: RawFd = owned.as_raw_fd();
        println!("[fileio] dropping OwnedFd {raw}");
        drop(owned);
        println!("[fileio] end of scope: File (fd {}) is dropped", file.as_raw_fd());
    }

    // The failing call is an openat that returns -1/ENOENT; std turns errno into io::Error.
    let err = File::open("/definitely/not/here").unwrap_err();
    println!(
        "[fileio] open error: {err} | raw_os_error = {:?} | kind = {:?}",
        err.raw_os_error(),
        err.kind()
    );

    // Both earlier fds are closed, so the lowest free number is handed out again.
    let again = File::open(&path)?;
    println!("[fileio] reopen -> fd {}", again.as_raw_fd());
    drop(again);
    fs::remove_file(&path)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_linux::not_linux("fileio");
}
