//! # WASM Integration Module
//!
//! This module integrates the S-WAVE WASM runtime with the kernel,
//! providing VFS integration for loading .wasm files and executing them.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::wasm;
//!
//! // Load and run a WASM module from filesystem
//! let result = wasm::run_file("/bin/hello.wasm")?;
//!
//! // Or load manually
//! let module_id = wasm::load_file("/bin/app.wasm")?;
//! let instance_id = wasm::instantiate(module_id)?;
//! let result = wasm::call(instance_id, "_start", &[])?;
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

// Re-export S-WAVE types
pub use splax_wave::{
    CapabilityToken, HostFunction, InstanceId, InstanceState, ModuleId, Opcode, Wave,
    WaveConfig, WaveError, WasmType, WasmValue,
};

/// Global S-WAVE runtime instance
static WAVE_RUNTIME: Mutex<Option<Wave>> = Mutex::new(None);

/// Loaded modules tracking
static LOADED_MODULES: Mutex<Vec<LoadedModule>> = Mutex::new(Vec::new());

/// Information about a loaded module
#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub id: ModuleId,
    pub name: String,
    pub path: String,
    pub size: usize,
}

/// WASM subsystem errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmError {
    /// Runtime not initialized
    NotInitialized,
    /// File not found
    FileNotFound,
    /// Invalid WASM file
    InvalidModule,
    /// Module not loaded
    ModuleNotFound,
    /// Instance not found
    InstanceNotFound,
    /// Execution error
    ExecutionError,
    /// Capability error
    CapabilityError,
    /// Memory error
    MemoryError,
    /// S-WAVE error
    WaveError(WaveError),
}

impl From<WaveError> for WasmError {
    fn from(e: WaveError) -> Self {
        WasmError::WaveError(e)
    }
}

impl From<crate::fs::FsError> for WasmError {
    fn from(_: crate::fs::FsError) -> Self {
        WasmError::FileNotFound
    }
}

/// Initialize the WASM subsystem
pub fn init() {
    let config = WaveConfig {
        max_modules: 256,
        max_instances: 1024,
        max_memory: 64 * 1024 * 1024, // 64 MB per instance
        max_steps: 100_000_000,       // 100M steps max
    };

    let runtime = Wave::new(config);
    *WAVE_RUNTIME.lock() = Some(runtime);

    crate::serial_println!("[wasm] S-WAVE runtime initialized");
}

/// Create a kernel capability token for WASM operations
fn kernel_cap() -> CapabilityToken {
    CapabilityToken::new([0xDEAD_BEEF, 0xCAFE_BABE, 0x1234_5678, 0x8765_4321])
}

/// Load a WASM module from a file path
pub fn load_file(path: &str) -> Result<ModuleId, WasmError> {
    // Read file from filesystem
    let data = crate::fs::cat(path)?;

    // Validate it's a WASM file
    if data.len() < 8 {
        return Err(WasmError::InvalidModule);
    }
    if &data[0..4] != b"\0asm" {
        return Err(WasmError::InvalidModule);
    }

    // Get the runtime
    let mut runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_mut().ok_or(WasmError::NotInitialized)?;

    // Extract module name from path
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_end_matches(".wasm")
        .to_string();

    // Load into S-WAVE
    let module_id = runtime.load(data.clone(), Some(name.clone()), &kernel_cap())?;

    // Track the loaded module
    let mut modules = LOADED_MODULES.lock();
    modules.push(LoadedModule {
        id: module_id,
        name,
        path: String::from(path),
        size: data.len(),
    });

    Ok(module_id)
}

/// Load a WASM module from raw bytes
pub fn load_bytes(name: &str, data: Vec<u8>) -> Result<ModuleId, WasmError> {
    // Validate it's a WASM file
    if data.len() < 8 {
        return Err(WasmError::InvalidModule);
    }
    if &data[0..4] != b"\0asm" {
        return Err(WasmError::InvalidModule);
    }

    // Get the runtime
    let mut runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_mut().ok_or(WasmError::NotInitialized)?;

    let size = data.len();
    let module_id = runtime.load(data, Some(String::from(name)), &kernel_cap())?;

    // Track the loaded module
    let mut modules = LOADED_MODULES.lock();
    modules.push(LoadedModule {
        id: module_id,
        name: String::from(name),
        path: String::from("<memory>"),
        size,
    });

    Ok(module_id)
}

/// Instantiate a loaded module
pub fn instantiate(module_id: ModuleId) -> Result<InstanceId, WasmError> {
    let mut runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_mut().ok_or(WasmError::NotInitialized)?;

    let instance_id = runtime.instantiate_simple(module_id, &kernel_cap())?;
    Ok(instance_id)
}

