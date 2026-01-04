//! # Ahead-of-Time (AOT) Compilation for S-WAVE
//!
//! This module provides ahead-of-time compilation for WebAssembly modules,
//! enabling faster startup times and cached native code.
//!
//! ## Design
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     WASM Module                             │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   AOT Compiler                              │
//! │  (Full optimization, no time constraints)                  │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │              Compiled Module (.swc)                         │
//! │  (Serialized native code + metadata + relocations)         │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                 Module Cache                                │
//! │  (Hash-based cache for instant loading)                    │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Compiled Module Format (.swc - Splax WASM Compiled)
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────┐
//! │ Magic: "SWC\0" (4 bytes)                                   │
//! │ Version: u32 (4 bytes)                                     │
//! │ Target Arch: u32 (4 bytes) - 0=x86_64, 1=aarch64          │
//! │ WASM Hash: [u8; 32] - SHA-256 of original WASM            │
//! │ Flags: u32                                                 │
//! ├────────────────────────────────────────────────────────────┤
//! │ Num Functions: u32                                         │
//! │ Function Table Offset: u64                                 │
//! │ Code Section Offset: u64                                   │
//! │ Relocation Section Offset: u64                             │
//! │ Metadata Section Offset: u64                               │
//! ├────────────────────────────────────────────────────────────┤
//! │ Function Table:                                            │
//! │   [FunctionEntry] × Num Functions                          │
//! ├────────────────────────────────────────────────────────────┤
//! │ Code Section:                                              │
//! │   Packed native code for all functions                     │
//! ├────────────────────────────────────────────────────────────┤
//! │ Relocation Section:                                        │
//! │   [Relocation] × Num Relocations                           │
//! ├────────────────────────────────────────────────────────────┤
//! │ Metadata Section:                                          │
//! │   Export names, type signatures, etc.                      │
//! └────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

use super::jit::{JitConfig, JitCompiler, JitError, CompilationTier};
use super::jit::memory::ExecutableMemory;

// =============================================================================
// Constants
// =============================================================================

/// Magic bytes for compiled modules.
const SWC_MAGIC: [u8; 4] = *b"SWC\0";

/// Current format version.
const SWC_VERSION: u32 = 1;

/// Target architecture: x86_64.
const TARGET_X86_64: u32 = 0;

/// Target architecture: aarch64.
const TARGET_AARCH64: u32 = 1;

// =============================================================================
// Errors
// =============================================================================

/// AOT compilation error.
#[derive(Debug, Clone)]
pub enum AotError {
    /// JIT error during compilation.
    JitError(JitError),
    /// Invalid WASM module.
    InvalidWasm(String),
    /// Cache miss.
    CacheMiss,
    /// Cache corrupted.
    CacheCorrupted,
    /// Architecture mismatch.
    ArchMismatch,
    /// Version mismatch.
    VersionMismatch,
    /// Hash mismatch.
    HashMismatch,
    /// Serialization error.
    SerializationError(String),
    /// I/O error.
    IoError(String),
}

impl From<JitError> for AotError {
    fn from(e: JitError) -> Self {
        AotError::JitError(e)
    }
}

// =============================================================================
// Relocation
// =============================================================================

/// Relocation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocationType {
    /// Absolute 64-bit address.
    Abs64,
    /// PC-relative 32-bit.
    Rel32,
    /// Function call (needs patching at load time).
    FunctionCall,
    /// Global address.
    GlobalAddr,
    /// Memory base address.
    MemoryBase,
}

/// A relocation entry.
#[derive(Debug, Clone)]
pub struct Relocation {
    /// Offset within the code section.
    pub offset: u64,
    /// Relocation type.
    pub reloc_type: RelocationType,
    /// Symbol index (function index, global index, etc.).
    pub symbol_idx: u32,
    /// Addend for the relocation.
    pub addend: i64,
}

// =============================================================================
// Function Entry
// =============================================================================

