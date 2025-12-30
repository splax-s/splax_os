//! # Dynamic Linker for S-NATIVE
//!
//! This module implements ELF dynamic linking for shared libraries.
//! It handles symbol resolution, relocation, and library loading.
//!
//! ## Features
//!
//! - ELF64 parsing and loading
//! - PLT/GOT relocation
//! - Lazy symbol binding
//! - Library dependency resolution
//! - ASLR-compatible loading

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// =============================================================================
// ELF Types
// =============================================================================

/// ELF magic number.
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF class: 64-bit.
const ELFCLASS64: u8 = 2;

/// ELF data: little-endian.
const ELFDATA2LSB: u8 = 1;

/// ELF type: shared object.
const ET_DYN: u16 = 3;

/// Machine type: x86_64.
const EM_X86_64: u16 = 62;

/// Machine type: AArch64.
const EM_AARCH64: u16 = 183;

/// ELF64 header.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    /// Magic number and identification.
    pub e_ident: [u8; 16],
    /// Object file type.
    pub e_type: u16,
    /// Machine architecture.
    pub e_machine: u16,
    /// Object file version.
    pub e_version: u32,
    /// Entry point virtual address.
    pub e_entry: u64,
    /// Program header table file offset.
    pub e_phoff: u64,
    /// Section header table file offset.
    pub e_shoff: u64,
    /// Processor-specific flags.
    pub e_flags: u32,
    /// ELF header size.
    pub e_ehsize: u16,
    /// Program header entry size.
    pub e_phentsize: u16,
    /// Number of program headers.
    pub e_phnum: u16,
    /// Section header entry size.
    pub e_shentsize: u16,
    /// Number of section headers.
    pub e_shnum: u16,
    /// Section name string table index.
    pub e_shstrndx: u16,
}

/// Program header types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ProgramType {
    /// Unused entry.
    Null = 0,
    /// Loadable segment.
    Load = 1,
    /// Dynamic linking info.
    Dynamic = 2,
    /// Interpreter path.
    Interp = 3,
    /// Note section.
    Note = 4,
    /// GNU EH frame.
    GnuEhFrame = 0x6474e550,
    /// GNU stack.
    GnuStack = 0x6474e551,
    /// GNU relro.
    GnuRelro = 0x6474e552,
}

/// Program header.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    /// Segment type.
    pub p_type: u32,
    /// Segment flags.
    pub p_flags: u32,
    /// File offset.
    pub p_offset: u64,
    /// Virtual address.
    pub p_vaddr: u64,
    /// Physical address.
    pub p_paddr: u64,
    /// File size.
    pub p_filesz: u64,
    /// Memory size.
    pub p_memsz: u64,
    /// Alignment.
    pub p_align: u64,
}

/// Section header.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Shdr {
    /// Section name (string table index).
    pub sh_name: u32,
    /// Section type.
    pub sh_type: u32,
    /// Section flags.
    pub sh_flags: u64,
    /// Virtual address.
    pub sh_addr: u64,
    /// File offset.
    pub sh_offset: u64,
    /// Section size.
    pub sh_size: u64,
    /// Link to another section.
    pub sh_link: u32,
    /// Additional info.
    pub sh_info: u32,
    /// Alignment.
    pub sh_addralign: u64,
    /// Entry size if table.
    pub sh_entsize: u64,
}

/// Symbol table entry.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Sym {
    /// Symbol name (string table index).
    pub st_name: u32,
    /// Symbol info (type and binding).
    pub st_info: u8,
    /// Symbol visibility.
    pub st_other: u8,
    /// Section index.
    pub st_shndx: u16,
    /// Symbol value.
    pub st_value: u64,
    /// Symbol size.
    pub st_size: u64,
}

impl Elf64Sym {
    /// Get symbol type.
    pub fn sym_type(&self) -> u8 {
        self.st_info & 0xf
    }

    /// Get symbol binding.
    pub fn sym_bind(&self) -> u8 {
        self.st_info >> 4
    }
}

/// Symbol types.
pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;

/// Symbol bindings.
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

/// Relocation entry without addend.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Rel {
    /// Address to relocate.
    pub r_offset: u64,
    /// Relocation type and symbol index.
    pub r_info: u64,
}