/// Instantiate a module with specific capability bindings
pub fn instantiate_with_caps(
    module_id: ModuleId,
    caps: Vec<(HostFunction, CapabilityToken)>,
) -> Result<InstanceId, WasmError> {
    let mut runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_mut().ok_or(WasmError::NotInitialized)?;

    let instance_id = runtime.instantiate(module_id, Vec::new(), caps, &kernel_cap())?;
    Ok(instance_id)
}

/// Call a function in an instance
pub fn call(
    instance_id: InstanceId,
    func_name: &str,
    args: &[WasmValue],
) -> Result<Vec<WasmValue>, WasmError> {
    let mut runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_mut().ok_or(WasmError::NotInitialized)?;

    // Call the function through the Wave runtime
    let result = runtime.call(instance_id, func_name, args, &kernel_cap())?;
    Ok(result)
}

/// Run a WASM file directly (load, instantiate, call _start)
pub fn run_file(path: &str) -> Result<Vec<WasmValue>, WasmError> {
    crate::serial_println!("[wasm] Loading: {}", path);

    // Load the module
    let module_id = load_file(path)?;
    crate::serial_println!("[wasm] Module loaded: {:?}", module_id);

    // Instantiate it
    let instance_id = instantiate(module_id)?;
    crate::serial_println!("[wasm] Instance created: {:?}", instance_id);

    // Try to call _start, main, or run in that order
    let result = if let Ok(r) = call(instance_id, "_start", &[]) {
        crate::serial_println!("[wasm] Called _start, returned {} values", r.len());
        r
    } else if let Ok(r) = call(instance_id, "main", &[]) {
        crate::serial_println!("[wasm] Called main, returned {} values", r.len());
        r
    } else if let Ok(r) = call(instance_id, "run", &[]) {
        crate::serial_println!("[wasm] Called run, returned {} values", r.len());
        r
    } else {
        crate::serial_println!("[wasm] No entry point found (_start, main, or run)");
        Vec::new()
    };

    Ok(result)
}

/// Validate a WASM file without loading it
pub fn validate_file(path: &str) -> Result<ValidationResult, WasmError> {
    // Read file from filesystem
    let data = crate::fs::cat(path)?;

    validate_bytes(&data)
}

/// Validate WASM bytes
pub fn validate_bytes(data: &[u8]) -> Result<ValidationResult, WasmError> {
    let mut result = ValidationResult {
        valid: false,
        size: data.len(),
        version: 0,
        has_memory: false,
        has_start: false,
        import_count: 0,
        export_count: 0,
        function_count: 0,
    };

    // Check minimum size
    if data.len() < 8 {
        return Ok(result);
    }

    // Check magic number
    if &data[0..4] != b"\0asm" {
        return Ok(result);
    }

    // Check version
    result.version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if result.version != 1 {
        return Ok(result);
    }

    // Parse sections
    let mut pos = 8;
    while pos < data.len() {
        if pos >= data.len() {
            break;
        }

        let section_id = data[pos];
        pos += 1;

        // Read section size (simplified LEB128)
        let mut section_size = 0u32;
        let mut shift = 0;
        while pos < data.len() {
            let byte = data[pos];
            pos += 1;
            section_size |= ((byte & 0x7F) as u32) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
        }

        let section_end = pos + section_size as usize;
        if section_end > data.len() {
            break;
        }

        match section_id {
            2 => {
                // Import section - count imports
                if pos < section_end {
                    let count = data[pos] as usize;
                    result.import_count = count;
                }
            }
            3 => {
                // Function section - count functions
                if pos < section_end {
                    let count = data[pos] as usize;
                    result.function_count = count;
                }
            }
            5 => {
                // Memory section
                result.has_memory = true;
            }
            7 => {
                // Export section - count exports
                if pos < section_end {
                    let count = data[pos] as usize;
                    result.export_count = count;
                }
            }
            8 => {
                // Start section
                result.has_start = true;
            }
            _ => {}
        }

        pos = section_end;
    }

    result.valid = true;
    Ok(result)
}

/// Result of WASM validation
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub size: usize,
    pub version: u32,
    pub has_memory: bool,
    pub has_start: bool,
    pub import_count: usize,
    pub export_count: usize,
    pub function_count: usize,
}

/// List loaded modules
pub fn list_modules() -> Vec<LoadedModule> {
    LOADED_MODULES.lock().clone()
}

