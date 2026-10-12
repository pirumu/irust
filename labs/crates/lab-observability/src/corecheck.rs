//! "Why is there no core file?" — the checks of chapter 05 §2–§4 as pure functions.
//!
//! `crash_lab preflight` reads `RLIMIT_CORE`, `/proc/sys/kernel/core_pattern`,
//! `/proc/sys/kernel/core_uses_pid` and `/proc/self/coredump_filter` and feeds the raw text to
//! the functions below; keeping the reasoning here, away from `/proc`, is what makes it testable
//! on any OS.

use std::num::ParseIntError;

/// What `/proc/sys/kernel/core_pattern` asks the kernel to do (core(5), "Piping core dumps to a program").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorePattern {
    /// A file name template (`core`, `core.%p`, `/var/crash/%e.%p`, ...).
    File(String),
    /// The first character is `|`: a user-space program gets the core on its standard input.
    /// The string is the command line after the pipe symbol.
    Pipe(String),
}

/// Classifies the raw content of `core_pattern` (a trailing newline is ignored).
pub fn classify_core_pattern(raw: &str) -> CorePattern {
    let text = raw.trim_end_matches('\n');
    match text.strip_prefix('|') {
        Some(command) => CorePattern::Pipe(command.to_string()),
        None => CorePattern::File(text.to_string()),
    }
}

/// The soft `RLIMIT_CORE` of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreLimit {
    /// 0: "no core file is created" (getrlimit(2)).
    Zero,
    /// A core larger than this many bytes is truncated.
    Bytes(u64),
    /// `RLIM_INFINITY`.
    Unlimited,
}

impl CoreLimit {
    /// Builds a limit from a raw `rlim_cur`; `infinity` is the platform's `RLIM_INFINITY`.
    pub fn from_rlim(soft: u64, infinity: u64) -> CoreLimit {
        match soft {
            0 => CoreLimit::Zero,
            s if s == infinity => CoreLimit::Unlimited,
            s => CoreLimit::Bytes(s),
        }
    }
}

/// Why the kernel writes no core file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoCore {
    /// `RLIMIT_CORE` is 0 and the core is not piped to a program.
    LimitIsZero,
    /// `core_pattern` is empty and `core_uses_pid` is 0.
    EmptyPattern,
}

/// What would happen to a core dump, given only the limit and the pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// No core file is produced.
    NoCore(NoCore),
    /// A file named by the template; `truncated_to` is set when the limit would cut it.
    File { template: String, truncated_to: Option<u64> },
    /// The core goes to a program's stdin and `RLIMIT_CORE` is not applied to it;
    /// the program decides whether to keep anything.
    HandlerDecides { command: String },
}

/// Combines `RLIMIT_CORE`, `core_pattern` and `core_uses_pid` the way core(5) describes.
///
/// The case worth remembering is the pipe: `ulimit -c 0` does **not** stop a piped core, because
/// "RLIMIT_CORE will be ignored if the system is configured to pipe core dumps to a program".
/// It does not model the other reasons in core(5) (unwritable directory, full filesystem,
/// unreadable binary, set-user-ID program, ...): those depend on the crashing process, not on
/// these three inputs.
pub fn verdict(limit: CoreLimit, pattern: &CorePattern, core_uses_pid: bool) -> Verdict {
    match pattern {
        CorePattern::Pipe(command) => Verdict::HandlerDecides { command: command.clone() },
        CorePattern::File(template) => {
            if template.is_empty() && !core_uses_pid {
                return Verdict::NoCore(NoCore::EmptyPattern);
            }
            match limit {
                CoreLimit::Zero => Verdict::NoCore(NoCore::LimitIsZero),
                CoreLimit::Unlimited => Verdict::File { template: template.clone(), truncated_to: None },
                CoreLimit::Bytes(n) => Verdict::File { template: template.clone(), truncated_to: Some(n) },
            }
        }
    }
}

/// Parses `/proc/<pid>/coredump_filter`: a hexadecimal bit mask, printed as `00000033`
/// (a leading `0x` is accepted too).
pub fn parse_coredump_filter(raw: &str) -> Result<u32, ParseIntError> {
    let text = raw.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u32::from_str_radix(digits, 16)
}

/// The mapping types selected by each bit of `coredump_filter` (core(5), "/proc/pid/coredump_filter").
const FILTER_BITS: [&str; 9] = [
    "anonymous private mappings",
    "anonymous shared mappings",
    "file-backed private mappings",
    "file-backed shared mappings",
    "ELF headers",
    "private huge pages",
    "shared huge pages",
    "private DAX pages",
    "shared DAX pages",
];