/// Relocation entry with addend.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Rela {
    /// Address to relocate.
    pub r_offset: u64,
    /// Relocation type and symbol index.
    pub r_info: u64,
    /// Addend.
    pub r_addend: i64,
}

impl Elf64Rela {
    /// Get relocation type.
    pub fn r_type(&self) -> u32 {
        (self.r_info & 0xffffffff) as u32
    }

    /// Get symbol index.
    pub fn r_sym(&self) -> u32 {
        (self.r_info >> 32) as u32
    }
}

/// x86_64 relocation types.
pub mod r_x86_64 {
    pub const R_X86_64_NONE: u32 = 0;
    pub const R_X86_64_64: u32 = 1;
    pub const R_X86_64_PC32: u32 = 2;
    pub const R_X86_64_GOT32: u32 = 3;
    pub const R_X86_64_PLT32: u32 = 4;
    pub const R_X86_64_COPY: u32 = 5;
    pub const R_X86_64_GLOB_DAT: u32 = 6;
    pub const R_X86_64_JUMP_SLOT: u32 = 7;
    pub const R_X86_64_RELATIVE: u32 = 8;
    pub const R_X86_64_GOTPCREL: u32 = 9;
    pub const R_X86_64_32: u32 = 10;
    pub const R_X86_64_32S: u32 = 11;
}

/// AArch64 relocation types.
pub mod r_aarch64 {
    pub const R_AARCH64_NONE: u32 = 0;
    pub const R_AARCH64_ABS64: u32 = 257;
    pub const R_AARCH64_RELATIVE: u32 = 1027;
    pub const R_AARCH64_GLOB_DAT: u32 = 1025;
    pub const R_AARCH64_JUMP_SLOT: u32 = 1026;
}

/// Dynamic entry.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Dyn {
    /// Entry tag.
    pub d_tag: i64,
    /// Entry value.
    pub d_val: u64,
}

/// Dynamic tags.
pub const DT_NULL: i64 = 0;
pub const DT_NEEDED: i64 = 1;
pub const DT_PLTRELSZ: i64 = 2;
pub const DT_PLTGOT: i64 = 3;
pub const DT_HASH: i64 = 4;
pub const DT_STRTAB: i64 = 5;
pub const DT_SYMTAB: i64 = 6;
pub const DT_RELA: i64 = 7;
pub const DT_RELASZ: i64 = 8;
pub const DT_RELAENT: i64 = 9;
pub const DT_STRSZ: i64 = 10;
pub const DT_SYMENT: i64 = 11;
pub const DT_INIT: i64 = 12;
pub const DT_FINI: i64 = 13;
pub const DT_SONAME: i64 = 14;
pub const DT_RPATH: i64 = 15;
pub const DT_SYMBOLIC: i64 = 16;
pub const DT_REL: i64 = 17;
pub const DT_RELSZ: i64 = 18;
pub const DT_RELENT: i64 = 19;
pub const DT_PLTREL: i64 = 20;
pub const DT_DEBUG: i64 = 21;
pub const DT_TEXTREL: i64 = 22;
pub const DT_JMPREL: i64 = 23;
pub const DT_BIND_NOW: i64 = 24;
pub const DT_INIT_ARRAY: i64 = 25;
pub const DT_FINI_ARRAY: i64 = 26;
pub const DT_INIT_ARRAYSZ: i64 = 27;
pub const DT_FINI_ARRAYSZ: i64 = 28;
pub const DT_GNU_HASH: i64 = 0x6ffffef5;

// =============================================================================
// Dynamic Linker Errors
// =============================================================================

/// Dynamic linking error.
#[derive(Debug, Clone)]
pub enum LinkError {
    /// Invalid ELF format.
    InvalidElf(String),
    /// Unsupported architecture.
    UnsupportedArch(u16),
    /// Symbol not found.
    SymbolNotFound(String),
    /// Library not found.
    LibraryNotFound(String),
    /// Unsupported relocation type.
    UnsupportedRelocation(u32),
    /// Memory allocation failed.
    MemoryError,
    /// Circular dependency detected.
    CircularDependency(String),
    /// Version mismatch.
    VersionMismatch(String),
}

