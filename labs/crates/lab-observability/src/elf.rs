//! A small ELF64 reader, in the spirit of `readelf` (chapter 06; chapter 04 §4–§7 for symbols,
//! debug sections and the build ID).
//!
//! Scope: ELF64, little-endian — the format of every x86-64 and AArch64 Linux binary the labs
//! produce. It reads the **file only** (no `/proc`, no loader), so it runs on any OS: on macOS
//! it can inspect the Linux binaries that the lab container builds, and it says "this is not
//! ELF" about a macOS executable.
//!
//! Every offset and length taken from the file is checked against the file size with checked
//! arithmetic; a corrupt or truncated file yields an [`ElfError`], never a panic.

use std::fmt;

/// `e_type` values.
pub const ET_REL: u16 = 1;
pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;
pub const ET_CORE: u16 = 4;

/// `p_type` values.
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;
pub const PT_GNU_EH_FRAME: u32 = 0x6474_e550;
pub const PT_GNU_STACK: u32 = 0x6474_e551;
pub const PT_GNU_RELRO: u32 = 0x6474_e552;

/// `sh_type` values.
pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_DYNAMIC: u32 = 6;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_DYNSYM: u32 = 11;

/// `d_tag` values of the `.dynamic` array.
pub const DT_NULL: i64 = 0;
pub const DT_NEEDED: i64 = 1;
pub const DT_STRTAB: i64 = 5;
pub const DT_STRSZ: i64 = 10;
pub const DT_SONAME: i64 = 14;
pub const DT_RPATH: i64 = 15;
pub const DT_BIND_NOW: i64 = 24;
pub const DT_RUNPATH: i64 = 29;
pub const DT_FLAGS: i64 = 30;
/// `DT_FLAGS` bit: process all relocations before transferring control (no lazy binding).
pub const DF_BIND_NOW: u64 = 0x8;

/// `PN_XNUM` / `SHN_XINDEX`: "the real value is in section header 0".
const XNUM: u16 = 0xffff;

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const SHDR_SIZE: usize = 64;
const SYM_SIZE: usize = 24;
const DYN_SIZE: usize = 16;

/// Why a file could not be read as an ELF64 little-endian image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    /// The file ends before a fixed-size part of the format does.
    TooShort { what: &'static str, need: usize, have: usize },
    /// The first four bytes are not `\x7fELF`.
    BadMagic([u8; 4]),
    /// `EI_CLASS` is not 2 (`ELFCLASS64`).
    UnsupportedClass(u8),
    /// `EI_DATA` is not 1 (`ELFDATA2LSB`).
    UnsupportedEncoding(u8),
    /// A table or blob described by the file lies (partly) outside the file, or its entry size is too small.
    BadTable { what: &'static str, offset: u64, size: u64, file_len: usize },
    /// A name offset points outside its string table, or the string is not NUL-terminated.
    BadString { what: &'static str, offset: u64 },
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ElfError::TooShort { what, need, have } => write!(f, "file too short for {what}: need {need} bytes, have {have}"),
            ElfError::BadMagic(m) => write!(
                f,
                "not an ELF file: first bytes are {:02x} {:02x} {:02x} {:02x}, expected 7f 45 4c 46",
                m[0], m[1], m[2], m[3]
            ),
            ElfError::UnsupportedClass(c) => write!(f, "unsupported EI_CLASS {c} (only 2 = ELFCLASS64 is handled)"),
            ElfError::UnsupportedEncoding(d) => write!(f, "unsupported EI_DATA {d} (only 1 = little-endian is handled)"),
            ElfError::BadTable { what, offset, size, file_len } => {
                write!(f, "{what} at offset {offset:#x} with size {size:#x} does not fit in a file of {file_len} bytes")
            }
            ElfError::BadString { what, offset } => write!(f, "{what}: no NUL-terminated string at offset {offset:#x}"),
        }
    }
}

impl std::error::Error for ElfError {}

/// The ELF header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub os_abi: u8,
    pub e_type: u16,
    pub e_machine: u16,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

/// One program header: a **segment**, the loader's unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/// One section header: a **section**, the linker's unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionHeader {
    pub name: String,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
}

/// One entry of `.symtab` or `.dynsym`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub info: u8,
    pub shndx: u16,
    pub value: u64,
    pub size: u64,
}

impl Symbol {
    /// `STT_*`: the low nibble of `st_info` (2 = function, 1 = data object).
    pub fn kind(&self) -> u8 {
        self.info & 0xf
    }

    /// `STB_*`: the high nibble of `st_info` (0 = local, 1 = global, 2 = weak).
    pub fn binding(&self) -> u8 {
        self.info >> 4
    }

    pub fn is_function(&self) -> bool {
        self.kind() == 2
    }
}

/// What the loader reads from `PT_DYNAMIC` (`readelf -d`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DynamicInfo {
    pub needed: Vec<String>,
    pub soname: Option<String>,
    pub rpath: Option<String>,
    pub runpath: Option<String>,
    /// The raw `DT_FLAGS` value, if present.
    pub flags: Option<u64>,
    /// `DT_BIND_NOW` is present, or `DT_FLAGS` has `DF_BIND_NOW`: no lazy binding.
    pub bind_now: bool,
}

