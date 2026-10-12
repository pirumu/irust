//! Lab (d): the same echo server on Tokio.
//!
//! Usage: tokio_echo [PORT=7103] [CONNS=4] [--current-thread]
//!   default          multi-thread scheduler (one worker per CPU core)
//!   --current-thread everything runs on the thread that calls block_on
//! Runs on macOS and Linux; thread counts are printed only where /proc exists.

use std::io;
use std::net::SocketAddr;

use lab_io_async::{ServerArgs, thread_count_text};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Builder;

fn main() -> io::Result<()> {
    let args = ServerArgs::parse(7103)?;
    let current_thread = args.has_flag("--current-thread");
    let rt = if current_thread {
        Builder::new_current_thread().enable_io().build()?
    } else {
        Builder::new_multi_thread().enable_io().build()?
    };
    eprintln!(
        "[tokio] scheduler={}, threads right after build={}",
        if current_thread { "current_thread" } else { "multi_thread" },
        thread_count_text()
    );

    // block_on: this thread drives the outer future until it completes.
    rt.block_on(async move {
        let addr: SocketAddr = ([127, 0, 0, 1], args.port).into();
        let listener = TcpListener::bind(addr).await?;
        eprintln!("[tokio] listening on {addr}");
        let mut handles = Vec::with_capacity(args.conns);
        for _ in 0..args.conns {
            let (stream, peer) = listener.accept().await?;
            eprintln!("[tokio] accepted {peer}");
            // One task per connection: a heap-allocated future, not an OS thread.
            handles.push(tokio::spawn(echo(stream)));
        }
        for h in handles {
            if let Err(e) = h.await.expect("echo task panicked") {
                eprintln!("[tokio] conn error: {e}");
            }
        }
        println!(
            "[tokio] served {} conns, threads={}",
            args.conns,
            thread_count_text()
        );
        Ok(())
    })
}

async fn echo(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        stream.write_all(&buf[..n]).await?;
    }
}
