//! Lab (d): what a `tracing` subscriber is told, and what holding `Span::enter` across `.await` does.
//!
//! Two tasks, A and B, run on a current-thread runtime and each yields once in the middle.
//! A hand-written `Layer` prints every notification (span created / entered / exited, event) with
//! the span path the event was reported in. No timing is involved, so the output is the same on
//! every run and on every OS.
//!
//!   cargo run -q -p lab-observability --bin spans                         # all three styles
//!   cargo run -q -p lab-observability --bin spans -- enter-across-await   # just one

use lab_observability::spans::{Style, transcript};

fn main() {
    let styles: Vec<Style> = match std::env::args().nth(1) {
        None => vec![Style::EnterAcrossAwait, Style::Instrument, Style::InstrumentAttribute],
        Some(arg) => match Style::parse(&arg) {
            Some(style) => vec![style],
            None => {
                eprintln!("usage: spans [enter-across-await | instrument | instrument-attribute]");
                std::process::exit(2);
            }
        },
    };
    for style in styles {
        println!("== {} ==", style.name());
        for line in transcript(style) {
            println!("{line}");
        }
        println!();
    }
}
