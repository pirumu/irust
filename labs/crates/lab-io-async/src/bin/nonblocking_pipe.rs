//! Lab (0): blocking vs non-blocking read(2) on a pipe.
//!
//! 1. A blocking read on an empty pipe puts the thread to sleep until a writer
//!    thread writes 200 ms later.
//! 2. After fcntl(F_SETFL, flags | O_NONBLOCK) the same read on an empty pipe
//!    returns -1 with errno EAGAIN at once, which std maps to ErrorKind::WouldBlock.
//!
//! Runs on any Unix (macOS too); errno numbers differ per OS.

use std::io;
use std::thread;
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let [rfd, wfd] = pipe()?;

    // 1. Blocking read: the writer only writes after 200 ms.
    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        write_all(wfd, b"hello").expect("write to pipe");
        wfd
    });
    let mut buf = [0u8; 64];
    let start = Instant::now();
    let n = read(rfd, &mut buf)?;
    println!(
        "blocking read   -> {n} bytes {:?} after {} ms (the thread slept in read(2))",
        String::from_utf8_lossy(&buf[..n]),
        start.elapsed().as_millis()
    );
    let wfd = writer.join().expect("writer thread");

    // 2. Switch the read end to non-blocking with fcntl(2).
    let flags = cvt(unsafe { libc::fcntl(rfd, libc::F_GETFL) })?;
    println!("F_GETFL         -> {flags:#o} (O_NONBLOCK set: {})", flags & libc::O_NONBLOCK != 0);
    cvt(unsafe { libc::fcntl(rfd, libc::F_SETFL, flags | libc::O_NONBLOCK) })?;
    let flags = cvt(unsafe { libc::fcntl(rfd, libc::F_GETFL) })?;
    println!("F_SETFL         -> {flags:#o} (O_NONBLOCK set: {})", flags & libc::O_NONBLOCK != 0);

    // Empty pipe: no sleeping, an error instead.
    let start = Instant::now();
    let err = read(rfd, &mut buf).expect_err("empty non-blocking pipe must not return data");
    println!(
        "non-block read  -> errno {} ({}), ErrorKind::{:?}, after {} us",
        err.raw_os_error().unwrap_or(-1),
        errno_name(err.raw_os_error()),
        err.kind(),
        start.elapsed().as_micros()
    );
    println!(
        "EAGAIN = {}, EWOULDBLOCK = {} on this OS",
        libc::EAGAIN,
        libc::EWOULDBLOCK
    );

    // Data present: the non-blocking read behaves like a normal read.
    write_all(wfd, b"world")?;
    let n = read(rfd, &mut buf)?;
    println!("non-block read  -> {n} bytes {:?}", String::from_utf8_lossy(&buf[..n]));
    let err = read(rfd, &mut buf).expect_err("pipe drained");
    println!("non-block read  -> {:?} again: the pipe is drained", err.kind());

    unsafe {
        libc::close(rfd);
        libc::close(wfd);
    }
    Ok(())
}

fn pipe() -> io::Result<[libc::c_int; 2]> {
    let mut fds = [0; 2];
    cvt(unsafe { libc::pipe(fds.as_mut_ptr()) })?;
    Ok(fds)
}

fn read(fd: libc::c_int, buf: &mut [u8]) -> io::Result<usize> {
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

fn write_all(fd: libc::c_int, mut data: &[u8]) -> io::Result<()> {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr().cast(), data.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        data = &data[n as usize..];
    }
    Ok(())
}

fn cvt(ret: libc::c_int) -> io::Result<libc::c_int> {
    if ret < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

fn errno_name(code: Option<i32>) -> &'static str {
    match code {
        Some(c) if c == libc::EAGAIN => "EAGAIN",
        Some(c) if c == libc::EWOULDBLOCK => "EWOULDBLOCK",
        _ => "other",
    }
}
