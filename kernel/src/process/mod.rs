//! # Process Management Module
//!
//! This module provides process creation, lifecycle management, and execution.
//!
//! ## Submodules
//!
//! - `elf`: ELF binary parser
//! - `exec`: Process execution (loading and running binaries)
//! - `signal`: Signal handling (POSIX-style async events)
//! - `wait`: Process exit and wait (child reaping)
//!
//! ## Core Types
//!
//! - `Process`: Process descriptor with context, stacks, capabilities
//! - `ProcessManager`: Global process manager singleton
//! - `ProcessError`: Process operation errors

pub mod elf;
pub mod exec;
pub mod signal;
pub mod wait;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::cap::CapabilityToken;
use crate::sched::{ProcessId, ProcessState, SchedulingClass};

#[cfg(target_arch = "x86_64")]
use crate::arch::x86_64::context::Context;

/// Stack size for kernel threads (64 KB)
pub const KERNEL_STACK_SIZE: usize = 64 * 1024;

/// Stack size for user processes (1 MB)
pub const USER_STACK_SIZE: usize = 1024 * 1024;

/// Default user stack top address
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;

/// Default user code start address  
pub const USER_CODE_START: u64 = 0x0000_0000_0040_0000;

/// Process descriptor.
#[derive(Debug)]
pub struct Process {
    /// Unique process ID
    pub pid: ProcessId,
    /// Process name (for debugging)
    pub name: String,
    /// Parent process ID (0 for init)
    pub parent: ProcessId,
    /// Current state
    pub state: ProcessState,
    /// Scheduling class
    pub class: SchedulingClass,
    /// Priority within class
    pub priority: u8,
    /// CPU context for this process
    #[cfg(target_arch = "x86_64")]
    pub context: Context,
    /// Page table base address (CR3)
    pub page_table: u64,
    /// Kernel stack base
    pub kernel_stack: u64,
    /// Kernel stack size
    pub kernel_stack_size: usize,
    /// User stack base (if user process)
    pub user_stack: Option<u64>,
    /// Capability token for this process
    pub cap_token: CapabilityToken,
    /// Exit code (set when terminated)
    pub exit_code: Option<i32>,
    /// Child process IDs
    pub children: Vec<ProcessId>,
    /// Total CPU time used (in ticks)
    pub cpu_time: u64,
    /// Time when process was created
    pub created_at: u64,
    /// Program break (for brk syscall)
    pub brk: u64,
    /// Working directory
    pub cwd: String,
}

impl Process {
    /// Creates a new kernel process.
    pub fn new_kernel(
        pid: ProcessId,
        name: String,
        parent: ProcessId,
        entry: extern "C" fn() -> !,
        kernel_stack: u64,
        cap_token: CapabilityToken,
    ) -> Self {
        #[cfg(target_arch = "x86_64")]
        let context = {
            let cr3 = crate::arch::x86_64::paging::read_cr3();
            Context::new_kernel(entry, kernel_stack + KERNEL_STACK_SIZE as u64, cr3)
        };

        Self {
            pid,
            name,
            parent,
            state: ProcessState::Ready,
            class: SchedulingClass::Interactive,
            priority: 128,
            #[cfg(target_arch = "x86_64")]
            context,
            page_table: 0, // Use kernel page table
            kernel_stack,
            kernel_stack_size: KERNEL_STACK_SIZE,
            user_stack: None,
            cap_token,
            exit_code: None,
            children: Vec::new(),
            cpu_time: 0,
            created_at: 0,
            brk: 0,
            cwd: String::from("/"),
        }
    }

    /// Creates a new user process.
    pub fn new_user(
        pid: ProcessId,
        name: String,
        parent: ProcessId,
        entry: u64,
        page_table: u64,
        kernel_stack: u64,
        user_stack: u64,
        cap_token: CapabilityToken,
    ) -> Self {
        #[cfg(target_arch = "x86_64")]
        let context = Context::new_user(entry, user_stack, page_table);

        Self {
            pid,
            name,
            parent,
            state: ProcessState::Ready,
            class: SchedulingClass::Interactive,
            priority: 128,
            #[cfg(target_arch = "x86_64")]
            context,
            page_table,
            kernel_stack,
            kernel_stack_size: KERNEL_STACK_SIZE,
            user_stack: Some(user_stack),
            cap_token,
            exit_code: None,
            children: Vec::new(),
            cpu_time: 0,
            created_at: 0,
            brk: 0,
            cwd: String::from("/"),
        }
    }

