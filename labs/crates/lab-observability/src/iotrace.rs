//! The workload of lab (b) `trace_target`: write one line per request either straight to the
//! file (one `write(2)` and a few allocations per request) or through a `BufWriter` and a reused
//! `String` (a handful of `write(2)` calls and no allocation in the formatting).
//!
//! The functions count what they do — `write` calls on the underlying writer and allocator
//! calls — so the numbers a tracer reports from the outside (`strace -c`, `bpftrace`) have
//! something exact to be compared with. Allocator calls are only counted when the binary
//! installed [`crate::alloc::CountingAlloc`].

use crate::alloc::{self, Stats};
use std::fmt::Write as _;
use std::io::{self, BufWriter, Write};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `format!` a line and `write_all` it to the raw writer, once per request.
    Unbuffered,
    /// Format into a reused `String`, write through a `BufWriter`, flush once at the end.
    Buffered,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Unbuffered => "unbuffered",
            Mode::Buffered => "buffered",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        [Mode::Unbuffered, Mode::Buffered].into_iter().find(|m| m.name() == s)
    }
}

/// Counts calls to `write` on the wrapped writer. With a `File` underneath, each call is one
/// `write(2)` (when the writes are small and the target is not a pipe, none of them is short).
pub struct CountingWriter<W> {
    inner: W,
    pub calls: u64,
    pub bytes: u64,
}

impl<W> CountingWriter<W> {
    pub fn new(inner: W) -> CountingWriter<W> {
        CountingWriter { inner, calls: 0, bytes: 0 }
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.calls += 1;
        let n = self.inner.write(buf)?;
        self.bytes += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// What one run did.
pub struct Report {
    /// Calls to `write` on the writer underneath.
    pub write_calls: u64,
    pub bytes: u64,
    /// Allocator calls during the run (zero unless the binary installed the counting allocator).
    pub allocs: Stats,
    pub elapsed: Duration,
}

/// Issues `requests` requests: for each id, `handle(id)` returns a length, then the line
/// `"{id} {len}\n"` is written to `sink` the way `mode` says.
pub fn run_with<W: Write>(mode: Mode, requests: u64, sink: W, handle: fn(u64) -> u64) -> io::Result<Report> {
    let mut sink = CountingWriter::new(sink);
    let started = Instant::now();
    let (result, allocs) = alloc::measure(|| -> io::Result<()> {
        match mode {
            Mode::Unbuffered => {
                for id in 0..requests {
                    let len = handle(id);
                    let line = format!("{id} {len}\n");
                    sink.write_all(line.as_bytes())?;
                }
            }
            Mode::Buffered => {
                let mut out = BufWriter::new(&mut sink);
                let mut line = String::with_capacity(64);
                for id in 0..requests {
                    let len = handle(id);
                    line.clear();
                    writeln!(line, "{id} {len}").expect("writing to a String cannot fail");
                    out.write_all(line.as_bytes())?;
                }
                out.flush()?;
            }
        }
        Ok(())
    });
    result?;
    Ok(Report { write_calls: sink.calls, bytes: sink.bytes, allocs, elapsed: started.elapsed() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for `handle_request`: one small allocation and a length, like the real one.
    fn handle(id: u64) -> u64 {
        std::hint::black_box(format!("request {id} handled")).len() as u64
    }

    /// The writer receives byte-for-byte the same output in both modes; only the number of
    /// `write` calls differs. Compares against the output computed independently.
    #[test]
    fn both_modes_write_the_same_bytes() {
        let expected: String = (0..500u64).map(|id| format!("{id} {}\n", handle(id))).collect();
        for mode in [Mode::Unbuffered, Mode::Buffered] {
            let mut out = Vec::new();
            let report = run_with(mode, 500, &mut out, handle).unwrap();
            assert_eq!(String::from_utf8(out).unwrap(), expected, "{}", mode.name());
            assert_eq!(report.bytes, expected.len() as u64);
        }
    }

    /// One `write` per request without buffering; with buffering, orders of magnitude fewer.
    #[test]
    fn buffering_collapses_write_calls() {
        let n = 5000;
        let raw = run_with(Mode::Unbuffered, n, io::sink(), handle).unwrap();
        let buffered = run_with(Mode::Buffered, n, io::sink(), handle).unwrap();
        assert_eq!(raw.write_calls, n, "one write(2) per request");
        assert!(buffered.write_calls >= 1);
        assert!(buffered.write_calls * 100 < raw.write_calls, "{} buffered calls vs {}", buffered.write_calls, raw.write_calls);
        assert_eq!(raw.bytes, buffered.bytes);
    }

    /// With a writer that can only take a few bytes at a time, `write_all` loops and the count
    /// goes up: "calls to write" equals syscalls only when writes are not short.
    #[test]
    fn short_writes_make_more_calls_than_requests() {
        struct Trickle(Vec<u8>);
        impl Write for Trickle {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                let n = buf.len().min(3);
                self.0.extend_from_slice(&buf[..n]);
                Ok(n)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut sink = Trickle(Vec::new());
        let report = run_with(Mode::Unbuffered, 100, &mut sink, handle).unwrap();
        assert!(report.write_calls > 100, "{}", report.write_calls);
        assert_eq!(sink.0.len() as u64, report.bytes);
    }

    /// The unbuffered loop allocates at least once per request for the line, plus whatever
    /// `handle` does; the buffered loop's formatting allocates nothing, so its only allocations are
    /// `handle`'s plus the two buffers it sets up.
    #[test]
    fn buffered_formatting_does_not_allocate_per_request() {
        let n = 2000;
        let raw = run_with(Mode::Unbuffered, n, io::sink(), handle).unwrap();
        let buffered = run_with(Mode::Buffered, n, io::sink(), handle).unwrap();
        let handle_only = n; // `handle` allocates once per call
        assert!(raw.allocs.events() >= 2 * n, "{:?}", raw.allocs);
        assert!(buffered.allocs.events() <= handle_only + 4, "{:?}", buffered.allocs);
        assert!(buffered.allocs.events() >= handle_only, "{:?}", buffered.allocs);
    }

    #[test]
    fn mode_names_round_trip() {
        for m in [Mode::Unbuffered, Mode::Buffered] {
            assert_eq!(Mode::parse(m.name()), Some(m));
        }
        assert_eq!(Mode::parse("fast"), None);
    }
}
