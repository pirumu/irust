//! Lab (c): walk one TCP connection through its states and print `ss -tan` at each step.
//!
//! Part 1: the client closes first  -> client ends in TIME-WAIT.
//! Part 2: the server closes first  -> the server side ends in TIME-WAIT,
//!         and std can still re-bind the port because it sets SO_REUSEADDR.

#[cfg(target_os = "linux")]
fn main() -> std::io::Result<()> {
    linux::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("{}", lab_networking::LINUX_ONLY.replace("<name>", "tcp_states"));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{self, Read};
    use std::net::{TcpListener, TcpStream};
    use std::thread::sleep;
    use std::time::Duration;

    use lab_networking::ss;

    /// Lets the FIN/ACK segments of the previous step cross loopback before we look.
    const SETTLE: Duration = Duration::from_millis(200);

    fn show(step: &str, port: u16) -> io::Result<()> {
        let p = format!(":{port}");
        // Only sockets whose local or remote port is the listener's port.
        let out = ss(&["-tan", "(", "sport", "=", &p, "or", "dport", "=", &p, ")"])?;
        println!("--- {step}\n{}", out.trim_end());
        Ok(())
    }

    pub fn run() -> io::Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        println!("listener on 127.0.0.1:{port}");
        show("1. after bind+listen", port)?;

        let client = TcpStream::connect(("127.0.0.1", port))?;
        let (server_side, _) = listener.accept()?;
        show("2. after connect + accept", port)?;

        drop(client); // client close(): sends FIN
        sleep(SETTLE);
        show("3. client closed, server has not", port)?;

        let mut rest = Vec::new();
        let mut s = server_side;
        let n = s.read_to_end(&mut rest)?; // the FIN shows up as EOF
        println!("    server read_to_end -> {n} bytes (EOF = peer's FIN)");
        drop(s); // server close(): sends its FIN
        sleep(SETTLE);
        show("4. server closed too", port)?;

        // Part 2: server closes first.
        let client = TcpStream::connect(("127.0.0.1", port))?;
        let (server_side, _) = listener.accept()?;
        drop(server_side);
        sleep(SETTLE);
        let mut c = client;
        let n = c.read(&mut [0u8; 16])?;
        println!("    client read -> {n} bytes (EOF)");
        drop(c);
        sleep(SETTLE);
        show("5. second connection, server closed first", port)?;

        drop(listener);
        // A TIME-WAIT socket still holds 127.0.0.1:port; bind succeeds because
        // TcpListener::bind sets SO_REUSEADDR before bind(2).
        let again = TcpListener::bind(("127.0.0.1", port))?;
        println!("--- 6. re-bind 127.0.0.1:{port} while in TIME-WAIT -> ok ({})", again.local_addr()?);
        Ok(())
    }
}
