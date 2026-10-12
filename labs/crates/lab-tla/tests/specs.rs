//! The TLA+ specs in `specs/` are meant for the TLA+ tools (Java), which cargo does not run, and
//! the author of this lab did not run them either. This test is the mechanical check that is
//! possible without them. It proves three things and nothing more:
//!
//! 1. every `.tla` file parses as TLA+ (with embedded PlusCal) under the tree-sitter grammar,
//!    which is permissive and is *not* SANY: it catches typos and unbalanced constructs, not
//!    undefined names or level errors;
//! 2. every `.cfg` names only things its module defines, and assigns exactly the constants the
//!    module declares;
//! 3. the Rust models in this crate use the labels, actions and variables of the specs, so "the
//!    same model in Rust" is more than a claim.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use lab_tla::commit::{TCommit, TwoPhase};
use lab_tla::counter::{Counter, LOCKED, LOST, Program, SPLIT};
use lab_tla::mc::Model;
use lab_tla::queue::Queue;
use lab_tla::request::RequestService;
use tree_sitter::{Node, Parser};

fn specs_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("specs")
}

fn files_with_extension(ext: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(specs_dir())
        .expect("specs/ exists")
        .map(|e| e.expect("readable entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    out.sort();
    out
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .expect("utf-8 file name")
        .to_string()
}

fn module_source(module: &str) -> String {
    read(&specs_dir().join(format!("{module}.tla")))
}

// ---------------------------------------------------------------- 1. syntax

fn collect_errors(node: Node<'_>, src: &str, out: &mut Vec<String>) {
    if node.is_error() || node.is_missing() {
        let p = node.start_position();
        let snippet: String = src[node.byte_range()].chars().take(50).collect();
        out.push(format!(
            "{}:{} {} {snippet:?}",
            p.row + 1,
            p.column + 1,
            if node.is_missing() {
                "MISSING"
            } else {
                "ERROR"
            }
        ));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_errors(child, src, out);
    }
}

fn syntax_errors(src: &str) -> Vec<String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_tlaplus::LANGUAGE.into())
        .expect("the TLA+ grammar loads");
    let tree = parser.parse(src, None).expect("the parser returns a tree");
    let mut errors = Vec::new();
    collect_errors(tree.root_node(), src, &mut errors);
    errors
}

#[test]
fn every_tla_file_parses_without_syntax_errors() {
    let files = files_with_extension("tla");
    assert!(
        files.len() >= 8,
        "expected the lab's specs, found {files:?}"
    );
    for path in files {
        let errors = syntax_errors(&read(&path));
        assert!(errors.is_empty(), "{}: {errors:#?}", path.display());
    }
}

#[test]
fn the_parser_does_reject_broken_specs() {
    // Guard against a vacuous check: the same parser must flag typos in TLA+ and in PlusCal.
    assert!(
        !syntax_errors(
            "---- MODULE M ----\nVARIABLE x\nInit == x = 0\nSpec == Init /\\ [][x' = x]_<<x\n====\n"
        )
        .is_empty()
    );
    let pluscal = module_source("CounterLost");
    assert!(!syntax_errors(&pluscal.replace("end process;", "end procss;")).is_empty());
    assert!(!syntax_errors(&pluscal.replace("tmp := counter;", "tmp := := counter;")).is_empty());
}

// ------------------------------------------------------ 2. cfg versus module

const TRANSLATED: [&str; 6] = ["Init", "Next", "Spec", "Termination", "vars", "ProcSet"];

/// Does a line of `src` start a definition `name == ...` or `name(args) == ...`?
fn defines(src: &str, name: &str) -> bool {
    src.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix(name) else {
            return false;
        };
        let rest = rest.trim_start();
        match rest.strip_prefix('(') {
            Some(after) => after
                .split_once(')')
                .is_some_and(|(_, tail)| tail.trim_start().starts_with("==")),
            None => rest.starts_with("=="),
        }
    })
}

fn strip_line_comment(line: &str) -> &str {
    line.split("\\*").next().unwrap_or("")
}

/// Names after `CONSTANT`/`CONSTANTS` at the start of a line, one declaration per line.
fn declared_constants(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        let line = strip_line_comment(line).trim();
        let rest = line
            .strip_prefix("CONSTANTS")
            .or_else(|| line.strip_prefix("CONSTANT"));
        if let Some(rest) = rest.filter(|r| r.starts_with(char::is_whitespace)) {
            out.extend(
                rest.split(',')
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty()),
            );
        }
    }
    out
}

#[derive(Default, Debug)]
struct Cfg {
    specification: Vec<String>,
    invariants: Vec<String>,
    properties: Vec<String>,
    constants: BTreeSet<String>,
    check_deadlock: Option<bool>,
}

