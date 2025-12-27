//! # ELF Parser and Loader
//!
//! Parses and loads ELF (Executable and Linkable Format) binaries.
//!
//! ## Supported Features
//!
//! - ELF64 format (x86_64)
//! - Executable files (ET_EXEC)
//! - PIE executables (ET_DYN) 
//! - Program segments (PT_LOAD)
//! - Static linking only (no dynamic loader yet)
//!
//! ## ELF Structure
//!
//! ```text
//! ┌─────────────────────┐
//! │     ELF Header      │  64 bytes (identifies the file)
//! ├─────────────────────┤
//! │   Program Headers   │  Array of segment descriptors
//! ├─────────────────────┤
//! │                     │
//! │      Segments       │  Loadable code/data
//! │                     │
//! ├─────────────────────┤
//! │   Section Headers   │  Array of section descriptors
//! └─────────────────────┘
//! ```

use alloc::string::String;
use alloc::vec::Vec;

/// ELF magic number
pub const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

/// ELF class - 64 bit
pub const ELFCLASS64: u8 = 2;

/// ELF data encoding - little endian
pub const ELFDATA2LSB: u8 = 1;

/// ELF version
pub const EV_CURRENT: u8 = 1;

/// ELF OS/ABI - System V
pub const ELFOSABI_NONE: u8 = 0;

/// ELF type - executable
pub const ET_EXEC: u16 = 2;

/// ELF type - shared object (PIE)
pub const ET_DYN: u16 = 3;

/// Machine type - x86_64
pub const EM_X86_64: u16 = 62;

/// Machine type - AArch64
pub const EM_AARCH64: u16 = 183;

/// Program header type - loadable segment
pub const PT_LOAD: u32 = 1;

/// Program header type - dynamic linking info
pub const PT_DYNAMIC: u32 = 2;

/// Program header type - interpreter path
pub const PT_INTERP: u32 = 3;

/// Program header type - note
pub const PT_NOTE: u32 = 4;

/// Program header type - program header table
pub const PT_PHDR: u32 = 6;

/// Program header type - thread local storage
pub const PT_TLS: u32 = 7;

/// Segment flag - executable
pub const PF_X: u32 = 1;

/// Segment flag - writable
pub const PF_W: u32 = 2;

/// Segment flag - readable
pub const PF_R: u32 = 4;

/// ELF64 header
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Elf64Header {
    /// Magic number and other info
    pub e_ident: [u8; 16],
    /// Object file type
    pub e_type: u16,
    /// Machine type
    pub e_machine: u16,
    /// Object file version
    pub e_version: u32,
    /// Entry point virtual address
    pub e_entry: u64,
    /// Program header table file offset
    pub e_phoff: u64,
    /// Section header table file offset
    pub e_shoff: u64,
    /// Processor-specific flags
    pub e_flags: u32,
    /// ELF header size
    pub e_ehsize: u16,
    /// Program header table entry size
    pub e_phentsize: u16,
    /// Program header table entry count
    pub e_phnum: u16,
    /// Section header table entry size
    pub e_shentsize: u16,
    /// Section header table entry count
    pub e_shnum: u16,
    /// Section header string table index
    pub e_shstrndx: u16,
}

/// ELF64 program header
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Elf64Phdr {
    /// Segment type
    pub p_type: u32,
    /// Segment flags
    pub p_flags: u32,
    /// File offset
    pub p_offset: u64,
    /// Virtual address
    pub p_vaddr: u64,
    /// Physical address
    pub p_paddr: u64,
    /// Size in file
    pub p_filesz: u64,
    /// Size in memory
    pub p_memsz: u64,
    /// Alignment
    pub p_align: u64,
}

/// ELF64 section header
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Elf64Shdr {
    /// Section name (index into string table)
    pub sh_name: u32,
    /// Section type
    pub sh_type: u32,
    /// Section flags
    pub sh_flags: u64,
    /// Virtual address
    pub sh_addr: u64,
    /// File offset
    pub sh_offset: u64,
    /// Section size
    pub sh_size: u64,
    /// Link to another section
    pub sh_link: u32,
    /// Additional info
    pub sh_info: u32,
    /// Address alignment
    pub sh_addralign: u64,
    /// Entry size if section holds table
    pub sh_entsize: u64,
}

/// ELF parsing errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    /// File too small
    TooSmall,
    /// Invalid magic number
    InvalidMagic,
    /// Unsupported ELF class (not 64-bit)
    UnsupportedClass,
    /// Unsupported endianness
    UnsupportedEndian,
    /// Unsupported ELF version
    UnsupportedVersion,
    /// Unsupported file type
    UnsupportedType,
    /// Unsupported machine type
    UnsupportedMachine,
    /// Invalid program header
    InvalidPhdr,
    /// No loadable segments
    NoLoadableSegments,
    /// Segment out of bounds
    SegmentOutOfBounds,
    /// Overlapping segments
    OverlappingSegments,
    /// Invalid entry point
    InvalidEntryPoint,
}