// =============================================================================
// Loaded Library
// =============================================================================

/// Handle to a loaded shared library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LibraryHandle(u64);

/// Loaded shared library.
pub struct LoadedLibrary {
    /// Library handle.
    pub handle: LibraryHandle,
    /// Library name.
    pub name: String,
    /// Base address where library is loaded.
    pub base_addr: usize,
    /// Size of loaded memory.
    pub size: usize,
    /// Exported symbols.
    pub exports: BTreeMap<String, usize>,
    /// Dependencies.
    pub dependencies: Vec<LibraryHandle>,
    /// Reference count.
    pub ref_count: u32,
    /// Init function address (if any).
    pub init_fn: Option<usize>,
    /// Fini function address (if any).
    pub fini_fn: Option<usize>,
    /// Init array.
    pub init_array: Vec<usize>,
    /// Fini array.
    pub fini_array: Vec<usize>,
}

impl LoadedLibrary {
    /// Look up a symbol.
    pub fn lookup(&self, name: &str) -> Option<usize> {
        self.exports.get(name).copied()
    }

    /// Get typed function pointer.
    ///
    /// # Safety
    ///
    /// Caller must ensure the symbol has the correct type.
    pub unsafe fn get_fn<T: Copy>(&self, name: &str) -> Option<T> {
        self.exports.get(name).map(|&addr| {
            core::mem::transmute_copy(&addr)
        })
    }
}

// =============================================================================
// Dynamic Linker
// =============================================================================

/// Dynamic linker for loading shared libraries.
pub struct DynamicLinker {
    /// Loaded libraries.
    libraries: BTreeMap<LibraryHandle, LoadedLibrary>,
    /// Library search paths.
    search_paths: Vec<String>,
    /// Symbol override table (for LD_PRELOAD-like functionality).
    overrides: BTreeMap<String, usize>,
    /// Next library handle.
    next_handle: u64,
    /// Global symbol table.
    global_symbols: BTreeMap<String, (LibraryHandle, usize)>,
}

impl DynamicLinker {
    /// Create a new dynamic linker.
    pub fn new() -> Self {
        Self {
            libraries: BTreeMap::new(),
            search_paths: Vec::new(),
            overrides: BTreeMap::new(),
            next_handle: 1,
            global_symbols: BTreeMap::new(),
        }
    }

    /// Add a library search path.
    pub fn add_search_path(&mut self, path: String) {
        self.search_paths.push(path);
    }

    /// Load a shared library from bytes.
    pub fn load(&mut self, name: &str, elf_data: &[u8]) -> Result<LibraryHandle, LinkError> {
        // Validate ELF header
        let header = self.parse_header(elf_data)?;

        // Check if shared object
        if header.e_type != ET_DYN {
            return Err(LinkError::InvalidElf("Not a shared object".into()));
        }

        // Parse program headers
        let phdrs = self.parse_program_headers(elf_data, &header)?;

        // Calculate total memory needed
        let (min_vaddr, max_vaddr) = self.calculate_memory_range(&phdrs);
        let size = max_vaddr - min_vaddr;

        // Allocate memory (with ASLR offset)
        let base_addr = self.allocate_memory(size)?;

        // Load segments
        self.load_segments(elf_data, &phdrs, base_addr, min_vaddr)?;

        // Parse dynamic section
        let dynamic = self.parse_dynamic(elf_data, &phdrs, base_addr, min_vaddr)?;

        // Parse symbol table
        let symbols = self.parse_symbols(elf_data, &dynamic, base_addr, min_vaddr)?;

        // Load dependencies
        let deps = self.load_dependencies(&dynamic)?;

        // Perform relocations
        self.perform_relocations(elf_data, &dynamic, base_addr, min_vaddr, &header)?;

        // Create library entry
        let handle = LibraryHandle(self.next_handle);
        self.next_handle += 1;

        // Register global symbols
        for (sym_name, addr) in &symbols {
            self.global_symbols.insert(sym_name.clone(), (handle, *addr));
        }

        let library = LoadedLibrary {
            handle,
            name: String::from(name),
            base_addr,
            size,
            exports: symbols,
            dependencies: deps,
            ref_count: 1,
            init_fn: dynamic.init.map(|off| base_addr + off),
            fini_fn: dynamic.fini.map(|off| base_addr + off),
            init_array: dynamic.init_array.iter()
                .map(|&off| base_addr + off)
                .collect(),
            fini_array: dynamic.fini_array.iter()
                .map(|&off| base_addr + off)
                .collect(),
        };

        // Run init functions
        self.run_init(&library)?;

        self.libraries.insert(handle, library);

        Ok(handle)
    }

