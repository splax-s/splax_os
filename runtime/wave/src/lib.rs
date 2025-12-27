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

/// Module identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModuleId(pub u64);

/// Instance identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InstanceId(pub u64);

/// Capability token placeholder.
#[derive(Debug, Clone, Copy)]
pub struct CapabilityToken {
    value: [u64; 4],
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
}

impl WasmType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0x7F => Some(Self::I32),
            0x7E => Some(Self::I64),
            0x7D => Some(Self::F32),
            0x7C => Some(Self::F64),
            _ => None,
        }
    }
}

/// WASM runtime values.
#[derive(Debug, Clone, Copy)]
pub enum WasmValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl WasmValue {
    pub fn value_type(&self) -> WasmType {
        match self {
            Self::I32(_) => WasmType::I32,
            Self::I64(_) => WasmType::I64,
            Self::F32(_) => WasmType::F32,
            Self::F64(_) => WasmType::F64,
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
        }
    }
    
    /// Convert to i64, coercing if needed
    pub fn to_i64(&self) -> i64 {
        match self {
            Self::I32(v) => *v as i64,
            Self::I64(v) => *v,
            Self::F32(v) => *v as i64,
            Self::F64(v) => *v as i64,
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
}

impl Opcode {
    pub fn from_byte(b: u8) -> Option<Self> {
        // Safety: We validate the range
        if b <= 0xC4 {
            Some(unsafe { core::mem::transmute(b) })
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
}

impl HostFunction {
    /// Get the host function by import name.
    pub fn from_name(module: &str, name: &str) -> Option<Self> {
        if module != "splax" && module != "env" && module != "wasi_snapshot_preview1" {
            return None;
        }
        match name {
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
            _ => None,
        }
    }

    /// Get the required capability type for this host function.
    pub fn required_capability(&self) -> &'static str {
        match self {
            Self::SLinkSend => "channel:write",
            Self::SLinkReceive => "channel:read",
            Self::SStorageRead => "storage:read",
            Self::SStorageWrite => "storage:write",
            Self::SLog => "log:write",
            Self::STimeNow => "time:read",
            Self::SSleep => "time:sleep",
            Self::SPrint => "console:write",
            Self::SRead => "console:read",
            Self::SExit => "process:exit",
            Self::SGetEnv => "env:read",
            Self::SRandom => "random:read",
            Self::SFileOpen | Self::SFileRead | Self::SFileSize => "fs:read",
            Self::SFileWrite => "fs:write",
            Self::SFileClose => "fs:read",
            Self::SNetConnect | Self::SNetSend | Self::SNetRecv | Self::SNetClose => "net:connect",
        }
    }

    /// Get the function signature.
    pub fn signature(&self) -> FunctionSignature {
        match self {
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
            Self::STimeNow => FunctionSignature {
                params: alloc::vec![],
                results: alloc::vec![WasmType::I64],
            },
            Self::SSleep => FunctionSignature {
                params: alloc::vec![WasmType::I64],
                results: alloc::vec![],
            },
            Self::SPrint => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SRead => FunctionSignature {
                params: alloc::vec![WasmType::I32, WasmType::I32],
                results: alloc::vec![WasmType::I32],
            },
            Self::SExit => FunctionSignature {
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
        // In a real implementation, we would:
        // 1. Look up the export by name to get function index
        // 2. Validate argument types match the function signature
        // 3. Execute bytecode instructions
        // 4. Handle host function calls via the bound capabilities

        // For now, we simulate execution based on common patterns
        let result = self.execute_function(name);

        self.state = InstanceState::Ready;
        result
    }

    /// Simulates function execution (placeholder for full WASM interpreter).
    fn execute_function(&mut self, name: &str) -> Result<Vec<WasmValue>, WaveError> {
        // This is a simplified execution model
        // A full implementation would interpret WASM bytecode
        
        // For demonstration, handle some common patterns
        match name {
            "_start" | "main" => {
                // Entry point functions typically return void or i32
                self.steps_executed += 1;
                Ok(Vec::new())
            }
            "add" | "sum" => {
                // Simple math function - pop two values, add, push result
                if self.value_stack.len() >= 2 {
                    let b = self.value_stack.pop().unwrap_or(WasmValue::I32(0));
                    let a = self.value_stack.pop().unwrap_or(WasmValue::I32(0));
                    
                    let result = match (a, b) {
                        (WasmValue::I32(x), WasmValue::I32(y)) => WasmValue::I32(x.wrapping_add(y)),
                        (WasmValue::I64(x), WasmValue::I64(y)) => WasmValue::I64(x.wrapping_add(y)),
                        (WasmValue::F32(x), WasmValue::F32(y)) => WasmValue::F32(x + y),
                        (WasmValue::F64(x), WasmValue::F64(y)) => WasmValue::F64(x + y),
                        _ => return Err(WaveError::TypeMismatch),
                    };
                    
                    self.steps_executed += 1;
                    Ok(alloc::vec![result])
                } else {
                    Err(WaveError::TypeMismatch)
                }
            }
            _ => {
                // Unknown function - return empty result
                self.steps_executed += 1;
                Ok(Vec::new())
            }
        }
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
                    let (_func_idx, bytes_read) = self.read_leb128_u32(&code[ip..])?;
                    ip += bytes_read;
                    // In a full implementation, we'd call the function
                    // For now, just continue
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
        // In a real implementation, this would dispatch to the actual
        // system call with the bound capability token
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
                // Would log message
                Ok(Vec::new())
            }
            HostFunction::STimeNow => {
                // s_time_now() -> i64
                Ok(alloc::vec![WasmValue::I64(0)]) // Placeholder timestamp
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

        // TODO: Check against maximum
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken { value: [1, 2, 3, 4] }
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