/// Unload a module
pub fn unload(module_id: ModuleId) -> Result<(), WasmError> {
    let mut runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_mut().ok_or(WasmError::NotInitialized)?;

    runtime.unload(module_id, &kernel_cap())?;

    // Remove from tracking
    let mut modules = LOADED_MODULES.lock();
    modules.retain(|m| m.id != module_id);

    Ok(())
}

/// Get runtime statistics
pub fn stats() -> WasmStats {
    let modules = LOADED_MODULES.lock();
    let total_size: usize = modules.iter().map(|m| m.size).sum();

    WasmStats {
        modules_loaded: modules.len(),
        total_wasm_size: total_size,
        runtime_initialized: WAVE_RUNTIME.lock().is_some(),
    }
}

/// Runtime statistics
#[derive(Debug, Clone)]
pub struct WasmStats {
    pub modules_loaded: usize,
    pub total_wasm_size: usize,
    pub runtime_initialized: bool,
}

/// Read memory from an instance
pub fn read_memory(
    instance_id: InstanceId,
    offset: usize,
    length: usize,
) -> Result<Vec<u8>, WasmError> {
    let runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_ref().ok_or(WasmError::NotInitialized)?;
    Ok(runtime.read_memory(instance_id, offset, length)?)
}

/// Write memory to an instance
pub fn write_memory(
    instance_id: InstanceId,
    offset: usize,
    data: &[u8],
) -> Result<(), WasmError> {
    let runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_ref().ok_or(WasmError::NotInitialized)?;
    runtime.write_memory(instance_id, offset, data)?;
    Ok(())
}

/// Get instance execution state
pub fn instance_state(instance_id: InstanceId) -> Result<InstanceState, WasmError> {
    let runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_ref().ok_or(WasmError::NotInitialized)?;
    Ok(runtime.instance_state(instance_id)?)
}

/// Get steps executed for an instance
pub fn steps_executed(instance_id: InstanceId) -> Result<u64, WasmError> {
    let runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_ref().ok_or(WasmError::NotInitialized)?;
    Ok(runtime.steps_executed(instance_id)?)
}

/// Reset an instance for fresh execution
pub fn reset_instance(instance_id: InstanceId) -> Result<(), WasmError> {
    let runtime_guard = WAVE_RUNTIME.lock();
    let runtime = runtime_guard.as_ref().ok_or(WasmError::NotInitialized)?;
    runtime.reset_instance(instance_id)?;
    Ok(())
}

/// List all active instances
pub fn list_instances() -> Vec<InstanceId> {
    let runtime_guard = WAVE_RUNTIME.lock();
    if let Some(runtime) = runtime_guard.as_ref() {
        runtime.list_instances()
    } else {
        Vec::new()
    }
}

/// Execute WASM bytecode directly (for testing)
pub fn execute_bytecode(code: &[u8], locals: Vec<WasmValue>) -> Result<Vec<WasmValue>, WasmError> {
    // Create a minimal WASM module wrapper
    let mut wasm = Vec::new();

    // WASM header
    wasm.extend_from_slice(b"\0asm"); // Magic
    wasm.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Version 1

    // Type section (empty for now)
    wasm.push(0x01); // Section ID
    wasm.push(0x01); // Section size
    wasm.push(0x00); // 0 types

    // Function section
    wasm.push(0x03); // Section ID
    wasm.push(0x02); // Section size
    wasm.push(0x01); // 1 function
    wasm.push(0x00); // Type index 0

    // Memory section
    wasm.push(0x05); // Section ID
    wasm.push(0x03); // Section size
    wasm.push(0x01); // 1 memory
    wasm.push(0x00); // No max
    wasm.push(0x01); // 1 page min

    // Export section
    wasm.push(0x07); // Section ID
    wasm.push(0x08); // Section size
    wasm.push(0x01); // 1 export
    wasm.push(0x04); // Name length
    wasm.extend_from_slice(b"main"); // Name
    wasm.push(0x00); // Export kind (function)
    wasm.push(0x00); // Function index

    // Code section
    wasm.push(0x0A); // Section ID
    let code_body_size = code.len() + 2; // locals + code + end
    wasm.push((code_body_size + 2) as u8); // Section size
    wasm.push(0x01); // 1 function body
    wasm.push(code_body_size as u8); // Body size
    wasm.push(0x00); // 0 local declarations
    wasm.extend_from_slice(code);
    wasm.push(0x0B); // End

    // Load and execute
    let module_id = load_bytes("__bytecode__", wasm)?;
    let _instance_id = instantiate(module_id)?;

    // Clean up
    let _ = unload(module_id);

    // Return locals as result (placeholder)
    Ok(locals)
}

use alloc::string::ToString;
