//! Lab (g): a small `readelf` — what is inside an ELF executable (chapters 04 §4–§7, 06).
//!
//!   elf_probe <file> [--sections]     header, segments, interpreter, dynamic section, symbol/debug summary
//!   elf_probe sym <file> <text>       symbols whose name contains <text>, with the file offset a uprobe needs
//!   elf_probe dump <file> <section> [max_bytes=256]   hex dump of one section (`readelf -x`)
//!   elf_probe self                    the same summary for this very executable
//!
//! It reads the file only, so it runs on any OS: on macOS it can look inside the Linux binaries
//! that `./labs/linux.sh` builds under `labs/target-linux/`, and it says "not an ELF file" about
//! a macOS executable.

use lab_observability::elf::{
    Elf, ElfError, Kind, SHT_NOBITS, machine_name, section_flags, section_type_name, segment_flags, segment_type_name, type_name,
};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["self"] => std::env::current_exe().map_err(|e| e.to_string()).and_then(|p| summary(&p, false)),
        ["sym", file, needle] => symbols(&PathBuf::from(file), needle),
        ["dump", file, section] => dump(&PathBuf::from(file), section, 256),
        ["dump", file, section, max] => dump(&PathBuf::from(file), section, max.parse().unwrap_or(256)),
        [file] => summary(&PathBuf::from(file), false),
        [file, "--sections"] => summary(&PathBuf::from(file), true),
        _ => {
            eprintln!("usage: elf_probe <file> [--sections] | sym <file> <text> | dump <file> <section> [max_bytes] | self");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("elf_probe: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn load(path: &std::path::Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn parse<'a>(path: &std::path::Path, data: &'a [u8]) -> Result<Elf<'a>, String> {
    Elf::parse(data).map_err(|e: ElfError| format!("{}: {e}", path.display()))
}

fn summary(path: &std::path::Path, all_sections: bool) -> Result<(), String> {
    let data = load(path)?;
    let elf = parse(path, &data)?;
    let h = &elf.header;
    let fail = |e: ElfError| format!("{}: {e}", path.display());

    println!("file: {} ({} bytes)", path.display(), data.len());
    println!("ELF header:");
    println!("  type:     {}", type_name(h.e_type));
    println!("  machine:  {}", machine_name(h.e_machine));
    println!("  entry:    {:#x}", h.e_entry);
    println!("  segments: {} program headers at offset {} ({} bytes each)", elf.program_headers.len(), h.e_phoff, h.e_phentsize);
    println!("  sections: {} section headers at offset {} ({} bytes each)", elf.sections.len(), h.e_shoff, h.e_shentsize);
    println!("kind: {}", elf.kind().describe());
    println!("interpreter: {}", elf.interp().map_err(fail)?.as_deref().unwrap_or("none (no PT_INTERP)"));

    println!("segments (the loader's view):");
    println!("  {:<13} {:<5} {:>10} {:>12} {:>9} {:>9} {:>7}", "type", "flags", "offset", "vaddr", "filesz", "memsz", "align");
    for p in &elf.program_headers {
        println!(
            "  {:<13} {:<5} {:>#10x} {:>#12x} {:>#9x} {:>#9x} {:>#7x}",
            segment_type_name(p.p_type),
            segment_flags(p.p_flags),
            p.p_offset,
            p.p_vaddr,
            p.p_filesz,
            p.p_memsz,
            p.p_align
        );
    }

    match elf.dynamic().map_err(fail)? {
        None => println!("dynamic (what ld.so reads): none (no PT_DYNAMIC)"),
        Some(d) => {
            println!("dynamic (what ld.so reads):");
            for lib in &d.needed {
                println!("  NEEDED   {lib}");
            }
            println!("  SONAME   {}", d.soname.as_deref().unwrap_or("-"));
            println!("  RPATH    {}", d.rpath.as_deref().unwrap_or("-"));
            println!("  RUNPATH  {}", d.runpath.as_deref().unwrap_or("-"));
            println!("  FLAGS    {}", d.flags.map_or_else(|| "-".to_string(), |f| format!("{f:#x}")));
            println!("  bind now (no lazy binding): {}", if d.bind_now { "yes" } else { "no" });
        }
    }
    let relro = elf.program_headers.iter().any(|p| p.p_type == lab_observability::elf::PT_GNU_RELRO);
    println!("PT_GNU_RELRO present: {}", if relro { "yes" } else { "no" });

    println!("symbols and debug data (the tools' view):");
    let count = |dynamic: bool| elf.symbols(dynamic).map(|s| s.len()).map_err(fail);
    for (name, dynamic) in [(".symtab", false), (".dynsym", true)] {
        match elf.section(name) {
            Some(_) => println!("  {name}: present, {} symbols", count(dynamic)?),
            None => println!("  {name}: absent"),
        }
    }
    let debug = elf.debug_sections();
    if debug.is_empty() {
        println!("  .debug_* sections: none");
    } else {
        let total: u64 = debug.iter().map(|s| s.sh_size).sum();
        println!("  .debug_* sections: {} sections, {total} bytes in total", debug.len());
        for s in &debug {
            println!("    {:<18} {:>10} bytes", s.name, s.sh_size);
        }
    }
    for name in [".eh_frame", ".eh_frame_hdr", ".gnu_debuglink"] {
        match elf.section(name) {
            Some(s) => println!("  {name}: present, {} bytes", s.sh_size),
            None => println!("  {name}: absent"),
        }
    }
    println!("  build id: {}", elf.build_id().map_err(fail)?.as_deref().unwrap_or("none"));

    if all_sections {
        println!("sections (the linker's view):");
        println!("  {:>4} {:<24} {:<9} {:<6} {:>12} {:>10} {:>10}", "nr", "name", "type", "flags", "addr", "offset", "size");
        for (i, s) in elf.sections.iter().enumerate() {
            let in_file = if s.sh_type == SHT_NOBITS { " (no file bytes)" } else { "" };
            println!(
                "  {i:>4} {:<24} {:<9} {:<6} {:>#12x} {:>#10x} {:>#10x}{in_file}",
                s.name,
                section_type_name(s.sh_type),
                section_flags(s.sh_flags),
                s.sh_addr,
                s.sh_offset,
                s.sh_size
            );
        }
    }
    if matches!(elf.kind(), Kind::Relocatable) {
        println!("note: a relocatable object has no PT_LOAD segments; addresses are section-relative");
    }
    Ok(())
}

/// `readelf -x`-style hex dump of the first `max` bytes of a section: offset, hex, printable ASCII.
fn dump(path: &std::path::Path, name: &str, max: usize) -> Result<(), String> {
    let data = load(path)?;
    let elf = parse(path, &data)?;
    let fail = |e: ElfError| format!("{}: {e}", path.display());
    let section = elf.section(name).ok_or_else(|| format!("{}: no section named {name}", path.display()))?;
    let bytes = elf.section_bytes(section).map_err(fail)?;
    println!("section {name}: {} bytes (showing {})", bytes.len(), bytes.len().min(max));
    for (i, chunk) in bytes.iter().take(max).copied().collect::<Vec<u8>>().chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }).collect();
        println!("  {:#06x}  {:<47}  |{ascii}|", i * 16, hex.join(" "));
    }
    Ok(())
}

fn symbols(path: &std::path::Path, needle: &str) -> Result<(), String> {
    let data = load(path)?;
    let elf = parse(path, &data)?;
    let fail = |e: ElfError| format!("{}: {e}", path.display());
    println!("symbols containing {needle:?} in {}", path.display());
    let mut found = 0;
    for (table, dynamic) in [(".symtab", false), (".dynsym", true)] {
        let syms = elf.symbols(dynamic).map_err(fail)?;
        for s in syms.iter().filter(|s| s.name.contains(needle) && s.shndx != 0) {
            found += 1;
            let offset = elf.vaddr_to_offset(s.value).map_or_else(|| "(not in a PT_LOAD file image)".to_string(), |o| format!("{o:#x}"));
            println!(
                "  [{table}] {} value={:#x} size={} type={} binding={}",
                s.name,
                s.value,
                s.size,
                s.kind(),
                s.binding()
            );
            if s.is_function() {
                println!("           uprobe offset (value -> file offset through its PT_LOAD): {offset}");
            }
        }
    }
    if found == 0 {
        println!("  none (is the binary stripped, or is the name inlined away?)");
    }
    Ok(())
}
