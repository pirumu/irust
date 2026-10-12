//! Baseline for lab (d): blocking echo server, one OS thread per connection.
//!
//! Usage: thread_echo [PORT=7104] [CONNS=4]
//! Same protocol and exit rule as the other servers, so `strace -f -c` can
//! compare them; P4 (`lab-networking --bin echo_server`) explains the blocking calls.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Barrier};
use std::thread;

use lab_io_async::{ServerArgs, thread_count_text};

fn main() -> io::Result<()> {
    let args = ServerArgs::parse(7104)?;
    let listener = TcpListener::bind(("127.0.0.1", args.port))?;
    eprintln!("[thread] listening on {}", listener.local_addr()?);

    // Every worker waits here after its echo loop, so the main thread can count
    // the threads while they all still exist.
    let done = Arc::new(Barrier::new(args.conns + 1));
    let mut workers = Vec::with_capacity(args.conns);
    for _ in 0..args.conns {
        let (stream, peer) = listener.accept()?;
        eprintln!("[thread] accepted {peer}");
        let done = Arc::clone(&done);
        workers.push(thread::spawn(move || {
            if let Err(e) = echo(stream) {
                eprintln!("[thread] {peer}: {e}");
            }
            done.wait();
        }));
    }
    // No worker can pass the barrier before the main thread joins it, so all are alive here.
    let peak = thread_count_text();
    done.wait();
    for w in workers {
        w.join().expect("worker panicked");
    }
    println!("[thread] served {} conns, threads at peak={peak}", args.conns);
    Ok(())
}

fn echo(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        stream.write_all(&buf[..n])?;
    }
}
