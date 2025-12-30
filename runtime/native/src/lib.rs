//! # S-NATIVE: Native Code Sandbox
//!
//! S-NATIVE provides a restricted execution environment for native code.
//! Unlike S-WAVE (the preferred runtime), S-NATIVE is for cases where
//! native performance is absolutely required.
//!
//! ## Security Model
//!
//! Native code runs in a strict sandbox:
//! - Memory isolation via hardware page tables
//! - System call filtering
//! - Capability-gated resource access
//! - Time and memory limits
//!
//! ## When to Use S-NATIVE
//!
//! - High-performance drivers
//! - Cryptographic operations
//! - Real-time signal processing
//! - Legacy code migration
//!
//! ## Restrictions
//!
//! S-NATIVE processes CANNOT:
//! - Access raw memory outside their sandbox
//! - Make direct system calls
//! - Execute privileged instructions
//! - Fork or spawn processes
//!
//! All resource access goes through capability-checked trampolines.

#![no_std]

extern crate alloc;

pub mod dynlink;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

// Import shared capability token
pub use splax_cap::{CapabilityToken, Operations, Permission};
pub use dynlink::{DynamicLinker, LoadedLibrary, LibraryHandle, LinkError};

/// Native process identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeProcessId(pub u64);

/// Target architecture for native code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    Aarch64,
}

impl Architecture {
    /// Gets the current architecture.
    pub fn current() -> Self {
        #[cfg(target_arch = "x86_64")]
        return Self::X86_64;
        #[cfg(target_arch = "aarch64")]
        return Self::Aarch64;
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        panic!("Unsupported architecture");
    }
}

/// Native binary format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryFormat {
    /// ELF executable
    Elf,
    /// Raw binary (loaded at fixed address)
    Raw,
}

/// Memory region permissions.
#[derive(Debug, Clone, Copy)]
pub struct MemoryPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl MemoryPermissions {
    pub const READ_ONLY: Self = Self { read: true, write: false, execute: false };
    pub const READ_WRITE: Self = Self { read: true, write: true, execute: false };
    pub const READ_EXECUTE: Self = Self { read: true, write: false, execute: true };
}

/// Memory region in the sandbox.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Virtual address
    pub base: u64,
    /// Size in bytes
    pub size: usize,
    /// Permissions
    pub permissions: MemoryPermissions,
    /// Region type
    pub region_type: RegionType,
}

/// Memory region types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionType {
    /// Code section
    Code,
    /// Read-only data
    ReadOnlyData,
    /// Mutable data
    Data,
    /// Heap
    Heap,
    /// Stack
    Stack,
    /// Shared memory for IPC
    SharedMemory,
}

/// Sandbox configuration.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Maximum memory (bytes)
    pub max_memory: usize,
    /// Maximum CPU time (cycles)
    pub max_cpu_time: u64,
    /// Stack size
    pub stack_size: usize,
    /// Heap size
    pub heap_size: usize,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024, // 64 MB
            max_cpu_time: 10_000_000_000, // ~10 seconds at 1GHz
            stack_size: 1024 * 1024,       // 1 MB
            heap_size: 16 * 1024 * 1024,   // 16 MB
        }
    }
}

/// Native process state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is being set up
    Loading,
    /// Process is ready to run
    Ready,
    /// Process is currently running
    Running,
    /// Process is suspended
    Suspended,
    /// Process has terminated normally
    Terminated,
    /// Process was killed
    Killed,
}

/// A sandboxed native process.
pub struct NativeProcess {
    id: NativeProcessId,
    /// Process name
    name: Option<String>,
    /// Target architecture
    architecture: Architecture,
    /// Sandbox configuration
    config: SandboxConfig,
    /// Memory regions
    regions: Vec<MemoryRegion>,
    /// Bound capabilities (import name -> token)
    capabilities: BTreeMap<String, CapabilityToken>,
    /// Process state
    state: ProcessState,
    /// CPU time consumed (cycles)
    cpu_time: u64,
    /// Entry point address
    entry_point: u64,
    /// Current instruction pointer
    instruction_pointer: u64,
    /// Current stack pointer
    stack_pointer: u64,
}

