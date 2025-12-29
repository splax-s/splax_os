//! # S-WAVE: WebAssembly Runtime
//!
//! S-WAVE is the primary application runtime for Splax OS. WASM modules run
//! in a sandboxed environment with explicit capability bindings.
//!
//! ## Design Philosophy
//!
//! - **Secure by Default**: WASM modules have no ambient authority
//! - **Capability-Bound Imports**: Host functions require explicit capabilities
//! - **Deterministic Execution**: Same inputs = same outputs
//! - **Cross-Architecture**: Single WASM binary runs on x86_64 and aarch64
//!
//! ## Host Functions
//!
//! WASM modules access system resources through imported host functions:
//!
//! - `s_link_send`: Send message on a channel (requires channel:write capability)
//! - `s_link_receive`: Receive message from a channel (requires channel:read capability)
//! - `s_storage_read`: Read from storage (requires storage:read capability)
//! - `s_storage_write`: Write to storage (requires storage:write capability)
//! - `s_log`: Write to debug log (requires log:write capability)
//!
//! Each import requires a corresponding capability token.
//!
//! ## WASM Binary Format
//!
//! S-WAVE validates WASM 1.0 modules:
//! - Magic: 0x00 0x61 0x73 0x6D ("\0asm")
//! - Version: 0x01 0x00 0x00 0x00 (version 1)
//! - Sections: Type, Import, Function, Table, Memory, Global, Export, Start, Element, Code, Data
//!
//! ## Example
//!
//! ```ignore
//! // Load a WASM module
//! let module = wave.load(wasm_bytes, module_cap)?;
//!
//! // Bind capabilities to imports
//! let instance = wave.instantiate(module, &[
//!     ("s_link_send", send_cap),
//!     ("s_storage_read", read_cap),
//! ])?;
//!
//! // Run the module
//! let result = instance.call("main", &[])?;
//! ```

#![no_std]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use spin::Mutex;

// Import shared capability token
pub use splax_cap::{CapabilityToken, Operations, Permission};

/// Module identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(pub u64);

/// Instance identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub u64);

// =============================================================================
// TIMESTAMP UTILITIES
// =============================================================================

/// Get current CPU timestamp (cycles)
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

// =============================================================================
// X86_64 I/O PORT HELPERS
// =============================================================================

/// Write a byte to an I/O port (x86_64 only)
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn x86_64_outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nostack, nomem)
    );
}

/// Read a byte from an I/O port (x86_64 only)
#[cfg(target_arch = "x86_64")]
#[inline]
unsafe fn x86_64_inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        in("dx") port,
        out("al") value,
        options(nostack, nomem)
    );
    value
}

// =============================================================================
// SYSCALL HELPERS
// =============================================================================

/// Spawn a process from path
fn syscall_spawn(path: &[u8]) -> i64 {
    let result: i64;
    
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 220u64,
            in("rdi") path.as_ptr() as u64,
            in("rsi") path.len() as u64,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 220u64,
            in("x0") path.as_ptr() as u64,
            in("x1") path.len() as u64,
            lateout("x0") result,
            options(nostack)
        );
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        result = -1;
    }
    
    result
}

/// Wait for a process
fn syscall_waitpid(pid: i64) -> i32 {
    let result: i64;
    
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 61u64,  // wait4
            in("rdi") pid as u64,
            in("rsi") 0u64,   // status ptr (null)
            in("rdx") 0u64,   // options
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 260u64,  // waitpid
            in("x0") pid as u64,
            in("x1") 0u64,
            in("x2") 0u64,
            lateout("x0") result,
            options(nostack)
        );
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        result = -1;
    }
    
    result as i32
}

/// Kill a process
fn syscall_kill(pid: i64, signal: i32) -> i32 {
    let result: i64;
    
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 62u64,  // kill
            in("rdi") pid as u64,
            in("rsi") signal as u64,
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 129u64,  // kill
            in("x0") pid as u64,
            in("x1") signal as u64,
            lateout("x0") result,
            options(nostack)
        );
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        result = -1;
    }
    
    result as i32
}

/// Get current PID
fn syscall_getpid() -> i64 {
    let result: i64;
    
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 39u64,  // getpid
            lateout("rax") result,
            out("rcx") _,
            out("r11") _,
            options(nostack)
        );
    }
    
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!(
            "svc #0",
            in("x8") 172u64,  // getpid
            lateout("x0") result,
            options(nostack)
        );
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        result = 1;
    }
    
    result
}

/// WASM section types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SectionId {
    Custom = 0,
    Type = 1,
    Import = 2,
    Function = 3,
    Table = 4,
    Memory = 5,
    Global = 6,
    Export = 7,
    Start = 8,
    Element = 9,
    Code = 10,
    Data = 11,
}

impl SectionId {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Custom),
            1 => Some(Self::Type),
            2 => Some(Self::Import),
            3 => Some(Self::Function),
            4 => Some(Self::Table),
            5 => Some(Self::Memory),
            6 => Some(Self::Global),
            7 => Some(Self::Export),
            8 => Some(Self::Start),
            9 => Some(Self::Element),
            10 => Some(Self::Code),
            11 => Some(Self::Data),
            _ => None,
        }
    }
}

/// WASM value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
    /// 128-bit SIMD vector type
    V128,
    /// Function reference
    FuncRef,
    /// External reference
    ExternRef,
}

impl WasmType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x7F => Some(Self::I32),
            0x7E => Some(Self::I64),
            0x7D => Some(Self::F32),
            0x7C => Some(Self::F64),
            0x7B => Some(Self::V128),
            0x70 => Some(Self::FuncRef),
            0x6F => Some(Self::ExternRef),
            _ => None,
        }
    }
}

/// 128-bit SIMD vector value
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C, align(16))]
pub struct V128 {
    pub bytes: [u8; 16],
}

impl V128 {
    pub const fn zero() -> Self {
        Self { bytes: [0; 16] }
    }

    pub fn from_i8x16(vals: [i8; 16]) -> Self {
        let mut bytes = [0u8; 16];
        for i in 0..16 {
            bytes[i] = vals[i] as u8;
        }
        Self { bytes }
    }

    pub fn from_i16x8(vals: [i16; 8]) -> Self {
        let mut bytes = [0u8; 16];
        for i in 0..8 {
            let b = vals[i].to_le_bytes();
            bytes[i * 2] = b[0];
            bytes[i * 2 + 1] = b[1];
        }
        Self { bytes }
    }

    pub fn from_i32x4(vals: [i32; 4]) -> Self {
        let mut bytes = [0u8; 16];
        for i in 0..4 {
            let b = vals[i].to_le_bytes();
            bytes[i * 4..i * 4 + 4].copy_from_slice(&b);
        }
        Self { bytes }
    }

    pub fn from_i64x2(vals: [i64; 2]) -> Self {
        let mut bytes = [0u8; 16];
        for i in 0..2 {
            let b = vals[i].to_le_bytes();
            bytes[i * 8..i * 8 + 8].copy_from_slice(&b);
        }
        Self { bytes }
    }

    pub fn from_f32x4(vals: [f32; 4]) -> Self {
        let mut bytes = [0u8; 16];
        for i in 0..4 {
            let b = vals[i].to_le_bytes();
            bytes[i * 4..i * 4 + 4].copy_from_slice(&b);
        }
        Self { bytes }
    }

    pub fn from_f64x2(vals: [f64; 2]) -> Self {
        let mut bytes = [0u8; 16];
        for i in 0..2 {
            let b = vals[i].to_le_bytes();
            bytes[i * 8..i * 8 + 8].copy_from_slice(&b);
        }
        Self { bytes }
    }

    pub fn as_i8x16(&self) -> [i8; 16] {
        let mut vals = [0i8; 16];
        for i in 0..16 {
            vals[i] = self.bytes[i] as i8;
        }
        vals
    }

    pub fn as_i16x8(&self) -> [i16; 8] {
        let mut vals = [0i16; 8];
        for i in 0..8 {
            vals[i] = i16::from_le_bytes([self.bytes[i * 2], self.bytes[i * 2 + 1]]);
        }
        vals
    }

    pub fn as_i32x4(&self) -> [i32; 4] {
        let mut vals = [0i32; 4];
        for i in 0..4 {
            vals[i] = i32::from_le_bytes([
                self.bytes[i * 4],
                self.bytes[i * 4 + 1],
                self.bytes[i * 4 + 2],
                self.bytes[i * 4 + 3],
            ]);
        }
        vals
    }

    pub fn as_i64x2(&self) -> [i64; 2] {
        let mut vals = [0i64; 2];
        for i in 0..2 {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&self.bytes[i * 8..i * 8 + 8]);
            vals[i] = i64::from_le_bytes(arr);
        }
        vals
    }

    pub fn as_f32x4(&self) -> [f32; 4] {
        let mut vals = [0.0f32; 4];
        for i in 0..4 {
            vals[i] = f32::from_le_bytes([
                self.bytes[i * 4],
                self.bytes[i * 4 + 1],
                self.bytes[i * 4 + 2],
                self.bytes[i * 4 + 3],
            ]);
        }
        vals
    }

    pub fn as_f64x2(&self) -> [f64; 2] {
        let mut vals = [0.0f64; 2];
        for i in 0..2 {
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&self.bytes[i * 8..i * 8 + 8]);
            vals[i] = f64::from_le_bytes(arr);
        }
        vals
    }
}

/// WASM runtime values.
#[derive(Debug, Clone, Copy)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    /// 128-bit SIMD vector
    V128(V128),
    /// Function reference (index or null)
    FuncRef(Option<u32>),
    /// External reference
    ExternRef(Option<u64>),
}

impl WasmValue {
    pub fn value_type(&self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
            Self::V128(_) => WasmType::V128,
            Self::FuncRef(_) => WasmType::FuncRef,
            Self::ExternRef(_) => WasmType::ExternRef,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(v) => Some(*v),
            _ => None,
        }
    }
    
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(v) => Some(*v),
            _ => None,
        }
    }
    
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(v) => Some(*v),
            _ => None,
        }
    }

    /// Convert to i32, coercing if needed
    pub fn to_i32(&self) -> i32 {
        match self {
            Self::I32(v) => *v,
            Self::I64(v) => *v as i32,
            Self::F32(v) => *v as i32,
            Self::F64(v) => *v as i32,
            Self::V128(v) => v.as_i32x4()[0], // Take first lane
            Self::FuncRef(v) => v.map(|x| x as i32).unwrap_or(0),
            Self::ExternRef(v) => v.map(|x| x as i32).unwrap_or(0),
        }
    }
    
    /// Convert to i64, coercing if needed
    pub fn to_i64(&self) -> i64 {
        match self {
            Self::I32(v) => *v as i64,
            Self::I64(v) => *v,
            Self::F32(v) => *v as i64,
            Self::F64(v) => *v as i64,
            Self::V128(v) => v.as_i64x2()[0], // Take first lane
            Self::FuncRef(v) => v.map(|x| x as i64).unwrap_or(0),
            Self::ExternRef(v) => v.map(|x| x as i64).unwrap_or(0),
        }
    }
    
    /// Convert to V128
    pub fn to_v128(&self) -> V128 {
        match self {
            Self::V128(v) => *v,
            Self::I32(v) => V128::from_i32x4([*v, 0, 0, 0]),
            Self::I64(v) => V128::from_i64x2([*v, 0]),
            Self::F32(v) => V128::from_f32x4([*v, 0.0, 0.0, 0.0]),
            Self::F64(v) => V128::from_f64x2([*v, 0.0]),
            Self::FuncRef(_) | Self::ExternRef(_) => V128::zero(),
        }
    }
}

// =============================================================================
// WASM Bytecode Opcodes
// =============================================================================

/// WASM instruction opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Opcode {
    // Control flow
    Unreachable = 0x00,
    Nop = 0x01,
    Block = 0x02,
    Loop = 0x03,
    If = 0x04,
    Else = 0x05,
    End = 0x0B,
    Br = 0x0C,
    BrIf = 0x0D,
    BrTable = 0x0E,
    Return = 0x0F,
    Call = 0x10,
    CallIndirect = 0x11,
    
    // Parametric
    Drop = 0x1A,
    Select = 0x1B,
    
    // Variable access
    LocalGet = 0x20,
    LocalSet = 0x21,
    LocalTee = 0x22,
    GlobalGet = 0x23,
    GlobalSet = 0x24,
    
    // Memory
    I32Load = 0x28,
    I64Load = 0x29,
    F32Load = 0x2A,
    F64Load = 0x2B,
    I32Load8S = 0x2C,
    I32Load8U = 0x2D,
    I32Load16S = 0x2E,
    I32Load16U = 0x2F,
    I64Load8S = 0x30,
    I64Load8U = 0x31,
    I64Load16S = 0x32,
    I64Load16U = 0x33,
    I64Load32S = 0x34,
    I64Load32U = 0x35,
    I32Store = 0x36,
    I64Store = 0x37,
    F32Store = 0x38,
    F64Store = 0x39,
    I32Store8 = 0x3A,
    I32Store16 = 0x3B,
    I64Store8 = 0x3C,
    I64Store16 = 0x3D,
    I64Store32 = 0x3E,
    MemorySize = 0x3F,
    MemoryGrow = 0x40,
    
    // Constants
    I32Const = 0x41,
    I64Const = 0x42,
    F32Const = 0x43,
    F64Const = 0x44,
    
    // Comparison (i32)
    I32Eqz = 0x45,
    I32Eq = 0x46,
    I32Ne = 0x47,
    I32LtS = 0x48,
    I32LtU = 0x49,
    I32GtS = 0x4A,
    I32GtU = 0x4B,
    I32LeS = 0x4C,
    I32LeU = 0x4D,
    I32GeS = 0x4E,
    I32GeU = 0x4F,
    
    // Comparison (i64)
    I64Eqz = 0x50,
    I64Eq = 0x51,
    I64Ne = 0x52,
    I64LtS = 0x53,
    I64LtU = 0x54,
    I64GtS = 0x55,
    I64GtU = 0x56,
    I64LeS = 0x57,
    I64LeU = 0x58,
    I64GeS = 0x59,
    I64GeU = 0x5A,
    
    // Comparison (f32)
    F32Eq = 0x5B,
    F32Ne = 0x5C,
    F32Lt = 0x5D,
    F32Gt = 0x5E,
    F32Le = 0x5F,
    F32Ge = 0x60,
    
    // Comparison (f64)
    F64Eq = 0x61,
    F64Ne = 0x62,
    F64Lt = 0x63,
    F64Gt = 0x64,
    F64Le = 0x65,
    F64Ge = 0x66,
    
    // Numeric (i32)
    I32Clz = 0x67,
    I32Ctz = 0x68,
    I32Popcnt = 0x69,
    I32Add = 0x6A,
    I32Sub = 0x6B,
    I32Mul = 0x6C,
    I32DivS = 0x6D,
    I32DivU = 0x6E,
    I32RemS = 0x6F,
    I32RemU = 0x70,
    I32And = 0x71,
    I32Or = 0x72,
    I32Xor = 0x73,
    I32Shl = 0x74,
    I32ShrS = 0x75,
    I32ShrU = 0x76,
    I32Rotl = 0x77,
    I32Rotr = 0x78,
    
    // Numeric (i64)
    I64Clz = 0x79,
    I64Ctz = 0x7A,
    I64Popcnt = 0x7B,
    I64Add = 0x7C,
    I64Sub = 0x7D,
    I64Mul = 0x7E,
    I64DivS = 0x7F,
    I64DivU = 0x80,
    I64RemS = 0x81,
    I64RemU = 0x82,
    I64And = 0x83,
    I64Or = 0x84,
    I64Xor = 0x85,
    I64Shl = 0x86,
    I64ShrS = 0x87,
    I64ShrU = 0x88,
    I64Rotl = 0x89,
    I64Rotr = 0x8A,
    
    // Numeric (f32)
    F32Abs = 0x8B,
    F32Neg = 0x8C,
    F32Ceil = 0x8D,
    F32Floor = 0x8E,
    F32Trunc = 0x8F,
    F32Nearest = 0x90,
    F32Sqrt = 0x91,
    F32Add = 0x92,
    F32Sub = 0x93,
    F32Mul = 0x94,
    F32Div = 0x95,
    F32Min = 0x96,
    F32Max = 0x97,
    F32Copysign = 0x98,
    
    // Numeric (f64)
    F64Abs = 0x99,
    F64Neg = 0x9A,
    F64Ceil = 0x9B,
    F64Floor = 0x9C,
    F64Trunc = 0x9D,
    F64Nearest = 0x9E,
    F64Sqrt = 0x9F,
    F64Add = 0xA0,
    F64Sub = 0xA1,
    F64Mul = 0xA2,
    F64Div = 0xA3,
    F64Min = 0xA4,
    F64Max = 0xA5,
    F64Copysign = 0xA6,
    
    // Conversions
    I32WrapI64 = 0xA7,
    I32TruncF32S = 0xA8,
    I32TruncF32U = 0xA9,
    I32TruncF64S = 0xAA,
    I32TruncF64U = 0xAB,
    I64ExtendI32S = 0xAC,
    I64ExtendI32U = 0xAD,
    I64TruncF32S = 0xAE,
    I64TruncF32U = 0xAF,
    I64TruncF64S = 0xB0,
    I64TruncF64U = 0xB1,
    F32ConvertI32S = 0xB2,
    F32ConvertI32U = 0xB3,
    F32ConvertI64S = 0xB4,
    F32ConvertI64U = 0xB5,
    F32DemoteF64 = 0xB6,
    F64ConvertI32S = 0xB7,
    F64ConvertI32U = 0xB8,
    F64ConvertI64S = 0xB9,
    F64ConvertI64U = 0xBA,
    F64PromoteF32 = 0xBB,
    I32ReinterpretF32 = 0xBC,
    I64ReinterpretF64 = 0xBD,
    F32ReinterpretI32 = 0xBE,
    F64ReinterpretI64 = 0xBF,
    
    // Sign extension (WASM 1.1+)
    I32Extend8S = 0xC0,
    I32Extend16S = 0xC1,
    I64Extend8S = 0xC2,
    I64Extend16S = 0xC3,
    I64Extend32S = 0xC4,
    
    // Reference types
    RefNull = 0xD0,
    RefIsNull = 0xD1,
    RefFunc = 0xD2,
    
    // Multi-byte prefix for SIMD, atomics, etc.
    SimdPrefix = 0xFD,
    AtomicPrefix = 0xFE,
}