impl core::fmt::Display for ElfError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ElfError::TooSmall => write!(f, "File too small"),
            ElfError::InvalidMagic => write!(f, "Invalid ELF magic"),
            ElfError::UnsupportedClass => write!(f, "Unsupported ELF class"),
            ElfError::UnsupportedEndian => write!(f, "Unsupported endianness"),
            ElfError::UnsupportedVersion => write!(f, "Unsupported ELF version"),
            ElfError::UnsupportedType => write!(f, "Unsupported file type"),
            ElfError::UnsupportedMachine => write!(f, "Unsupported machine type"),
            ElfError::InvalidPhdr => write!(f, "Invalid program header"),
            ElfError::NoLoadableSegments => write!(f, "No loadable segments"),
            ElfError::SegmentOutOfBounds => write!(f, "Segment out of bounds"),
            ElfError::OverlappingSegments => write!(f, "Overlapping segments"),
            ElfError::InvalidEntryPoint => write!(f, "Invalid entry point"),
        }
    }
}

/// Memory protection flags for loaded segments
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryProtection {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl MemoryProtection {
    pub fn from_flags(flags: u32) -> Self {
        Self {
            read: flags & PF_R != 0,
            write: flags & PF_W != 0,
            execute: flags & PF_X != 0,
        }
    }
}

/// A loadable segment parsed from ELF
#[derive(Debug, Clone)]
pub struct LoadableSegment {
    /// Virtual address to load at
    pub vaddr: u64,
    /// Physical address (usually same as vaddr)
    pub paddr: u64,
    /// Size in file
    pub file_size: u64,
    /// Size in memory (>= file_size, extra is zeroed)
    pub mem_size: u64,
    /// File offset
    pub file_offset: u64,
    /// Memory protection
    pub prot: MemoryProtection,
    /// Alignment requirement
    pub align: u64,
}

/// Parsed ELF file information
#[derive(Debug, Clone)]
pub struct ElfInfo {
    /// Entry point address
    pub entry: u64,
    /// Loadable segments
    pub segments: Vec<LoadableSegment>,
    /// Whether this is a PIE (Position Independent Executable)
    pub is_pie: bool,
    /// Base address (lowest vaddr)
    pub base_addr: u64,
    /// Top address (highest vaddr + memsz)
    pub top_addr: u64,
    /// Program header virtual address (for PT_PHDR)
    pub phdr_vaddr: Option<u64>,
    /// Interpreter path (for dynamic executables)
    pub interp: Option<String>,
}

/// Parse ELF header
pub fn parse_header(data: &[u8]) -> Result<&Elf64Header, ElfError> {
    if data.len() < core::mem::size_of::<Elf64Header>() {
        return Err(ElfError::TooSmall);
    }

    // SAFETY: We verified the size above
    let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

    // Verify magic
    if header.e_ident[0..4] != ELF_MAGIC {
        return Err(ElfError::InvalidMagic);
    }

    // Check class (must be 64-bit)
    if header.e_ident[4] != ELFCLASS64 {
        return Err(ElfError::UnsupportedClass);
    }

    // Check endianness (must be little-endian)
    if header.e_ident[5] != ELFDATA2LSB {
        return Err(ElfError::UnsupportedEndian);
    }

    // Check version
    if header.e_ident[6] != EV_CURRENT {
        return Err(ElfError::UnsupportedVersion);
    }

    // Check file type
    let e_type = header.e_type;
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(ElfError::UnsupportedType);
    }

    // Check machine type
    #[cfg(target_arch = "x86_64")]
    if header.e_machine != EM_X86_64 {
        return Err(ElfError::UnsupportedMachine);
    }

    #[cfg(target_arch = "aarch64")]
    if header.e_machine != EM_AARCH64 {
        return Err(ElfError::UnsupportedMachine);
    }

    Ok(header)
}

/// Parse program headers
pub fn parse_program_headers(data: &[u8], header: &Elf64Header) -> Result<Vec<Elf64Phdr>, ElfError> {
    let phoff = header.e_phoff as usize;
    let phentsize = header.e_phentsize as usize;
    let phnum = header.e_phnum as usize;

    if phentsize < core::mem::size_of::<Elf64Phdr>() {
        return Err(ElfError::InvalidPhdr);
    }

    let phdrs_end = phoff + phnum * phentsize;
    if phdrs_end > data.len() {
        return Err(ElfError::InvalidPhdr);
    }

    let mut phdrs = Vec::with_capacity(phnum);

    for i in 0..phnum {
        let offset = phoff + i * phentsize;
        // SAFETY: We verified bounds above
        let phdr = unsafe { &*(data.as_ptr().add(offset) as *const Elf64Phdr) };
        phdrs.push(*phdr);
    }

    Ok(phdrs)
}