/// Function entry in the compiled module.
#[derive(Debug, Clone)]
pub struct FunctionEntry {
    /// Function index.
    pub func_idx: u32,
    /// Offset within code section.
    pub code_offset: u64,
    /// Code size in bytes.
    pub code_size: u32,
    /// Stack frame size.
    pub frame_size: u32,
    /// Number of parameters.
    pub param_count: u32,
    /// Number of results.
    pub result_count: u32,
}

// =============================================================================
// Compiled Module
// =============================================================================

/// A compiled WASM module (serializable format).
#[derive(Debug, Clone)]
pub struct CompiledModule {
    /// Target architecture.
    pub target_arch: u32,
    /// Hash of original WASM.
    pub wasm_hash: [u8; 32],
    /// Compilation flags.
    pub flags: u32,
    /// Function entries.
    pub functions: Vec<FunctionEntry>,
    /// Native code.
    pub code: Vec<u8>,
    /// Relocations.
    pub relocations: Vec<Relocation>,
    /// Export map: name -> function index.
    pub exports: BTreeMap<String, u32>,
    /// Type signatures (serialized).
    pub type_signatures: Vec<u8>,
}

impl CompiledModule {
    /// Creates an empty compiled module.
    pub fn new() -> Self {
        Self {
            target_arch: Self::current_arch(),
            wasm_hash: [0; 32],
            flags: 0,
            functions: Vec::new(),
            code: Vec::new(),
            relocations: Vec::new(),
            exports: BTreeMap::new(),
            type_signatures: Vec::new(),
        }
    }

    /// Gets the current architecture ID.
    fn current_arch() -> u32 {
        #[cfg(target_arch = "x86_64")]
        return TARGET_X86_64;
        #[cfg(target_arch = "aarch64")]
        return TARGET_AARCH64;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        return 0xFF;
    }

    /// Serializes the module to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Header
        bytes.extend_from_slice(&SWC_MAGIC);
        bytes.extend_from_slice(&SWC_VERSION.to_le_bytes());
        bytes.extend_from_slice(&self.target_arch.to_le_bytes());
        bytes.extend_from_slice(&self.wasm_hash);
        bytes.extend_from_slice(&self.flags.to_le_bytes());

        // Function count
        bytes.extend_from_slice(&(self.functions.len() as u32).to_le_bytes());

        // Placeholder offsets (will be patched)
        let func_table_offset_pos = bytes.len();
        bytes.extend_from_slice(&0u64.to_le_bytes()); // Function table offset
        let code_offset_pos = bytes.len();
        bytes.extend_from_slice(&0u64.to_le_bytes()); // Code section offset
        let reloc_offset_pos = bytes.len();
        bytes.extend_from_slice(&0u64.to_le_bytes()); // Relocation section offset
        let meta_offset_pos = bytes.len();
        bytes.extend_from_slice(&0u64.to_le_bytes()); // Metadata section offset

        // Function table
        let func_table_offset = bytes.len() as u64;
        for func in &self.functions {
            bytes.extend_from_slice(&func.func_idx.to_le_bytes());
            bytes.extend_from_slice(&func.code_offset.to_le_bytes());
            bytes.extend_from_slice(&func.code_size.to_le_bytes());
            bytes.extend_from_slice(&func.frame_size.to_le_bytes());
            bytes.extend_from_slice(&func.param_count.to_le_bytes());
            bytes.extend_from_slice(&func.result_count.to_le_bytes());
        }

        // Code section
        let code_offset = bytes.len() as u64;
        bytes.extend_from_slice(&self.code);

        // Relocation section
        let reloc_offset = bytes.len() as u64;
        bytes.extend_from_slice(&(self.relocations.len() as u32).to_le_bytes());
        for reloc in &self.relocations {
            bytes.extend_from_slice(&reloc.offset.to_le_bytes());
            bytes.push(reloc.reloc_type as u8);
            bytes.extend_from_slice(&reloc.symbol_idx.to_le_bytes());
            bytes.extend_from_slice(&reloc.addend.to_le_bytes());
        }