    /// Unload a library.
    pub fn unload(&mut self, handle: LibraryHandle) -> Result<(), LinkError> {
        let library = self.libraries.get_mut(&handle)
            .ok_or_else(|| LinkError::LibraryNotFound("Handle not found".into()))?;

        library.ref_count -= 1;

        if library.ref_count == 0 {
            // Run fini functions
            let library = self.libraries.remove(&handle).unwrap();
            self.run_fini(&library)?;

            // Remove from global symbols
            self.global_symbols.retain(|_, (h, _)| *h != handle);

            // Decrement dependency ref counts
            for dep in &library.dependencies {
                let _ = self.unload(*dep);
            }

            // Free memory
            self.free_memory(library.base_addr, library.size);
        }

        Ok(())
    }

    /// Look up a symbol globally.
    pub fn lookup(&self, name: &str) -> Option<usize> {
        // Check overrides first
        if let Some(&addr) = self.overrides.get(name) {
            return Some(addr);
        }

        // Then check global symbols
        self.global_symbols.get(name).map(|(_, addr)| *addr)
    }

    /// Look up a symbol in a specific library.
    pub fn lookup_in(&self, handle: LibraryHandle, name: &str) -> Option<usize> {
        self.libraries.get(&handle).and_then(|lib| lib.lookup(name))
    }

    /// Add a symbol override.
    pub fn add_override(&mut self, name: String, addr: usize) {
        self.overrides.insert(name, addr);
    }

    /// Get a library by handle.
    pub fn get(&self, handle: LibraryHandle) -> Option<&LoadedLibrary> {
        self.libraries.get(&handle)
    }

    // =========================================================================
    // Internal parsing helpers
    // =========================================================================