/// Parse an ELF file and extract loading information
pub fn parse(data: &[u8]) -> Result<ElfInfo, ElfError> {
    let header = parse_header(data)?;
    let phdrs = parse_program_headers(data, header)?;

    let is_pie = header.e_type == ET_DYN;
    let entry = header.e_entry;

    let mut segments = Vec::new();
    let mut base_addr = u64::MAX;
    let mut top_addr = 0u64;
    let mut phdr_vaddr = None;
    let mut interp = None;

    for phdr in &phdrs {
        match phdr.p_type {
            PT_LOAD => {
                // Validate segment bounds
                let file_end = phdr.p_offset.saturating_add(phdr.p_filesz);
                if file_end > data.len() as u64 {
                    return Err(ElfError::SegmentOutOfBounds);
                }

                let segment = LoadableSegment {
                    vaddr: phdr.p_vaddr,
                    paddr: phdr.p_paddr,
                    file_size: phdr.p_filesz,
                    mem_size: phdr.p_memsz,
                    file_offset: phdr.p_offset,
                    prot: MemoryProtection::from_flags(phdr.p_flags),
                    align: phdr.p_align,
                };

                // Track address range
                if segment.vaddr < base_addr {
                    base_addr = segment.vaddr;
                }
                let seg_end = segment.vaddr.saturating_add(segment.mem_size);
                if seg_end > top_addr {
                    top_addr = seg_end;
                }

                segments.push(segment);
            }
            PT_PHDR => {
                phdr_vaddr = Some(phdr.p_vaddr);
            }
            PT_INTERP => {
                // Extract interpreter path
                let start = phdr.p_offset as usize;
                let end = start + phdr.p_filesz as usize;
                if end <= data.len() {
                    let path_bytes = &data[start..end];
                    // Remove null terminator if present
                    let path_len = path_bytes.iter().position(|&b| b == 0).unwrap_or(path_bytes.len());
                    if let Ok(path) = core::str::from_utf8(&path_bytes[..path_len]) {
                        interp = Some(String::from(path));
                    }
                }
            }
            _ => {} // Ignore other segment types
        }
    }

    if segments.is_empty() {
        return Err(ElfError::NoLoadableSegments);
    }

    // Validate entry point is within loaded segments
    let entry_valid = segments.iter().any(|seg| {
        entry >= seg.vaddr && entry < seg.vaddr.saturating_add(seg.mem_size)
    });
    if !entry_valid && !is_pie {
        return Err(ElfError::InvalidEntryPoint);
    }

    Ok(ElfInfo {
        entry,
        segments,
        is_pie,
        base_addr,
        top_addr,
        phdr_vaddr,
        interp,
    })
}

/// Get the data for a segment from the ELF file
pub fn segment_data<'a>(data: &'a [u8], segment: &LoadableSegment) -> &'a [u8] {
    let start = segment.file_offset as usize;
    let end = start + segment.file_size as usize;
    &data[start..end]
}

/// Calculate the total memory needed for an ELF
pub fn memory_needed(info: &ElfInfo) -> u64 {
    info.top_addr - info.base_addr
}

/// Validate that an ELF is suitable for loading
pub fn validate_for_exec(info: &ElfInfo) -> Result<(), ElfError> {
    // Check for overlapping segments
    for (i, seg1) in info.segments.iter().enumerate() {
        for seg2 in info.segments.iter().skip(i + 1) {
            let s1_end = seg1.vaddr + seg1.mem_size;
            let s2_end = seg2.vaddr + seg2.mem_size;
            
            // Check overlap
            if !(s1_end <= seg2.vaddr || s2_end <= seg1.vaddr) {
                return Err(ElfError::OverlappingSegments);
            }
        }
    }

    Ok(())
}

/// Dump ELF info for debugging
pub fn dump_info(info: &ElfInfo) -> String {
    use alloc::format;
    
    let mut output = String::new();
    
    output.push_str(&format!("Entry point: 0x{:016x}\n", info.entry));
    output.push_str(&format!("PIE: {}\n", info.is_pie));
    output.push_str(&format!("Base addr: 0x{:016x}\n", info.base_addr));
    output.push_str(&format!("Top addr: 0x{:016x}\n", info.top_addr));
    output.push_str(&format!("Memory needed: {} bytes\n", memory_needed(info)));
    
    if let Some(ref interp) = info.interp {
        output.push_str(&format!("Interpreter: {}\n", interp));
    }
    
    output.push_str(&format!("\nSegments ({}):\n", info.segments.len()));
    
    for (i, seg) in info.segments.iter().enumerate() {
        output.push_str(&format!(
            "  [{}] vaddr=0x{:016x} filesz=0x{:x} memsz=0x{:x} {}{}{}",
            i,
            seg.vaddr,
            seg.file_size,
            seg.mem_size,
            if seg.prot.read { "r" } else { "-" },
            if seg.prot.write { "w" } else { "-" },
            if seg.prot.execute { "x" } else { "-" },
        ));
        output.push('\n');
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_magic() {
        let bad_data = [0u8; 64];
        assert!(matches!(parse(&bad_data), Err(ElfError::InvalidMagic)));
    }
}