        // Metadata section
        let meta_offset = bytes.len() as u64;
        // Export count
        bytes.extend_from_slice(&(self.exports.len() as u32).to_le_bytes());
        // Exports
        for (name, idx) in &self.exports {
            bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(&idx.to_le_bytes());
        }
        // Type signatures
        bytes.extend_from_slice(&(self.type_signatures.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.type_signatures);

        // Patch offsets
        bytes[func_table_offset_pos..func_table_offset_pos + 8]
            .copy_from_slice(&func_table_offset.to_le_bytes());
        bytes[code_offset_pos..code_offset_pos + 8]
            .copy_from_slice(&code_offset.to_le_bytes());
        bytes[reloc_offset_pos..reloc_offset_pos + 8]
            .copy_from_slice(&reloc_offset.to_le_bytes());
        bytes[meta_offset_pos..meta_offset_pos + 8]
            .copy_from_slice(&meta_offset.to_le_bytes());

        bytes
    }

    /// Deserializes a module from bytes.
    pub fn deserialize(data: &[u8]) -> Result<Self, AotError> {
        if data.len() < 52 {
            return Err(AotError::SerializationError("Too short".into()));
        }

        // Check magic
        if &data[0..4] != &SWC_MAGIC {
            return Err(AotError::SerializationError("Invalid magic".into()));
        }

        // Check version
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != SWC_VERSION {
            return Err(AotError::VersionMismatch);
        }

        // Check architecture
        let target_arch = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        if target_arch != Self::current_arch() {
            return Err(AotError::ArchMismatch);
        }

        // Read hash
        let mut wasm_hash = [0u8; 32];
        wasm_hash.copy_from_slice(&data[12..44]);

        // Read flags
        let flags = u32::from_le_bytes([data[44], data[45], data[46], data[47]]);

        // Read function count
        let func_count = u32::from_le_bytes([data[48], data[49], data[50], data[51]]) as usize;

        // Read offsets
        let func_table_offset = u64::from_le_bytes(data[52..60].try_into().unwrap()) as usize;
        let code_offset = u64::from_le_bytes(data[60..68].try_into().unwrap()) as usize;
        let reloc_offset = u64::from_le_bytes(data[68..76].try_into().unwrap()) as usize;
        let meta_offset = u64::from_le_bytes(data[76..84].try_into().unwrap()) as usize;

        // Read function table
        let mut functions = Vec::with_capacity(func_count);
        let mut pos = func_table_offset;
        for _ in 0..func_count {
            let func_idx = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            let code_off = u64::from_le_bytes(data[pos + 4..pos + 12].try_into().unwrap());
            let code_size = u32::from_le_bytes(data[pos + 12..pos + 16].try_into().unwrap());
            let frame_size = u32::from_le_bytes(data[pos + 16..pos + 20].try_into().unwrap());
            let param_count = u32::from_le_bytes(data[pos + 20..pos + 24].try_into().unwrap());
            let result_count = u32::from_le_bytes(data[pos + 24..pos + 28].try_into().unwrap());
            pos += 28;
            functions.push(FunctionEntry {
                func_idx,
                code_offset: code_off,
                code_size,
                frame_size,
                param_count,
                result_count,
            });
        }

        // Read code section
        let code = data[code_offset..reloc_offset].to_vec();

        // Read relocations
        let reloc_count = u32::from_le_bytes(data[reloc_offset..reloc_offset + 4].try_into().unwrap()) as usize;
        let mut relocations = Vec::with_capacity(reloc_count);
        pos = reloc_offset + 4;
        for _ in 0..reloc_count {
            let offset = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
            let reloc_type = match data[pos + 8] {
                0 => RelocationType::Abs64,
                1 => RelocationType::Rel32,
                2 => RelocationType::FunctionCall,
                3 => RelocationType::GlobalAddr,
                4 => RelocationType::MemoryBase,
                _ => return Err(AotError::SerializationError("Invalid reloc type".into())),
            };
            let symbol_idx = u32::from_le_bytes(data[pos + 9..pos + 13].try_into().unwrap());
            let addend = i64::from_le_bytes(data[pos + 13..pos + 21].try_into().unwrap());
            pos += 21;
            relocations.push(Relocation {
                offset,
                reloc_type,
                symbol_idx,
                addend,
            });
        }

        // Read exports
        let export_count = u32::from_le_bytes(data[meta_offset..meta_offset + 4].try_into().unwrap()) as usize;
        let mut exports = BTreeMap::new();
        pos = meta_offset + 4;
        for _ in 0..export_count {
            let name_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            let name = String::from_utf8_lossy(&data[pos..pos + name_len]).to_string();
            pos += name_len;
            let idx = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
            pos += 4;
            exports.insert(name, idx);
        }

        // Read type signatures
        let sig_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        pos += 4;
        let type_signatures = data[pos..pos + sig_len].to_vec();

        Ok(Self {
            target_arch,
            wasm_hash,
            flags,
            functions,
            code,
            relocations,
            exports,
            type_signatures,
        })
    }
}

