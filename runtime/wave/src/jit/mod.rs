//! # JIT Compiler for S-WAVE
//!
//! This module implements a Just-In-Time compiler for WebAssembly,
//! providing significant performance improvements over interpretation.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     WASM Bytecode                           │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   IR Generation                             │
//! │  (Convert WASM ops to typed IR with SSA form)              │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Optimization Passes                       │
//! │  (Constant folding, dead code elimination, etc.)           │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  Register Allocation                        │
//! │  (Linear scan allocator for physical registers)            │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   Code Generation                           │
//! │  (Emit native x86_64 or aarch64 machine code)              │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  Executable Memory                          │
//! │  (mmap with PROT_EXEC, code patching, relocations)         │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![allow(dead_code)]

pub mod ir;
pub mod regalloc;
pub mod x86_64;
pub mod memory;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use ir::{IrFunction, IrModule};
use regalloc::RegisterAllocator;
use memory::ExecutableMemory;

/// JIT compilation error.
#[derive(Debug, Clone)]
pub enum JitError {
    /// Invalid WASM bytecode.
    InvalidBytecode(String),
    /// Unsupported instruction.
    UnsupportedInstruction(u8),
    /// Register allocation failed.
    RegisterAllocationFailed,
    /// Code generation failed.
    CodeGenFailed(String),
    /// Memory allocation failed.
    MemoryAllocationFailed,
    /// Function not found.
    FunctionNotFound(u32),
}

/// Compilation tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompilationTier {
    /// Baseline: fast compilation, slower execution.
    Baseline,
    /// Optimizing: slower compilation, faster execution.
    Optimizing,
}

/// JIT compiler configuration.
#[derive(Debug, Clone)]
pub struct JitConfig {
    /// Initial compilation tier.
    pub initial_tier: CompilationTier,
    /// Threshold for tier-up (number of calls before optimizing).
    pub tier_up_threshold: u32,
    /// Enable bounds check elimination.
    pub eliminate_bounds_checks: bool,
    /// Enable constant folding.
    pub constant_folding: bool,
    /// Enable dead code elimination.
    pub dead_code_elimination: bool,
    /// Enable inlining.
    pub inlining: bool,
    /// Maximum inline size (in IR instructions).
    pub max_inline_size: usize,
}

impl Default for JitConfig {
    fn default() -> Self {
        Self {
            initial_tier: CompilationTier::Baseline,
            tier_up_threshold: 1000,
            eliminate_bounds_checks: true,
            constant_folding: true,
            dead_code_elimination: true,
            inlining: true,
            max_inline_size: 32,
        }
    }
}

/// Compiled function metadata.
#[derive(Debug)]
pub struct CompiledFunction {
    /// Function index in the module.
    pub func_idx: u32,
    /// Pointer to executable code.
    pub code_ptr: *const u8,
    /// Code size in bytes.
    pub code_size: usize,
    /// Compilation tier.
    pub tier: CompilationTier,
    /// Call count for tier-up tracking.
    pub call_count: u32,
    /// Stack frame size.
    pub frame_size: u32,
}

impl CompiledFunction {
    /// Get function pointer for calling.
    pub fn as_fn_ptr<T>(&self) -> T
    where
        T: Copy,
    {
        unsafe { core::mem::transmute_copy(&self.code_ptr) }
    }
}

/// JIT compiler.
pub struct JitCompiler {
    /// Configuration.
    config: JitConfig,
    /// Compiled functions.
    functions: BTreeMap<u32, CompiledFunction>,
    /// Executable memory manager.
    memory: ExecutableMemory,
    /// Register allocator.
    regalloc: RegisterAllocator,
}

impl JitCompiler {
    /// Create a new JIT compiler.
    pub fn new(config: JitConfig) -> Self {
        Self {
            config,
            functions: BTreeMap::new(),
            memory: ExecutableMemory::new(),
            regalloc: RegisterAllocator::new(),
        }
    }

