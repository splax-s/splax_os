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
}

impl HostFunction {
    /// Get the host function by import name.
    pub fn from_name(module: &str, name: &str) -> Option<Self> {
        if module != "splax" {
            return None;
        }
        match name {
            "s_link_send" => Some(Self::SLinkSend),
            "s_link_receive" => Some(Self::SLinkReceive),
            "s_storage_read" => Some(Self::SStorageRead),
            "s_storage_write" => Some(Self::SStorageWrite),
            "s_log" => Some(Self::SLog),
            "s_time_now" => Some(Self::STimeNow),
            "s_sleep" => Some(Self::SSleep),
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