/// Coarse classification from `e_type` and the presence of `PT_INTERP` (chapter 06 §1, §3, §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `ET_REL`: an object file (`.o`).
    Relocatable,
    /// `ET_CORE`: a core dump.
    Core,
    /// `ET_EXEC` with an interpreter: linked at a fixed address, needs `ld.so`.
    DynamicFixedAddress,
    /// `ET_EXEC` without an interpreter: self-contained, fixed address (a classic static executable).
    StaticFixedAddress,
    /// `ET_DYN` with an interpreter: a dynamically linked PIE executable.
    DynamicPie,
    /// `ET_DYN` without an interpreter: a static-PIE executable or a shared object.
    DynNoInterp,
    /// Any other `e_type`.
    Other(u16),
}

impl Kind {
    pub fn describe(self) -> String {
        match self {
            Kind::Relocatable => "relocatable object (ET_REL)".into(),
            Kind::Core => "core file (ET_CORE)".into(),
            Kind::DynamicFixedAddress => "ET_EXEC + PT_INTERP: dynamic executable at a fixed address (not PIE)".into(),
            Kind::StaticFixedAddress => "ET_EXEC, no PT_INTERP: static executable at a fixed address (not PIE)".into(),
            Kind::DynamicPie => "ET_DYN + PT_INTERP: dynamically linked PIE executable".into(),
            Kind::DynNoInterp => "ET_DYN, no PT_INTERP: static-PIE executable or shared object".into(),
            Kind::Other(t) => format!("unknown e_type {t}"),
        }
    }
}

/// A parsed ELF64 image borrowing the file's bytes.
#[derive(Debug)]
pub struct Elf<'a> {
    data: &'a [u8],
    pub header: Header,
    pub program_headers: Vec<ProgramHeader>,
    pub sections: Vec<SectionHeader>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// `data[offset .. offset + len]`, or `BadTable` if any part lies outside the file.
fn region<'a>(data: &'a [u8], what: &'static str, offset: u64, len: u64) -> Result<&'a [u8], ElfError> {
    let bad = ElfError::BadTable { what, offset, size: len, file_len: data.len() };
    let end = offset.checked_add(len).ok_or_else(|| bad.clone())?;
    if end > data.len() as u64 {
        return Err(bad);
    }
    Ok(&data[offset as usize..end as usize])
}

/// The NUL-terminated string at `offset` inside `table`.
fn cstr<'a>(table: &'a [u8], what: &'static str, offset: u64) -> Result<std::borrow::Cow<'a, str>, ElfError> {
    let bad = ElfError::BadString { what, offset };
    let tail = table.get(usize::try_from(offset).map_err(|_| bad.clone())?..).ok_or_else(|| bad.clone())?;
    let end = tail.iter().position(|&b| b == 0).ok_or(bad)?;
    Ok(String::from_utf8_lossy(&tail[..end]))
}

fn section_header_at(table: &[u8], entsize: usize, index: usize) -> (SectionHeader, u32) {
    let e = &table[index * entsize..index * entsize + SHDR_SIZE];
    let name_off = u32_at(e, 0);
    (
        SectionHeader {
            name: String::new(),
            sh_type: u32_at(e, 4),
            sh_flags: u64_at(e, 8),
            sh_addr: u64_at(e, 16),
            sh_offset: u64_at(e, 24),
            sh_size: u64_at(e, 32),
            sh_link: u32_at(e, 40),
            sh_info: u32_at(e, 44),
        },
        name_off,
    )
}