    /// Creates a user process from an ELF binary.
    pub fn from_elf(
        pid: ProcessId,
        name: String,
        parent: ProcessId,
        elf_data: &[u8],
        page_table: u64,
        kernel_stack: u64,
        cap_token: CapabilityToken,
    ) -> Result<Self, exec::ExecError> {
        // Parse ELF and prepare execution context
        let (elf_info, ctx, _stack_data) = exec::prepare_exec(elf_data, &[&name], &[])?;
        
        #[cfg(target_arch = "x86_64")]
        let context = Context::new_user(ctx.entry, ctx.stack_ptr, page_table);

        Ok(Self {
            pid,
            name,
            parent,
            state: ProcessState::Ready,
            class: SchedulingClass::Interactive,
            priority: 128,
            #[cfg(target_arch = "x86_64")]
            context,
            page_table,
            kernel_stack,
            kernel_stack_size: KERNEL_STACK_SIZE,
            user_stack: Some(ctx.stack_ptr),
            cap_token,
            exit_code: None,
            children: Vec::new(),
            cpu_time: 0,
            created_at: 0,
            brk: ctx.brk,
            cwd: String::from("/"),
        })
    }
}

/// Process manager.
pub struct ProcessManager {
    /// All processes
    processes: Mutex<BTreeMap<ProcessId, Process>>,
    /// Currently running process
    current: Mutex<Option<ProcessId>>,
    /// Next process ID to allocate
    next_pid: AtomicU64,
}

impl ProcessManager {
    /// Creates a new process manager.
    pub const fn new() -> Self {
        Self {
            processes: Mutex::new(BTreeMap::new()),
            current: Mutex::new(None),
            next_pid: AtomicU64::new(1),
        }
    }

    /// Allocates a new process ID.
    pub fn alloc_pid(&self) -> ProcessId {
        ProcessId::new(self.next_pid.fetch_add(1, Ordering::SeqCst))
    }

    /// Spawns a new kernel thread.
    pub fn spawn_kernel(
        &self,
        name: String,
        entry: extern "C" fn() -> !,
        cap_token: CapabilityToken,
    ) -> Result<ProcessId, ProcessError> {
        let pid = self.alloc_pid();
        let parent = self.current_pid().unwrap_or(ProcessId::KERNEL);
        
        // Allocate kernel stack using frame allocator
        let stack_frames = KERNEL_STACK_SIZE / crate::mm::PAGE_SIZE;
        let kernel_stack = crate::mm::FRAME_ALLOCATOR
            .allocate_contiguous(stack_frames)
            .map_err(|_| ProcessError::OutOfMemory)?
            .address() + KERNEL_STACK_SIZE as u64;
        
        let process = Process::new_kernel(pid, name, parent, entry, kernel_stack, cap_token);
        
        self.processes.lock().insert(pid, process);
        
        Ok(pid)
    }

    /// Spawns a new user process.
    pub fn spawn_user(
        &self,
        name: String,
        entry: u64,
        page_table: u64,
        cap_token: CapabilityToken,
    ) -> Result<ProcessId, ProcessError> {
        let pid = self.alloc_pid();
        let parent = self.current_pid().unwrap_or(ProcessId::KERNEL);
        
        // Allocate kernel stack using frame allocator
        let stack_frames = KERNEL_STACK_SIZE / crate::mm::PAGE_SIZE;
        let kernel_stack = crate::mm::FRAME_ALLOCATOR
            .allocate_contiguous(stack_frames)
            .map_err(|_| ProcessError::OutOfMemory)?
            .address() + KERNEL_STACK_SIZE as u64;
        let user_stack = USER_STACK_TOP;
        
        let process = Process::new_user(
            pid, name, parent, entry, page_table, kernel_stack, user_stack, cap_token
        );
        
        self.processes.lock().insert(pid, process);
        
        Ok(pid)
    }

    /// Spawns a process from an ELF binary.
    pub fn spawn_elf(
        &self,
        name: String,
        elf_data: &[u8],
        page_table: u64,
        cap_token: CapabilityToken,
    ) -> Result<ProcessId, ProcessError> {
        let pid = self.alloc_pid();
        let parent = self.current_pid().unwrap_or(ProcessId::KERNEL);
        
        // Allocate kernel stack
        let kernel_stack = 0x0000_0001_0000_0000 + (pid.0 * KERNEL_STACK_SIZE as u64);
        
        let process = Process::from_elf(
            pid, name, parent, elf_data, page_table, kernel_stack, cap_token
        ).map_err(|_| ProcessError::InvalidElf)?;
        
        self.processes.lock().insert(pid, process);
        
        Ok(pid)
    }

    /// Returns the current process ID.
    pub fn current_pid(&self) -> Option<ProcessId> {
        *self.current.lock()
    }

    /// Returns a reference to the current process.
    pub fn current_process(&self) -> Option<ProcessId> {
        *self.current.lock()
    }

    /// Sets the current process.
    pub fn set_current(&self, pid: ProcessId) {
        *self.current.lock() = Some(pid);
    }

