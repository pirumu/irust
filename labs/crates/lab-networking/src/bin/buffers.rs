//! Socket buffers and flow control on Linux.
//!
//! 1. Print the kernel defaults (tcp_rmem/tcp_wmem) and SO_SNDBUF/SO_RCVBUF of a live socket.
//! 2. Show that setsockopt(SO_RCVBUF, n) reads back as 2n.
//! 3. Show TCP_NODELAY before/after `set_nodelay(true)`.
//! 4. Flow control: write without the peer reading until the kernel refuses
//!    (WouldBlock), then let the peer read and watch room open up again.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("{}", lab_networking::LINUX_ONLY.replace("<name>", "buffers"));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{self, Read, Write};
    use std::mem::size_of;
    use std::net::{TcpListener, TcpStream};
    use std::os::fd::AsRawFd;

    use lab_networking::ss;

    fn getsockopt_int(s: &impl AsRawFd, level: i32, name: i32) -> io::Result<i32> {
        let mut val: libc::c_int = 0;
        let mut len = size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: val/len point to a c_int and its size, as getsockopt(2) requires.
        let rc = unsafe {
            libc::getsockopt(s.as_raw_fd(), level, name, (&raw mut val).cast(), &mut len)
        };
        if rc == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(val)
    }

    fn setsockopt_int(s: &impl AsRawFd, level: i32, name: i32, val: i32) -> io::Result<()> {
        // SAFETY: we pass a pointer to a live c_int and its exact size.
        let rc = unsafe {
            libc::setsockopt(
                s.as_raw_fd(),
                level,
                name,
                (&raw const val).cast(),
                size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        if rc == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn sysctl(path: &str) -> io::Result<String> {
        Ok(std::fs::read_to_string(path)?.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    fn pair() -> io::Result<(TcpStream, TcpStream)> {
        let l = TcpListener::bind("127.0.0.1:0")?;
        let a = TcpStream::connect(l.local_addr()?)?;
        let (b, _) = l.accept()?;
        Ok((a, b))
    }

    pub fn run() -> io::Result<()> {
        println!("== 1. defaults ==");
        for p in ["/proc/sys/net/ipv4/tcp_rmem", "/proc/sys/net/ipv4/tcp_wmem"] {
            println!("  {p} = {}", sysctl(p)?);
        }
        let (a, b) = pair()?;
        println!(
            "  live socket: SO_SNDBUF={} SO_RCVBUF={}",
            getsockopt_int(&a, libc::SOL_SOCKET, libc::SO_SNDBUF)?,
            getsockopt_int(&a, libc::SOL_SOCKET, libc::SO_RCVBUF)?
        );

        println!("== 2. SO_RCVBUF is doubled ==");
        setsockopt_int(&b, libc::SOL_SOCKET, libc::SO_RCVBUF, 4096)?;
        println!(
            "  setsockopt(SO_RCVBUF, 4096) -> getsockopt = {}",
            getsockopt_int(&b, libc::SOL_SOCKET, libc::SO_RCVBUF)?
        );

        println!("== 3. TCP_NODELAY ==");
        println!(
            "  default: nodelay()={} getsockopt(TCP_NODELAY)={}",
            a.nodelay()?,
            getsockopt_int(&a, libc::IPPROTO_TCP, libc::TCP_NODELAY)?
        );
        a.set_nodelay(true)?;
        println!(
            "  after set_nodelay(true): nodelay()={} getsockopt(TCP_NODELAY)={}",
            a.nodelay()?,
            getsockopt_int(&a, libc::IPPROTO_TCP, libc::TCP_NODELAY)?
        );
        drop((a, b));

        println!("== 4. flow control: writer vs. a reader that does not read ==");
        let (mut writer, mut reader) = pair()?;
        let wport = writer.local_addr()?.port();
        // Non-blocking only so the demo can stop at "full" instead of hanging in write().
        writer.set_nonblocking(true)?;
        let chunk = vec![0u8; 64 * 1024];
        let mut sent = 0usize;
        loop {
            match writer.write(&chunk) {
                Ok(n) => sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        println!("  wrote {sent} bytes before WouldBlock (a blocking write would sleep here)");
        let p = format!(":{wport}");
        let q = ss(&["-tn", "(", "sport", "=", &p, "or", "dport", "=", &p, ")"])?;
        println!("{}", q.trim_end());

        let mut sink = vec![0u8; 1024 * 1024];
        let mut drained = 0usize;
        while drained < sent / 2 {
            drained += reader.read(&mut sink)?;
        }
        println!("  reader consumed {drained} bytes");
        // Give the window-update ACKs a moment to cross loopback.
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut more = 0usize;
        loop {
            match writer.write(&chunk) {
                Ok(n) => more += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        println!("  writer could then push {more} more bytes");
        Ok(())
    }
}
