//! Phase 1 labs. Each binary in `src/bin/` is one lab from `docs/labs/01-linux-lab.md`.

/// Printed by every lab binary when it is built for a target other than Linux.
pub fn not_linux(bin: &str) {
    eprintln!(
        "{bin}: this lab uses Linux-only interfaces (/proc, strace, Linux errno values).\n\
         Run it in the lab container from the repository root:\n  \
         ./labs/linux.sh cargo run -p lab-linux --bin {bin}"
    );
}
