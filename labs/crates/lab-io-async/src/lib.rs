//! Shared helpers for the P7 labs: command-line parsing for the echo servers and
//! a thread counter, so every server variant reports the same numbers.

use std::io;

/// Arguments common to every echo server in this crate.
///
/// `SERVER [PORT] [CONNS] [FLAGS...]`: listen on 127.0.0.1:PORT, exit after CONNS
/// connections have been served and closed (so `strace -c` gets a summary).
pub struct ServerArgs {
    pub port: u16,
    pub conns: usize,
    pub flags: Vec<String>,
}

impl ServerArgs {
    pub fn parse(default_port: u16) -> io::Result<Self> {
        let mut args = std::env::args().skip(1);
        let port = match args.next() {
            Some(p) => p.parse().map_err(|_| invalid("PORT must be a number"))?,
            None => default_port,
        };
        let conns = match args.next() {
            Some(n) => n.parse().map_err(|_| invalid("CONNS must be a number"))?,
            None => 4,
        };
        Ok(Self {
            port,
            conns,
            flags: args.collect(),
        })
    }

    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// Number of threads in this process, read from `/proc/self/task` (one entry per thread).
/// `None` where `/proc` does not exist (macOS).
pub fn thread_count() -> Option<usize> {
    std::fs::read_dir("/proc/self/task").ok().map(|d| d.count())
}

/// Formats `thread_count()` for the servers' final report line.
pub fn thread_count_text() -> String {
    match thread_count() {
        Some(n) => n.to_string(),
        None => "n/a (no /proc; run under ./labs/linux.sh)".to_string(),
    }
}