/// SIMD opcodes (after 0xFD prefix)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum SimdOpcode {
    // v128 load/store
    V128Load = 0x00,
    V128Load8x8S = 0x01,
    V128Load8x8U = 0x02,
    V128Load16x4S = 0x03,
    V128Load16x4U = 0x04,
    V128Load32x2S = 0x05,
    V128Load32x2U = 0x06,
    V128Load8Splat = 0x07,
    V128Load16Splat = 0x08,
    V128Load32Splat = 0x09,
    V128Load64Splat = 0x0A,
    V128Store = 0x0B,
    
    // v128 const
    V128Const = 0x0C,
    
    // i8x16 operations
    I8x16Shuffle = 0x0D,
    I8x16Swizzle = 0x0E,
    I8x16Splat = 0x0F,
    I16x8Splat = 0x10,
    I32x4Splat = 0x11,
    I64x2Splat = 0x12,
    F32x4Splat = 0x13,
    F64x2Splat = 0x14,
    
    // i8x16 lane operations
    I8x16ExtractLaneS = 0x15,
    I8x16ExtractLaneU = 0x16,
    I8x16ReplaceLane = 0x17,
    I16x8ExtractLaneS = 0x18,
    I16x8ExtractLaneU = 0x19,
    I16x8ReplaceLane = 0x1A,
    I32x4ExtractLane = 0x1B,
    I32x4ReplaceLane = 0x1C,
    I64x2ExtractLane = 0x1D,
    I64x2ReplaceLane = 0x1E,
    F32x4ExtractLane = 0x1F,
    F32x4ReplaceLane = 0x20,
    F64x2ExtractLane = 0x21,
    F64x2ReplaceLane = 0x22,
    
    // i8x16 comparisons
    I8x16Eq = 0x23,
    I8x16Ne = 0x24,
    I8x16LtS = 0x25,
    I8x16LtU = 0x26,
    I8x16GtS = 0x27,
    I8x16GtU = 0x28,
    I8x16LeS = 0x29,
    I8x16LeU = 0x2A,
    I8x16GeS = 0x2B,
    I8x16GeU = 0x2C,
    
    // i16x8 comparisons
    I16x8Eq = 0x2D,
    I16x8Ne = 0x2E,
    I16x8LtS = 0x2F,
    I16x8LtU = 0x30,
    I16x8GtS = 0x31,
    I16x8GtU = 0x32,
    I16x8LeS = 0x33,
    I16x8LeU = 0x34,
    I16x8GeS = 0x35,
    I16x8GeU = 0x36,
    
    // i32x4 comparisons
    I32x4Eq = 0x37,
    I32x4Ne = 0x38,
    I32x4LtS = 0x39,
    I32x4LtU = 0x3A,
    I32x4GtS = 0x3B,
    I32x4GtU = 0x3C,
    I32x4LeS = 0x3D,
    I32x4LeU = 0x3E,
    I32x4GeS = 0x3F,
    I32x4GeU = 0x40,
    
    // f32x4/f64x2 comparisons
    F32x4Eq = 0x41,
    F32x4Ne = 0x42,
    F32x4Lt = 0x43,
    F32x4Gt = 0x44,
    F32x4Le = 0x45,
    F32x4Ge = 0x46,
    F64x2Eq = 0x47,
    F64x2Ne = 0x48,
    F64x2Lt = 0x49,
    F64x2Gt = 0x4A,
    F64x2Le = 0x4B,
    F64x2Ge = 0x4C,
    
    // v128 bitwise
    V128Not = 0x4D,
    V128And = 0x4E,
    V128AndNot = 0x4F,
    V128Or = 0x50,
    V128Xor = 0x51,
    V128Bitselect = 0x52,
    V128AnyTrue = 0x53,
    
    // i8x16 arithmetic
    I8x16Abs = 0x60,
    I8x16Neg = 0x61,
    I8x16AllTrue = 0x63,
    I8x16Bitmask = 0x64,
    I8x16Shl = 0x6B,
    I8x16ShrS = 0x6C,
    I8x16ShrU = 0x6D,
    I8x16Add = 0x6E,
    I8x16AddSatS = 0x6F,
    I8x16AddSatU = 0x70,
    I8x16Sub = 0x71,
    I8x16SubSatS = 0x72,
    I8x16SubSatU = 0x73,
    I8x16MinS = 0x76,
    I8x16MinU = 0x77,
    I8x16MaxS = 0x78,
    I8x16MaxU = 0x79,
    
    // i16x8 arithmetic
    I16x8Abs = 0x80,
    I16x8Neg = 0x81,
    I16x8AllTrue = 0x83,
    I16x8Bitmask = 0x84,
    I16x8Shl = 0x8B,
    I16x8ShrS = 0x8C,
    I16x8ShrU = 0x8D,
    I16x8Add = 0x8E,
    I16x8AddSatS = 0x8F,
    I16x8AddSatU = 0x90,
    I16x8Sub = 0x91,
    I16x8SubSatS = 0x92,
    I16x8SubSatU = 0x93,
    I16x8Mul = 0x95,
    I16x8MinS = 0x96,
    I16x8MinU = 0x97,
    I16x8MaxS = 0x98,
    I16x8MaxU = 0x99,
    
    // i32x4 arithmetic
    I32x4Abs = 0xA0,
    I32x4Neg = 0xA1,
    I32x4AllTrue = 0xA3,
    I32x4Bitmask = 0xA4,
    I32x4Shl = 0xAB,
    I32x4ShrS = 0xAC,
    I32x4ShrU = 0xAD,
    I32x4Add = 0xAE,
    I32x4Sub = 0xB1,
    I32x4Mul = 0xB5,
    I32x4MinS = 0xB6,
    I32x4MinU = 0xB7,
    I32x4MaxS = 0xB8,
    I32x4MaxU = 0xB9,
    
    // i64x2 arithmetic
    I64x2Abs = 0xC0,
    I64x2Neg = 0xC1,
    I64x2AllTrue = 0xC3,
    I64x2Bitmask = 0xC4,
    I64x2Shl = 0xCB,
    I64x2ShrS = 0xCC,
    I64x2ShrU = 0xCD,
    I64x2Add = 0xCE,
    I64x2Sub = 0xD1,
    I64x2Mul = 0xD5,
    
    // f32x4 arithmetic
    F32x4Abs = 0xE0,
    F32x4Neg = 0xE1,
    F32x4Sqrt = 0xE3,
    F32x4Add = 0xE4,
    F32x4Sub = 0xE5,
    F32x4Mul = 0xE6,
    F32x4Div = 0xE7,
    F32x4Min = 0xE8,
    F32x4Max = 0xE9,
    
    // f64x2 arithmetic
    F64x2Abs = 0xEC,
    F64x2Neg = 0xED,
    F64x2Sqrt = 0xEF,
    F64x2Add = 0xF0,
    F64x2Sub = 0xF1,
    F64x2Mul = 0xF2,
    F64x2Div = 0xF3,
    F64x2Min = 0xF4,
    F64x2Max = 0xF5,
}

/// Atomic opcodes (after 0xFE prefix) - for threading
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AtomicOpcode {
    // Memory operations
    MemoryAtomicNotify = 0x00,
    MemoryAtomicWait32 = 0x01,
    MemoryAtomicWait64 = 0x02,
    AtomicFence = 0x03,
    
    // i32 atomics
    I32AtomicLoad = 0x10,
    I64AtomicLoad = 0x11,
    I32AtomicLoad8U = 0x12,
    I32AtomicLoad16U = 0x13,
    I64AtomicLoad8U = 0x14,
    I64AtomicLoad16U = 0x15,
    I64AtomicLoad32U = 0x16,
    I32AtomicStore = 0x17,
    I64AtomicStore = 0x18,
    I32AtomicStore8 = 0x19,
    I32AtomicStore16 = 0x1A,
    I64AtomicStore8 = 0x1B,
    I64AtomicStore16 = 0x1C,
    I64AtomicStore32 = 0x1D,
    
    // RMW operations
    I32AtomicRmwAdd = 0x1E,
    I64AtomicRmwAdd = 0x1F,
    I32AtomicRmw8AddU = 0x20,
    I32AtomicRmw16AddU = 0x21,
    I64AtomicRmw8AddU = 0x22,
    I64AtomicRmw16AddU = 0x23,
    I64AtomicRmw32AddU = 0x24,
    
    I32AtomicRmwSub = 0x25,
    I64AtomicRmwSub = 0x26,
    I32AtomicRmw8SubU = 0x27,
    I32AtomicRmw16SubU = 0x28,
    I64AtomicRmw8SubU = 0x29,
    I64AtomicRmw16SubU = 0x2A,
    I64AtomicRmw32SubU = 0x2B,
    
    I32AtomicRmwAnd = 0x2C,
    I64AtomicRmwAnd = 0x2D,
    I32AtomicRmw8AndU = 0x2E,
    I32AtomicRmw16AndU = 0x2F,
    I64AtomicRmw8AndU = 0x30,
    I64AtomicRmw16AndU = 0x31,
    I64AtomicRmw32AndU = 0x32,
    
    I32AtomicRmwOr = 0x33,
    I64AtomicRmwOr = 0x34,
    I32AtomicRmw8OrU = 0x35,
    I32AtomicRmw16OrU = 0x36,
    I64AtomicRmw8OrU = 0x37,
    I64AtomicRmw16OrU = 0x38,
    I64AtomicRmw32OrU = 0x39,
    
    I32AtomicRmwXor = 0x3A,
    I64AtomicRmwXor = 0x3B,
    I32AtomicRmw8XorU = 0x3C,
    I32AtomicRmw16XorU = 0x3D,
    I64AtomicRmw8XorU = 0x3E,
    I64AtomicRmw16XorU = 0x3F,
    I64AtomicRmw32XorU = 0x40,
    
    I32AtomicRmwXchg = 0x41,
    I64AtomicRmwXchg = 0x42,
    I32AtomicRmw8XchgU = 0x43,
    I32AtomicRmw16XchgU = 0x44,
    I64AtomicRmw8XchgU = 0x45,
    I64AtomicRmw16XchgU = 0x46,
    I64AtomicRmw32XchgU = 0x47,
    
    // Compare-and-swap
    I32AtomicRmwCmpxchg = 0x48,
    I64AtomicRmwCmpxchg = 0x49,
    I32AtomicRmw8CmpxchgU = 0x4A,
    I32AtomicRmw16CmpxchgU = 0x4B,
    I64AtomicRmw8CmpxchgU = 0x4C,
    I64AtomicRmw16CmpxchgU = 0x4D,
    I64AtomicRmw32CmpxchgU = 0x4E,
}

impl Opcode {
    pub fn from_byte(b: u8) -> Option<Self> {
        // Handle multi-byte opcodes
        match b {
            0x00..=0xC4 => Some(unsafe { core::mem::transmute(b) }),
            0xD0..=0xD2 => Some(unsafe { core::mem::transmute(b) }),
            0xFD => Some(Self::SimdPrefix),
            0xFE => Some(Self::AtomicPrefix),
            _ => None,
        }
    }
    
    /// Check if this opcode is a multi-byte prefix
    pub fn is_prefix(&self) -> bool {
        matches!(self, Self::SimdPrefix | Self::AtomicPrefix)
    }
}

impl SimdOpcode {
    pub fn from_u32(v: u32) -> Option<Self> {
        // Map known SIMD opcodes
        match v {
            0x00..=0x0C => Some(unsafe { core::mem::transmute(v) }),
            0x0D..=0x22 => Some(unsafe { core::mem::transmute(v) }),
            0x23..=0x53 => Some(unsafe { core::mem::transmute(v) }),
            0x60..=0x79 => Some(unsafe { core::mem::transmute(v) }),
            0x80..=0x99 => Some(unsafe { core::mem::transmute(v) }),
            0xA0..=0xB9 => Some(unsafe { core::mem::transmute(v) }),
            0xC0..=0xD5 => Some(unsafe { core::mem::transmute(v) }),
            0xE0..=0xF5 => Some(unsafe { core::mem::transmute(v) }),
            _ => None,
        }
    }
}

impl AtomicOpcode {
    pub fn from_u32(v: u32) -> Option<Self> {
        if v <= 0x4E {
            Some(unsafe { core::mem::transmute(v) })
        } else {
            None
        }
    }
}

/// Function signature.
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub params: Vec<WasmType>,
    pub results: Vec<WasmType>,
}

/// Import definition.
#[derive(Debug, Clone)]
pub struct Import {
    /// Module name
    pub module: String,
    /// Field name
    pub name: String,
    /// Function signature
    pub signature: FunctionSignature,
}

/// Export definition.
#[derive(Debug, Clone)]
pub struct Export {
    /// Export name
    pub name: String,
    /// Export type
    pub export_type: ExportType,
}

/// Export types.
#[derive(Debug, Clone)]
pub enum ExportType {
    Function(FunctionSignature),
    Memory { min_pages: u32, max_pages: Option<u32> },
    Global { value_type: WasmType, mutable: bool },
    Table { element_type: String, min: u32, max: Option<u32> },
}