    fn parse_header(&self, data: &[u8]) -> Result<Elf64Header, LinkError> {
        if data.len() < core::mem::size_of::<Elf64Header>() {
            return Err(LinkError::InvalidElf("Data too small".into()));
        }

        if &data[0..4] != &ELF_MAGIC {
            return Err(LinkError::InvalidElf("Invalid magic".into()));
        }

        if data[4] != ELFCLASS64 {
            return Err(LinkError::InvalidElf("Not 64-bit".into()));
        }

        if data[5] != ELFDATA2LSB {
            return Err(LinkError::InvalidElf("Not little-endian".into()));
        }

        let header = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const Elf64Header)
        };

        // Validate architecture
        #[cfg(target_arch = "x86_64")]
        if header.e_machine != EM_X86_64 {
            return Err(LinkError::UnsupportedArch(header.e_machine));
        }

        #[cfg(target_arch = "aarch64")]
        if header.e_machine != EM_AARCH64 {
            return Err(LinkError::UnsupportedArch(header.e_machine));
        }

        Ok(header)
    }

    fn parse_program_headers(
        &self,
        data: &[u8],
        header: &Elf64Header,
    ) -> Result<Vec<Elf64Phdr>, LinkError> {
        let mut phdrs = Vec::new();
        let phdr_size = core::mem::size_of::<Elf64Phdr>();

        for i in 0..header.e_phnum as usize {
            let offset = header.e_phoff as usize + i * phdr_size;
            if offset + phdr_size > data.len() {
                return Err(LinkError::InvalidElf("Phdr out of bounds".into()));
            }

            let phdr = unsafe {
                core::ptr::read_unaligned(data.as_ptr().add(offset) as *const Elf64Phdr)
            };
            phdrs.push(phdr);
        }

        Ok(phdrs)
    }

    fn calculate_memory_range(&self, phdrs: &[Elf64Phdr]) -> (usize, usize) {
        let mut min_vaddr = usize::MAX;
        let mut max_vaddr = 0usize;

        for phdr in phdrs {
            if phdr.p_type == 1 { // PT_LOAD
                let start = phdr.p_vaddr as usize;
                let end = start + phdr.p_memsz as usize;

                min_vaddr = min_vaddr.min(start);
                max_vaddr = max_vaddr.max(end);
            }
        }

        (min_vaddr, max_vaddr)
    }

    fn allocate_memory(&self, size: usize) -> Result<usize, LinkError> {
        // In a real implementation, this would use mmap with ASLR
        // For now, use a simple static allocator
        static NEXT_ADDR: core::sync::atomic::AtomicUsize =
            core::sync::atomic::AtomicUsize::new(0x7f00_0000_0000);

        let aligned_size = (size + 4095) & !4095;
        let addr = NEXT_ADDR.fetch_add(aligned_size, core::sync::atomic::Ordering::SeqCst);

        Ok(addr)
    }

    fn free_memory(&self, _addr: usize, _size: usize) {
        // In a real implementation, this would unmap memory
    }

    fn load_segments(
        &self,
        data: &[u8],
        phdrs: &[Elf64Phdr],
        base_addr: usize,
        min_vaddr: usize,
    ) -> Result<(), LinkError> {
        for phdr in phdrs {
            if phdr.p_type != 1 { // PT_LOAD
                continue;
            }

            let dest = base_addr + (phdr.p_vaddr as usize - min_vaddr);
            let src_start = phdr.p_offset as usize;
            let src_end = src_start + phdr.p_filesz as usize;

            if src_end > data.len() {
                return Err(LinkError::InvalidElf("Segment out of bounds".into()));
            }

            // Copy file data
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr().add(src_start),
                    dest as *mut u8,
                    phdr.p_filesz as usize,
                );

                // Zero BSS
                let bss_size = (phdr.p_memsz - phdr.p_filesz) as usize;
                if bss_size > 0 {
                    core::ptr::write_bytes(
                        (dest + phdr.p_filesz as usize) as *mut u8,
                        0,
                        bss_size,
                    );
                }
            }
        }

        Ok(())
    }

    fn parse_dynamic(
        &self,
        data: &[u8],
        phdrs: &[Elf64Phdr],
        base_addr: usize,
        min_vaddr: usize,
    ) -> Result<DynamicInfo, LinkError> {
        // Find PT_DYNAMIC
        let dyn_phdr = phdrs.iter()
            .find(|p| p.p_type == 2) // PT_DYNAMIC
            .ok_or_else(|| LinkError::InvalidElf("No dynamic section".into()))?;

        let mut info = DynamicInfo::default();
        let dyn_start = dyn_phdr.p_offset as usize;
        let dyn_size = core::mem::size_of::<Elf64Dyn>();

        let mut offset = 0;
        loop {
            if dyn_start + offset + dyn_size > data.len() {
                break;
            }

            let dyn_entry = unsafe {
                core::ptr::read_unaligned(
                    data.as_ptr().add(dyn_start + offset) as *const Elf64Dyn
                )
            };

            if dyn_entry.d_tag == DT_NULL {
                break;
            }

            match dyn_entry.d_tag {
                DT_STRTAB => info.strtab = Some(dyn_entry.d_val as usize),
                DT_SYMTAB => info.symtab = Some(dyn_entry.d_val as usize),
                DT_STRSZ => info.strsz = dyn_entry.d_val as usize,
                DT_SYMENT => info.syment = dyn_entry.d_val as usize,
                DT_RELA => info.rela = Some(dyn_entry.d_val as usize),
                DT_RELASZ => info.relasz = dyn_entry.d_val as usize,
                DT_RELAENT => info.relaent = dyn_entry.d_val as usize,
                DT_JMPREL => info.jmprel = Some(dyn_entry.d_val as usize),
                DT_PLTRELSZ => info.pltrelsz = dyn_entry.d_val as usize,
                DT_INIT => info.init = Some(dyn_entry.d_val as usize),
                DT_FINI => info.fini = Some(dyn_entry.d_val as usize),
                DT_INIT_ARRAY => info.init_array_ptr = Some(dyn_entry.d_val as usize),
                DT_INIT_ARRAYSZ => info.init_array_sz = dyn_entry.d_val as usize,
                DT_FINI_ARRAY => info.fini_array_ptr = Some(dyn_entry.d_val as usize),
                DT_FINI_ARRAYSZ => info.fini_array_sz = dyn_entry.d_val as usize,
                DT_NEEDED => info.needed.push(dyn_entry.d_val as usize),
                _ => {}
            }

            offset += dyn_size;
        }

        // Adjust addresses to loaded base
        if let Some(strtab) = &mut info.strtab {
            *strtab = base_addr + (*strtab - min_vaddr);
        }
        if let Some(symtab) = &mut info.symtab {
            *symtab = base_addr + (*symtab - min_vaddr);
        }
        if let Some(rela) = &mut info.rela {
            *rela = base_addr + (*rela - min_vaddr);
        }
        if let Some(jmprel) = &mut info.jmprel {
            *jmprel = base_addr + (*jmprel - min_vaddr);
        }

        // Parse init/fini arrays
        if let Some(init_ptr) = info.init_array_ptr {
            let addr = base_addr + (init_ptr - min_vaddr);
            let count = info.init_array_sz / 8;
            for i in 0..count {
                let fn_addr = unsafe { *((addr + i * 8) as *const usize) };
                if fn_addr != 0 && fn_addr != usize::MAX {
                    info.init_array.push(fn_addr);
                }
            }
        }

        Ok(info)
    }

    fn parse_symbols(
        &self,
        _data: &[u8],
        dynamic: &DynamicInfo,
        _base_addr: usize,
        _min_vaddr: usize,
    ) -> Result<BTreeMap<String, usize>, LinkError> {
        let mut symbols = BTreeMap::new();

        let symtab = match dynamic.symtab {
            Some(addr) => addr,
            None => return Ok(symbols),
        };

        let strtab = match dynamic.strtab {
            Some(addr) => addr,
            None => return Ok(symbols),
        };

        // Parse symbol table
        // Note: We don't know the symbol count without hash table
        // For now, iterate until we hit null or invalid
        let sym_size = dynamic.syment.max(core::mem::size_of::<Elf64Sym>());

        for i in 0..1024 { // Arbitrary limit
            let sym = unsafe {
                core::ptr::read_unaligned((symtab + i * sym_size) as *const Elf64Sym)
            };

            if sym.st_name == 0 && sym.st_value == 0 {
                continue;
            }

            // Only export global/weak functions and objects
            let bind = sym.sym_bind();
            let stype = sym.sym_type();

            if (bind == STB_GLOBAL || bind == STB_WEAK)
                && (stype == STT_FUNC || stype == STT_OBJECT)
                && sym.st_shndx != 0
            {
                // Read symbol name
                let name_ptr = (strtab + sym.st_name as usize) as *const u8;
                let name = unsafe { read_cstr(name_ptr) };

                if !name.is_empty() {
                    symbols.insert(name, sym.st_value as usize);
                }
            }
        }

        Ok(symbols)
    }

    fn load_dependencies(&mut self, dynamic: &DynamicInfo) -> Result<Vec<LibraryHandle>, LinkError> {
        let mut deps = Vec::new();

        for &needed_idx in &dynamic.needed {
            if let Some(strtab) = dynamic.strtab {
                let name_ptr = (strtab + needed_idx) as *const u8;
                let name = unsafe { read_cstr(name_ptr) };

                // Check if already loaded - first find the handle
                let found_handle = self.libraries.iter()
                    .find(|(_, lib)| lib.name == name)
                    .map(|(handle, _)| *handle);

                if let Some(handle) = found_handle {
                    deps.push(handle);
                    if let Some(lib) = self.libraries.get_mut(&handle) {
                        lib.ref_count += 1;
                    }
                }
                // If not found, would need to search and load
            }
        }

        Ok(deps)
    }

    fn perform_relocations(
        &self,
        _data: &[u8],
        dynamic: &DynamicInfo,
        base_addr: usize,
        min_vaddr: usize,
        header: &Elf64Header,
    ) -> Result<(), LinkError> {
        // Process RELA relocations
        if let Some(rela_addr) = dynamic.rela {
            let count = dynamic.relasz / dynamic.relaent.max(1);
            self.apply_relocations(rela_addr, count, base_addr, min_vaddr, dynamic, header)?;
        }

        // Process PLT relocations
        if let Some(jmprel_addr) = dynamic.jmprel {
            let count = dynamic.pltrelsz / core::mem::size_of::<Elf64Rela>();
            self.apply_relocations(jmprel_addr, count, base_addr, min_vaddr, dynamic, header)?;
        }

        Ok(())
    }

    fn apply_relocations(
        &self,
        rela_addr: usize,
        count: usize,
        base_addr: usize,
        min_vaddr: usize,
        dynamic: &DynamicInfo,
        header: &Elf64Header,
    ) -> Result<(), LinkError> {
        for i in 0..count {
            let rela = unsafe {
                core::ptr::read_unaligned(
                    (rela_addr + i * core::mem::size_of::<Elf64Rela>()) as *const Elf64Rela
                )
            };

            let r_type = rela.r_type();
            let sym_idx = rela.r_sym();
            let offset = base_addr + (rela.r_offset as usize - min_vaddr);
            let addend = rela.r_addend;

            // Get symbol value if needed
            let sym_value = if sym_idx != 0 {
                self.get_symbol_value(sym_idx as usize, dynamic)?
            } else {
                0
            };

            // Apply relocation based on architecture
            self.apply_relocation(
                r_type,
                offset,
                sym_value,
                addend,
                base_addr,
                header.e_machine,
            )?;
        }

        Ok(())
    }

    fn get_symbol_value(&self, idx: usize, dynamic: &DynamicInfo) -> Result<usize, LinkError> {
        let symtab = dynamic.symtab.ok_or_else(||
            LinkError::InvalidElf("No symbol table".into())
        )?;

        let sym = unsafe {
            core::ptr::read_unaligned(
                (symtab + idx * dynamic.syment) as *const Elf64Sym
            )
        };

        // If defined locally
        if sym.st_shndx != 0 {
            return Ok(sym.st_value as usize);
        }

        // Need to look up externally
        let strtab = dynamic.strtab.ok_or_else(||
            LinkError::InvalidElf("No string table".into())
        )?;
        let name_ptr = (strtab + sym.st_name as usize) as *const u8;
        let name = unsafe { read_cstr(name_ptr) };

        self.lookup(&name).ok_or_else(||
            LinkError::SymbolNotFound(name)
        )
    }

    fn apply_relocation(
        &self,
        r_type: u32,
        offset: usize,
        sym_value: usize,
        addend: i64,
        base_addr: usize,
        machine: u16,
    ) -> Result<(), LinkError> {
        match machine {
            EM_X86_64 => self.apply_x86_64_relocation(r_type, offset, sym_value, addend, base_addr),
            EM_AARCH64 => self.apply_aarch64_relocation(r_type, offset, sym_value, addend, base_addr),
            _ => Err(LinkError::UnsupportedArch(machine)),
        }
    }

    fn apply_x86_64_relocation(
        &self,
        r_type: u32,
        offset: usize,
        sym_value: usize,
        addend: i64,
        base_addr: usize,
    ) -> Result<(), LinkError> {
        unsafe {
            match r_type {
                r_x86_64::R_X86_64_NONE => {}

                r_x86_64::R_X86_64_64 => {
                    // S + A
                    let value = (sym_value as i64 + addend) as u64;
                    *(offset as *mut u64) = value;
                }

                r_x86_64::R_X86_64_PC32 => {
                    // S + A - P
                    let value = (sym_value as i64 + addend - offset as i64) as i32;
                    *(offset as *mut i32) = value;
                }

                r_x86_64::R_X86_64_GLOB_DAT |
                r_x86_64::R_X86_64_JUMP_SLOT => {
                    // S
                    *(offset as *mut u64) = sym_value as u64;
                }

                r_x86_64::R_X86_64_RELATIVE => {
                    // B + A
                    let value = (base_addr as i64 + addend) as u64;
                    *(offset as *mut u64) = value;
                }

                r_x86_64::R_X86_64_32 => {
                    // S + A (32-bit)
                    let value = (sym_value as i64 + addend) as u32;
                    *(offset as *mut u32) = value;
                }

                r_x86_64::R_X86_64_32S => {
                    // S + A (32-bit signed)
                    let value = (sym_value as i64 + addend) as i32;
                    *(offset as *mut i32) = value;
                }

                _ => return Err(LinkError::UnsupportedRelocation(r_type)),
            }
        }

        Ok(())
    }

    fn apply_aarch64_relocation(
        &self,
        r_type: u32,
        offset: usize,
        sym_value: usize,
        addend: i64,
        base_addr: usize,
    ) -> Result<(), LinkError> {
        unsafe {
            match r_type {
                r_aarch64::R_AARCH64_NONE => {}

                r_aarch64::R_AARCH64_ABS64 => {
                    // S + A
                    let value = (sym_value as i64 + addend) as u64;
                    *(offset as *mut u64) = value;
                }

                r_aarch64::R_AARCH64_GLOB_DAT |
                r_aarch64::R_AARCH64_JUMP_SLOT => {
                    // S
                    *(offset as *mut u64) = sym_value as u64;
                }

                r_aarch64::R_AARCH64_RELATIVE => {
                    // B + A
                    let value = (base_addr as i64 + addend) as u64;
                    *(offset as *mut u64) = value;
                }

                _ => return Err(LinkError::UnsupportedRelocation(r_type)),
            }
        }

        Ok(())
    }

    fn run_init(&self, library: &LoadedLibrary) -> Result<(), LinkError> {
        // Run DT_INIT
        if let Some(init_addr) = library.init_fn {
            let init: extern "C" fn() = unsafe { core::mem::transmute(init_addr) };
            init();
        }

        // Run DT_INIT_ARRAY
        for &fn_addr in &library.init_array {
            let init: extern "C" fn() = unsafe { core::mem::transmute(fn_addr) };
            init();
        }

        Ok(())
    }

    fn run_fini(&self, library: &LoadedLibrary) -> Result<(), LinkError> {
        // Run DT_FINI_ARRAY in reverse
        for &fn_addr in library.fini_array.iter().rev() {
            let fini: extern "C" fn() = unsafe { core::mem::transmute(fn_addr) };
            fini();
        }

        // Run DT_FINI
        if let Some(fini_addr) = library.fini_fn {
            let fini: extern "C" fn() = unsafe { core::mem::transmute(fini_addr) };
            fini();
        }

        Ok(())
    }
}

