//! Phase 8 labs. Each binary in `src/bin/` is one lab from `docs/labs/08-containers-lab.md`.

/// Printed by every lab binary when it is built for a target other than Linux.
pub fn not_linux(bin: &str) {
    eprintln!(
        "{bin}: this lab uses Linux-only interfaces (/proc/self/ns, cgroup v2, unshare, pivot_root).\n\
         Build it in the lab container from the repository root:\n  \
         ./labs/linux.sh cargo build -p lab-containers --bin {bin}\n\
         and see docs/labs/08-containers-lab.md for the exact run command (some labs need extra privileges)."
    );
}