impl<'a> Elf<'a> {
    /// Parses the header, the program header table and the section header table (with names).
    pub fn parse(data: &'a [u8]) -> Result<Elf<'a>, ElfError> {
        if data.len() < 4 {
            return Err(ElfError::TooShort { what: "ELF magic", need: 4, have: data.len() });
        }
        if data[..4] != *b"\x7fELF" {
            return Err(ElfError::BadMagic([data[0], data[1], data[2], data[3]]));
        }
        if data.len() < EHDR_SIZE {
            return Err(ElfError::TooShort { what: "ELF header", need: EHDR_SIZE, have: data.len() });
        }
        if data[4] != 2 {
            return Err(ElfError::UnsupportedClass(data[4]));
        }
        if data[5] != 1 {
            return Err(ElfError::UnsupportedEncoding(data[5]));
        }
        let header = Header {
            os_abi: data[7],
            e_type: u16_at(data, 16),
            e_machine: u16_at(data, 18),
            e_entry: u64_at(data, 24),
            e_phoff: u64_at(data, 32),
            e_shoff: u64_at(data, 40),
            e_flags: u32_at(data, 48),
            e_phentsize: u16_at(data, 54),
            e_phnum: u16_at(data, 56),
            e_shentsize: u16_at(data, 58),
            e_shnum: u16_at(data, 60),
            e_shstrndx: u16_at(data, 62),
        };

        // Counts that do not fit the 16-bit header fields are stored in section header 0
        // (PN_XNUM -> sh_info, e_shnum == 0 -> sh_size, SHN_XINDEX -> sh_link).
        let mut phnum = u64::from(header.e_phnum);
        let mut shnum = u64::from(header.e_shnum);
        let mut shstrndx = u64::from(header.e_shstrndx);
        let wants_extension = header.e_phnum == XNUM || header.e_shnum == 0 || header.e_shstrndx == XNUM;
        if header.e_shoff != 0 && wants_extension {
            let first = region(data, "section header 0", header.e_shoff, SHDR_SIZE as u64)?;
            let (s0, _) = section_header_at(first, SHDR_SIZE, 0);
            if header.e_phnum == XNUM {
                phnum = u64::from(s0.sh_info);
            }
            if header.e_shnum == 0 {
                shnum = s0.sh_size;
            }
            if header.e_shstrndx == XNUM {
                shstrndx = u64::from(s0.sh_link);
            }
        }

        let program_headers = Self::read_program_headers(data, &header, phnum)?;
        let sections = Self::read_sections(data, &header, shnum, shstrndx)?;
        Ok(Elf { data, header, program_headers, sections })
    }

    fn read_program_headers(data: &[u8], h: &Header, phnum: u64) -> Result<Vec<ProgramHeader>, ElfError> {
        if h.e_phoff == 0 || phnum == 0 {
            return Ok(Vec::new());
        }
        let entsize = usize::from(h.e_phentsize);
        let total = phnum.saturating_mul(entsize as u64);
        let table = region(data, "program header table", h.e_phoff, total)?;
        if entsize < PHDR_SIZE {
            return Err(ElfError::BadTable { what: "program header entry size", offset: h.e_phoff, size: entsize as u64, file_len: data.len() });
        }
        Ok(table
            .chunks_exact(entsize)
            .map(|e| ProgramHeader {
                p_type: u32_at(e, 0),
                p_flags: u32_at(e, 4),
                p_offset: u64_at(e, 8),
                p_vaddr: u64_at(e, 16),
                p_filesz: u64_at(e, 32),
                p_memsz: u64_at(e, 40),
                p_align: u64_at(e, 48),
            })
            .collect())
    }

    fn read_sections(data: &[u8], h: &Header, shnum: u64, shstrndx: u64) -> Result<Vec<SectionHeader>, ElfError> {
        if h.e_shoff == 0 || shnum == 0 {
            return Ok(Vec::new());
        }
        let entsize = usize::from(h.e_shentsize);
        let total = shnum.saturating_mul(entsize as u64);
        let table = region(data, "section header table", h.e_shoff, total)?;
        if entsize < SHDR_SIZE {
            return Err(ElfError::BadTable { what: "section header entry size", offset: h.e_shoff, size: entsize as u64, file_len: data.len() });
        }
        let mut raw: Vec<(SectionHeader, u32)> = (0..shnum as usize).map(|i| section_header_at(table, entsize, i)).collect();
        if shstrndx != 0 {
            let names_sec = raw.get(shstrndx as usize).map(|(s, _)| s.clone()).ok_or(ElfError::BadTable {
                what: "e_shstrndx",
                offset: shstrndx,
                size: 0,
                file_len: data.len(),
            })?;
            let names = region(data, "section name string table", names_sec.sh_offset, names_sec.sh_size)?;
            for (sec, name_off) in &mut raw {
                sec.name = cstr(names, "section name", u64::from(*name_off))?.into_owned();
            }
        }
        Ok(raw.into_iter().map(|(s, _)| s).collect())
    }

    /// The whole file.
    pub fn bytes(&self) -> &'a [u8] {
        self.data
    }