/// Names of the mapping types a filter value dumps; bits above 8 are ignored.
pub fn filter_names(mask: u32) -> Vec<&'static str> {
    FILTER_BITS
        .iter()
        .enumerate()
        .filter(|&(bit, _)| mask & (1 << bit) != 0)
        .map(|(_, name)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEMD: &str = "|/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h\n";

    #[test]
    fn classifies_file_templates_and_pipes() {
        assert_eq!(classify_core_pattern("core\n"), CorePattern::File("core".into()));
        assert_eq!(classify_core_pattern("core.%p"), CorePattern::File("core.%p".into()));
        assert_eq!(classify_core_pattern(""), CorePattern::File(String::new()));
        assert_eq!(
            classify_core_pattern(SYSTEMD),
            CorePattern::Pipe("/usr/lib/systemd/systemd-coredump %P %u %g %s %t %c %h".into())
        );
        // Only the *first* character counts; a pipe symbol later in the template is part of the name.
        assert_eq!(classify_core_pattern("a|b"), CorePattern::File("a|b".into()));
    }

    #[test]
    fn rlimit_zero_means_no_core_for_a_file_pattern() {
        let p = CorePattern::File("core".into());
        assert_eq!(verdict(CoreLimit::Zero, &p, false), Verdict::NoCore(NoCore::LimitIsZero));
    }

    /// The counter-intuitive case of chapter 05 §3: with a piped pattern a zero limit changes nothing.
    #[test]
    fn rlimit_zero_does_not_stop_a_piped_core() {
        let p = classify_core_pattern(SYSTEMD);
        for limit in [CoreLimit::Zero, CoreLimit::Bytes(1), CoreLimit::Unlimited] {
            assert!(matches!(verdict(limit, &p, false), Verdict::HandlerDecides { .. }), "{limit:?}");
        }
    }

    #[test]
    fn a_nonzero_limit_truncates_instead_of_forbidding() {
        let p = CorePattern::File("core.%p".into());
        assert_eq!(
            verdict(CoreLimit::Bytes(1 << 20), &p, false),
            Verdict::File { template: "core.%p".into(), truncated_to: Some(1 << 20) }
        );
        assert_eq!(
            verdict(CoreLimit::Unlimited, &p, false),
            Verdict::File { template: "core.%p".into(), truncated_to: None }
        );
    }

    #[test]
    fn empty_pattern_needs_core_uses_pid() {
        let empty = CorePattern::File(String::new());
        assert_eq!(verdict(CoreLimit::Unlimited, &empty, false), Verdict::NoCore(NoCore::EmptyPattern));
        assert!(matches!(verdict(CoreLimit::Unlimited, &empty, true), Verdict::File { .. }));
    }

    #[test]
    fn rlimit_conversion_handles_the_three_cases() {
        let inf = u64::MAX;
        assert_eq!(CoreLimit::from_rlim(0, inf), CoreLimit::Zero);
        assert_eq!(CoreLimit::from_rlim(inf, inf), CoreLimit::Unlimited);
        assert_eq!(CoreLimit::from_rlim(4096, inf), CoreLimit::Bytes(4096));
        // On macOS RLIM_INFINITY is 2^63 - 1, not u64::MAX.
        assert_eq!(CoreLimit::from_rlim(i64::MAX as u64, i64::MAX as u64), CoreLimit::Unlimited);
    }

    /// 0x33 is the documented default: bits 0, 1, 4 and 5.
    #[test]
    fn default_filter_value_decodes_to_the_documented_bits() {
        let mask = parse_coredump_filter("00000033\n").unwrap();
        assert_eq!(mask, 0x33);
        assert_eq!(
            filter_names(mask),
            ["anonymous private mappings", "anonymous shared mappings", "ELF headers", "private huge pages"]
        );
    }

    #[test]
    fn filter_parsing_accepts_prefix_and_rejects_garbage() {
        assert_eq!(parse_coredump_filter("0x33").unwrap(), 0x33);
        assert_eq!(parse_coredump_filter("  1ff ").unwrap(), 0x1ff);
        assert_eq!(filter_names(0x1ff).len(), 9);
        assert!(parse_coredump_filter("").is_err());
        assert!(parse_coredump_filter("0xzz").is_err());
        assert!(parse_coredump_filter("100000000").is_err(), "does not fit in 32 bits");
        assert!(filter_names(0).is_empty());
    }
}