    /// Compile a WASM module to native code.
    pub fn compile_module(&mut self, wasm: &[u8]) -> Result<(), JitError> {
        // Parse WASM and generate IR
        let ir_module = self.generate_ir(wasm)?;

        // Compile each function
        for (idx, func) in ir_module.functions.iter().enumerate() {
            self.compile_function(idx as u32, func)?;
        }

        Ok(())
    }

    /// Compile a single function.
    pub fn compile_function(
        &mut self,
        func_idx: u32,
        ir_func: &IrFunction,
    ) -> Result<(), JitError> {
        // Apply optimizations if using optimizing tier
        let optimized = if self.config.initial_tier == CompilationTier::Optimizing {
            self.optimize(ir_func)?
        } else {
            ir_func.clone()
        };

        // Allocate registers
        let allocated = self.regalloc.allocate(&optimized)?;

        // Generate native code
        #[cfg(target_arch = "x86_64")]
        let code = x86_64::generate(&allocated, &self.config)?;

        #[cfg(not(target_arch = "x86_64"))]
        let code = Vec::new();

        // Allocate executable memory and copy code
        let code_ptr = self.memory.allocate(code.len())?;
        unsafe {
            core::ptr::copy_nonoverlapping(code.as_ptr(), code_ptr as *mut u8, code.len());
        }

        // Store compiled function
        self.functions.insert(
            func_idx,
            CompiledFunction {
                func_idx,
                code_ptr,
                code_size: code.len(),
                tier: self.config.initial_tier,
                call_count: 0,
                frame_size: allocated.frame_size,
            },
        );

        Ok(())
    }

    /// Get a compiled function.
    pub fn get_function(&self, func_idx: u32) -> Option<&CompiledFunction> {
        self.functions.get(&func_idx)
    }

    /// Generate IR from WASM bytecode.
    fn generate_ir(&self, wasm: &[u8]) -> Result<IrModule, JitError> {
        ir::generate_ir(wasm)
    }

    /// Apply optimization passes.
    fn optimize(&self, func: &IrFunction) -> Result<IrFunction, JitError> {
        let mut optimized = func.clone();

        if self.config.constant_folding {
            ir::optimize::constant_fold(&mut optimized);
        }

        if self.config.dead_code_elimination {
            ir::optimize::eliminate_dead_code(&mut optimized);
        }

        Ok(optimized)
    }

    /// Check if function should be tier-up compiled.
    pub fn should_tier_up(&self, func_idx: u32) -> bool {
        if let Some(func) = self.functions.get(&func_idx) {
            func.tier == CompilationTier::Baseline
                && func.call_count >= self.config.tier_up_threshold
        } else {
            false
        }
    }

    /// Increment call count for a function.
    pub fn record_call(&mut self, func_idx: u32) {
        if let Some(func) = self.functions.get_mut(&func_idx) {
            func.call_count = func.call_count.saturating_add(1);
        }
    }

    /// Tier-up a function to optimizing tier.
    pub fn tier_up(&mut self, func_idx: u32, ir_func: &IrFunction) -> Result<(), JitError> {
        // Save old config
        let old_tier = self.config.initial_tier;
        self.config.initial_tier = CompilationTier::Optimizing;

        // Recompile with optimizations
        let result = self.compile_function(func_idx, ir_func);

        // Restore config
        self.config.initial_tier = old_tier;

        result
    }
}

/// Statistics about JIT compilation.
#[derive(Debug, Default)]
pub struct JitStats {
    /// Total functions compiled.
    pub functions_compiled: u32,
    /// Functions at baseline tier.
    pub baseline_functions: u32,
    /// Functions at optimizing tier.
    pub optimizing_functions: u32,
    /// Total code size in bytes.
    pub total_code_size: usize,
    /// Time spent compiling (microseconds).
    pub compile_time_us: u64,
}

impl JitCompiler {
    /// Get compilation statistics.
    pub fn stats(&self) -> JitStats {
        let mut stats = JitStats::default();

        for func in self.functions.values() {
            stats.functions_compiled += 1;
            stats.total_code_size += func.code_size;

            match func.tier {
                CompilationTier::Baseline => stats.baseline_functions += 1,
                CompilationTier::Optimizing => stats.optimizing_functions += 1,
            }
        }

        stats
    }
}