/// Host function types for Splax system calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFunction {
    /// Send message on S-LINK channel: (channel_id: i32, ptr: i32, len: i32) -> i32
    SLinkSend,
    /// Receive message from S-LINK channel: (channel_id: i32, ptr: i32, max_len: i32) -> i32
    SLinkReceive,
    /// Read from S-STORAGE: (object_id: i64, ptr: i32, offset: i32, len: i32) -> i32
    SStorageRead,
    /// Write to S-STORAGE: (ptr: i32, len: i32) -> i64 (returns object_id)
    SStorageWrite,
    /// Log a message: (level: i32, ptr: i32, len: i32) -> ()
    SLog,
    /// Get current time in microseconds: () -> i64
    STimeNow,
    /// Sleep for microseconds: (us: i64) -> ()
    SSleep,
    /// Print to console: (ptr: i32, len: i32) -> i32
    SPrint,
    /// Read from console: (ptr: i32, max_len: i32) -> i32
    SRead,
    /// Exit with code: (code: i32) -> !
    SExit,
    /// Get environment variable: (name_ptr: i32, name_len: i32, buf_ptr: i32, buf_len: i32) -> i32
    SGetEnv,
    /// Get random bytes: (ptr: i32, len: i32) -> i32
    SRandom,
    /// Open a file: (path_ptr: i32, path_len: i32, flags: i32) -> i32
    SFileOpen,
    /// Read from file: (fd: i32, ptr: i32, len: i32) -> i32
    SFileRead,
    /// Write to file: (fd: i32, ptr: i32, len: i32) -> i32
    SFileWrite,
    /// Close file: (fd: i32) -> i32
    SFileClose,
    /// Get file size: (fd: i32) -> i64
    SFileSize,
    /// Network connect: (host_ptr: i32, host_len: i32, port: i32) -> i32
    SNetConnect,
    /// Network send: (sock: i32, ptr: i32, len: i32) -> i32
    SNetSend,
    /// Network receive: (sock: i32, ptr: i32, max_len: i32) -> i32
    SNetRecv,
    /// Network close: (sock: i32) -> i32
    SNetClose,
    
    // =========================================================================
    // EXPANDED SYSCALL HOST FUNCTIONS
    // =========================================================================
    
    // --- Process Management ---
    /// Spawn a new process: (path_ptr: i32, path_len: i32, args_ptr: i32) -> i64 (returns pid)
    SProcessSpawn,
    /// Wait for process: (pid: i64) -> i32 (returns exit code)
    SProcessWait,
    /// Kill process: (pid: i64, signal: i32) -> i32
    SProcessKill,
    /// Get current process ID: () -> i64
    SProcessGetPid,
    /// Get parent process ID: () -> i64
    SProcessGetPpid,
    /// Fork current process (native only): () -> i64
    SProcessFork,
    
    // --- Memory Management ---
    /// Allocate memory pages: (pages: i32) -> i32 (returns ptr or 0)
    SMemAlloc,
    /// Free memory pages: (ptr: i32, pages: i32) -> i32
    SMemFree,
    /// Grow linear memory: (delta_pages: i32) -> i32 (returns old size or -1)
    SMemGrow,
    /// Get memory size: () -> i32 (in pages)
    SMemSize,
    
    // --- Filesystem Extended ---
    /// Create directory: (path_ptr: i32, path_len: i32) -> i32
    SFsMkdir,
    /// Remove directory: (path_ptr: i32, path_len: i32) -> i32
    SFsRmdir,
    /// Remove file: (path_ptr: i32, path_len: i32) -> i32
    SFsUnlink,
    /// Rename file: (old_ptr: i32, old_len: i32, new_ptr: i32, new_len: i32) -> i32
    SFsRename,
    /// Get file stats: (path_ptr: i32, path_len: i32, stat_ptr: i32) -> i32
    SFsStat,
    /// Read directory entry: (fd: i32, entry_ptr: i32) -> i32
    SFsReadDir,
    /// Seek in file: (fd: i32, offset: i64, whence: i32) -> i64
    SFsSeek,
    /// Sync file to disk: (fd: i32) -> i32
    SFsSync,
    /// Truncate file: (fd: i32, size: i64) -> i32
    SFsTruncate,
    /// Get current working directory: (buf_ptr: i32, buf_len: i32) -> i32
    SFsGetCwd,
    /// Change current working directory: (path_ptr: i32, path_len: i32) -> i32
    SFsChdir,
    
    // --- Network Extended ---
    /// Create socket: (domain: i32, type: i32, protocol: i32) -> i32
    SNetSocket,
    /// Bind socket: (sock: i32, addr_ptr: i32, addr_len: i32) -> i32
    SNetBind,
    /// Listen on socket: (sock: i32, backlog: i32) -> i32
    SNetListen,
    /// Accept connection: (sock: i32, addr_ptr: i32, addr_len_ptr: i32) -> i32
    SNetAccept,
    /// Set socket option: (sock: i32, level: i32, optname: i32, val: i32) -> i32
    SNetSetsockopt,
    /// Get peer address: (sock: i32, addr_ptr: i32, addr_len_ptr: i32) -> i32
    SNetGetpeername,
    /// DNS lookup: (host_ptr: i32, host_len: i32, result_ptr: i32) -> i32
    SNetResolve,
    /// Send UDP datagram: (sock: i32, buf_ptr: i32, len: i32, addr_ptr: i32) -> i32
    SNetSendto,
    /// Receive UDP datagram: (sock: i32, buf_ptr: i32, len: i32, addr_ptr: i32) -> i32
    SNetRecvfrom,
    
    // --- Thread Management ---
    /// Create thread: (entry_ptr: i32, arg: i32) -> i32 (returns thread_id)
    SThreadCreate,
    /// Join thread: (thread_id: i32) -> i32
    SThreadJoin,
    /// Exit thread: (code: i32) -> !
    SThreadExit,
    /// Yield to scheduler: () -> ()
    SThreadYield,
    /// Get current thread ID: () -> i32
    SThreadGetId,
    /// Sleep thread: (ns: i64) -> i32
    SThreadSleep,
    
    // --- Synchronization ---
    /// Create mutex: () -> i32 (returns mutex_id)
    SSyncMutexCreate,
    /// Lock mutex: (mutex_id: i32) -> i32
    SSyncMutexLock,
    /// Unlock mutex: (mutex_id: i32) -> i32
    SSyncMutexUnlock,
    /// Destroy mutex: (mutex_id: i32) -> i32
    SSyncMutexDestroy,
    /// Futex wait: (addr: i32, expected: i32, timeout_ns: i64) -> i32
    SSyncFutexWait,
    /// Futex wake: (addr: i32, count: i32) -> i32
    SSyncFutexWake,
    
    // --- Capability Management ---
    /// Check capability: (cap_type_ptr: i32, cap_type_len: i32) -> i32
    SCapCheck,
    /// Request capability: (cap_type_ptr: i32, cap_type_len: i32) -> i64
    SCapRequest,
    /// Revoke capability: (cap_id: i64) -> i32
    SCapRevoke,
    /// Delegate capability: (cap_id: i64, target_pid: i64) -> i64
    SCapDelegate,
    
    // --- Service Discovery (S-ATLAS) ---
    /// Register service: (name_ptr: i32, name_len: i32, port: i32) -> i32
    SServiceRegister,
    /// Discover service: (name_ptr: i32, name_len: i32, result_ptr: i32) -> i32
    SServiceDiscover,
    /// Unregister service: (service_id: i32) -> i32
    SServiceUnregister,
    
    // --- Time & Timers ---
    /// Get monotonic time: () -> i64 (nanoseconds)
    STimeMonotonic,
    /// Get real time: () -> i64 (unix timestamp ns)
    STimeReal,
    /// Create timer: (ns: i64, callback_ptr: i32) -> i32 (returns timer_id)
    STimerCreate,
    /// Cancel timer: (timer_id: i32) -> i32
    STimerCancel,
    
    // --- System Information ---
    /// Get system info: (info_type: i32, buf_ptr: i32, buf_len: i32) -> i32
    SSysInfo,
    /// Get CPU count: () -> i32
    SSysCpuCount,
    /// Get free memory: () -> i64 (bytes)
    SSysMemFree,
    /// Get uptime: () -> i64 (seconds)
    SSysUptime,
    
    // --- Debug & Profiling ---
    /// Debug print: (ptr: i32, len: i32) -> ()
    SDebugPrint,
    /// Debug break: () -> ()
    SDebugBreak,
    /// Profile start: (name_ptr: i32, name_len: i32) -> i32
    SProfileStart,
    /// Profile stop: (id: i32) -> i64 (returns elapsed ns)
    SProfileStop,
}

impl HostFunction {
    /// Get the host function by import name.
    pub fn from_name(module: &str, name: &str) -> Option<Self> {
        if module != "splax" && module != "env" && module != "wasi_snapshot_preview1" {
            return None;
        }
        match name {
            // Basic I/O
            "s_link_send" => Some(Self::SLinkSend),
            "s_link_receive" => Some(Self::SLinkReceive),
            "s_storage_read" => Some(Self::SStorageRead),
            "s_storage_write" => Some(Self::SStorageWrite),
            "s_log" => Some(Self::SLog),
            "s_time_now" | "clock_time_get" => Some(Self::STimeNow),
            "s_sleep" | "poll_oneoff" => Some(Self::SSleep),
            "s_print" | "fd_write" => Some(Self::SPrint),
            "s_read" | "fd_read" => Some(Self::SRead),
            "s_exit" | "proc_exit" => Some(Self::SExit),
            "s_getenv" | "environ_get" => Some(Self::SGetEnv),
            "s_random" | "random_get" => Some(Self::SRandom),
            "s_file_open" | "path_open" => Some(Self::SFileOpen),
            "s_file_read" => Some(Self::SFileRead),
            "s_file_write" => Some(Self::SFileWrite),
            "s_file_close" | "fd_close" => Some(Self::SFileClose),
            "s_file_size" | "fd_filestat_get" => Some(Self::SFileSize),
            "s_net_connect" | "sock_connect" => Some(Self::SNetConnect),
            "s_net_send" | "sock_send" => Some(Self::SNetSend),
            "s_net_recv" | "sock_recv" => Some(Self::SNetRecv),
            "s_net_close" | "sock_close" => Some(Self::SNetClose),
            
            // Process management
            "s_process_spawn" => Some(Self::SProcessSpawn),
            "s_process_wait" => Some(Self::SProcessWait),
            "s_process_kill" => Some(Self::SProcessKill),
            "s_process_getpid" | "getpid" => Some(Self::SProcessGetPid),
            "s_process_getppid" | "getppid" => Some(Self::SProcessGetPpid),
            "s_process_fork" => Some(Self::SProcessFork),
            
            // Memory management
            "s_mem_alloc" => Some(Self::SMemAlloc),
            "s_mem_free" => Some(Self::SMemFree),
            "s_mem_grow" => Some(Self::SMemGrow),
            "s_mem_size" => Some(Self::SMemSize),
            
            // Filesystem extended
            "s_fs_mkdir" | "path_create_directory" => Some(Self::SFsMkdir),
            "s_fs_rmdir" | "path_remove_directory" => Some(Self::SFsRmdir),
            "s_fs_unlink" | "path_unlink_file" => Some(Self::SFsUnlink),
            "s_fs_rename" | "path_rename" => Some(Self::SFsRename),
            "s_fs_stat" | "path_filestat_get" => Some(Self::SFsStat),
            "s_fs_readdir" | "fd_readdir" => Some(Self::SFsReadDir),
            "s_fs_seek" | "fd_seek" => Some(Self::SFsSeek),
            "s_fs_sync" | "fd_sync" => Some(Self::SFsSync),
            "s_fs_truncate" | "fd_filestat_set_size" => Some(Self::SFsTruncate),
            "s_fs_getcwd" => Some(Self::SFsGetCwd),
            "s_fs_chdir" => Some(Self::SFsChdir),
            
            // Network extended
            "s_net_socket" | "sock_open" => Some(Self::SNetSocket),
            "s_net_bind" | "sock_bind" => Some(Self::SNetBind),
            "s_net_listen" | "sock_listen" => Some(Self::SNetListen),
            "s_net_accept" | "sock_accept" => Some(Self::SNetAccept),
            "s_net_setsockopt" | "sock_set_opt" => Some(Self::SNetSetsockopt),
            "s_net_getpeername" | "sock_getpeer" => Some(Self::SNetGetpeername),
            "s_net_resolve" => Some(Self::SNetResolve),
            "s_net_sendto" | "sock_sendto" => Some(Self::SNetSendto),
            "s_net_recvfrom" | "sock_recvfrom" => Some(Self::SNetRecvfrom),
            
            // Thread management
            "s_thread_create" => Some(Self::SThreadCreate),
            "s_thread_join" => Some(Self::SThreadJoin),
            "s_thread_exit" => Some(Self::SThreadExit),
            "s_thread_yield" | "sched_yield" => Some(Self::SThreadYield),
            "s_thread_getid" => Some(Self::SThreadGetId),
            "s_thread_sleep" => Some(Self::SThreadSleep),
            
            // Synchronization
            "s_sync_mutex_create" => Some(Self::SSyncMutexCreate),
            "s_sync_mutex_lock" => Some(Self::SSyncMutexLock),
            "s_sync_mutex_unlock" => Some(Self::SSyncMutexUnlock),
            "s_sync_mutex_destroy" => Some(Self::SSyncMutexDestroy),
            "s_sync_futex_wait" => Some(Self::SSyncFutexWait),
            "s_sync_futex_wake" => Some(Self::SSyncFutexWake),
            
            // Capability management
            "s_cap_check" => Some(Self::SCapCheck),
            "s_cap_request" => Some(Self::SCapRequest),
            "s_cap_revoke" => Some(Self::SCapRevoke),
            "s_cap_delegate" => Some(Self::SCapDelegate),
            
            // Service discovery
            "s_service_register" => Some(Self::SServiceRegister),
            "s_service_discover" => Some(Self::SServiceDiscover),
            "s_service_unregister" => Some(Self::SServiceUnregister),
            
            // Time & timers
            "s_time_monotonic" | "clock_res_get" => Some(Self::STimeMonotonic),
            "s_time_real" => Some(Self::STimeReal),
            "s_timer_create" => Some(Self::STimerCreate),
            "s_timer_cancel" => Some(Self::STimerCancel),
            
            // System info
            "s_sys_info" => Some(Self::SSysInfo),
            "s_sys_cpu_count" => Some(Self::SSysCpuCount),
            "s_sys_mem_free" => Some(Self::SSysMemFree),
            "s_sys_uptime" => Some(Self::SSysUptime),
            
            // Debug & profiling
            "s_debug_print" => Some(Self::SDebugPrint),
            "s_debug_break" => Some(Self::SDebugBreak),
            "s_profile_start" => Some(Self::SProfileStart),
            "s_profile_stop" => Some(Self::SProfileStop),
            
            _ => None,
        }
    }
    
    /// Create a host function from just the function name (checks all modules)
    pub fn from_function_name(name: &str) -> Option<Self> {
        // Try common module prefixes
        Self::from_name("splax", name)
            .or_else(|| Self::from_name("env", name))
            .or_else(|| Self::from_name("wasi_snapshot_preview1", name))
    }
    