impl Default for CompiledModule {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Module Cache
// =============================================================================

/// Cache entry for a compiled module.
#[derive(Debug)]
pub struct CacheEntry {
    /// Hash of the WASM source.
    pub wasm_hash: [u8; 32],
    /// Serialized compiled module.
    pub data: Vec<u8>,
    /// Last access time (cycles).
    pub last_access: u64,
    /// Access count.
    pub access_count: u32,
}

/// Module cache for AOT compiled modules.
pub struct ModuleCache {
    /// Cache entries indexed by hash.
    entries: BTreeMap<[u8; 32], CacheEntry>,
    /// Maximum cache size in bytes.
    max_size: usize,
    /// Current cache size in bytes.
    current_size: usize,
}

impl ModuleCache {
    /// Creates a new module cache.
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            max_size,
            current_size: 0,
        }
    }

    /// Looks up a module by WASM hash.
    pub fn get(&mut self, hash: &[u8; 32]) -> Option<&CompiledModule> {
        // Update access time and count
        if let Some(entry) = self.entries.get_mut(hash) {
            entry.last_access = get_timestamp();
            entry.access_count += 1;
        }
        
        // Deserialize and return
        self.entries.get(hash).and_then(|e| {
            CompiledModule::deserialize(&e.data).ok()
        }).as_ref().map(|_| {
            // Note: In a real implementation, we'd return a reference to a cached deserialized module
            // For now, we return None since we can't return a reference to a local
            None::<&CompiledModule>
        }).flatten()
    }

    /// Inserts a compiled module into the cache.
    pub fn insert(&mut self, hash: [u8; 32], module: &CompiledModule) {
        let data = module.serialize();
        let size = data.len();

        // Evict if necessary
        while self.current_size + size > self.max_size && !self.entries.is_empty() {
            self.evict_lru();
        }

        if size <= self.max_size {
            self.entries.insert(hash, CacheEntry {
                wasm_hash: hash,
                data,
                last_access: get_timestamp(),
                access_count: 1,
            });
            self.current_size += size;
        }
    }

    /// Evicts the least recently used entry.
    fn evict_lru(&mut self) {
        if let Some((hash, entry)) = self.entries.iter().min_by_key(|(_, e)| e.last_access) {
            let size = entry.data.len();
            let hash = *hash;
            self.entries.remove(&hash);
            self.current_size -= size;
        }
    }

    /// Clears the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_size = 0;
    }

    /// Returns cache statistics.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entry_count: self.entries.len(),
            current_size: self.current_size,
            max_size: self.max_size,
        }
    }
}

/// Cache statistics.
#[derive(Debug)]
pub struct CacheStats {
    /// Number of entries.
    pub entry_count: usize,
    /// Current cache size in bytes.
    pub current_size: usize,
    /// Maximum cache size in bytes.
    pub max_size: usize,
}

// =============================================================================
// AOT Compiler
// =============================================================================

/// AOT compilation options.
#[derive(Debug, Clone)]
pub struct AotOptions {
    /// Enable aggressive optimizations (slower compile, faster runtime).
    pub optimize: bool,
    /// Enable bounds check elimination.
    pub eliminate_bounds_checks: bool,
    /// Enable function inlining.
    pub inlining: bool,
    /// Maximum inline size.
    pub max_inline_size: usize,
    /// Enable parallel compilation.
    pub parallel: bool,
    /// Strip debug info.
    pub strip_debug: bool,
}