    /// Gets a process by ID.
    pub fn get(&self, pid: ProcessId) -> Option<Process> {
        self.processes.lock().get(&pid).map(|p| Process {
            pid: p.pid,
            name: p.name.clone(),
            parent: p.parent,
            state: p.state,
            class: p.class,
            priority: p.priority,
            #[cfg(target_arch = "x86_64")]
            context: p.context.clone(),
            page_table: p.page_table,
            kernel_stack: p.kernel_stack,
            kernel_stack_size: p.kernel_stack_size,
            user_stack: p.user_stack,
            cap_token: p.cap_token.clone(),
            exit_code: p.exit_code,
            children: p.children.clone(),
            cpu_time: p.cpu_time,
            created_at: p.created_at,
            brk: p.brk,
            cwd: p.cwd.clone(),
        })
    }

    /// Updates process state.
    pub fn set_state(&self, pid: ProcessId, state: ProcessState) -> Result<(), ProcessError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(ProcessError::NotFound)?;
        process.state = state;
        Ok(())
    }

    /// Updates process brk (for memory allocation).
    pub fn set_brk(&self, pid: ProcessId, brk: u64) -> Result<u64, ProcessError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(ProcessError::NotFound)?;
        let old_brk = process.brk;
        process.brk = brk;
        Ok(old_brk)
    }

    /// Gets process brk.
    pub fn get_brk(&self, pid: ProcessId) -> Result<u64, ProcessError> {
        let processes = self.processes.lock();
        let process = processes.get(&pid).ok_or(ProcessError::NotFound)?;
        Ok(process.brk)
    }

    /// Terminates a process.
    pub fn terminate(&self, pid: ProcessId, exit_code: i32) -> Result<(), ProcessError> {
        let mut processes = self.processes.lock();
        let process = processes.get_mut(&pid).ok_or(ProcessError::NotFound)?;
        process.state = ProcessState::Terminated;
        process.exit_code = Some(exit_code);
        
        // Get parent for notification
        let parent = process.parent;
        
        // Free kernel stack memory
        // kernel_stack points to top of stack, subtract size to get base
        let kernel_stack_base = process.kernel_stack.saturating_sub(KERNEL_STACK_SIZE as u64);
        if kernel_stack_base > 0 {
            let stack_frames = KERNEL_STACK_SIZE / crate::mm::PAGE_SIZE;
            let frame = crate::mm::FrameNumber::from_address(kernel_stack_base);
            crate::mm::FRAME_ALLOCATOR.free_contiguous(frame, stack_frames);
        }
        
        // Notify parent via wait subsystem
        drop(processes); // Release lock before calling wait manager
        crate::process::wait::WAIT_MANAGER.do_exit(
            pid, 
            parent, 
            exit_code, 
            crate::process::wait::ResourceUsage::default()
        );
        
        Ok(())
    }

    /// Performs a context switch from current to target process.
    #[cfg(target_arch = "x86_64")]
    pub unsafe fn switch_to(&self, target_pid: ProcessId) -> Result<(), ProcessError> {
        let mut processes = self.processes.lock();
        let mut current_lock = self.current.lock();
        
        // Get target process
        let target = processes.get_mut(&target_pid).ok_or(ProcessError::NotFound)?;
        let target_context = &target.context as *const Context;
        
        // Save current process context and switch
        if let Some(current_pid) = *current_lock {
            if current_pid != target_pid {
                let current = processes.get_mut(&current_pid).ok_or(ProcessError::NotFound)?;
                let current_context = &mut current.context as *mut Context;
                
                // Update current
                *current_lock = Some(target_pid);
                
                // Drop locks before context switch
                drop(current_lock);
                drop(processes);
                
                // Perform the actual context switch
                // SAFETY: Both contexts are valid and we've dropped the locks
                unsafe {
                    crate::arch::x86_64::context::switch_context(current_context, target_context);
                }
            }
        } else {
            // No current process, just init the new context
            *current_lock = Some(target_pid);
            drop(current_lock);
            drop(processes);
            
            // SAFETY: The context is valid
            unsafe {
                crate::arch::x86_64::context::init_context(target_context);
            }
        }
        
        Ok(())
    }

    /// Returns the number of processes.
    pub fn process_count(&self) -> usize {
        self.processes.lock().len()
    }

    /// Lists all processes.
    pub fn list(&self) -> Vec<(ProcessId, String, ProcessState)> {
        self.processes.lock()
            .iter()
            .map(|(pid, p)| (*pid, p.name.clone(), p.state))
            .collect()
    }
}

/// Process-related errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessError {
    /// Process not found
    NotFound,
    /// Cannot allocate resources
    OutOfMemory,
    /// Permission denied
    PermissionDenied,
    /// Invalid state transition
    InvalidState,
    /// Too many processes
    LimitReached,
    /// Invalid ELF binary
    InvalidElf,
}

/// Global process manager instance.
pub static PROCESS_MANAGER: ProcessManager = ProcessManager::new();