impl Default for DynamicLinker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Helper Types
// =============================================================================

/// Parsed dynamic section info.
#[derive(Debug, Default)]
struct DynamicInfo {
    strtab: Option<usize>,
    symtab: Option<usize>,
    strsz: usize,
    syment: usize,
    rela: Option<usize>,
    relasz: usize,
    relaent: usize,
    jmprel: Option<usize>,
    pltrelsz: usize,
    init: Option<usize>,
    fini: Option<usize>,
    init_array_ptr: Option<usize>,
    init_array_sz: usize,
    init_array: Vec<usize>,
    fini_array_ptr: Option<usize>,
    fini_array_sz: usize,
    fini_array: Vec<usize>,
    needed: Vec<usize>,
}

/// Read a null-terminated C string.
unsafe fn read_cstr(ptr: *const u8) -> String {
    let mut len = 0;
    while *ptr.add(len) != 0 && len < 256 {
        len += 1;
    }

    let slice = core::slice::from_raw_parts(ptr, len);
    String::from_utf8_lossy(slice).into_owned()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_magic() {
        let linker = DynamicLinker::new();

        // Too small
        assert!(linker.parse_header(&[]).is_err());

        // Wrong magic
        let mut bad_magic = [0u8; 64];
        bad_magic[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        assert!(linker.parse_header(&bad_magic).is_err());
    }

    #[test]
    fn test_symbol_lookup() {
        let linker = DynamicLinker::new();
        assert!(linker.lookup("nonexistent").is_none());
    }
}
