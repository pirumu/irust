//! Backlog lab: connections complete the handshake and wait in the accept queue
//! even if the server never calls accept().
//!
//! Usage: backlog [CLIENTS]   (default 5)

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("{}", lab_networking::LINUX_ONLY.replace("<name>", "backlog"));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io;
    use std::net::{TcpListener, TcpStream};

    use lab_networking::ss;

    fn show(step: &str, port: u16) -> io::Result<()> {
        let out = ss(&["-ltn", "(", "sport", "=", &format!(":{port}"), ")"])?;
        println!("--- {step}\n{}", out.trim_end());
        Ok(())
    }

    pub fn run() -> io::Result<()> {
        let clients: usize = std::env::args()
            .nth(1)
            .map(|s| s.parse())
            .transpose()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "CLIENTS must be a number"))?
            .unwrap_or(5);

        println!(
            "somaxconn = {}",
            std::fs::read_to_string("/proc/sys/net/core/somaxconn")?.trim()
        );
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        show("listening, nobody connected", port)?;

        // connect() returns as soon as the kernel finished the handshake;
        // the server process is not involved.
        let conns: Vec<TcpStream> = (0..clients)
            .map(|_| TcpStream::connect(("127.0.0.1", port)))
            .collect::<io::Result<_>>()?;
        println!("{} connect() calls returned Ok without any accept()", conns.len());
        show(&format!("{clients} clients connected, 0 accepted"), port)?;

        let (_first, peer) = listener.accept()?;
        println!("accept() -> {peer}");
        show("after one accept()", port)?;
        Ok(())
    }
}