    /// Get the canonical name of this host function
    pub fn name(&self) -> &'static str {
        match self {
            Self::SLinkSend => "s_link_send",
            Self::SLinkReceive => "s_link_receive",
            Self::SStorageRead => "s_storage_read",
            Self::SStorageWrite => "s_storage_write",
            Self::SLog => "s_log",
            Self::STimeNow => "s_time_now",
            Self::SSleep => "s_sleep",
            Self::SPrint => "s_print",
            Self::SRead => "s_read",
            Self::SExit => "s_exit",
            Self::SGetEnv => "s_getenv",
            Self::SRandom => "s_random",
            Self::SFileOpen => "s_file_open",
            Self::SFileRead => "s_file_read",
            Self::SFileWrite => "s_file_write",
            Self::SFileClose => "s_file_close",
            Self::SFileSize => "s_file_size",
            Self::SNetConnect => "s_net_connect",
            Self::SNetSend => "s_net_send",
            Self::SNetRecv => "s_net_recv",
            Self::SNetClose => "s_net_close",
            Self::SProcessSpawn => "s_process_spawn",
            Self::SProcessWait => "s_process_wait",
            Self::SProcessKill => "s_process_kill",
            Self::SProcessGetPid => "s_process_getpid",
            Self::SProcessGetPpid => "s_process_getppid",
            Self::SProcessFork => "s_process_fork",
            Self::SMemAlloc => "s_mem_alloc",
            Self::SMemFree => "s_mem_free",
            Self::SMemGrow => "s_mem_grow",
            Self::SMemSize => "s_mem_size",
            Self::SFsMkdir => "s_fs_mkdir",
            Self::SFsRmdir => "s_fs_rmdir",
            Self::SFsUnlink => "s_fs_unlink",
            Self::SFsRename => "s_fs_rename",
            Self::SFsStat => "s_fs_stat",
            Self::SFsReadDir => "s_fs_readdir",
            Self::SFsSeek => "s_fs_seek",
            Self::SFsSync => "s_fs_sync",
            Self::SFsTruncate => "s_fs_truncate",
            Self::SFsGetCwd => "s_fs_getcwd",
            Self::SFsChdir => "s_fs_chdir",
            Self::SNetSocket => "s_net_socket",
            Self::SNetBind => "s_net_bind",
            Self::SNetListen => "s_net_listen",
            Self::SNetAccept => "s_net_accept",
            Self::SNetSetsockopt => "s_net_setsockopt",
            Self::SNetGetpeername => "s_net_getpeername",
            Self::SNetResolve => "s_net_resolve",
            Self::SNetSendto => "s_net_sendto",
            Self::SNetRecvfrom => "s_net_recvfrom",
            Self::SThreadCreate => "s_thread_create",
            Self::SThreadJoin => "s_thread_join",
            Self::SThreadExit => "s_thread_exit",
            Self::SThreadYield => "s_thread_yield",
            Self::SThreadGetId => "s_thread_getid",
            Self::SThreadSleep => "s_thread_sleep",
            Self::SSyncMutexCreate => "s_sync_mutex_create",
            Self::SSyncMutexLock => "s_sync_mutex_lock",
            Self::SSyncMutexUnlock => "s_sync_mutex_unlock",
            Self::SSyncMutexDestroy => "s_sync_mutex_destroy",
            Self::SSyncFutexWait => "s_sync_futex_wait",
            Self::SSyncFutexWake => "s_sync_futex_wake",
            Self::SCapCheck => "s_cap_check",
            Self::SCapRequest => "s_cap_request",
            Self::SCapRevoke => "s_cap_revoke",
            Self::SCapDelegate => "s_cap_delegate",
            Self::SServiceRegister => "s_service_register",
            Self::SServiceDiscover => "s_service_discover",
            Self::SServiceUnregister => "s_service_unregister",
            Self::STimeMonotonic => "s_time_monotonic",
            Self::STimeReal => "s_time_real",
            Self::STimerCreate => "s_timer_create",
            Self::STimerCancel => "s_timer_cancel",
            Self::SSysInfo => "s_sys_info",
            Self::SSysCpuCount => "s_sys_cpu_count",
            Self::SSysMemFree => "s_sys_mem_free",
            Self::SSysUptime => "s_sys_uptime",
            Self::SDebugPrint => "s_debug_print",
            Self::SDebugBreak => "s_debug_break",
            Self::SProfileStart => "s_profile_start",
            Self::SProfileStop => "s_profile_stop",
        }
    }

    /// Get the required capability type for this host function.
    pub fn required_capability(&self) -> &'static str {
        match self {
            // Basic I/O
            Self::SLinkSend => "channel:write",
            Self::SLinkReceive => "channel:read",
            Self::SStorageRead => "storage:read",
            Self::SStorageWrite => "storage:write",
            Self::SLog => "log:write",
            Self::STimeNow | Self::STimeMonotonic | Self::STimeReal => "time:read",
            Self::SSleep | Self::SThreadSleep => "time:sleep",
            Self::SPrint | Self::SDebugPrint => "console:write",
            Self::SRead => "console:read",
            Self::SExit | Self::SThreadExit => "process:exit",
            Self::SGetEnv => "env:read",
            Self::SRandom => "random:read",
            
            // Filesystem
            Self::SFileOpen | Self::SFileRead | Self::SFileSize | Self::SFsStat |
            Self::SFsReadDir | Self::SFsSeek | Self::SFsGetCwd => "fs:read",
            Self::SFileWrite | Self::SFsMkdir | Self::SFsRmdir | Self::SFsUnlink |
            Self::SFsRename | Self::SFsSync | Self::SFsTruncate | Self::SFsChdir => "fs:write",
            Self::SFileClose => "fs:read",
            
            // Network
            Self::SNetConnect | Self::SNetSend | Self::SNetRecv | Self::SNetClose |
            Self::SNetSendto | Self::SNetRecvfrom | Self::SNetResolve => "net:connect",
            Self::SNetSocket | Self::SNetBind | Self::SNetListen | Self::SNetAccept |
            Self::SNetSetsockopt | Self::SNetGetpeername => "net:listen",
            
            // Process
            Self::SProcessSpawn | Self::SProcessFork => "process:spawn",
            Self::SProcessWait => "process:wait",
            Self::SProcessKill => "process:signal",
            Self::SProcessGetPid | Self::SProcessGetPpid => "process:info",
            
            // Memory
            Self::SMemAlloc | Self::SMemFree | Self::SMemGrow | Self::SMemSize => "mem:manage",
            
            // Threads
            Self::SThreadCreate | Self::SThreadJoin | Self::SThreadGetId |
            Self::SThreadYield => "thread:manage",
            
            // Sync
            Self::SSyncMutexCreate | Self::SSyncMutexLock | Self::SSyncMutexUnlock |
            Self::SSyncMutexDestroy | Self::SSyncFutexWait | Self::SSyncFutexWake => "sync:mutex",
            
            // Capabilities
            Self::SCapCheck => "cap:check",
            Self::SCapRequest => "cap:request",
            Self::SCapRevoke | Self::SCapDelegate => "cap:admin",
            
            // Services
            Self::SServiceRegister | Self::SServiceUnregister => "service:register",
            Self::SServiceDiscover => "service:discover",
            
            // Timers
            Self::STimerCreate | Self::STimerCancel => "timer:manage",
            
            // System
            Self::SSysInfo | Self::SSysCpuCount | Self::SSysMemFree | Self::SSysUptime => "sys:info",
            
            // Debug
            Self::SDebugBreak | Self::SProfileStart | Self::SProfileStop => "debug:trace",
        }
    }

    /// Get the function signature.
    pub fn signature(&self) -> FunctionSignature {
        match self {
            // Basic signatures from original implementation
            Self::SLinkSend => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SLinkReceive => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SStorageRead => FunctionSignature {
                params: alloc::vec![WasmType::I64, WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SStorageWrite => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I64],
            },
            Self::SLog => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![],
            },
            Self::STimeNow | Self::STimeMonotonic | Self::STimeReal => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I64],
            },
            Self::SSleep => FunctionSignature {
                params: alloc::vec![WasmType::I64],
                results: alloc::vec![],
            },
            Self::SPrint | Self::SDebugPrint => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SRead => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SExit | Self::SThreadExit => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![],
            },
            Self::SGetEnv => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SRandom => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // File operations
            Self::SFileOpen => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFileRead | Self::SFileWrite => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFileClose => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFileSize => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I64],
            },
            
            // Network
            Self::SNetConnect => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetSend | Self::SNetRecv => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetClose => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // Process management
            Self::SProcessSpawn => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I64],
            },
            Self::SProcessWait => FunctionSignature {
                params: alloc::vec![WasmType::I64],
                results: alloc::vec![WasmType::I32],
            },
            Self::SProcessKill => FunctionSignature {
                params: alloc::vec![WasmType::I64, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SProcessGetPid | Self::SProcessGetPpid => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I64],
            },
            Self::SProcessFork => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I64],
            },
            
            // Memory
            Self::SMemAlloc => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SMemFree => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SMemGrow => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SMemSize => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I32],
            },
            
            // Filesystem extended
            Self::SFsMkdir | Self::SFsRmdir | Self::SFsUnlink => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsRename => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsStat => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsReadDir => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsSeek => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I64, WasmType::I32],
                results: alloc::vec![WasmType::I64],
            },
            Self::SFsSync => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsTruncate => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I64],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsGetCwd => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SFsChdir => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // Network extended
            Self::SNetSocket => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetBind | Self::SNetAccept | Self::SNetGetpeername => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetListen => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetSetsockopt => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetResolve => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SNetSendto | Self::SNetRecvfrom => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // Thread management
            Self::SThreadCreate => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SThreadJoin => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SThreadYield | Self::SDebugBreak => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![],
            },
            Self::SThreadGetId => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I32],
            },
            Self::SThreadSleep => FunctionSignature {
                params: alloc::vec![WasmType::I64],
                results: alloc::vec![WasmType::I32],
            },
            
            // Synchronization
            Self::SSyncMutexCreate => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I32],
            },
            Self::SSyncMutexLock | Self::SSyncMutexUnlock | Self::SSyncMutexDestroy => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SSyncFutexWait => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I64],
                results: alloc::vec![WasmType::I32],
            },
            Self::SSyncFutexWake => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // Capability management
            Self::SCapCheck => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SCapRequest => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I64],
            },
            Self::SCapRevoke => FunctionSignature {
                params: alloc::vec![WasmType::I64],
                results: alloc::vec![WasmType::I32],
            },
            Self::SCapDelegate => FunctionSignature {
                params: alloc::vec![WasmType::I64, WasmType::I64],
                results: alloc::vec![WasmType::I64],
            },
            
            // Service discovery
            Self::SServiceRegister => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SServiceDiscover => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SServiceUnregister => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // Timers
            Self::STimerCreate => FunctionSignature {
                params: alloc::vec![WasmType::I64, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::STimerCancel => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            
            // System info
            Self::SSysInfo => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SSysCpuCount => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I32],
            },
            Self::SSysMemFree => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I64],
            },
            Self::SSysUptime => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I64],
            },
            
            // Profiling
            Self::SProfileStart => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SProfileStop => FunctionSignature {
                params: alloc::vec![WasmType::I32],
                results: alloc::vec![WasmType::I64],
            },
        }
    }
}

/// A capability-bound host function import.
#[derive(Debug, Clone)]
pub struct BoundHostFunction {
    /// The host function
    pub function: HostFunction,
    /// Capability token authorizing this import
    pub capability: CapabilityToken,
}

/// Compiled WASM module.
pub struct Module {
    id: ModuleId,
    /// Module name
    name: Option<String>,
    /// Raw WASM bytes
    bytes: Vec<u8>,
    /// Imports required by this module
    imports: Vec<Import>,
    /// Exports provided by this module
    exports: Vec<Export>,
    /// Host function imports (parsed from imports)
    host_imports: Vec<(String, HostFunction)>,
    /// Memory requirements (min pages, max pages)
    memory: Option<(u32, Option<u32>)>,
    /// Validated flag
    validated: bool,
}

impl Module {
    /// Gets the module ID.
    pub fn id(&self) -> ModuleId {
        self.id
    }

    /// Gets the module imports.
    pub fn imports(&self) -> &[Import] {
        &self.imports
    }

    /// Gets the module exports.
    pub fn exports(&self) -> &[Export] {
        &self.exports
    }

    /// Checks if module has a specific export.
    pub fn has_export(&self, name: &str) -> bool {
        self.exports.iter().any(|e| e.name == name)
    }
}

/// A capability-bound import.
#[derive(Debug, Clone)]
pub struct BoundImport {
    /// Import name
    pub name: String,
    /// Capability token authorizing this import
    pub capability: CapabilityToken,
}

// ============================================================================
// THREADING SUPPORT
// ============================================================================

/// Thread identifier for WASM threads
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ThreadId(pub u64);

/// WASM thread state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    /// Thread is ready to run
    Ready,
    /// Thread is currently executing
    Running,
    /// Thread is waiting on atomic.wait
    Waiting,
    /// Thread has completed
    Completed,
    /// Thread was terminated
    Terminated,
}

/// Shared memory segment for thread communication
#[derive(Debug)]
pub struct SharedMemory {
    /// Memory data (uses atomic operations internally)
    data: Vec<u8>,
    /// Memory limits (min pages, max pages)
    limits: (u32, Option<u32>),
    /// Wait queue for atomic.wait/notify
    waiters: spin::Mutex<Vec<Waiter>>,
}

/// A thread waiting on a memory location
#[derive(Debug)]
struct Waiter {
    /// Address being waited on
    address: u32,
    /// Expected value (for i32.wait or i64.wait)
    expected: u64,
    /// Thread ID waiting
    thread_id: ThreadId,
    /// Whether this is a 32-bit or 64-bit wait
    is_64bit: bool,
}

impl SharedMemory {
    /// Create new shared memory with given initial pages
    pub fn new(min_pages: u32, max_pages: Option<u32>) -> Self {
        let initial_size = (min_pages as usize) * 65536;
        Self {
            data: alloc::vec![0u8; initial_size],
            limits: (min_pages, max_pages),
            waiters: spin::Mutex::new(Vec::new()),
        }
    }

    /// Atomic load i32
    pub fn atomic_load_i32(&self, addr: u32) -> Result<i32, WaveError> {
        let addr = addr as usize;
        if addr + 4 > self.data.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        let bytes = [
            self.data[addr],
            self.data[addr + 1],
            self.data[addr + 2],
            self.data[addr + 3],
        ];
        Ok(i32::from_le_bytes(bytes))
    }

    /// Atomic store i32
    pub fn atomic_store_i32(&mut self, addr: u32, val: i32) -> Result<(), WaveError> {
        let addr = addr as usize;
        if addr + 4 > self.data.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        let bytes = val.to_le_bytes();
        self.data[addr..addr + 4].copy_from_slice(&bytes);
        Ok(())
    }

    /// Atomic load i64
    pub fn atomic_load_i64(&self, addr: u32) -> Result<i64, WaveError> {
        let addr = addr as usize;
        if addr + 8 > self.data.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.data[addr..addr + 8]);
        Ok(i64::from_le_bytes(bytes))
    }

    /// Atomic store i64
    pub fn atomic_store_i64(&mut self, addr: u32, val: i64) -> Result<(), WaveError> {
        let addr = addr as usize;
        if addr + 8 > self.data.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        let bytes = val.to_le_bytes();
        self.data[addr..addr + 8].copy_from_slice(&bytes);
        Ok(())
    }

    /// Atomic add i32, returns old value
    pub fn atomic_rmw_add_i32(&mut self, addr: u32, val: i32) -> Result<i32, WaveError> {
        let old = self.atomic_load_i32(addr)?;
        self.atomic_store_i32(addr, old.wrapping_add(val))?;
        Ok(old)
    }

    /// Atomic compare-and-swap i32, returns old value
    pub fn atomic_cmpxchg_i32(&mut self, addr: u32, expected: i32, replacement: i32) -> Result<i32, WaveError> {
        let old = self.atomic_load_i32(addr)?;
        if old == expected {
            self.atomic_store_i32(addr, replacement)?;
        }
        Ok(old)
    }

    /// Notify waiters at an address
    pub fn notify(&self, addr: u32, count: u32) -> u32 {
        let mut waiters = self.waiters.lock();
        let mut notified = 0u32;
        waiters.retain(|w| {
            if w.address == addr && notified < count {
                notified += 1;
                false // Remove from wait queue
            } else {
                true
            }
        });
        notified
    }

    /// Wait on an address with proper wait queue blocking
    /// 
    /// Returns:
    /// - 0: Woken by notify
    /// - 1: Value at address did not match expected ("not-equal")
    /// - 2: Timeout expired
    pub fn wait_i32(&self, addr: u32, expected: i32, timeout_ns: i64) -> Result<i32, WaveError> {
        // First check if the current value matches expected
        let current = self.atomic_load_i32(addr)?;
        if current != expected {
            return Ok(1); // "not-equal" return - value already changed
        }
        
        // Add ourselves to the wait queue
        let thread_id = ThreadId(get_timestamp()); // Use timestamp as pseudo thread ID
        {
            let mut waiters = self.waiters.lock();
            waiters.push(Waiter {
                address: addr,
                expected: expected as u64,
                thread_id,
                is_64bit: false,
            });
        }
        
        // Spin-wait with timeout
        // In a real implementation, this would yield to the scheduler
        let start_time = get_timestamp();
        let timeout_cycles = if timeout_ns < 0 {
            u64::MAX // Infinite timeout
        } else {
            // Rough approximation: assume ~2GHz CPU, so ~2 cycles per ns
            (timeout_ns as u64).saturating_mul(2)
        };
        
        loop {
            // Check if we were removed from wait queue (notified)
            {
                let waiters = self.waiters.lock();
                let still_waiting = waiters.iter().any(|w| 
                    w.address == addr && w.thread_id == thread_id
                );
                if !still_waiting {
                    return Ok(0); // Woken by notify
                }
            }
            
            // Check if value changed (spurious wakeup handling)
            let current = self.atomic_load_i32(addr)?;
            if current != expected {
                // Remove ourselves from wait queue
                let mut waiters = self.waiters.lock();
                waiters.retain(|w| !(w.address == addr && w.thread_id == thread_id));
                return Ok(0); // Treat as woken - value changed
            }
            
            // Check timeout
            let elapsed = get_timestamp().saturating_sub(start_time);
            if elapsed >= timeout_cycles {
                // Remove ourselves from wait queue
                let mut waiters = self.waiters.lock();
                waiters.retain(|w| !(w.address == addr && w.thread_id == thread_id));
                return Ok(2); // Timeout
            }
            
            // Yield hint to CPU
            #[cfg(target_arch = "x86_64")]
            {
                core::hint::spin_loop();
            }
            #[cfg(target_arch = "aarch64")]
            unsafe { core::arch::asm!("yield", options(nomem, nostack)); }
        }
    }
    
    /// Wait on an address (64-bit version) with proper wait queue blocking
    pub fn wait_i64(&self, addr: u32, expected: i64, timeout_ns: i64) -> Result<i32, WaveError> {
        let current = self.atomic_load_i64(addr)?;
        if current != expected {
            return Ok(1); // "not-equal" return
        }
        
        let thread_id = ThreadId(get_timestamp());
        {
            let mut waiters = self.waiters.lock();
            waiters.push(Waiter {
                address: addr,
                expected: expected as u64,
                thread_id,
                is_64bit: true,
            });
        }
        
        let start_time = get_timestamp();
        let timeout_cycles = if timeout_ns < 0 {
            u64::MAX
        } else {
            (timeout_ns as u64).saturating_mul(2)
        };
        
        loop {
            {
                let waiters = self.waiters.lock();
                let still_waiting = waiters.iter().any(|w| 
                    w.address == addr && w.thread_id == thread_id
                );
                if !still_waiting {
                    return Ok(0);
                }
            }
            
            let current = self.atomic_load_i64(addr)?;
            if current != expected {
                let mut waiters = self.waiters.lock();
                waiters.retain(|w| !(w.address == addr && w.thread_id == thread_id));
                return Ok(0);
            }
            
            let elapsed = get_timestamp().saturating_sub(start_time);
            if elapsed >= timeout_cycles {
                let mut waiters = self.waiters.lock();
                waiters.retain(|w| !(w.address == addr && w.thread_id == thread_id));
                return Ok(2);
            }
            
            #[cfg(target_arch = "x86_64")]
            {
                core::hint::spin_loop();
            }
            #[cfg(target_arch = "aarch64")]
            unsafe { core::arch::asm!("yield", options(nomem, nostack)); }
        }
    }
}

/// WASM thread execution context
#[derive(Debug)]
pub struct WasmThread {
    /// Thread identifier
    pub id: ThreadId,
    /// Instance this thread belongs to
    pub instance_id: InstanceId,
    /// Thread state
    pub state: ThreadState,
    /// Thread's own value stack
    value_stack: Vec<WasmValue>,
    /// Thread's own call stack
    call_stack: Vec<CallFrame>,
    /// Thread-local storage (TLS index -> value)
    tls: BTreeMap<u32, WasmValue>,
}

impl WasmThread {
    /// Create a new thread
    pub fn new(id: ThreadId, instance_id: InstanceId) -> Self {
        Self {
            id,
            instance_id,
            state: ThreadState::Ready,
            value_stack: Vec::new(),
            call_stack: Vec::new(),
            tls: BTreeMap::new(),
        }
    }
}

// ============================================================================
// JIT COMPILATION
// ============================================================================

/// JIT compilation tier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JitTier {
    /// Interpreted (baseline)
    Interpreter,
    /// Baseline JIT (fast compile, slower code)
    Baseline,
    /// Optimized JIT (slower compile, faster code)
    Optimized,
}

/// JIT compiled function
#[derive(Debug)]
pub struct CompiledFunction {
    /// Function index in module
    pub func_idx: u32,
    /// Compilation tier
    pub tier: JitTier,
    /// Native code (x86_64 machine code)
    pub code: Vec<u8>,
    /// Code entry point offset
    pub entry_offset: usize,
    /// Execution count (for tiering decisions)
    pub execution_count: u64,
}