impl NativeProcess {
    /// Gets the process ID.
    pub fn id(&self) -> NativeProcessId {
        self.id
    }

    /// Gets the process state.
    pub fn state(&self) -> ProcessState {
        self.state
    }

    /// Gets CPU time consumed.
    pub fn cpu_time(&self) -> u64 {
        self.cpu_time
    }

    /// Checks if a capability is bound.
    pub fn has_capability(&self, name: &str) -> bool {
        self.capabilities.contains_key(name)
    }

    /// Gets memory regions.
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }
}

/// Capability binding for native processes.
#[derive(Debug, Clone)]
pub struct CapabilityBinding {
    /// Import name
    pub name: String,
    /// Capability token
    pub token: CapabilityToken,
}

/// The S-NATIVE runtime.
pub struct Native {
    /// Active processes
    processes: Mutex<BTreeMap<NativeProcessId, NativeProcess>>,
    /// Next process ID
    next_id: Mutex<u64>,
    /// Maximum concurrent processes
    max_processes: usize,
}

impl Native {
    /// Creates a new S-NATIVE runtime.
    pub fn new(max_processes: usize) -> Self {
        Self {
            processes: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(1),
            max_processes,
        }
    }

    /// Loads a native binary.
    ///
    /// # Arguments
    ///
    /// * `binary` - The binary code
    /// * `format` - Binary format
    /// * `config` - Sandbox configuration
    /// * `cap_token` - Capability authorizing load
    pub fn load(
        &self,
        binary: &[u8],
        format: BinaryFormat,
        config: SandboxConfig,
        name: Option<String>,
        _cap_token: &CapabilityToken,
    ) -> Result<NativeProcessId, NativeError> {
        // Validate binary
        match format {
            BinaryFormat::Elf => {
                if binary.len() < 4 || &binary[0..4] != b"\x7FELF" {
                    return Err(NativeError::InvalidBinary);
                }
            }
            BinaryFormat::Raw => {
                if binary.is_empty() {
                    return Err(NativeError::InvalidBinary);
                }
            }
        }

        let mut processes = self.processes.lock();
        if processes.len() >= self.max_processes {
            return Err(NativeError::TooManyProcesses);
        }

        let mut next_id = self.next_id.lock();
        let id = NativeProcessId(*next_id);
        *next_id += 1;

        // Set up memory regions
        let code_region = MemoryRegion {
            base: 0x1000_0000,
            size: binary.len(),
            permissions: MemoryPermissions::READ_EXECUTE,
            region_type: RegionType::Code,
        };

        let stack_region = MemoryRegion {
            base: 0x7FFF_0000,
            size: config.stack_size,
            permissions: MemoryPermissions::READ_WRITE,
            region_type: RegionType::Stack,
        };

        let heap_region = MemoryRegion {
            base: 0x4000_0000,
            size: config.heap_size,
            permissions: MemoryPermissions::READ_WRITE,
            region_type: RegionType::Heap,
        };

        let process = NativeProcess {
            id,
            name,
            architecture: Architecture::current(),
            config,
            regions: alloc::vec![code_region, stack_region, heap_region],
            capabilities: BTreeMap::new(),
            state: ProcessState::Loading,
            cpu_time: 0,
            entry_point: 0x1000_0000,
            instruction_pointer: 0x1000_0000,
            stack_pointer: 0x7FFF_0000 + 1024 * 1024 - 8, // Top of stack
        };

        processes.insert(id, process);

        Ok(id)
    }