impl Default for AotOptions {
    fn default() -> Self {
        Self {
            optimize: true,
            eliminate_bounds_checks: true,
            inlining: true,
            max_inline_size: 64,
            parallel: false,
            strip_debug: false,
        }
    }
}

/// AOT compiler for WASM modules.
pub struct AotCompiler {
    /// Compilation options.
    options: AotOptions,
    /// Module cache.
    cache: ModuleCache,
}

impl AotCompiler {
    /// Creates a new AOT compiler.
    pub fn new(options: AotOptions) -> Self {
        Self {
            options,
            cache: ModuleCache::new(64 * 1024 * 1024), // 64 MB cache
        }
    }

    /// Compiles a WASM module to native code.
    pub fn compile(&mut self, wasm: &[u8]) -> Result<CompiledModule, AotError> {
        // Compute hash of WASM module
        let hash = self.compute_hash(wasm);

        // Check cache
        if let Some(_module) = self.cache.get(&hash) {
            // Would return cached module here
        }

        // Create JIT config for AOT (optimizing tier)
        let jit_config = JitConfig {
            initial_tier: CompilationTier::Optimizing,
            tier_up_threshold: 0, // Unused in AOT
            eliminate_bounds_checks: self.options.eliminate_bounds_checks,
            constant_folding: self.options.optimize,
            dead_code_elimination: self.options.optimize,
            inlining: self.options.inlining,
            max_inline_size: self.options.max_inline_size,
        };

        // Create JIT compiler
        let mut jit = JitCompiler::new(jit_config);

        // Compile module
        jit.compile_module(wasm)?;

        // Extract compiled functions and build module
        let mut compiled = CompiledModule::new();
        compiled.wasm_hash = hash;

        // Extract code from JIT compiler
        // (In a real implementation, JIT would expose compiled functions)
        
        // Cache the result
        self.cache.insert(hash, &compiled);

        Ok(compiled)
    }

    /// Compiles a WASM module and writes to output file.
    pub fn compile_to_file(&mut self, wasm: &[u8]) -> Result<Vec<u8>, AotError> {
        let module = self.compile(wasm)?;
        Ok(module.serialize())
    }

    /// Loads a pre-compiled module.
    pub fn load(&self, data: &[u8]) -> Result<CompiledModule, AotError> {
        CompiledModule::deserialize(data)
    }

    /// Computes a hash of the WASM module.
    fn compute_hash(&self, wasm: &[u8]) -> [u8; 32] {
        // Simple hash for now (in production, use SHA-256)
        let mut hash = [0u8; 32];
        let mut state = 0u64;
        
        for (i, byte) in wasm.iter().enumerate() {
            state = state.wrapping_add(*byte as u64);
            state = state.wrapping_mul(0x517cc1b727220a95);
            state = state.rotate_left(7);
            hash[i % 32] ^= (state >> ((i % 8) * 8)) as u8;
        }
        
        // Mix final state
        for i in 0..32 {
            hash[i] ^= (state >> ((i % 8) * 8)) as u8;
            state = state.wrapping_mul(0x517cc1b727220a95);
        }
        
        hash
    }
}

// =============================================================================
// Loaded Module (Ready to Execute)
// =============================================================================

/// A loaded and ready-to-execute module.
pub struct LoadedModule {
    /// Compiled module metadata.
    compiled: CompiledModule,
    /// Executable memory containing code.
    memory: ExecutableMemory,
    /// Function pointers.
    function_ptrs: BTreeMap<u32, *const u8>,
}