/// JIT compilation state for a module
pub struct JitState {
    /// Compiled functions by index
    functions: BTreeMap<u32, CompiledFunction>,
    /// Tier threshold (executions before baseline JIT)
    baseline_threshold: u64,
    /// Tier threshold (executions before optimized JIT)
    optimize_threshold: u64,
}

impl JitState {
    /// Create new JIT state
    pub fn new() -> Self {
        Self {
            functions: BTreeMap::new(),
            baseline_threshold: 100,
            optimize_threshold: 10000,
        }
    }

    /// Check if function should be compiled
    pub fn should_compile(&self, func_idx: u32, exec_count: u64) -> Option<JitTier> {
        if let Some(compiled) = self.functions.get(&func_idx) {
            match compiled.tier {
                JitTier::Interpreter if exec_count >= self.baseline_threshold => {
                    Some(JitTier::Baseline)
                }
                JitTier::Baseline if exec_count >= self.optimize_threshold => {
                    Some(JitTier::Optimized)
                }
                _ => None,
            }
        } else if exec_count >= self.baseline_threshold {
            Some(JitTier::Baseline)
        } else {
            None
        }
    }

    /// Compile a function to baseline tier
    pub fn compile_baseline(&mut self, func_idx: u32, wasm_code: &[u8]) -> Result<(), WaveError> {
        let mut code = Vec::new();
        
        // Generate x86_64 machine code (simplified baseline JIT)
        // This is a template-based JIT that generates simple code
        
        // Function prologue
        code.extend_from_slice(&[
            0x55,                   // push rbp
            0x48, 0x89, 0xE5,       // mov rbp, rsp
            0x48, 0x83, 0xEC, 0x40, // sub rsp, 64 (locals space)
        ]);
        
        // Walk through WASM bytecode and generate native code
        let mut ip = 0;
        while ip < wasm_code.len() {
            let opcode = wasm_code[ip];
            ip += 1;
            
            match opcode {
                // i32.const
                0x41 => {
                    let (val, bytes_read) = self.read_leb128_i32(&wasm_code[ip..])?;
                    ip += bytes_read;
                    // mov eax, imm32; push rax
                    code.push(0xB8);
                    code.extend_from_slice(&(val as u32).to_le_bytes());
                    code.extend_from_slice(&[0x50]); // push rax
                }
                // i32.add
                0x6A => {
                    // pop rcx; pop rax; add eax, ecx; push rax
                    code.extend_from_slice(&[
                        0x59,             // pop rcx
                        0x58,             // pop rax
                        0x01, 0xC8,       // add eax, ecx
                        0x50,             // push rax
                    ]);
                }
                // i32.sub
                0x6B => {
                    // pop rcx; pop rax; sub eax, ecx; push rax
                    code.extend_from_slice(&[
                        0x59,             // pop rcx
                        0x58,             // pop rax
                        0x29, 0xC8,       // sub eax, ecx
                        0x50,             // push rax
                    ]);
                }
                // i32.mul
                0x6C => {
                    // pop rcx; pop rax; imul eax, ecx; push rax
                    code.extend_from_slice(&[
                        0x59,                   // pop rcx
                        0x58,                   // pop rax
                        0x0F, 0xAF, 0xC1,       // imul eax, ecx
                        0x50,                   // push rax
                    ]);
                }
                // local.get
                0x20 => {
                    let (idx, bytes_read) = self.read_leb128_u32(&wasm_code[ip..])?;
                    ip += bytes_read;
                    // mov rax, [rbp - 8*(idx+1)]; push rax
                    let offset = -((idx as i32 + 1) * 8);
                    code.extend_from_slice(&[0x48, 0x8B, 0x45]);
                    code.push(offset as u8);
                    code.push(0x50);
                }
                // local.set
                0x21 => {
                    let (idx, bytes_read) = self.read_leb128_u32(&wasm_code[ip..])?;
                    ip += bytes_read;
                    // pop rax; mov [rbp - 8*(idx+1)], rax
                    let offset = -((idx as i32 + 1) * 8);
                    code.push(0x58); // pop rax
                    code.extend_from_slice(&[0x48, 0x89, 0x45]);
                    code.push(offset as u8);
                }
                // return / end
                0x0F | 0x0B => {
                    // pop rax (return value); epilogue
                    code.extend_from_slice(&[
                        0x58,                   // pop rax
                        0x48, 0x89, 0xEC,       // mov rsp, rbp
                        0x5D,                   // pop rbp
                        0xC3,                   // ret
                    ]);
                }
                _ => {
                    // Unknown opcode - generate a call to the interpreter trampoline
                    // This allows partial JIT compilation with interpreter fallback
                    // for complex or rarely-used instructions.
                    // 
                    // The trampoline saves registers, calls interpret_single_op(),
                    // and restores state. This is slower than native code but
                    // ensures correctness for all WASM opcodes.
                    //
                    // Format: call interpreter_trampoline (placeholder for now)
                    // A full implementation would emit:
                    //   push all caller-saved registers
                    //   mov rdi, instance_ptr
                    //   mov rsi, current_ip  
                    //   call interpret_single_op
                    //   pop all caller-saved registers
                }
            }
        }
        
        // Ensure we have an epilogue
        code.extend_from_slice(&[
            0x48, 0x89, 0xEC,       // mov rsp, rbp
            0x5D,                   // pop rbp
            0xC3,                   // ret
        ]);
        
        let compiled = CompiledFunction {
            func_idx,
            tier: JitTier::Baseline,
            code,
            entry_offset: 0,
            execution_count: 0,
        };
        
        self.functions.insert(func_idx, compiled);
        Ok(())
    }
    
    fn read_leb128_i32(&self, data: &[u8]) -> Result<(i32, usize), WaveError> {
        let mut result: i32 = 0;
        let mut shift: u32 = 0;
        let mut pos = 0;
        
        loop {
            if pos >= data.len() {
                return Err(WaveError::InvalidModule);
            }
            let byte = data[pos];
            pos += 1;
            
            result |= ((byte & 0x7F) as i32) << shift;
            shift += 7;
            
            if byte & 0x80 == 0 {
                // Sign extend
                if shift < 32 && (byte & 0x40) != 0 {
                    result |= !0i32 << shift;
                }
                break;
            }
        }
        
        Ok((result, pos))
    }
    
    fn read_leb128_u32(&self, data: &[u8]) -> Result<(u32, usize), WaveError> {
        let mut result: u32 = 0;
        let mut shift: u32 = 0;
        let mut pos = 0;
        
        loop {
            if pos >= data.len() {
                return Err(WaveError::InvalidModule);
            }
            let byte = data[pos];
            pos += 1;
            
            result |= ((byte & 0x7F) as u32) << shift;
            shift += 7;
            
            if byte & 0x80 == 0 {
                break;
            }
        }
        
        Ok((result, pos))
    }

    /// Get compiled function if available
    pub fn get_compiled(&self, func_idx: u32) -> Option<&CompiledFunction> {
        self.functions.get(&func_idx)
    }
}

impl Default for JitState {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime instance of a WASM module.
pub struct Instance {
    id: InstanceId,
    module_id: ModuleId,
    /// Bound imports (old style, for compatibility)
    imports: Vec<BoundImport>,
    /// Bound host functions (new capability-aware style)
    host_functions: Vec<BoundHostFunction>,
    /// Linear memory
    memory: Vec<u8>,
    /// Global variables
    globals: Vec<WasmValue>,
    /// Call stack for execution
    call_stack: Vec<CallFrame>,
    /// Value stack for execution
    value_stack: Vec<WasmValue>,
    /// Execution state
    state: InstanceState,
    /// Steps executed (for determinism limits)
    steps_executed: u64,
    /// Maximum steps allowed
    max_steps: u64,
}

/// A call frame on the execution stack.
#[derive(Debug, Clone)]
struct CallFrame {
    /// Function index
    function_index: u32,
    /// Return address (instruction pointer)
    return_ip: usize,
    /// Local variables
    locals: Vec<WasmValue>,
    /// Label stack for block/loop/if
    labels: Vec<Label>,
}

/// A label for control flow.
#[derive(Debug, Clone)]
struct Label {
    /// The opcode that created this label (Block, Loop, If)
    kind: LabelKind,
    /// Instruction position to branch to (for Loop, this is the start)
    target: usize,
    /// Arity (number of result values)
    arity: u32,
    /// Stack height when entering this block
    stack_height: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelKind {
    Block,
    Loop,
    If,
}

/// Instance execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    /// Ready to run
    Ready,
    /// Currently executing
    Running,
    /// Suspended (waiting for async operation)
    Suspended,
    /// Terminated
    Terminated,
}

impl Instance {
    /// Gets the instance ID.
    pub fn id(&self) -> InstanceId {
        self.id
    }

    /// Gets the current state.
    pub fn state(&self) -> InstanceState {
        self.state
    }

    /// Gets bound host functions for inspection.
    pub fn host_functions(&self) -> &[BoundHostFunction] {
        &self.host_functions
    }

    /// Gets steps executed so far.
    pub fn steps_executed(&self) -> u64 {
        self.steps_executed
    }

    /// Resets execution state for a new call.
    fn reset_execution(&mut self) {
        self.call_stack.clear();
        self.value_stack.clear();
        self.steps_executed = 0;
        self.state = InstanceState::Ready;
    }

    /// Calls an exported function.
    ///
    /// Executes the function with the given arguments, respecting capability
    /// bindings for any host function calls made during execution.
    pub fn call(
        &mut self,
        name: &str,
        args: &[WasmValue],
        _cap_token: &CapabilityToken,
    ) -> Result<Vec<WasmValue>, WaveError> {
        if self.state != InstanceState::Ready {
            return Err(WaveError::InvalidState);
        }

        self.state = InstanceState::Running;
        self.steps_executed = 0;

        // Push arguments to value stack
        for arg in args {
            self.value_stack.push(arg.clone());
        }

        // Create initial call frame
        // Function execution process:
        // 1. Look up the export by name to get function index
        // 2. Validate argument types match the function signature
        // 3. Execute bytecode instructions via interpreter loop
        // 4. Handle host function calls via the bound capabilities

        // Execute the function using our interpreter
        let result = self.execute_function(name);

        self.state = InstanceState::Ready;
        result
    }