    /// The first section with this name.
    pub fn section(&self, name: &str) -> Option<&SectionHeader> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The file bytes of a section (empty for `SHT_NOBITS`, which occupies no space in the file).
    pub fn section_bytes(&self, section: &SectionHeader) -> Result<&'a [u8], ElfError> {
        if section.sh_type == SHT_NOBITS {
            return Ok(&[]);
        }
        region(self.data, "section contents", section.sh_offset, section.sh_size)
    }

    /// The program interpreter named by `PT_INTERP` (e.g. `/lib/ld-linux-aarch64.so.1`), if any.
    pub fn interp(&self) -> Result<Option<String>, ElfError> {
        let Some(ph) = self.program_headers.iter().find(|p| p.p_type == PT_INTERP) else {
            return Ok(None);
        };
        let bytes = region(self.data, "PT_INTERP", ph.p_offset, ph.p_filesz)?;
        Ok(Some(cstr(bytes, "PT_INTERP", 0)?.into_owned()))
    }

    /// Classification from `e_type` and `PT_INTERP`.
    pub fn kind(&self) -> Kind {
        let has_interp = self.program_headers.iter().any(|p| p.p_type == PT_INTERP);
        match (self.header.e_type, has_interp) {
            (ET_REL, _) => Kind::Relocatable,
            (ET_CORE, _) => Kind::Core,
            (ET_EXEC, true) => Kind::DynamicFixedAddress,
            (ET_EXEC, false) => Kind::StaticFixedAddress,
            (ET_DYN, true) => Kind::DynamicPie,
            (ET_DYN, false) => Kind::DynNoInterp,
            (other, _) => Kind::Other(other),
        }
    }

    /// Maps a virtual address to a file offset through the `PT_LOAD` segment that holds it.
    /// `None` for addresses outside every segment's *file* image (including the zero-filled
    /// tail of a segment whose `p_memsz` exceeds `p_filesz`, the `.bss`).
    pub fn vaddr_to_offset(&self, vaddr: u64) -> Option<u64> {
        self.program_headers
            .iter()
            .filter(|p| p.p_type == PT_LOAD)
            .find(|p| vaddr >= p.p_vaddr && vaddr - p.p_vaddr < p.p_filesz)
            .and_then(|p| p.p_offset.checked_add(vaddr - p.p_vaddr))
    }

    /// Symbols of `.symtab` (`dynamic = false`) or `.dynsym` (`dynamic = true`); empty if the section is absent.
    pub fn symbols(&self, dynamic: bool) -> Result<Vec<Symbol>, ElfError> {
        let wanted = if dynamic { SHT_DYNSYM } else { SHT_SYMTAB };
        let Some(sec) = self.sections.iter().find(|s| s.sh_type == wanted) else {
            return Ok(Vec::new());
        };
        let entries = self.section_bytes(sec)?;
        let strtab_sec = self.sections.get(sec.sh_link as usize).ok_or(ElfError::BadTable {
            what: "symbol table sh_link",
            offset: u64::from(sec.sh_link),
            size: 0,
            file_len: self.data.len(),
        })?;
        let strtab = self.section_bytes(strtab_sec)?;
        entries
            .chunks_exact(SYM_SIZE)
            .map(|e| {
                Ok(Symbol {
                    name: cstr(strtab, "symbol name", u64::from(u32_at(e, 0)))?.into_owned(),
                    info: e[4],
                    shndx: u16_at(e, 6),
                    value: u64_at(e, 8),
                    size: u64_at(e, 16),
                })
            })
            .collect()
    }

    /// What the loader reads from `PT_DYNAMIC`: needed libraries, `RPATH`/`RUNPATH`, bind-now.
    pub fn dynamic(&self) -> Result<Option<DynamicInfo>, ElfError> {
        let Some(ph) = self.program_headers.iter().find(|p| p.p_type == PT_DYNAMIC) else {
            return Ok(None);
        };
        let raw = region(self.data, "PT_DYNAMIC", ph.p_offset, ph.p_filesz)?;
        let entries: Vec<(i64, u64)> = raw
            .chunks_exact(DYN_SIZE)
            .map(|e| (u64_at(e, 0) as i64, u64_at(e, 8)))
            .take_while(|&(tag, _)| tag != DT_NULL)
            .collect();
        let value_of = |tag: i64| entries.iter().find(|&&(t, _)| t == tag).map(|&(_, v)| v);

        let strtab: &[u8] = match (value_of(DT_STRTAB), value_of(DT_STRSZ)) {
            (Some(addr), Some(size)) => {
                let off = self.vaddr_to_offset(addr).ok_or(ElfError::BadTable {
                    what: "DT_STRTAB (address in no PT_LOAD)",
                    offset: addr,
                    size,
                    file_len: self.data.len(),
                })?;
                region(self.data, "dynamic string table", off, size)?
            }
            _ => &[],
        };
        let string = |tag: i64| -> Result<Option<String>, ElfError> {
            value_of(tag).map(|off| cstr(strtab, "dynamic string", off).map(|s| s.into_owned())).transpose()
        };

        let mut info = DynamicInfo::default();
        for &(tag, val) in &entries {
            if tag == DT_NEEDED {
                info.needed.push(cstr(strtab, "DT_NEEDED", val)?.into_owned());
            }
        }
        info.soname = string(DT_SONAME)?;
        info.rpath = string(DT_RPATH)?;
        info.runpath = string(DT_RUNPATH)?;
        info.flags = value_of(DT_FLAGS);
        info.bind_now = value_of(DT_BIND_NOW).is_some() || info.flags.is_some_and(|f| f & DF_BIND_NOW != 0);
        Ok(Some(info))
    }

    /// The names of the `.debug_*` sections: the DWARF that `-C debuginfo` produced (chapter 04 §5).
    pub fn debug_sections(&self) -> Vec<&SectionHeader> {
        self.sections.iter().filter(|s| s.name.starts_with(".debug_")).collect()
    }

    /// The GNU build ID (`NT_GNU_BUILD_ID` in `.note.gnu.build-id`) as lowercase hex (chapter 04 §7).
    pub fn build_id(&self) -> Result<Option<String>, ElfError> {
        let Some(sec) = self.section(".note.gnu.build-id") else {
            return Ok(None);
        };
        let note = self.section_bytes(sec)?;
        if note.len() < 12 {
            return Err(ElfError::TooShort { what: "build-id note header", need: 12, have: note.len() });
        }
        let (namesz, descsz, kind) = (u32_at(note, 0) as usize, u32_at(note, 4) as usize, u32_at(note, 8));
        const NT_GNU_BUILD_ID: u32 = 3;
        if kind != NT_GNU_BUILD_ID {
            return Ok(None);
        }
        let desc_start = 12usize.saturating_add(namesz.saturating_add(3) & !3);
        let desc = note.get(desc_start..desc_start.saturating_add(descsz)).ok_or(ElfError::TooShort {
            what: "build-id descriptor",
            need: desc_start.saturating_add(descsz),
            have: note.len(),
        })?;
        Ok(Some(desc.iter().map(|b| format!("{b:02x}")).collect()))
    }
}

/// `e_type` as readelf spells it.
pub fn type_name(e_type: u16) -> String {
    match e_type {
        ET_REL => "REL (relocatable file)".into(),
        ET_EXEC => "EXEC (executable file)".into(),
        ET_DYN => "DYN (shared object or PIE)".into(),
        ET_CORE => "CORE (core file)".into(),
        other => format!("{other:#x}"),
    }
}

/// `e_machine` for the architectures the labs meet.
pub fn machine_name(e_machine: u16) -> String {
    match e_machine {
        3 => "Intel 80386".into(),
        40 => "ARM".into(),
        62 => "Advanced Micro Devices X86-64".into(),
        183 => "AArch64".into(),
        243 => "RISC-V".into(),
        other => format!("{other}"),
    }
}

/// `p_type` as readelf spells it.
pub fn segment_type_name(p_type: u32) -> String {
    match p_type {
        0 => "NULL".into(),
        PT_LOAD => "LOAD".into(),
        PT_DYNAMIC => "DYNAMIC".into(),
        PT_INTERP => "INTERP".into(),
        PT_NOTE => "NOTE".into(),
        PT_PHDR => "PHDR".into(),
        PT_TLS => "TLS".into(),
        PT_GNU_EH_FRAME => "GNU_EH_FRAME".into(),
        PT_GNU_STACK => "GNU_STACK".into(),
        PT_GNU_RELRO => "GNU_RELRO".into(),
        other => format!("{other:#x}"),
    }
}