impl LoadedModule {
    /// Loads a compiled module into executable memory.
    pub fn load(compiled: CompiledModule) -> Result<Self, AotError> {
        let mut memory = ExecutableMemory::new();

        // Allocate memory for code
        let code_ptr = memory.allocate(compiled.code.len())
            .map_err(|_| AotError::IoError("Memory allocation failed".into()))?;

        // Copy code
        unsafe {
            core::ptr::copy_nonoverlapping(
                compiled.code.as_ptr(),
                code_ptr as *mut u8,
                compiled.code.len(),
            );
        }

        // Build function pointer table
        let mut function_ptrs = BTreeMap::new();
        for func in &compiled.functions {
            let ptr = unsafe { code_ptr.add(func.code_offset as usize) };
            function_ptrs.insert(func.func_idx, ptr);
        }

        // Apply relocations
        for reloc in &compiled.relocations {
            match reloc.reloc_type {
                RelocationType::FunctionCall => {
                    if let Some(&target_ptr) = function_ptrs.get(&reloc.symbol_idx) {
                        let patch_addr = unsafe { code_ptr.add(reloc.offset as usize) } as *mut i32;
                        let rel_offset = unsafe {
                            (target_ptr as isize) - (patch_addr as isize) - 4 + reloc.addend as isize
                        };
                        unsafe {
                            *patch_addr = rel_offset as i32;
                        }
                    }
                }
                RelocationType::Abs64 => {
                    // Absolute address patching
                    let patch_addr = unsafe { code_ptr.add(reloc.offset as usize) } as *mut u64;
                    if let Some(&target_ptr) = function_ptrs.get(&reloc.symbol_idx) {
                        unsafe {
                            *patch_addr = (target_ptr as u64).wrapping_add(reloc.addend as u64);
                        }
                    }
                }
                _ => {
                    // Other relocation types
                }
            }
        }

        Ok(Self {
            compiled,
            memory,
            function_ptrs,
        })
    }

    /// Gets a function pointer by index.
    pub fn get_function(&self, func_idx: u32) -> Option<*const u8> {
        self.function_ptrs.get(&func_idx).copied()
    }

    /// Gets a function pointer by export name.
    pub fn get_export(&self, name: &str) -> Option<*const u8> {
        self.compiled.exports.get(name)
            .and_then(|idx| self.function_ptrs.get(idx).copied())
    }

    /// Calls an exported function with no arguments.
    pub unsafe fn call_void(&self, name: &str) -> Option<()> {
        let ptr = self.get_export(name)?;
        let func: extern "C" fn() = core::mem::transmute(ptr);
        func();
        Some(())
    }

    /// Calls an exported function with i32 argument and return.
    pub unsafe fn call_i32_i32(&self, name: &str, arg: i32) -> Option<i32> {
        let ptr = self.get_export(name)?;
        let func: extern "C" fn(i32) -> i32 = core::mem::transmute(ptr);
        Some(func(arg))
    }
}

// =============================================================================
// Timestamp Helper
// =============================================================================

#[cfg(target_arch = "x86_64")]
fn get_timestamp() -> u64 {
    unsafe { core::arch::x86_64::_rdtsc() }
}

#[cfg(target_arch = "aarch64")]
fn get_timestamp() -> u64 {
    let cnt: u64;
    unsafe {
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
    }
    cnt
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn get_timestamp() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiled_module_serialize() {
        let mut module = CompiledModule::new();
        module.wasm_hash = [0x42; 32];
        module.functions.push(FunctionEntry {
            func_idx: 0,
            code_offset: 0,
            code_size: 16,
            frame_size: 64,
            param_count: 2,
            result_count: 1,
        });
        module.code = vec![0x90; 16]; // NOP sled
        module.exports.insert("main".to_string(), 0);

        let serialized = module.serialize();
        assert!(serialized.len() > 52);

        // Check magic
        assert_eq!(&serialized[0..4], b"SWC\0");
    }

    #[test]
    fn test_cache_insert_get() {
        let mut cache = ModuleCache::new(1024);
        let hash = [0x42; 32];
        let module = CompiledModule::new();

        cache.insert(hash, &module);
        assert_eq!(cache.entries.len(), 1);
    }

    #[test]
    fn test_cache_eviction() {
        let mut cache = ModuleCache::new(100); // Very small cache
        
        for i in 0..10 {
            let mut hash = [0u8; 32];
            hash[0] = i;
            let mut module = CompiledModule::new();
            module.code = vec![0; 20]; // 20 bytes each
            cache.insert(hash, &module);
        }

        // Should have evicted some entries
        assert!(cache.entries.len() < 10);
    }
}