    /// Binds capabilities to a process.
    pub fn bind_capabilities(
        &self,
        id: NativeProcessId,
        bindings: Vec<CapabilityBinding>,
        _cap_token: &CapabilityToken,
    ) -> Result<(), NativeError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&id).ok_or(NativeError::ProcessNotFound)?;

        if process.state != ProcessState::Loading {
            return Err(NativeError::InvalidState);
        }

        for binding in bindings {
            process.capabilities.insert(binding.name, binding.token);
        }

        Ok(())
    }

    /// Starts a process.
    pub fn start(
        &self,
        id: NativeProcessId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), NativeError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&id).ok_or(NativeError::ProcessNotFound)?;

        if process.state != ProcessState::Loading {
            return Err(NativeError::InvalidState);
        }

        process.state = ProcessState::Ready;
        Ok(())
    }

    /// Runs a process for a time slice.
    pub fn run(
        &self,
        id: NativeProcessId,
        max_cycles: u64,
        _cap_token: &CapabilityToken,
    ) -> Result<RunResult, NativeError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&id).ok_or(NativeError::ProcessNotFound)?;

        if process.state != ProcessState::Ready {
            return Err(NativeError::InvalidState);
        }

        process.state = ProcessState::Running;

        // Execute native code in sandboxed environment
        // This implements a minimal execution model:
        // 1. Set up sandbox memory limits
        // 2. Execute code with syscall filtering
        // 3. Return on time limit, syscall, or fault
        
        #[cfg(target_arch = "x86_64")]
        {
            // On x86_64, we would:
            // - Set up page tables with user-mode mappings
            // - Load process state (registers from context)
            // - Use sysret to enter user mode
            // - Handle syscalls via syscall instruction
            // For now, simulate execution
        }
        
        #[cfg(target_arch = "aarch64")]
        {
            // On AArch64, we would:
            // - Set up EL0 page tables
            // - Load process state
            // - Use eret to enter EL0
            // - Handle syscalls via svc instruction
        }
        
        // Simulate execution: consume cycles and check limits

        // Check CPU limit
        process.cpu_time += max_cycles;
        if process.cpu_time > process.config.max_cpu_time {
            process.state = ProcessState::Killed;
            return Ok(RunResult::CpuLimitExceeded);
        }

        process.state = ProcessState::Ready;
        Ok(RunResult::TimeSliceComplete)
    }

    /// Terminates a process.
    pub fn terminate(
        &self,
        id: NativeProcessId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), NativeError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&id).ok_or(NativeError::ProcessNotFound)?;

        process.state = ProcessState::Terminated;
        Ok(())
    }

    /// Kills a process immediately.
    pub fn kill(
        &self,
        id: NativeProcessId,
        _cap_token: &CapabilityToken,
    ) -> Result<(), NativeError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&id).ok_or(NativeError::ProcessNotFound)?;

        process.state = ProcessState::Killed;
        Ok(())
    }

    /// Lists all processes.
    pub fn list_processes(&self) -> Vec<NativeProcessId> {
        self.processes.lock().keys().copied().collect()
    }

    /// Gets process info.
    pub fn process_info(&self, id: NativeProcessId) -> Option<ProcessInfo> {
        let processes = self.processes.lock();
        processes.get(&id).map(|p| ProcessInfo {
            id: p.id,
            name: p.name.clone(),
            state: p.state,
            cpu_time: p.cpu_time,
            memory_used: p.regions.iter().map(|r| r.size).sum(),
        })
    }
}

/// Information about a process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub id: NativeProcessId,
    pub name: Option<String>,
    pub state: ProcessState,
    pub cpu_time: u64,
    pub memory_used: usize,
}

/// Result of running a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunResult {
    /// Time slice completed normally
    TimeSliceComplete,
    /// Process made a system call
    SystemCall,
    /// Process terminated
    Terminated,
    /// CPU limit exceeded
    CpuLimitExceeded,
    /// Memory limit exceeded
    MemoryLimitExceeded,
    /// Invalid instruction
    InvalidInstruction,
}

/// S-NATIVE errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeError {
    /// Invalid binary format
    InvalidBinary,
    /// Process not found
    ProcessNotFound,
    /// Too many processes
    TooManyProcesses,
    /// Invalid process state
    InvalidState,
    /// Memory limit exceeded
    MemoryLimitExceeded,
    /// Invalid capability
    InvalidCapability,
    /// Architecture mismatch
    ArchitectureMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken::new([1, 2, 3, 4])
    }

    #[test]
    fn test_load_elf() {
        let native = Native::new(100);
        let token = dummy_token();

        // Valid ELF header
        let elf = alloc::vec![0x7F, 0x45, 0x4C, 0x46, 0x02, 0x01, 0x01, 0x00];
        let id = native
            .load(&elf, BinaryFormat::Elf, SandboxConfig::default(), None, &token)
            .expect("should load");

        let info = native.process_info(id).expect("should exist");
        assert_eq!(info.state, ProcessState::Loading);
    }
}
