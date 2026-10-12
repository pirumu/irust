//! Lab (d): write to fd 1 without std::io — through the libc crate, through our own
//! `unsafe extern "C"` declaration, and through the raw `syscall(2)` entry point.
//! Then provoke an error and read errno.

#[cfg(target_os = "linux")]
mod ffi {
    use std::ffi::{c_int, c_void};

    unsafe extern "C" {
        /// glibc's `write` wrapper, declared by hand instead of taken from the libc crate.
        #[link_name = "write"]
        pub fn c_write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    }
}

#[cfg(target_os = "linux")]
fn main() {
    use std::io::Error;
    use std::os::fd::{AsFd, AsRawFd};

    let a = b"[rawwrite] 1: libc::write(1, ...)\n";
    let n = unsafe { libc::write(libc::STDOUT_FILENO, a.as_ptr().cast(), a.len()) };
    println!("[rawwrite]    -> returned {n}");

    let b = b"[rawwrite] 2: own unsafe extern \"C\" fn write\n";
    let n = unsafe { ffi::c_write(1, b.as_ptr().cast(), b.len()) };
    println!("[rawwrite]    -> returned {n}");

    let c = b"[rawwrite] 3: libc::syscall(SYS_write, 1, ...)\n";
    let n = unsafe { libc::syscall(libc::SYS_write, 1, c.as_ptr(), c.len()) };
    println!("[rawwrite]    -> returned {n}");

    // std's handles sit on the same fds 0/1/2.
    let stdout = std::io::stdout();
    println!(
        "[rawwrite] stdin/stdout/stderr raw fds = {}/{}/{}; stdout.as_fd() = {:?}",
        std::io::stdin().as_raw_fd(),
        stdout.as_raw_fd(),
        std::io::stderr().as_raw_fd(),
        stdout.as_fd()
    );
    eprintln!("[rawwrite] 4: this line goes to fd 2 (stderr)");

    // fd 99 is not open: the wrapper returns -1 and stores EBADF in errno.
    let ret = unsafe { libc::write(99, a.as_ptr().cast(), a.len()) };
    let errno = unsafe { *libc::__errno_location() };
    let err = Error::last_os_error();
    println!("[rawwrite] write(99, ...) -> {ret}; errno = {errno} (EBADF = {})", libc::EBADF);
    println!("[rawwrite] io::Error::last_os_error() = {err:?}");
    println!("[rawwrite] Display: {err}");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    lab_linux::not_linux("rawwrite");
}