/// `p_flags` as `R W E` letters (`R E` for text, `RW ` for data).
pub fn segment_flags(p_flags: u32) -> String {
    format!(
        "{}{}{}",
        if p_flags & 4 != 0 { 'R' } else { ' ' },
        if p_flags & 2 != 0 { 'W' } else { ' ' },
        if p_flags & 1 != 0 { 'E' } else { ' ' }
    )
}

/// `sh_type` as readelf spells it.
pub fn section_type_name(sh_type: u32) -> String {
    match sh_type {
        0 => "NULL".into(),
        SHT_PROGBITS => "PROGBITS".into(),
        SHT_SYMTAB => "SYMTAB".into(),
        SHT_STRTAB => "STRTAB".into(),
        SHT_RELA => "RELA".into(),
        5 => "HASH".into(),
        SHT_DYNAMIC => "DYNAMIC".into(),
        SHT_NOTE => "NOTE".into(),
        SHT_NOBITS => "NOBITS".into(),
        9 => "REL".into(),
        SHT_DYNSYM => "DYNSYM".into(),
        14 => "INIT_ARRAY".into(),
        15 => "FINI_ARRAY".into(),
        0x6fff_fff6 => "GNU_HASH".into(),
        0x6fff_fffd => "VERDEF".into(),
        0x6fff_fffe => "VERNEED".into(),
        0x6fff_ffff => "VERSYM".into(),
        other => format!("{other:#x}"),
    }
}