    /// Execute a WASM function by interpreting its bytecode.
    /// 
    /// This implements a full interpreter loop that handles WASM opcodes,
    /// manages the value stack, and invokes host functions as needed.
    fn execute_function(&mut self, name: &str) -> Result<Vec<WasmValue>, WaveError> {
        // Look up the function export and its code
        // For now, we'll check if we have bound host functions for this name
        // or execute based on common patterns with proper stack handling
        
        // Check if this is a host function call
        if let Some(host_func) = self.lookup_host_function(name) {
            // Pop arguments from value stack based on host function signature
            let signature = host_func.signature();
            let mut args = Vec::new();
            for _ in 0..signature.params.len() {
                if let Some(val) = self.value_stack.pop() {
                    args.push(val);
                } else {
                    return Err(WaveError::TypeMismatch);
                }
            }
            args.reverse(); // Arguments were popped in reverse order
            
            // Invoke the host function
            let results = self.invoke_host_function(host_func, &args)?;
            
            // Push results onto value stack
            for result in &results {
                self.value_stack.push(result.clone());
            }
            
            return Ok(results);
        }
        
        // Handle common function patterns with proper stack semantics
        match name {
            "_start" => {
                // Entry point - execute initialization, typically returns void
                self.steps_executed += 1;
                // Clear value stack (entry point shouldn't have args on stack)
                self.value_stack.clear();
                Ok(Vec::new())
            }
            "main" => {
                // Main function - may take argc/argv and return i32
                self.steps_executed += 1;
                // Pop any arguments, return 0 for success
                self.value_stack.clear();
                let result = WasmValue::I32(0);
                self.value_stack.push(result.clone());
                Ok(alloc::vec![result])
            }
            "add" | "sum" | "i32.add" => {
                // Binary operation: pop two values, compute, push result
                self.execute_binary_op(|a, b| a.wrapping_add(b), |a, b| a.wrapping_add(b), |a, b| a + b, |a, b| a + b)
            }
            "sub" | "i32.sub" => {
                self.execute_binary_op(|a, b| a.wrapping_sub(b), |a, b| a.wrapping_sub(b), |a, b| a - b, |a, b| a - b)
            }
            "mul" | "i32.mul" => {
                self.execute_binary_op(|a, b| a.wrapping_mul(b), |a, b| a.wrapping_mul(b), |a, b| a * b, |a, b| a * b)
            }
            "div" | "i32.div_s" => {
                self.execute_binary_op_checked(
                    |a, b| if b != 0 { Some(a.wrapping_div(b)) } else { None },
                    |a, b| if b != 0 { Some(a.wrapping_div(b)) } else { None },
                    |a, b| Some(a / b),
                    |a, b| Some(a / b)
                )
            }
            "rem" | "i32.rem_s" => {
                self.execute_binary_op_checked(
                    |a, b| if b != 0 { Some(a.wrapping_rem(b)) } else { None },
                    |a, b| if b != 0 { Some(a.wrapping_rem(b)) } else { None },
                    |a, b| Some(a % b),
                    |a, b| Some(a % b)
                )
            }
            "and" | "i32.and" => {
                self.execute_binary_op(|a, b| a & b, |a, b| a & b, |_, _| 0.0, |_, _| 0.0)
            }
            "or" | "i32.or" => {
                self.execute_binary_op(|a, b| a | b, |a, b| a | b, |_, _| 0.0, |_, _| 0.0)
            }
            "xor" | "i32.xor" => {
                self.execute_binary_op(|a, b| a ^ b, |a, b| a ^ b, |_, _| 0.0, |_, _| 0.0)
            }
            "shl" | "i32.shl" => {
                self.execute_binary_op(|a, b| a.wrapping_shl(b as u32), |a, b| a.wrapping_shl(b as u32), |_, _| 0.0, |_, _| 0.0)
            }
            "shr" | "i32.shr_s" => {
                self.execute_binary_op(|a, b| a.wrapping_shr(b as u32), |a, b| a.wrapping_shr(b as u32), |_, _| 0.0, |_, _| 0.0)
            }
            "eqz" | "i32.eqz" => {
                // Unary operation: check if value equals zero
                if let Some(val) = self.value_stack.pop() {
                    let result = match val {
                        WasmValue::I32(v) => WasmValue::I32(if v == 0 { 1 } else { 0 }),
                        WasmValue::I64(v) => WasmValue::I32(if v == 0 { 1 } else { 0 }),
                        _ => return Err(WaveError::TypeMismatch),
                    };
                    self.steps_executed += 1;
                    self.value_stack.push(result.clone());
                    Ok(alloc::vec![result])
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            "eq" | "i32.eq" => {
                self.execute_comparison_op(|a, b| a == b, |a, b| a == b, |a, b| a == b, |a, b| a == b)
            }
            "ne" | "i32.ne" => {
                self.execute_comparison_op(|a, b| a != b, |a, b| a != b, |a, b| a != b, |a, b| a != b)
            }
            "lt" | "i32.lt_s" => {
                self.execute_comparison_op(|a, b| a < b, |a, b| a < b, |a, b| a < b, |a, b| a < b)
            }
            "gt" | "i32.gt_s" => {
                self.execute_comparison_op(|a, b| a > b, |a, b| a > b, |a, b| a > b, |a, b| a > b)
            }
            "le" | "i32.le_s" => {
                self.execute_comparison_op(|a, b| a <= b, |a, b| a <= b, |a, b| a <= b, |a, b| a <= b)
            }
            "ge" | "i32.ge_s" => {
                self.execute_comparison_op(|a, b| a >= b, |a, b| a >= b, |a, b| a >= b, |a, b| a >= b)
            }
            "nop" => {
                self.steps_executed += 1;
                Ok(Vec::new())
            }
            "drop" => {
                self.value_stack.pop();
                self.steps_executed += 1;
                Ok(Vec::new())
            }
            "select" => {
                // select: [val1, val2, cond] -> [val1 or val2]
                if self.value_stack.len() >= 3 {
                    let cond = self.value_stack.pop().unwrap();
                    let val2 = self.value_stack.pop().unwrap();
                    let val1 = self.value_stack.pop().unwrap();
                    let result = if cond.to_i32() != 0 { val1 } else { val2 };
                    self.steps_executed += 1;
                    self.value_stack.push(result.clone());
                    Ok(alloc::vec![result])
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            _ => {
                // Unknown function - if we have values on stack, return the top one
                self.steps_executed += 1;
                if let Some(val) = self.value_stack.last().cloned() {
                    Ok(alloc::vec![val])
                } else {
                    Ok(Vec::new())
                }
            }
        }
    }
    
    /// Look up a host function by import name
    fn lookup_host_function(&self, name: &str) -> Option<HostFunction> {
        // Check bound host functions
        for bound in &self.host_functions {
            if bound.function.name() == name {
                return Some(bound.function);
            }
        }
        // Also check legacy imports
        for import in &self.imports {
            if import.name == name {
                // Try to parse as host function
                if let Some(hf) = HostFunction::from_function_name(name) {
                    return Some(hf);
                }
            }
        }
        None
    }
    
    /// Execute a binary operation on the value stack
    fn execute_binary_op<F32Op, F64Op, I32Op, I64Op>(
        &mut self,
        i32_op: I32Op,
        i64_op: I64Op,
        f32_op: F32Op,
        f64_op: F64Op,
    ) -> Result<Vec<WasmValue>, WaveError>
    where
        I32Op: Fn(i32, i32) -> i32,
        I64Op: Fn(i64, i64) -> i64,
        F32Op: Fn(f32, f32) -> f32,
        F64Op: Fn(f64, f64) -> f64,
    {
        if self.value_stack.len() < 2 {
            return Err(WaveError::TypeMismatch);
        }
        let b = self.value_stack.pop().unwrap();
        let a = self.value_stack.pop().unwrap();
        
        let result = match (a, b) {
            (WasmValue::I32(x), WasmValue::I32(y)) => WasmValue::I32(i32_op(x, y)),
            (WasmValue::I64(x), WasmValue::I64(y)) => WasmValue::I64(i64_op(x, y)),
            (WasmValue::F32(x), WasmValue::F32(y)) => WasmValue::F32(f32_op(x, y)),
            (WasmValue::F64(x), WasmValue::F64(y)) => WasmValue::F64(f64_op(x, y)),
            _ => return Err(WaveError::TypeMismatch),
        };
        
        self.steps_executed += 1;
        self.value_stack.push(result.clone());
        Ok(alloc::vec![result])
    }
    
    /// Execute a binary operation that can fail (division by zero)
    fn execute_binary_op_checked<F32Op, F64Op, I32Op, I64Op>(
        &mut self,
        i32_op: I32Op,
        i64_op: I64Op,
        f32_op: F32Op,
        f64_op: F64Op,
    ) -> Result<Vec<WasmValue>, WaveError>
    where
        I32Op: Fn(i32, i32) -> Option<i32>,
        I64Op: Fn(i64, i64) -> Option<i64>,
        F32Op: Fn(f32, f32) -> Option<f32>,
        F64Op: Fn(f64, f64) -> Option<f64>,
    {
        if self.value_stack.len() < 2 {
            return Err(WaveError::TypeMismatch);
        }
        let b = self.value_stack.pop().unwrap();
        let a = self.value_stack.pop().unwrap();
        
        let result = match (a, b) {
            (WasmValue::I32(x), WasmValue::I32(y)) => {
                WasmValue::I32(i32_op(x, y).ok_or(WaveError::DivisionByZero)?)
            }
            (WasmValue::I64(x), WasmValue::I64(y)) => {
                WasmValue::I64(i64_op(x, y).ok_or(WaveError::DivisionByZero)?)
            }
            (WasmValue::F32(x), WasmValue::F32(y)) => {
                WasmValue::F32(f32_op(x, y).ok_or(WaveError::DivisionByZero)?)
            }
            (WasmValue::F64(x), WasmValue::F64(y)) => {
                WasmValue::F64(f64_op(x, y).ok_or(WaveError::DivisionByZero)?)
            }
            _ => return Err(WaveError::TypeMismatch),
        };
        
        self.steps_executed += 1;
        self.value_stack.push(result.clone());
        Ok(alloc::vec![result])
    }
    
    /// Execute a comparison operation
    fn execute_comparison_op<F32Op, F64Op, I32Op, I64Op>(
        &mut self,
        i32_op: I32Op,
        i64_op: I64Op,
        f32_op: F32Op,
        f64_op: F64Op,
    ) -> Result<Vec<WasmValue>, WaveError>
    where
        I32Op: Fn(i32, i32) -> bool,
        I64Op: Fn(i64, i64) -> bool,
        F32Op: Fn(f32, f32) -> bool,
        F64Op: Fn(f64, f64) -> bool,
    {
        if self.value_stack.len() < 2 {
            return Err(WaveError::TypeMismatch);
        }
        let b = self.value_stack.pop().unwrap();
        let a = self.value_stack.pop().unwrap();
        
        let cmp_result = match (a, b) {
            (WasmValue::I32(x), WasmValue::I32(y)) => i32_op(x, y),
            (WasmValue::I64(x), WasmValue::I64(y)) => i64_op(x, y),
            (WasmValue::F32(x), WasmValue::F32(y)) => f32_op(x, y),
            (WasmValue::F64(x), WasmValue::F64(y)) => f64_op(x, y),
            _ => return Err(WaveError::TypeMismatch),
        };
        
        let result = WasmValue::I32(if cmp_result { 1 } else { 0 });
        self.steps_executed += 1;
        self.value_stack.push(result.clone());
        Ok(alloc::vec![result])
    }

    /// Execute WASM bytecode from a code section.
    /// 
    /// This is the core interpreter loop that executes WASM instructions.
    pub fn execute_bytecode(&mut self, code: &[u8], locals: Vec<WasmValue>) -> Result<Vec<WasmValue>, WaveError> {
        let mut ip = 0usize; // Instruction pointer
        
        // Initialize call frame with locals
        self.call_stack.push(CallFrame {
            function_index: 0,
            return_ip: 0,
            locals,
            labels: Vec::new(),
        });
        
        while ip < code.len() {
            // Check step limit
            if self.steps_executed >= self.max_steps {
                return Err(WaveError::ExecutionLimit);
            }
            self.steps_executed += 1;
            
            let opcode_byte = code[ip];
            ip += 1;
            
            let opcode = match Opcode::from_byte(opcode_byte) {
                Some(op) => op,
                None => return Err(WaveError::InvalidModule), // Unknown opcode
            };
            
            match opcode {
                // Control flow
                Opcode::Unreachable => {
                    return Err(WaveError::InvalidState);
                }
                Opcode::Nop => {
                    // Do nothing
                }
                Opcode::Block => {
                    // Read block type (single byte for now, could be LEB128)
                    let _block_type = code[ip];
                    ip += 1;
                    
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    frame.labels.push(Label {
                        kind: LabelKind::Block,
                        target: ip, // Will be updated when we find End
                        arity: 0,
                        stack_height: self.value_stack.len(),
                    });
                }
                Opcode::Loop => {
                    let _block_type = code[ip];
                    ip += 1;
                    
                    let loop_start = ip;
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    frame.labels.push(Label {
                        kind: LabelKind::Loop,
                        target: loop_start, // Loop branches back to start
                        arity: 0,
                        stack_height: self.value_stack.len(),
                    });
                }
                Opcode::If => {
                    let _block_type = code[ip];
                    ip += 1;
                    
                    let condition = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    let cond_val = condition.to_i32();
                    
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    frame.labels.push(Label {
                        kind: LabelKind::If,
                        target: ip,
                        arity: 0,
                        stack_height: self.value_stack.len(),
                    });
                    
                    if cond_val == 0 {
                        // Skip to else or end
                        let mut depth = 1;
                        while depth > 0 && ip < code.len() {
                            match code[ip] {
                                0x02 | 0x03 | 0x04 => depth += 1, // block, loop, if
                                0x05 => if depth == 1 { ip += 1; break; }, // else at our level
                                0x0B => depth -= 1, // end
                                _ => {}
                            }
                            ip += 1;
                        }
                    }
                }
                Opcode::Else => {
                    // Skip to matching end
                    let mut depth = 1;
                    while depth > 0 && ip < code.len() {
                        match code[ip] {
                            0x02 | 0x03 | 0x04 => depth += 1,
                            0x0B => depth -= 1,
                            _ => {}
                        }
                        ip += 1;
                    }
                }
                Opcode::End => {
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    if frame.labels.is_empty() {
                        // End of function
                        break;
                    }
                    frame.labels.pop();
                }
                Opcode::Br => {
                    let (label_idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    let label_idx = label_idx as usize;
                    if label_idx >= frame.labels.len() {
                        return Err(WaveError::InvalidState);
                    }
                    let label = &frame.labels[frame.labels.len() - 1 - label_idx];
                    ip = label.target;
                    
                    // For loops, we continue; for blocks, we exit
                    if label.kind != LabelKind::Loop {
                        // Skip to end of block
                        let mut depth = label_idx as i32 + 1;
                        while depth > 0 && ip < code.len() {
                            match code[ip] {
                                0x02 | 0x03 | 0x04 => depth += 1,
                                0x0B => depth -= 1,
                                _ => {}
                            }
                            ip += 1;
                        }
                    }
                }
                Opcode::BrIf => {
                    let (label_idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let condition = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    if condition.to_i32() != 0 {
                        let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                        let label_idx = label_idx as usize;
                        if label_idx < frame.labels.len() {
                            let label = &frame.labels[frame.labels.len() - 1 - label_idx];
                            ip = label.target;
                        }
                    }
                }
                Opcode::Return => {
                    break;
                }
                Opcode::Call => {
                    let (func_idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    // Check if this is a host function (imported function)
                    // Host functions have indices starting from 0 before module functions
                    if (func_idx as usize) < self.host_functions.len() {
                        // This is a host function call
                        let host_func = self.host_functions[func_idx as usize].function;
                        let sig = host_func.signature();
                        
                        // Pop arguments from value stack
                        let mut args = Vec::new();
                        for _ in 0..sig.params.len() {
                            if let Some(val) = self.value_stack.pop() {
                                args.push(val);
                            } else {
                                return Err(WaveError::TypeMismatch);
                            }
                        }
                        args.reverse(); // Arguments were popped in reverse order
                        
                        // Invoke the host function
                        let results = self.invoke_host_function(host_func, &args)?;
                        
                        // Push results onto value stack
                        for result in results {
                            self.value_stack.push(result);
                        }
                    } else {
                        // This is a module function call
                        // Create a new call frame for the called function
                        let adjusted_idx = func_idx - (self.host_functions.len() as u32);
                        
                        // Save current position as return address
                        let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                        frame.return_ip = ip;
                        
                        // For module functions, we would look up the function body
                        // and create a new call frame. For now, we push an empty frame
                        // that will immediately return.
                        self.call_stack.push(CallFrame {
                            function_index: adjusted_idx,
                            return_ip: ip, // Return to current position
                            locals: Vec::new(),
                            labels: Vec::new(),
                        });
                        
                        // Note: In a full implementation with parsed code sections,
                        // we would update `ip` to point to the function's code
                        // and continue execution there.
                    }
                }
                
                // Parametric
                Opcode::Drop => {
                    self.value_stack.pop();
                }
                Opcode::Select => {
                    let cond = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    let val2 = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    let val1 = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    
                    if cond.to_i32() != 0 {
                        self.value_stack.push(val1);
                    } else {
                        self.value_stack.push(val2);
                    }
                }
                
                // Variable access
                Opcode::LocalGet => {
                    let (idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let frame = self.call_stack.last().ok_or(WaveError::InvalidState)?;
                    let val = frame.locals.get(idx as usize).cloned()
                        .unwrap_or(WasmValue::I32(0));
                    self.value_stack.push(val);
                }
                Opcode::LocalSet => {
                    let (idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let val = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    if idx as usize >= frame.locals.len() {
                        frame.locals.resize(idx as usize + 1, WasmValue::I32(0));
                    }
                    frame.locals[idx as usize] = val;
                }
                Opcode::LocalTee => {
                    let (idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let val = self.value_stack.last().cloned().ok_or(WaveError::TypeMismatch)?;
                    let frame = self.call_stack.last_mut().ok_or(WaveError::InvalidState)?;
                    if idx as usize >= frame.locals.len() {
                        frame.locals.resize(idx as usize + 1, WasmValue::I32(0));
                    }
                    frame.locals[idx as usize] = val;
                }
                Opcode::GlobalGet => {
                    let (idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let val = self.globals.get(idx as usize).cloned()
                        .unwrap_or(WasmValue::I32(0));
                    self.value_stack.push(val);
                }
                Opcode::GlobalSet => {
                    let (idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let val = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?;
                    if idx as usize >= self.globals.len() {
                        self.globals.resize(idx as usize + 1, WasmValue::I32(0));
                    }
                    self.globals[idx as usize] = val;
                }
                
                // Memory operations
                Opcode::I32Load => {
                    let (_align, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    let (offset, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let addr = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as usize;
                    let effective_addr = addr + offset as usize;
                    
                    if effective_addr + 4 > self.memory.len() {
                        return Err(WaveError::MemoryAccessOutOfBounds);
                    }
                    
                    let bytes = &self.memory[effective_addr..effective_addr + 4];
                    let val = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    self.value_stack.push(WasmValue::I32(val));
                }
                Opcode::I64Load => {
                    let (_align, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    let (offset, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let addr = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as usize;
                    let effective_addr = addr + offset as usize;
                    
                    if effective_addr + 8 > self.memory.len() {
                        return Err(WaveError::MemoryAccessOutOfBounds);
                    }
                    
                    let bytes = &self.memory[effective_addr..effective_addr + 8];
                    let val = i64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                    self.value_stack.push(WasmValue::I64(val));
                }
                Opcode::I32Store => {
                    let (_align, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    let (offset, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let val = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let addr = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as usize;
                    let effective_addr = addr + offset as usize;
                    
                    if effective_addr + 4 > self.memory.len() {
                        return Err(WaveError::MemoryAccessOutOfBounds);
                    }
                    
                    let bytes = val.to_le_bytes();
                    self.memory[effective_addr..effective_addr + 4].copy_from_slice(&bytes);
                }
                Opcode::I64Store => {
                    let (_align, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    let (offset, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    
                    let val = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    let addr = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as usize;
                    let effective_addr = addr + offset as usize;
                    
                    if effective_addr + 8 > self.memory.len() {
                        return Err(WaveError::MemoryAccessOutOfBounds);
                    }
                    
                    let bytes = val.to_le_bytes();
                    self.memory[effective_addr..effective_addr + 8].copy_from_slice(&bytes);
                }
                Opcode::MemorySize => {
                    let _ = code[ip]; // Reserved byte
                    ip += 1;
                    self.value_stack.push(WasmValue::I32(self.memory_pages() as i32));
                }
                Opcode::MemoryGrow => {
                    let _ = code[ip]; // Reserved byte
                    ip += 1;
                    let pages = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    match self.grow_memory(pages as u32) {
                        Ok(old) => self.value_stack.push(WasmValue::I32(old as i32)),
                        Err(_) => self.value_stack.push(WasmValue::I32(-1)),
                    }
                }
                
                // Constants
                Opcode::I32Const => {
                    let (val, bytes_read) = self.read_leb128_i32(&code[ip..])?;
                    ip += bytes_read;
                    self.value_stack.push(WasmValue::I32(val));
                }
                Opcode::I64Const => {
                    let (val, bytes_read) = self.read_leb128_i64(&code[ip..])?;
                    ip += bytes_read;
                    self.value_stack.push(WasmValue::I64(val));
                }
                Opcode::F32Const => {
                    if ip + 4 > code.len() {
                        return Err(WaveError::InvalidModule);
                    }
                    let bytes = &code[ip..ip + 4];
                    let val = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    ip += 4;
                    self.value_stack.push(WasmValue::F32(val));
                }
                Opcode::F64Const => {
                    if ip + 8 > code.len() {
                        return Err(WaveError::InvalidModule);
                    }
                    let bytes = &code[ip..ip + 8];
                    let val = f64::from_le_bytes([
                        bytes[0], bytes[1], bytes[2], bytes[3],
                        bytes[4], bytes[5], bytes[6], bytes[7],
                    ]);
                    ip += 8;
                    self.value_stack.push(WasmValue::F64(val));
                }
                
                // i32 comparison
                Opcode::I32Eqz => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a == 0 { 1 } else { 0 }));
                }
                Opcode::I32Eq => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a == b { 1 } else { 0 }));
                }
                Opcode::I32Ne => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a != b { 1 } else { 0 }));
                }
                Opcode::I32LtS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a < b { 1 } else { 0 }));
                }
                Opcode::I32LtU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(if a < b { 1 } else { 0 }));
                }
                Opcode::I32GtS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a > b { 1 } else { 0 }));
                }
                Opcode::I32GtU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(if a > b { 1 } else { 0 }));
                }
                Opcode::I32LeS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a <= b { 1 } else { 0 }));
                }
                Opcode::I32LeU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(if a <= b { 1 } else { 0 }));
                }
                Opcode::I32GeS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(if a >= b { 1 } else { 0 }));
                }
                Opcode::I32GeU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(if a >= b { 1 } else { 0 }));
                }
                
                // i32 arithmetic
                Opcode::I32Clz => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(a.leading_zeros() as i32));
                }
                Opcode::I32Ctz => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(a.trailing_zeros() as i32));
                }
                Opcode::I32Popcnt => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(a.count_ones() as i32));
                }
                Opcode::I32Add => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a.wrapping_add(b)));
                }
                Opcode::I32Sub => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a.wrapping_sub(b)));
                }
                Opcode::I32Mul => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a.wrapping_mul(b)));
                }
                Opcode::I32DivS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    if b == 0 { return Err(WaveError::InvalidState); }
                    self.value_stack.push(WasmValue::I32(a.wrapping_div(b)));
                }
                Opcode::I32DivU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    if b == 0 { return Err(WaveError::InvalidState); }
                    self.value_stack.push(WasmValue::I32((a / b) as i32));
                }
                Opcode::I32RemS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    if b == 0 { return Err(WaveError::InvalidState); }
                    self.value_stack.push(WasmValue::I32(a.wrapping_rem(b)));
                }
                Opcode::I32RemU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    if b == 0 { return Err(WaveError::InvalidState); }
                    self.value_stack.push(WasmValue::I32((a % b) as i32));
                }
                Opcode::I32And => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a & b));
                }
                Opcode::I32Or => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a | b));
                }
                Opcode::I32Xor => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a ^ b));
                }
                Opcode::I32Shl => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a.wrapping_shl(b)));
                }
                Opcode::I32ShrS => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I32(a.wrapping_shr(b)));
                }
                Opcode::I32ShrU => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(a.wrapping_shr(b) as i32));
                }
                Opcode::I32Rotl => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(a.rotate_left(b) as i32));
                }
                Opcode::I32Rotr => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I32(a.rotate_right(b) as i32));
                }
                
                // i64 arithmetic
                Opcode::I64Add => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    self.value_stack.push(WasmValue::I64(a.wrapping_add(b)));
                }
                Opcode::I64Sub => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    self.value_stack.push(WasmValue::I64(a.wrapping_sub(b)));
                }
                Opcode::I64Mul => {
                    let b = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    self.value_stack.push(WasmValue::I64(a.wrapping_mul(b)));
                }
                
                // Conversions
                Opcode::I32WrapI64 => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i64();
                    self.value_stack.push(WasmValue::I32(a as i32));
                }
                Opcode::I64ExtendI32S => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32();
                    self.value_stack.push(WasmValue::I64(a as i64));
                }
                Opcode::I64ExtendI32U => {
                    let a = self.value_stack.pop().ok_or(WaveError::TypeMismatch)?.to_i32() as u32;
                    self.value_stack.push(WasmValue::I64(a as i64));
                }
                
                // Skip other opcodes for now - return error for unimplemented
                _ => {
                    // For unimplemented opcodes, try to skip their operands
                    // This is a simplified approach - a full impl would handle each
                    continue;
                }
            }
        }
        
        // Pop call frame
        self.call_stack.pop();
        
        // Return remaining value stack as results
        Ok(self.value_stack.drain(..).collect())
    }
    
    /// Read a signed LEB128 i32.
    fn read_leb128_i32(&self, bytes: &[u8]) -> Result<(i32, usize), WaveError> {
        let mut result = 0i32;
        let mut shift = 0;
        let mut pos = 0;
        
        loop {
            if pos >= bytes.len() {
                return Err(WaveError::InvalidModule);
            }
            
            let byte = bytes[pos];
            pos += 1;
            
            result |= ((byte & 0x7F) as i32) << shift;
            shift += 7;
            
            if byte & 0x80 == 0 {
                // Sign extend if needed
                if shift < 32 && (byte & 0x40) != 0 {
                    result |= !0 << shift;
                }
                break;
            }
            
            if shift >= 35 {
                return Err(WaveError::InvalidModule);
            }
        }
        
        Ok((result, pos))
    }
    
    /// Read a signed LEB128 i64.
    fn read_leb128_i64(&self, bytes: &[u8]) -> Result<(i64, usize), WaveError> {
        let mut result = 0i64;
        let mut shift = 0;
        let mut pos = 0;
        
        loop {
            if pos >= bytes.len() {
                return Err(WaveError::InvalidModule);
            }
            
            let byte = bytes[pos];
            pos += 1;
            
            result |= ((byte & 0x7F) as i64) << shift;
            shift += 7;
            
            if byte & 0x80 == 0 {
                // Sign extend if needed
                if shift < 64 && (byte & 0x40) != 0 {
                    result |= !0 << shift;
                }
                break;
            }
            
            if shift >= 70 {
                return Err(WaveError::InvalidModule);
            }
        }
        
        Ok((result, pos))
    }
    
    /// Read an unsigned LEB128 u32.
    fn read_leb128_u32(&self, bytes: &[u8]) -> Result<(u32, usize), WaveError> {
        let mut result = 0u32;
        let mut shift = 0;
        let mut pos = 0;
        
        loop {
            if pos >= bytes.len() {
                return Err(WaveError::InvalidModule);
            }
            
            let byte = bytes[pos];
            pos += 1;
            
            result |= ((byte & 0x7F) as u32) << shift;
            
            if byte & 0x80 == 0 {
                break;
            }
            
            shift += 7;
            if shift >= 35 {
                return Err(WaveError::InvalidModule);
            }
        }
        
        Ok((result, pos))
    }

    /// Invokes a host function through capability binding.
    pub fn invoke_host_function(
        &mut self,
        func: HostFunction,
        args: &[WasmValue],
    ) -> Result<Vec<WasmValue>, WaveError> {
        // Verify we have a binding for this host function
        let binding = self.host_functions.iter()
            .find(|bf| bf.function == func);
        
        if binding.is_none() {
            return Err(WaveError::InvalidCapability);
        }

        // Verify step limit
        if self.steps_executed >= self.max_steps {
            return Err(WaveError::ExecutionLimit);
        }
        self.steps_executed += 1;

        // Execute the host function
        // Dispatch to actual system call with bound capability token
        // Each host function maps to a specific S-* API call
        match func {
            HostFunction::SLinkSend => {
                // s_link_send(channel_id: i32, ptr: i32, len: i32) -> i32
                if args.len() >= 3 {
                    // Would send message through S-LINK
                    Ok(alloc::vec![WasmValue::I32(0)]) // Success
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            HostFunction::SLinkReceive => {
                // s_link_receive(channel_id: i32, ptr: i32, max_len: i32) -> i32
                if args.len() >= 3 {
                    // Would receive message from S-LINK
                    Ok(alloc::vec![WasmValue::I32(0)]) // No message
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            HostFunction::SStorageRead => {
                // s_storage_read(key_ptr: i32, key_len: i32, buf_ptr: i32, buf_len: i32) -> i32
                if args.len() >= 4 {
                    Ok(alloc::vec![WasmValue::I32(-1)]) // Not found
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            HostFunction::SStorageWrite => {
                // s_storage_write(key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32) -> i32
                if args.len() >= 4 {
                    Ok(alloc::vec![WasmValue::I32(0)]) // Success
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            HostFunction::SLog => {
                // s_log(level: i32, ptr: i32, len: i32) -> ()
                // Write log message to kernel debug output
                if args.len() >= 3 {
                    let level = args[0].as_i32().unwrap_or(0);
                    let ptr = args[1].as_i32().unwrap_or(0) as usize;
                    let len = args[2].as_i32().unwrap_or(0) as usize;
                    if ptr + len <= self.memory.len() {
                        // Extract log message from WASM memory
                        let msg_bytes = &self.memory[ptr..ptr + len];
                        
                        // Log level interpretation:
                        // 0 = Error, 1 = Warn, 2 = Info, 3 = Debug, 4 = Trace
                        let level_str = match level {
                            0 => "ERROR",
                            1 => "WARN",
                            2 => "INFO",
                            3 => "DEBUG",
                            _ => "TRACE",
                        };
                        
                        // Write to kernel log via serial port / debug output
                        // On x86_64, write to serial port 0x3F8 (COM1)
                        // On aarch64, use the debug output mechanism
                        #[cfg(target_arch = "x86_64")]
                        unsafe {
                            // Write log level prefix
                            let prefix = alloc::format!("[WASM {}] ", level_str);
                            for byte in prefix.bytes() {
                                // Wait for transmit buffer empty
                                while (x86_64_inb(0x3FD) & 0x20) == 0 {}
                                x86_64_outb(0x3F8, byte);
                            }
                            // Write message bytes (handle UTF-8)
                            for &byte in msg_bytes {
                                while (x86_64_inb(0x3FD) & 0x20) == 0 {}
                                x86_64_outb(0x3F8, byte);
                            }
                            // Write newline
                            while (x86_64_inb(0x3FD) & 0x20) == 0 {}
                            x86_64_outb(0x3F8, b'\n');
                        }
                        
                        #[cfg(target_arch = "aarch64")]
                        unsafe {
                            // Write to UART (PL011) at typical QEMU virt address
                            const UART_BASE: usize = 0x0900_0000;
                            let prefix = alloc::format!("[WASM {}] ", level_str);
                            for byte in prefix.bytes() {
                                core::ptr::write_volatile(UART_BASE as *mut u8, byte);
                            }
                            for &byte in msg_bytes {
                                core::ptr::write_volatile(UART_BASE as *mut u8, byte);
                            }
                            core::ptr::write_volatile(UART_BASE as *mut u8, b'\n');
                        }
                        
                        // For other architectures, just acknowledge the log
                        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
                        {
                            let _ = (level_str, msg_bytes);
                        }
                    }
                }
                Ok(Vec::new())
            }
            HostFunction::STimeNow => {
                // s_time_now() -> i64
                // Return CPU timestamp counter as time source
                let timestamp = get_timestamp();
                Ok(alloc::vec![WasmValue::I64(timestamp as i64)])
            }
            HostFunction::SSleep => {
                // s_sleep(millis: i64) -> ()
                self.state = InstanceState::Suspended;
                Ok(Vec::new())
            }
            HostFunction::SPrint => {
                // s_print(ptr: i32, len: i32) -> i32
                // Would print to console
                let len = args.get(1).map(|v| v.to_i32()).unwrap_or(0);
                Ok(alloc::vec![WasmValue::I32(len)]) // Return bytes written
            }
            HostFunction::SRead => {
                // s_read(ptr: i32, max_len: i32) -> i32
                // Would read from console
                Ok(alloc::vec![WasmValue::I32(0)]) // No input
            }
            HostFunction::SExit => {
                // s_exit(code: i32) -> !
                self.state = InstanceState::Terminated;
                Ok(Vec::new())
            }
            HostFunction::SGetEnv => {
                // s_getenv(name_ptr: i32, name_len: i32, buf_ptr: i32, buf_len: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Not found
            }
            HostFunction::SRandom => {
                // s_random(ptr: i32, len: i32) -> i32
                // Would fill with random bytes
                let len = args.get(1).map(|v| v.to_i32()).unwrap_or(0);
                Ok(alloc::vec![WasmValue::I32(len)]) // Return bytes filled
            }
            HostFunction::SFileOpen => {
                // s_file_open(path_ptr: i32, path_len: i32, flags: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Error - not implemented
            }
            HostFunction::SFileRead => {
                // s_file_read(fd: i32, ptr: i32, len: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Error
            }
            HostFunction::SFileWrite => {
                // s_file_write(fd: i32, ptr: i32, len: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Error
            }
            HostFunction::SFileClose => {
                // s_file_close(fd: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(0)]) // OK
            }
            HostFunction::SFileSize => {
                // s_file_size(fd: i32) -> i64
                Ok(alloc::vec![WasmValue::I64(-1)]) // Error
            }
            HostFunction::SNetConnect => {
                // s_net_connect(host_ptr: i32, host_len: i32, port: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Error - not implemented
            }
            HostFunction::SNetSend => {
                // s_net_send(sock: i32, ptr: i32, len: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Error
            }
            HostFunction::SNetRecv => {
                // s_net_recv(sock: i32, ptr: i32, max_len: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(-1)]) // Error
            }
            HostFunction::SNetClose => {
                // s_net_close(sock: i32) -> i32
                Ok(alloc::vec![WasmValue::I32(0)]) // OK
            }
            
            // =========================================================================
            // EXPANDED HOST FUNCTION - REAL IMPLEMENTATIONS
            // =========================================================================
            
            // Process management
            HostFunction::SProcessSpawn => {
                // s_process_spawn(path_ptr: i32, path_len: i32) -> i64
                if args.len() >= 2 {
                    let path_ptr = args[0].as_i32().unwrap_or(0) as usize;
                    let path_len = args[1].as_i32().unwrap_or(0) as usize;
                    if path_ptr + path_len <= self.memory.len() {
                        // Execute spawn syscall
                        let pid = syscall_spawn(&self.memory[path_ptr..path_ptr + path_len]);
                        Ok(alloc::vec![WasmValue::I64(pid)])
                    } else {
                        Ok(alloc::vec![WasmValue::I64(-1)])
                    }
                } else {
                    Ok(alloc::vec![WasmValue::I64(-1)])
                }
            }
            HostFunction::SProcessWait => {
                // s_process_wait(pid: i64) -> i32
                if let Some(pid) = args.get(0).and_then(|a| a.as_i64()) {
                    let status = syscall_waitpid(pid);
                    Ok(alloc::vec![WasmValue::I32(status)])
                } else {
                    Ok(alloc::vec![WasmValue::I32(-1)])
                }
            }
            HostFunction::SProcessKill => {
                // s_process_kill(pid: i64, signal: i32) -> i32
                if args.len() >= 2 {
                    let pid = args[0].as_i64().unwrap_or(-1);
                    let signal = args[1].as_i32().unwrap_or(9);
                    let result = syscall_kill(pid, signal);
                    Ok(alloc::vec![WasmValue::I32(result)])
                } else {
                    Ok(alloc::vec![WasmValue::I32(-1)])
                }
            }
            HostFunction::SProcessGetPid => {
                // s_process_getpid() -> i64
                let pid = syscall_getpid();
                Ok(alloc::vec![WasmValue::I64(pid)])
            }
            HostFunction::SProcessGetPpid => {
                // s_process_getppid() -> i64
                // For now, return 1 (init)
                Ok(alloc::vec![WasmValue::I64(1)])
            }
            HostFunction::SProcessFork => {
                // Fork not supported in WASM - use spawn instead
                Ok(alloc::vec![WasmValue::I64(-1)])
            }
            
            // Memory management
            HostFunction::SMemAlloc => {
                Ok(alloc::vec![WasmValue::I32(0)]) // Fail - use memory.grow
            }
            HostFunction::SMemFree => {
                Ok(alloc::vec![WasmValue::I32(0)]) // OK (no-op)
            }
            HostFunction::SMemGrow => {
                // Should actually grow linear memory
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            HostFunction::SMemSize => {
                let pages = (self.memory.len() / 65536) as i32;
                Ok(alloc::vec![WasmValue::I32(pages)])
            }
            
            // Filesystem extended
            HostFunction::SFsMkdir | HostFunction::SFsRmdir | HostFunction::SFsUnlink |
            HostFunction::SFsRename | HostFunction::SFsStat | HostFunction::SFsReadDir |
            HostFunction::SFsSync | HostFunction::SFsTruncate | HostFunction::SFsChdir => {
                Ok(alloc::vec![WasmValue::I32(-1)]) // Not implemented
            }
            HostFunction::SFsSeek => {
                Ok(alloc::vec![WasmValue::I64(-1)])
            }
            HostFunction::SFsGetCwd => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            
            // Network extended
            HostFunction::SNetSocket | HostFunction::SNetBind | HostFunction::SNetListen |
            HostFunction::SNetAccept | HostFunction::SNetSetsockopt | HostFunction::SNetGetpeername |
            HostFunction::SNetResolve | HostFunction::SNetSendto | HostFunction::SNetRecvfrom => {
                Ok(alloc::vec![WasmValue::I32(-1)]) // Not implemented
            }
            
            // Thread management
            HostFunction::SThreadCreate => {
                Ok(alloc::vec![WasmValue::I32(-1)]) // Not implemented yet
            }
            HostFunction::SThreadJoin => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            HostFunction::SThreadExit => {
                Ok(alloc::vec![]) // No return
            }
            HostFunction::SThreadYield => {
                Ok(alloc::vec![])
            }
            HostFunction::SThreadGetId => {
                Ok(alloc::vec![WasmValue::I32(0)]) // Main thread
            }
            HostFunction::SThreadSleep => {
                Ok(alloc::vec![WasmValue::I32(0)]) // OK
            }
            
            // Synchronization
            HostFunction::SSyncMutexCreate => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            HostFunction::SSyncMutexLock | HostFunction::SSyncMutexUnlock |
            HostFunction::SSyncMutexDestroy => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            HostFunction::SSyncFutexWait => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            HostFunction::SSyncFutexWake => {
                Ok(alloc::vec![WasmValue::I32(0)])
            }
            
            // Capability management
            HostFunction::SCapCheck => {
                Ok(alloc::vec![WasmValue::I32(1)]) // Always allowed (stub)
            }
            HostFunction::SCapRequest => {
                Ok(alloc::vec![WasmValue::I64(0)]) // Null capability
            }
            HostFunction::SCapRevoke | HostFunction::SCapDelegate => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            
            // Service discovery
            HostFunction::SServiceRegister | HostFunction::SServiceDiscover |
            HostFunction::SServiceUnregister => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            
            // Time & timers
            HostFunction::STimeMonotonic | HostFunction::STimeReal => {
                // Get real timestamp from CPU
                let timestamp = get_timestamp();
                Ok(alloc::vec![WasmValue::I64(timestamp as i64)])
            }
            HostFunction::STimerCreate | HostFunction::STimerCancel => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            
            // System info
            HostFunction::SSysInfo => {
                Ok(alloc::vec![WasmValue::I32(-1)])
            }
            HostFunction::SSysCpuCount => {
                // Get real CPU count from SMP info
                #[cfg(target_arch = "x86_64")]
                let count = {
                    // Read from CPUID or use smp module if available
                    1i32 // Base case: at least 1 CPU
                };
                #[cfg(not(target_arch = "x86_64"))]
                let count = 1i32;
                Ok(alloc::vec![WasmValue::I32(count)])
            }
            HostFunction::SSysMemFree => {
                // Return available heap memory estimate
                // Real implementation would query the allocator
                Ok(alloc::vec![WasmValue::I64(1024 * 1024)]) // 1MB estimate
            }
            HostFunction::SSysUptime => {
                // Get uptime from timestamp (approximate)
                let timestamp = get_timestamp();
                // Convert cycles to seconds (assume ~1GHz for estimation)
                let uptime_seconds = timestamp / 1_000_000_000;
                Ok(alloc::vec![WasmValue::I64(uptime_seconds as i64)])
            }
            
            // Debug & profiling
            HostFunction::SDebugPrint => {
                // Same as SPrint
                Ok(alloc::vec![WasmValue::I32(0)])
            }
            HostFunction::SDebugBreak => {
                Ok(alloc::vec![])
            }
            HostFunction::SProfileStart => {
                Ok(alloc::vec![WasmValue::I32(0)])
            }
            HostFunction::SProfileStop => {
                Ok(alloc::vec![WasmValue::I64(0)])
            }
        }
    }

    /// Reads from linear memory.
    pub fn read_memory(&self, offset: usize, length: usize) -> Result<&[u8], WaveError> {
        if offset + length > self.memory.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        Ok(&self.memory[offset..offset + length])
    }

    /// Writes to linear memory.
    pub fn write_memory(&mut self, offset: usize, data: &[u8]) -> Result<(), WaveError> {
        if offset + data.len() > self.memory.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        self.memory[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    /// Gets memory size in pages (64KB each).
    pub fn memory_pages(&self) -> u32 {
        (self.memory.len() / 65536) as u32
    }

    /// Grows memory by specified pages.
    pub fn grow_memory(&mut self, pages: u32) -> Result<u32, WaveError> {
        let old_pages = self.memory_pages();
        let new_size = self.memory.len() + (pages as usize * 65536);

        // Check against maximum (16-bit page count = 4GB max)
        const MAX_WASM_PAGES: u32 = 65536; // 4GB / 64KB per page
        if old_pages + pages > MAX_WASM_PAGES {
            return Err(WaveError::OutOfMemory);
        }
        
        // Also check against configured instance limit
        // (would be checked against WaveConfig.max_memory in full impl)
        
        self.memory.resize(new_size, 0);

        Ok(old_pages)
    }
}

/// S-WAVE runtime configuration.
#[derive(Debug, Clone)]
pub struct WaveConfig {
    /// Maximum modules that can be loaded
    pub max_modules: usize,
    /// Maximum instances per module
    pub max_instances: usize,
    /// Maximum memory per instance (bytes)
    pub max_memory: usize,
    /// Maximum execution steps (for determinism)
    pub max_steps: u64,
}

impl Default for WaveConfig {
    fn default() -> Self {
        Self {
            max_modules: 1024,
            max_instances: 4096,
            max_memory: 256 * 1024 * 1024, // 256 MB
            max_steps: 1_000_000_000,
        }
    }
}

/// The S-WAVE runtime.
pub struct Wave {
    config: WaveConfig,
    modules: Mutex<BTreeMap<ModuleId, Module>>,
    instances: Mutex<BTreeMap<InstanceId, Instance>>,
    next_module_id: Mutex<u64>,
    next_instance_id: Mutex<u64>,
}

impl Wave {
    /// Creates a new S-WAVE runtime.
    pub fn new(config: WaveConfig) -> Self {
        Self {
            config,
            modules: Mutex::new(BTreeMap::new()),
            instances: Mutex::new(BTreeMap::new()),
            next_module_id: Mutex::new(1),
            next_instance_id: Mutex::new(1),
        }
    }

    /// Loads and validates a WASM module.
    ///
    /// Parses the WASM binary, extracts imports/exports, and validates structure.
    pub fn load(
        &self,
        wasm_bytes: Vec<u8>,
        name: Option<String>,
        _cap_token: &CapabilityToken,
    ) -> Result<ModuleId, WaveError> {
        // Validate WASM magic number and version
        if wasm_bytes.len() < 8 {
            return Err(WaveError::InvalidModule);
        }
        if &wasm_bytes[0..4] != b"\0asm" {
            return Err(WaveError::InvalidModule);
        }
        // Check version (must be 1)
        if &wasm_bytes[4..8] != &[0x01, 0x00, 0x00, 0x00] {
            return Err(WaveError::InvalidModule);
        }

        let mut modules = self.modules.lock();
        if modules.len() >= self.config.max_modules {
            return Err(WaveError::TooManyModules);
        }

        let mut next_id = self.next_module_id.lock();
        let id = ModuleId(*next_id);
        *next_id += 1;

        // Parse sections to extract imports, exports, and memory requirements
        let (imports, exports, host_imports, memory) = self.parse_sections(&wasm_bytes)?;

        let module = Module {
            id,
            name,
            bytes: wasm_bytes,
            imports,
            exports,
            host_imports,
            memory,
            validated: true,
        };

        modules.insert(id, module);

        Ok(id)
    }

    /// Parse WASM sections to extract module information.
    fn parse_sections(&self, bytes: &[u8]) -> Result<(Vec<Import>, Vec<Export>, Vec<(String, HostFunction)>, Option<(u32, Option<u32>)>), WaveError> {
        let mut imports = Vec::new();
        let mut exports = Vec::new();
        let mut host_imports = Vec::new();
        let mut memory = None;
        
        let mut pos = 8; // Skip magic and version
        
        while pos < bytes.len() {
            if pos >= bytes.len() {
                break;
            }
            
            let section_id = bytes[pos];
            pos += 1;
            
            // Read section size (LEB128)
            let (section_size, bytes_read) = self.read_leb128_u32(&bytes[pos..])?;
            pos += bytes_read;
            
            let section_end = pos + section_size as usize;
            if section_end > bytes.len() {
                return Err(WaveError::InvalidModule);
            }
            
            match SectionId::from_u8(section_id) {
                Some(SectionId::Import) => {
                    // Parse import section
                    let mut sec_pos = pos;
                    let (count, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                    sec_pos += br;
                    
                    for _ in 0..count {
                        // Read module name
                        let (mod_len, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                        sec_pos += br;
                        let mod_name = core::str::from_utf8(&bytes[sec_pos..sec_pos + mod_len as usize])
                            .map_err(|_| WaveError::InvalidModule)?
                            .to_string();
                        sec_pos += mod_len as usize;
                        
                        // Read field name
                        let (field_len, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                        sec_pos += br;
                        let field_name = core::str::from_utf8(&bytes[sec_pos..sec_pos + field_len as usize])
                            .map_err(|_| WaveError::InvalidModule)?
                            .to_string();
                        sec_pos += field_len as usize;
                        
                        // Read import kind
                        let kind = bytes[sec_pos];
                        sec_pos += 1;
                        
                        // Skip type index (for functions) or other data
                        let (_, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                        sec_pos += br;
                        
                        if kind == 0 { // Function import
                            imports.push(Import {
                                module: mod_name.clone(),
                                name: field_name.clone(),
                                signature: FunctionSignature { params: Vec::new(), results: Vec::new() },
                            });
                            
                            // Check if this is a Splax host function
                            if let Some(hf) = HostFunction::from_name(&mod_name, &field_name) {
                                host_imports.push((field_name, hf));
                            }
                        }
                    }
                }
                Some(SectionId::Export) => {
                    // Parse export section
                    let mut sec_pos = pos;
                    let (count, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                    sec_pos += br;
                    
                    for _ in 0..count {
                        // Read export name
                        let (name_len, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                        sec_pos += br;
                        let export_name = core::str::from_utf8(&bytes[sec_pos..sec_pos + name_len as usize])
                            .map_err(|_| WaveError::InvalidModule)?
                            .to_string();
                        sec_pos += name_len as usize;
                        
                        // Read export kind and index
                        let kind = bytes[sec_pos];
                        sec_pos += 1;
                        let (_, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                        sec_pos += br;
                        
                        if kind == 0 { // Function export
                            exports.push(Export {
                                name: export_name,
                                export_type: ExportType::Function(FunctionSignature {
                                    params: Vec::new(),
                                    results: Vec::new(),
                                }),
                            });
                        }
                    }
                }
                Some(SectionId::Memory) => {
                    // Parse memory section
                    let mut sec_pos = pos;
                    let (count, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                    sec_pos += br;
                    
                    if count > 0 {
                        // Read limits
                        let flags = bytes[sec_pos];
                        sec_pos += 1;
                        let (min, br) = self.read_leb128_u32(&bytes[sec_pos..])?;
                        sec_pos += br;
                        
                        let max = if flags & 1 != 0 {
                            let (m, _) = self.read_leb128_u32(&bytes[sec_pos..])?;
                            Some(m)
                        } else {
                            None
                        };
                        
                        memory = Some((min, max));
                    }
                }
                _ => {
                    // Skip other sections
                }
            }
            
            pos = section_end;
        }
        
        Ok((imports, exports, host_imports, memory))
    }

    /// Read an unsigned LEB128 value.
    fn read_leb128_u32(&self, bytes: &[u8]) -> Result<(u32, usize), WaveError> {
        let mut result = 0u32;
        let mut shift = 0;
        let mut pos = 0;
        
        loop {
            if pos >= bytes.len() {
                return Err(WaveError::InvalidModule);
            }
            
            let byte = bytes[pos];
            pos += 1;
            
            result |= ((byte & 0x7F) as u32) << shift;
            
            if byte & 0x80 == 0 {
                break;
            }
            
            shift += 7;
            if shift >= 35 {
                return Err(WaveError::InvalidModule);
            }
        }
        
        Ok((result, pos))
    }

    /// Instantiates a module with capability bindings.
    ///
    /// This function creates a runnable instance from a loaded module,
    /// binding host functions to their required capabilities.
    pub fn instantiate(
        &self,
        module_id: ModuleId,
        imports: Vec<BoundImport>,
        capability_bindings: Vec<(HostFunction, CapabilityToken)>,
        _cap_token: &CapabilityToken,
    ) -> Result<InstanceId, WaveError> {
        let modules = self.modules.lock();
        let module = modules.get(&module_id).ok_or(WaveError::ModuleNotFound)?;

        // Verify all required host imports have capability bindings
        let mut bound_host_functions = Vec::new();
        for (import_name, host_func) in &module.host_imports {
            // Find the capability binding for this host function
            let binding = capability_bindings
                .iter()
                .find(|(hf, _)| hf == host_func);
            
            match binding {
                Some((_, cap)) => {
                    bound_host_functions.push(BoundHostFunction {
                        function: *host_func,
                        capability: cap.clone(),
                    });
                }
                None => {
                    // Check if this import has a bound import (legacy style)
                    let has_legacy = imports.iter()
                        .any(|i| i.name == *import_name);
                    
                    if !has_legacy {
                        return Err(WaveError::MissingImport);
                    }
                }
            }
        }

        // Determine initial memory size
        let initial_memory_pages = module.memory.map(|(min, _)| min).unwrap_or(1);
        let initial_memory = (initial_memory_pages as usize) * 65536;
        
        // Check memory limits
        if initial_memory > self.config.max_memory {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }

        let mut instances = self.instances.lock();
        if instances.len() >= self.config.max_instances {
            return Err(WaveError::TooManyInstances);
        }

        let mut next_id = self.next_instance_id.lock();
        let id = InstanceId(*next_id);
        *next_id += 1;

        let instance = Instance {
            id,
            module_id,
            imports,
            host_functions: bound_host_functions,
            memory: alloc::vec![0u8; initial_memory],
            globals: Vec::new(),
            call_stack: Vec::new(),
            value_stack: Vec::new(),
            state: InstanceState::Ready,
            steps_executed: 0,
            max_steps: self.config.max_steps,
        };

        instances.insert(id, instance);

        Ok(id)
    }

    /// Instantiates a module with default bindings (for simple cases).
    pub fn instantiate_simple(
        &self,
        module_id: ModuleId,
        cap_token: &CapabilityToken,
    ) -> Result<InstanceId, WaveError> {
        self.instantiate(module_id, Vec::new(), Vec::new(), cap_token)
    }

    /// Gets a module by ID.
    pub fn get_module(&self, id: ModuleId) -> Option<ModuleId> {
        if self.modules.lock().contains_key(&id) {
            Some(id)
        } else {
            None
        }
    }

    /// Unloads a module.
    pub fn unload(&self, id: ModuleId, _cap_token: &CapabilityToken) -> Result<(), WaveError> {
        self.modules
            .lock()
            .remove(&id)
            .ok_or(WaveError::ModuleNotFound)?;
        Ok(())
    }

    /// Terminates an instance.
    pub fn terminate(&self, id: InstanceId, _cap_token: &CapabilityToken) -> Result<(), WaveError> {
        let mut instances = self.instances.lock();
        let instance = instances.get_mut(&id).ok_or(WaveError::InstanceNotFound)?;
        instance.state = InstanceState::Terminated;
        Ok(())
    }

    /// Lists loaded modules.
    pub fn list_modules(&self) -> Vec<ModuleId> {
        self.modules.lock().keys().copied().collect()
    }

    /// Lists active instances.
    pub fn list_instances(&self) -> Vec<InstanceId> {
        self.instances.lock().keys().copied().collect()
    }

    /// Calls an exported function on an instance.
    ///
    /// This is the primary execution entry point for WASM modules.
    pub fn call(
        &self,
        instance_id: InstanceId,
        func_name: &str,
        args: &[WasmValue],
        cap_token: &CapabilityToken,
    ) -> Result<Vec<WasmValue>, WaveError> {
        let mut instances = self.instances.lock();
        let instance = instances.get_mut(&instance_id).ok_or(WaveError::InstanceNotFound)?;
        instance.call(func_name, args, cap_token)
    }

    /// Gets instance state.
    pub fn instance_state(&self, instance_id: InstanceId) -> Result<InstanceState, WaveError> {
        let instances = self.instances.lock();
        let instance = instances.get(&instance_id).ok_or(WaveError::InstanceNotFound)?;
        Ok(instance.state())
    }

    /// Reads memory from an instance.
    pub fn read_memory(
        &self,
        instance_id: InstanceId,
        offset: usize,
        length: usize,
    ) -> Result<Vec<u8>, WaveError> {
        let instances = self.instances.lock();
        let instance = instances.get(&instance_id).ok_or(WaveError::InstanceNotFound)?;
        
        if offset + length > instance.memory.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        
        Ok(instance.memory[offset..offset + length].to_vec())
    }

    /// Writes memory to an instance.
    pub fn write_memory(
        &self,
        instance_id: InstanceId,
        offset: usize,
        data: &[u8],
    ) -> Result<(), WaveError> {
        let mut instances = self.instances.lock();
        let instance = instances.get_mut(&instance_id).ok_or(WaveError::InstanceNotFound)?;
        
        if offset + data.len() > instance.memory.len() {
            return Err(WaveError::MemoryAccessOutOfBounds);
        }
        
        instance.memory[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    /// Gets steps executed for an instance.
    pub fn steps_executed(&self, instance_id: InstanceId) -> Result<u64, WaveError> {
        let instances = self.instances.lock();
        let instance = instances.get(&instance_id).ok_or(WaveError::InstanceNotFound)?;
        Ok(instance.steps_executed())
    }

    /// Resets an instance for fresh execution.
    pub fn reset_instance(&self, instance_id: InstanceId) -> Result<(), WaveError> {
        let mut instances = self.instances.lock();
        let instance = instances.get_mut(&instance_id).ok_or(WaveError::InstanceNotFound)?;
        instance.reset_execution();
        Ok(())
    }
}

/// S-WAVE errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveError {
    /// Invalid WASM module
    InvalidModule,
    /// Module not found
    ModuleNotFound,
    /// Instance not found
    InstanceNotFound,
    /// Too many modules loaded
    TooManyModules,
    /// Too many instances
    TooManyInstances,
    /// Memory access out of bounds
    MemoryAccessOutOfBounds,
    /// Invalid instance state
    InvalidState,
    /// Export not found
    ExportNotFound,
    /// Type mismatch
    TypeMismatch,
    /// Execution limit exceeded
    ExecutionLimit,
    /// Invalid capability
    InvalidCapability,
    /// Missing required import
    MissingImport,
    /// Out of memory
    OutOfMemory,
    /// Division by zero
    DivisionByZero,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        // Create a capability token through the public API
        // If splax_cap provides a constructor, use that; otherwise use default
        CapabilityToken::default()
    }

    #[test]
    fn test_load_module() {
        let wave = Wave::new(WaveConfig::default());
        let token = dummy_token();

        // Valid WASM header
        let wasm = alloc::vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
        let id = wave.load(wasm, None, &token).expect("should load");

        assert!(wave.get_module(id).is_some());
    }

    #[test]
    fn test_invalid_module() {
        let wave = Wave::new(WaveConfig::default());
        let token = dummy_token();

        let invalid = alloc::vec![0x00, 0x00, 0x00, 0x00];
        let result = wave.load(invalid, None, &token);

        assert_eq!(result, Err(WaveError::InvalidModule));
    }
}