const KEYWORDS: [&str; 15] = [
    "SPECIFICATION",
    "INIT",
    "NEXT",
    "CONSTANT",
    "CONSTANTS",
    "INVARIANT",
    "INVARIANTS",
    "PROPERTY",
    "PROPERTIES",
    "CHECK_DEADLOCK",
    "SYMMETRY",
    "CONSTRAINT",
    "CONSTRAINTS",
    "VIEW",
    "ALIAS",
];

fn parse_cfg(src: &str) -> Cfg {
    let mut cfg = Cfg::default();
    let mut section = String::new();
    for line in src.lines() {
        let line = strip_line_comment(line).trim();
        if line.is_empty() {
            continue;
        }
        let (first, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = if KEYWORDS.contains(&first) {
            section = first.to_string();
            rest.trim()
        } else {
            line
        };
        match section.as_str() {
            "SPECIFICATION" => cfg
                .specification
                .extend(rest.split_whitespace().map(String::from)),
            "INVARIANT" | "INVARIANTS" => cfg
                .invariants
                .extend(rest.split_whitespace().map(String::from)),
            "PROPERTY" | "PROPERTIES" => cfg
                .properties
                .extend(rest.split_whitespace().map(String::from)),
            "CHECK_DEADLOCK" => cfg.check_deadlock = Some(rest == "TRUE"),
            "CONSTANT" | "CONSTANTS" if !rest.is_empty() => {
                let (name, value) = rest
                    .split_once('=')
                    .unwrap_or_else(|| panic!("constant without a value: {rest:?}"));
                assert!(
                    !value.trim().is_empty(),
                    "constant without a value: {rest:?}"
                );
                cfg.constants.insert(name.trim().to_string());
            }
            _ => {}
        }
    }
    cfg
}

/// The module a cfg belongs to: the longest module name that prefixes the cfg's file name
/// (`AtLeastOnceNoDedup.cfg` -> `AtLeastOnce`, `AtLeastOnceSplit.cfg` -> `AtLeastOnceSplit`).
fn module_of(cfg_stem: &str, modules: &[String]) -> String {
    modules
        .iter()
        .filter(|m| cfg_stem.starts_with(m.as_str()))
        .max_by_key(|m| m.len())
        .unwrap_or_else(|| panic!("no module for {cfg_stem}.cfg"))
        .clone()
}

#[test]
fn every_cfg_names_only_things_its_module_defines() {
    let modules: Vec<String> = files_with_extension("tla")
        .iter()
        .map(|p| stem(p))
        .collect();
    let cfgs = files_with_extension("cfg");
    assert!(cfgs.len() >= 14, "{cfgs:?}");
    for path in cfgs {
        let name = stem(&path);
        let module = module_of(&name, &modules);
        let src = module_source(&module);
        let translated = src.contains("--algorithm");
        let cfg = parse_cfg(&read(&path));
        let known = |n: &str| defines(&src, n) || (translated && TRANSLATED.contains(&n));
        assert_eq!(
            cfg.specification.len(),
            1,
            "{name}.cfg needs exactly one SPECIFICATION"
        );
        for n in cfg
            .specification
            .iter()
            .chain(&cfg.invariants)
            .chain(&cfg.properties)
        {
            assert!(known(n), "{name}.cfg: `{n}` is not defined in {module}.tla");
        }
        assert_eq!(
            cfg.constants,
            declared_constants(&src),
            "{name}.cfg must assign exactly the constants {module}.tla declares"
        );
        assert!(
            !cfg.invariants.is_empty() || !cfg.properties.is_empty(),
            "{name}.cfg checks nothing"
        );
        // A spec whose algorithm never terminates (the queue) or whose protocol ends in a state with no
        // step (TCommit) must switch the deadlock check off, as the cfg files of the Examples do.
        if module.starts_with("AtLeastOnce") || module == "TCommit" {
            assert_eq!(cfg.check_deadlock, Some(false), "{name}.cfg");
        }
    }
}

#[test]
fn pluscal_files_ship_an_empty_translation_for_pcal_trans_to_fill() {
    let mut checked = 0;
    for path in files_with_extension("tla") {
        let src = read(&path);
        if !src.contains("--algorithm") {
            continue;
        }
        checked += 1;
        let lines: Vec<&str> = src.lines().map(str::trim).collect();
        let begin = lines
            .iter()
            .position(|l| *l == "\\* BEGIN TRANSLATION")
            .expect("BEGIN TRANSLATION marker");
        assert_eq!(
            lines.get(begin + 1),
            Some(&"\\* END TRANSLATION"),
            "{}: the translation is generated, not hand-written",
            path.display()
        );
        assert!(src.contains("end algorithm; *)"), "{}", path.display());
    }
    assert_eq!(checked, 5);
}

// ------------------------------------------------- 3. Rust models <-> specs

fn contains_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// The labels of the PlusCal algorithm in `src`, in order of appearance.
fn pluscal_labels(src: &str) -> Vec<String> {
    let start = src.find("--algorithm").expect("a PlusCal algorithm");
    let end = src.find("end algorithm").expect("end of the algorithm");
    src[start..end]
        .lines()
        .filter_map(|line| {
            let line = strip_line_comment(line).trim_start();
            let (label, rest) = line.split_once(':')?;
            let valid = !label.is_empty()
                && label.chars().all(char::is_alphanumeric)
                && label.starts_with(char::is_uppercase);
            (valid && !rest.starts_with('=')).then(|| label.to_string())
        })
        .collect()
}

fn program_labels(program: Program) -> Vec<String> {
    program.iter().map(|(label, _)| label.to_string()).collect()
}

/// The action names of a model: `Load(1)` -> `Load`, `Ack(c1) [crash]` -> `Ack`.
fn action_names<M: Model>(model: &M) -> BTreeSet<String> {
    model
        .actions()
        .iter()
        .map(|a| format!("{a:?}"))
        .map(|s| s.split(['(', ' ']).next().unwrap_or("").to_string())
        .collect()
}

fn variables_shown(show: &str) -> Vec<String> {
    show.lines()
        .filter_map(|l| l.strip_prefix("/\\ "))
        .filter_map(|l| l.split_once(" ="))
        .map(|(n, _)| n.to_string())
        .collect()
}

#[test]
fn the_counter_models_have_the_labels_of_their_specs() {
    for (module, program) in [
        ("CounterLost", LOST),
        ("CounterLock", LOCKED),
        ("CounterLockSplit", SPLIT),
    ] {
        assert_eq!(
            pluscal_labels(&module_source(module)),
            program_labels(program),
            "{module}"
        );
    }
}

#[test]
fn the_queue_models_have_the_labels_of_their_specs() {
    for (module, model) in [
        ("AtLeastOnce", Queue::idempotent(1, 1, 1)),
        ("AtLeastOnceSplit", Queue::split(1, 1, 1)),
    ] {
        let from_spec: BTreeSet<String> =
            pluscal_labels(&module_source(module)).into_iter().collect();
        assert_eq!(from_spec, action_names(&model), "{module}");
    }
}

#[test]
fn the_other_models_have_the_actions_of_their_specs() {
    let two_phase = module_source("TwoPhase");
    let next = two_phase
        .split("TPNext ==")
        .nth(1)
        .expect("TPNext is defined")
        .split("-----")
        .next()
        .unwrap();
    for name in action_names(&TwoPhase::new(2)) {
        assert!(
            defines(&two_phase, &name),
            "TwoPhase.tla does not define {name}"
        );
        assert!(contains_word(next, &name), "TPNext does not mention {name}");
    }
    let tcommit = module_source("TCommit");
    for name in action_names(&TCommit::new(2)) {
        assert!(
            defines(&tcommit, &name),
            "TCommit.tla does not define {name}"
        );
    }
    let request = module_source("RequestService");
    for name in action_names(&RequestService { reqs: 2 }) {
        assert!(
            defines(&request, &name),
            "RequestService.tla does not define {name}"
        );
    }
}

#[test]
fn the_models_print_the_variables_of_their_specs() {
    // `pc` is introduced by the PlusCal translator, so it never appears in the source text.
    let check = |module: &str, shown: Vec<String>| {
        let src = module_source(module);
        for var in shown.iter().filter(|v| *v != "pc") {
            assert!(
                contains_word(&src, var),
                "{module}.tla never mentions the variable `{var}` that the Rust model prints"
            );
        }
    };
    for (module, model) in [
        ("CounterLost", Counter::lost_update(2)),
        ("CounterLock", Counter::with_lock(2)),
        ("CounterLockSplit", Counter::lock_split(2)),
    ] {
        check(module, variables_shown(&model.show(&model.init()[0])));
    }
    let queue = Queue::idempotent(1, 1, 1);
    check(
        "AtLeastOnce",
        variables_shown(&queue.show(&queue.init()[0])),
    );
    let two_phase = TwoPhase::new(2);
    check(
        "TwoPhase",
        variables_shown(&TwoPhase::show(&two_phase.init()[0])),
    );
    check(
        "TCommit",
        variables_shown(&TCommit::show(&TCommit::new(2).init()[0])),
    );
    check(
        "RequestService",
        variables_shown(&RequestService::show(&RequestService { reqs: 2 }.init()[0])),
    );
}