/// `sh_flags` as readelf's letters: `W`rite, `A`lloc, e`X`ecute, `M`erge, `S`trings, `I`nfo, `T`LS.
pub fn section_flags(sh_flags: u64) -> String {
    [(0x1, 'W'), (0x2, 'A'), (0x4, 'X'), (0x10, 'M'), (0x20, 'S'), (0x40, 'I'), (0x400, 'T')]
        .iter()
        .filter(|&&(bit, _)| sh_flags & bit != 0)
        .map(|&(_, c)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 0x40_0000;
    const DATA_BASE: u64 = 0x41_0000;

    /// Everything a test needs to know about the image `build` produced.
    struct Built {
        image: Vec<u8>,
        text_vaddr: u64,
        text_off: u64,
        data_vaddr: u64,
        data_off: u64,
        data_filesz: u64,
        bss_vaddr: u64,
    }

    #[derive(Clone, Copy, Default)]
    struct Opts {
        /// Store the section count in section header 0 (`e_shnum = 0`).
        xnum_shnum: bool,
        /// Store the name-table index in section header 0 (`e_shstrndx = 0xffff`).
        xnum_shstrndx: bool,
        /// Put `DF_BIND_NOW` into `DT_FLAGS`.
        bind_now: bool,
    }

    fn put(img: &mut Vec<u8>, at: usize, bytes: &[u8]) {
        if img.len() < at + bytes.len() {
            img.resize(at + bytes.len(), 0);
        }
        img[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn append(img: &mut Vec<u8>, bytes: &[u8]) -> usize {
        let at = img.len();
        img.extend_from_slice(bytes);
        at
    }

    fn align(img: &mut Vec<u8>, to: usize) {
        while !img.len().is_multiple_of(to) {
            img.push(0);
        }
    }

    fn phdr(p_type: u32, flags: u32, off: u64, vaddr: u64, filesz: u64, memsz: u64, align: u64) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend(p_type.to_le_bytes());
        e.extend(flags.to_le_bytes());
        for v in [off, vaddr, vaddr, filesz, memsz, align] {
            e.extend(v.to_le_bytes());
        }
        e
    }

    #[allow(clippy::too_many_arguments)]
    fn shdr(name: u32, ty: u32, flags: u64, addr: u64, off: u64, size: u64, link: u32, info: u32, entsize: u64) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend(name.to_le_bytes());
        e.extend(ty.to_le_bytes());
        e.extend(flags.to_le_bytes());
        e.extend(addr.to_le_bytes());
        e.extend(off.to_le_bytes());
        e.extend(size.to_le_bytes());
        e.extend(link.to_le_bytes());
        e.extend(info.to_le_bytes());
        e.extend(8u64.to_le_bytes());
        e.extend(entsize.to_le_bytes());
        e
    }

    fn sym(name: u32, info: u8, shndx: u16, value: u64, size: u64) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend(name.to_le_bytes());
        e.push(info);
        e.push(0);
        e.extend(shndx.to_le_bytes());
        e.extend(value.to_le_bytes());
        e.extend(size.to_le_bytes());
        e
    }

    fn dyn_entry(tag: i64, val: u64) -> Vec<u8> {
        let mut e = Vec::new();
        e.extend(tag.to_le_bytes());
        e.extend(val.to_le_bytes());
        e
    }

    /// Builds a small but structurally complete dynamically linked PIE-like image:
    /// PT_INTERP, two PT_LOAD with different vaddr-to-offset deltas (the second with a .bss tail),
    /// PT_DYNAMIC, and sections .interp .text .note.gnu.build-id .dynstr .dynamic .symtab .strtab .shstrtab.
    fn build(o: Opts) -> Built {
        let mut img = vec![0u8; EHDR_SIZE + 4 * PHDR_SIZE];

        let interp = b"/lib/ld-test.so.1\0";
        let interp_off = append(&mut img, interp);
        align(&mut img, 16);
        let text_off = append(&mut img, &[0x1f, 0x20, 0x03, 0xd5, 0xc0, 0x03, 0x5f, 0xd6, 0x1f, 0x20, 0x03, 0xd5, 0xc0, 0x03, 0x5f, 0xd6]);
        align(&mut img, 4);
        let mut note = Vec::new();
        note.extend(4u32.to_le_bytes());
        note.extend(20u32.to_le_bytes());
        note.extend(3u32.to_le_bytes());
        note.extend(b"GNU\0");
        note.extend((0..20u8).map(|i| 0xa0 + i));
        let note_off = append(&mut img, &note);
        align(&mut img, 4);
        // .dynstr: "\0libc.so.6\0libm.so.6\0/opt/lib\0demo.so\0"
        let dynstr = b"\0libc.so.6\0libm.so.6\0/opt/lib\0demo.so\0";
        let dynstr_off = append(&mut img, dynstr);
        let seg1_end = img.len();

        // Second segment (data): different file-offset-to-address delta, plus a .bss tail.
        align(&mut img, 0x100);
        let data_off = img.len();
        let mut dynamic = Vec::new();
        let str_off = |table: &[u8], s: &str| -> u64 {
            let needle = [b"\0", s.as_bytes(), b"\0"].concat();
            table.windows(needle.len()).position(|w| w == needle).unwrap() as u64 + 1
        };
        dynamic.extend(dyn_entry(DT_NEEDED, str_off(dynstr, "libc.so.6")));
        dynamic.extend(dyn_entry(DT_NEEDED, str_off(dynstr, "libm.so.6")));
        dynamic.extend(dyn_entry(DT_SONAME, str_off(dynstr, "demo.so")));
        dynamic.extend(dyn_entry(DT_RUNPATH, str_off(dynstr, "/opt/lib")));
        dynamic.extend(dyn_entry(DT_STRTAB, BASE + dynstr_off as u64));
        dynamic.extend(dyn_entry(DT_STRSZ, dynstr.len() as u64));
        if o.bind_now {
            dynamic.extend(dyn_entry(DT_FLAGS, DF_BIND_NOW));
        }
        dynamic.extend(dyn_entry(DT_NULL, 0));
        let dynamic_off = append(&mut img, &dynamic);
        let data_filesz = (img.len() - data_off) as u64;

        // Non-alloc sections: not covered by any segment.
        let strtab = b"\0main\0_ZN4demo3foo17h0123456789abcdefE\0";
        let strtab_off = append(&mut img, strtab);
        align(&mut img, 8);
        let text_vaddr = BASE + text_off as u64;
        let mut symtab = sym(0, 0, 0, 0, 0);
        symtab.extend(sym(1, 0x12, 2, text_vaddr, 8));
        symtab.extend(sym(6, 0x12, 2, text_vaddr + 8, 8));
        let symtab_off = append(&mut img, &symtab);

        let shstr = b"\0.interp\0.text\0.note.gnu.build-id\0.dynstr\0.dynamic\0.symtab\0.strtab\0.shstrtab\0";
        let name_of = |n: &str| -> u32 {
            let needle = format!("\0{n}\0");
            let hay = String::from_utf8_lossy(shstr);
            (hay.find(&needle).unwrap() + 1) as u32
        };
        let shstr_off = append(&mut img, shstr);
        align(&mut img, 8);

        let sections = [
            shdr(0, 0, 0, 0, 0, 0, 0, 0, 0),
            shdr(name_of(".interp"), SHT_PROGBITS, 2, BASE + interp_off as u64, interp_off as u64, interp.len() as u64, 0, 0, 0),
            shdr(name_of(".text"), SHT_PROGBITS, 6, text_vaddr, text_off as u64, 16, 0, 0, 0),
            shdr(name_of(".note.gnu.build-id"), SHT_NOTE, 2, BASE + note_off as u64, note_off as u64, note.len() as u64, 0, 0, 0),
            shdr(name_of(".dynstr"), SHT_STRTAB, 2, BASE + dynstr_off as u64, dynstr_off as u64, dynstr.len() as u64, 0, 0, 0),
            shdr(name_of(".dynamic"), SHT_DYNAMIC, 3, DATA_BASE + dynamic_off as u64, dynamic_off as u64, dynamic.len() as u64, 4, 0, 16),
            shdr(name_of(".symtab"), SHT_SYMTAB, 0, 0, symtab_off as u64, symtab.len() as u64, 7, 1, 24),
            shdr(name_of(".strtab"), SHT_STRTAB, 0, 0, strtab_off as u64, strtab.len() as u64, 0, 0, 0),
            shdr(name_of(".shstrtab"), SHT_STRTAB, 0, 0, shstr_off as u64, shstr.len() as u64, 0, 0, 0),
        ];
        let mut sections: Vec<Vec<u8>> = sections.to_vec();
        if o.xnum_shnum || o.xnum_shstrndx {
            // Section header 0 carries the real values.
            let count = sections.len() as u64;
            sections[0] = shdr(0, 0, 0, 0, 0, if o.xnum_shnum { count } else { 0 }, if o.xnum_shstrndx { 8 } else { 0 }, 0, 0);
        }
        let shoff = img.len();
        for s in &sections {
            img.extend_from_slice(s);
        }

        // Program headers.
        let phdrs = [
            phdr(PT_INTERP, 4, interp_off as u64, BASE + interp_off as u64, interp.len() as u64, interp.len() as u64, 1),
            phdr(PT_LOAD, 5, 0, BASE, seg1_end as u64, seg1_end as u64, 0x1000),
            phdr(PT_LOAD, 6, data_off as u64, DATA_BASE + data_off as u64, data_filesz, data_filesz + 0x80, 0x1000),
            phdr(PT_DYNAMIC, 6, dynamic_off as u64, DATA_BASE + dynamic_off as u64, dynamic.len() as u64, dynamic.len() as u64, 8),
        ];
        for (i, p) in phdrs.iter().enumerate() {
            put(&mut img, EHDR_SIZE + i * PHDR_SIZE, p);
        }

        // ELF header.
        let mut eh = Vec::new();
        eh.extend(b"\x7fELF");
        eh.extend([2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        eh.extend(ET_DYN.to_le_bytes());
        eh.extend(183u16.to_le_bytes());
        eh.extend(1u32.to_le_bytes());
        eh.extend(text_vaddr.to_le_bytes());
        eh.extend((EHDR_SIZE as u64).to_le_bytes());
        eh.extend((shoff as u64).to_le_bytes());
        eh.extend(0u32.to_le_bytes());
        eh.extend((EHDR_SIZE as u16).to_le_bytes());
        eh.extend((PHDR_SIZE as u16).to_le_bytes());
        eh.extend(4u16.to_le_bytes());
        eh.extend((SHDR_SIZE as u16).to_le_bytes());
        eh.extend((if o.xnum_shnum { 0 } else { sections.len() as u16 }).to_le_bytes());
        eh.extend((if o.xnum_shstrndx { XNUM } else { 8 }).to_le_bytes());
        assert_eq!(eh.len(), EHDR_SIZE);
        put(&mut img, 0, &eh);

        Built {
            image: img,
            text_vaddr,
            text_off: text_off as u64,
            data_vaddr: DATA_BASE + data_off as u64,
            data_off: data_off as u64,
            data_filesz,
            bss_vaddr: DATA_BASE + data_off as u64 + data_filesz,
        }
    }

    #[test]
    fn reads_header_segments_and_interpreter() {
        let b = build(Opts::default());
        let elf = Elf::parse(&b.image).unwrap();
        assert_eq!(elf.header.e_type, ET_DYN);
        assert_eq!(machine_name(elf.header.e_machine), "AArch64");
        assert_eq!(elf.header.e_entry, b.text_vaddr);
        let types: Vec<_> = elf.program_headers.iter().map(|p| segment_type_name(p.p_type)).collect();
        assert_eq!(types, ["INTERP", "LOAD", "LOAD", "DYNAMIC"]);
        let flags: Vec<_> = elf.program_headers.iter().map(|p| segment_flags(p.p_flags)).collect();
        assert_eq!(flags, ["R  ", "R E", "RW ", "RW "]);
        assert_eq!(elf.interp().unwrap().as_deref(), Some("/lib/ld-test.so.1"));
        assert_eq!(elf.kind(), Kind::DynamicPie);
    }

    #[test]
    fn section_names_types_and_flags() {
        let b = build(Opts::default());
        let elf = Elf::parse(&b.image).unwrap();
        let names: Vec<_> = elf.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["", ".interp", ".text", ".note.gnu.build-id", ".dynstr", ".dynamic", ".symtab", ".strtab", ".shstrtab"]);
        let text = elf.section(".text").unwrap();
        assert_eq!(section_flags(text.sh_flags), "AX");
        assert_eq!(section_type_name(elf.section(".dynamic").unwrap().sh_type), "DYNAMIC");
        assert_eq!(elf.section_bytes(text).unwrap().len(), 16);
        assert!(elf.debug_sections().is_empty());
    }

    /// Two segments with different address-to-offset deltas; `.bss` has no file offset (chapter 06 §1).
    #[test]
    fn maps_virtual_addresses_through_the_right_load_segment() {
        let b = build(Opts::default());
        let elf = Elf::parse(&b.image).unwrap();
        assert_eq!(elf.vaddr_to_offset(b.text_vaddr), Some(b.text_off));
        assert_eq!(elf.vaddr_to_offset(b.data_vaddr), Some(b.data_off));
        assert_eq!(elf.vaddr_to_offset(b.data_vaddr + b.data_filesz - 1), Some(b.data_off + b.data_filesz - 1));
        assert_eq!(elf.vaddr_to_offset(b.bss_vaddr), None, "the zero-filled tail is not in the file");
        assert_eq!(elf.vaddr_to_offset(BASE - 1), None);
        assert_eq!(elf.vaddr_to_offset(0), None);
        assert_eq!(elf.vaddr_to_offset(u64::MAX), None);
    }

    /// The uprobe offset of a symbol (chapter 02 §3): its value mapped back to a file offset.
    #[test]
    fn symbol_lookup_and_uprobe_offset() {
        let b = build(Opts::default());
        let elf = Elf::parse(&b.image).unwrap();
        let syms = elf.symbols(false).unwrap();
        assert_eq!(syms.len(), 3);
        let foo = syms.iter().find(|s| s.name.contains("foo")).unwrap();
        assert!(foo.is_function());
        assert_eq!(foo.binding(), 1);
        assert_eq!(foo.value, b.text_vaddr + 8);
        assert_eq!(elf.vaddr_to_offset(foo.value), Some(b.text_off + 8));
        assert!(elf.symbols(true).unwrap().is_empty(), "no .dynsym in this image");
    }

    #[test]
    fn dynamic_section_lists_needed_runpath_and_bind_now() {
        let lazy = Elf::parse(&build(Opts::default()).image).map(|e| e.dynamic().unwrap().unwrap()).unwrap();
        assert_eq!(lazy.needed, ["libc.so.6", "libm.so.6"]);
        assert_eq!(lazy.soname.as_deref(), Some("demo.so"));
        assert_eq!(lazy.runpath.as_deref(), Some("/opt/lib"));
        assert_eq!(lazy.rpath, None);
        assert!(!lazy.bind_now);
        let now_image = build(Opts { bind_now: true, ..Opts::default() }).image;
        let now = Elf::parse(&now_image).unwrap().dynamic().unwrap().unwrap();
        assert!(now.bind_now);
        assert_eq!(now.flags, Some(DF_BIND_NOW));
    }

    #[test]
    fn build_id_is_the_note_descriptor_in_hex() {
        let b = build(Opts::default());
        let id = Elf::parse(&b.image).unwrap().build_id().unwrap().unwrap();
        assert_eq!(id.len(), 40, "a SHA-1 build ID is 20 bytes");
        assert!(id.starts_with("a0a1a2") && id.ends_with("b3"), "{id}");
    }

    /// Chapter 06 §1 (Đi sâu): counts that do not fit the 16-bit header fields live in section header 0.
    #[test]
    fn extended_section_counts_are_read_from_section_header_zero() {
        let plain = Elf::parse(&build(Opts::default()).image).map(|e| e.sections).unwrap();
        for opts in [
            Opts { xnum_shnum: true, ..Opts::default() },
            Opts { xnum_shstrndx: true, ..Opts::default() },
            Opts { xnum_shnum: true, xnum_shstrndx: true, bind_now: false },
        ] {
            let image = build(opts).image;
            let elf = Elf::parse(&image).unwrap();
            let names = |s: &[SectionHeader]| s.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
            assert_eq!(names(&elf.sections), names(&plain), "xnum_shnum={} xnum_shstrndx={}", opts.xnum_shnum, opts.xnum_shstrndx);
        }
    }

    #[test]
    fn a_macos_executable_is_reported_as_not_elf() {
        // The first bytes of a 64-bit little-endian Mach-O file.
        let macho = [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0x00, 0x00, 0x01, 0, 0, 0, 0];
        let err = Elf::parse(&macho).unwrap_err();
        assert_eq!(err, ElfError::BadMagic([0xcf, 0xfa, 0xed, 0xfe]));
        assert!(err.to_string().contains("cf fa ed fe"), "{err}");
        assert_eq!(Elf::parse(b"\x7fE").unwrap_err(), ElfError::TooShort { what: "ELF magic", need: 4, have: 2 });
    }

    #[test]
    fn rejects_32_bit_and_big_endian_images() {
        let mut image = build(Opts::default()).image;
        image[4] = 1;
        assert_eq!(Elf::parse(&image).unwrap_err(), ElfError::UnsupportedClass(1));
        image[4] = 2;
        image[5] = 2;
        assert_eq!(Elf::parse(&image).unwrap_err(), ElfError::UnsupportedEncoding(2));
    }

    #[test]
    fn tables_outside_the_file_are_errors_not_panics() {
        let good = build(Opts::default()).image;
        // Program header table pointing past the end, and one whose size overflows u64.
        for phoff in [good.len() as u64, u64::MAX - 10] {
            let mut image = good.clone();
            image[32..40].copy_from_slice(&phoff.to_le_bytes());
            assert!(matches!(Elf::parse(&image), Err(ElfError::BadTable { what: "program header table", .. })), "phoff = {phoff:#x}");
        }
        // Section header table past the end.
        let mut image = good.clone();
        image[40..48].copy_from_slice(&(good.len() as u64 - 8).to_le_bytes());
        assert!(matches!(Elf::parse(&image), Err(ElfError::BadTable { what: "section header table", .. })));
        // Entry sizes smaller than the structures they hold.
        let mut image = good.clone();
        image[54..56].copy_from_slice(&8u16.to_le_bytes());
        assert!(matches!(Elf::parse(&image), Err(ElfError::BadTable { what: "program header entry size", .. })));
        // A name-table index beyond the table.
        let mut image = good;
        image[62..64].copy_from_slice(&200u16.to_le_bytes());
        assert!(matches!(Elf::parse(&image), Err(ElfError::BadTable { what: "e_shstrndx", .. })));
    }

    #[test]
    fn a_name_offset_outside_the_string_table_is_a_bad_string() {
        let b = build(Opts::default());
        let elf = Elf::parse(&b.image).unwrap();
        let shoff = elf.header.e_shoff as usize;
        let mut image = b.image.clone();
        // sh_name of section 1 -> far beyond .shstrtab
        image[shoff + SHDR_SIZE..shoff + SHDR_SIZE + 4].copy_from_slice(&9999u32.to_le_bytes());
        assert!(matches!(Elf::parse(&image), Err(ElfError::BadString { what: "section name", offset: 9999 })));
    }

    /// Robustness: cutting a valid image at *every* length, and flipping bytes of the
    /// structured part, must never panic (an error or a successful parse are both fine).
    #[test]
    fn truncation_and_corruption_never_panic() {
        let good = build(Opts::default()).image;
        for len in 0..good.len() {
            let _ = Elf::parse(&good[..len]);
        }
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..4000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let mut image = good.clone();
            let pos = (x % image.len() as u64) as usize;
            image[pos] = (x >> 32) as u8;
            if let Ok(elf) = Elf::parse(&image) {
                let _ = (elf.interp(), elf.dynamic(), elf.symbols(false), elf.symbols(true), elf.build_id());
                let _ = elf.vaddr_to_offset(x);
            }
        }
    }
}
