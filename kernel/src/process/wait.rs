//! # Process Exit and Wait
//!
//! Process termination and child reaping.
//!
//! ## Exit Process
//!
//! When a process exits:
//! 1. All open file descriptors are closed
//! 2. Memory is freed
//! 3. Parent is notified with SIGCHLD
//! 4. Process becomes a "zombie" until parent calls wait()
//!
//! ## Wait Process  
//!
//! Parent processes can wait for children:
//! - `wait()` - Wait for any child
//! - `waitpid()` - Wait for specific child
//! - `waitid()` - More flexible waiting
//!
//! ## Orphan Processes
//!
//! If a parent exits before its children:
//! - Children are "reparented" to init (PID 1)
//! - Init automatically reaps zombies

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use spin::Mutex;

use crate::sched::{ProcessId, ProcessState};
use crate::process::signal::{SignalInfo, SignalCode, SIGCHLD, SIGNAL_MANAGER};

/// Exit status of a process
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus(i32);

impl ExitStatus {
    /// Create exit status from exit code
    pub fn from_exit_code(code: i32) -> Self {
        // Exit code is in bits 8-15, with lower 8 bits = 0
        Self((code & 0xff) << 8)
    }

    /// Create exit status from signal
    pub fn from_signal(signal: u8) -> Self {
        // Signal is in bits 0-6, bit 7 = core dump
        Self(signal as i32 & 0x7f)
    }

    /// Create exit status from signal with core dump
    pub fn from_signal_core(signal: u8) -> Self {
        Self((signal as i32 & 0x7f) | 0x80)
    }

    /// Create stopped status
    pub fn new_stopped(signal: u8) -> Self {
        Self(0x7f | ((signal as i32) << 8))
    }

    /// Create continued status
    pub fn new_continued() -> Self {
        Self(0xffff)
    }

    /// Check if process exited normally
    pub fn exited(&self) -> bool {
        (self.0 & 0x7f) == 0
    }

    /// Get exit code (only valid if exited() is true)
    pub fn exit_code(&self) -> i32 {
        (self.0 >> 8) & 0xff
    }

    /// Check if process was killed by signal
    pub fn signaled(&self) -> bool {
        ((self.0 & 0x7f) + 1) >> 1 > 0 && !self.stopped() && !self.continued()
    }

    /// Get terminating signal (only valid if signaled() is true)
    pub fn term_signal(&self) -> u8 {
        (self.0 & 0x7f) as u8
    }

    /// Check if core dump was generated
    pub fn core_dump(&self) -> bool {
        self.signaled() && (self.0 & 0x80) != 0
    }

    /// Check if process is stopped
    pub fn stopped(&self) -> bool {
        (self.0 & 0xff) == 0x7f
    }

    /// Get stop signal (only valid if stopped() is true)
    pub fn stop_signal(&self) -> u8 {
        ((self.0 >> 8) & 0xff) as u8
    }

    /// Check if process was continued
    pub fn continued(&self) -> bool {
        self.0 == 0xffff
    }

    /// Raw status value
    pub fn raw(&self) -> i32 {
        self.0
    }
}

/// Information about a zombie (terminated child)
#[derive(Debug, Clone)]
pub struct ZombieInfo {
    /// Process ID
    pub pid: ProcessId,
    /// Parent process ID
    pub parent: ProcessId,
    /// Exit status
    pub status: ExitStatus,
    /// Resource usage
    pub rusage: ResourceUsage,
}

/// Resource usage statistics
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// User CPU time (microseconds)
    pub utime: u64,
    /// System CPU time (microseconds)
    pub stime: u64,
    /// Maximum resident set size (bytes)
    pub maxrss: usize,
    /// Page faults not requiring I/O
    pub minflt: u64,
    /// Page faults requiring I/O
    pub majflt: u64,
    /// Voluntary context switches
    pub nvcsw: u64,
    /// Involuntary context switches
    pub nivcsw: u64,
}

/// Wait options
#[derive(Debug, Clone, Copy, Default)]
pub struct WaitOptions {
    /// Don't block if no child is ready
    pub nohang: bool,
    /// Return for stopped children too
    pub wuntraced: bool,
    /// Return for continued children too
    pub wcontinued: bool,
    /// Don't remove child from zombie list
    pub wnowait: bool,
}

impl WaitOptions {
    pub const fn new() -> Self {
        Self {
            nohang: false,
            wuntraced: false,
            wcontinued: false,
            wnowait: false,
        }
    }

    pub const fn nohang() -> Self {
        Self {
            nohang: true,
            wuntraced: false,
            wcontinued: false,
            wnowait: false,
        }
    }
}

/// What to wait for
#[derive(Debug, Clone, Copy)]
pub enum WaitTarget {
    /// Wait for any child
    Any,
    /// Wait for specific PID
    Pid(ProcessId),
    /// Wait for any child in process group
    ProcessGroup(ProcessId),
}

/// Wait errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitError {
    /// No children to wait for
    NoChildren,
    /// No child is ready (with WNOHANG)
    WouldBlock,
    /// Interrupted by signal
    Interrupted,
    /// Invalid argument
    InvalidArgument,
    /// Child not found
    ChildNotFound,
}

/// Exit errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitError {
    /// Process not found
    ProcessNotFound,
    /// Cannot exit (init process)
    CannotExit,
}

/// Wait manager - tracks zombies and parent-child relationships
pub struct WaitManager {
    /// Parent -> children mapping
    children: Mutex<BTreeMap<ProcessId, Vec<ProcessId>>>,
    /// Zombie processes waiting to be reaped
    zombies: Mutex<BTreeMap<ProcessId, ZombieInfo>>,
    /// Waiters (processes blocked in wait())
    waiters: Mutex<BTreeMap<ProcessId, WaitRequest>>,
}

/// A pending wait request
#[derive(Debug)]
struct WaitRequest {
    target: WaitTarget,
    options: WaitOptions,
}

impl WaitManager {
    /// Create new wait manager
    pub const fn new() -> Self {
        Self {
            children: Mutex::new(BTreeMap::new()),
            zombies: Mutex::new(BTreeMap::new()),
            waiters: Mutex::new(BTreeMap::new()),
        }
    }

    /// Register a parent-child relationship
    pub fn add_child(&self, parent: ProcessId, child: ProcessId) {
        let mut children = self.children.lock();
        children.entry(parent).or_insert_with(Vec::new).push(child);
    }

    /// Remove a child from parent's list
    pub fn remove_child(&self, parent: ProcessId, child: ProcessId) {
        let mut children = self.children.lock();
        if let Some(kids) = children.get_mut(&parent) {
            kids.retain(|&c| c != child);
        }
    }

    /// Get all children of a process
    pub fn get_children(&self, parent: ProcessId) -> Vec<ProcessId> {
        self.children.lock()
            .get(&parent)
            .cloned()
            .unwrap_or_default()
    }

    /// Get parent of a process
    pub fn get_parent(&self, child: ProcessId) -> Option<ProcessId> {
        let children = self.children.lock();
        for (parent, kids) in children.iter() {
            if kids.contains(&child) {
                return Some(*parent);
            }
        }
        None
    }

    /// Process exited - convert to zombie
    pub fn do_exit(&self, pid: ProcessId, parent: ProcessId, exit_code: i32, rusage: ResourceUsage) {
        let zombie = ZombieInfo {
            pid,
            parent,
            status: ExitStatus::from_exit_code(exit_code),
            rusage,
        };

        // Add to zombies
        self.zombies.lock().insert(pid, zombie);

        // Send SIGCHLD to parent
        let info = SignalInfo {
            signo: SIGCHLD,
            errno: 0,
            code: SignalCode::ChildExited,
            sender_pid: Some(pid),
            sender_uid: 0,
            value: exit_code as u64,
            fault_addr: None,
        };
        let _ = SIGNAL_MANAGER.send(parent, SIGCHLD, info);

        // Wake up parent if it's waiting for children
        let waiters = self.waiters.lock();
        if waiters.contains_key(&parent) {
            drop(waiters);
            let _ = crate::sched::scheduler().wake(parent);
        }
    }

    /// Process was killed by signal
    pub fn do_signal_exit(&self, pid: ProcessId, parent: ProcessId, signal: u8, core_dump: bool, rusage: ResourceUsage) {
        let status = if core_dump {
            ExitStatus::from_signal_core(signal)
        } else {
            ExitStatus::from_signal(signal)
        };

        let zombie = ZombieInfo {
            pid,
            parent,
            status,
            rusage,
        };

        self.zombies.lock().insert(pid, zombie);

        // Send SIGCHLD to parent
        let info = SignalInfo {
            signo: SIGCHLD,
            errno: 0,
            code: SignalCode::ChildKilled,
            sender_pid: Some(pid),
            sender_uid: 0,
            value: signal as u64,
            fault_addr: None,
        };
        let _ = SIGNAL_MANAGER.send(parent, SIGCHLD, info);
    }

    /// Wait for a child process
    pub fn wait(&self, parent: ProcessId, target: WaitTarget, options: WaitOptions) -> Result<Option<ZombieInfo>, WaitError> {
        // Check if we have any children
        let kids = self.get_children(parent);
        if kids.is_empty() {
            return Err(WaitError::NoChildren);
        }

        // Try to find a matching zombie
        let mut zombies = self.zombies.lock();
        
        let matched = match target {
            WaitTarget::Any => {
                // Find any zombie child
                zombies.iter()
                    .find(|(_, z)| z.parent == parent)
                    .map(|(pid, _)| *pid)
            }
            WaitTarget::Pid(pid) => {
                // Find specific zombie
                zombies.get(&pid)
                    .filter(|z| z.parent == parent)
                    .map(|_| pid)
            }
            WaitTarget::ProcessGroup(pgid) => {
                // Find any zombie in the process group
                // For now, treat process group as just the leader process
                zombies.get(&pgid)
                    .filter(|z| z.parent == parent)
                    .map(|_| pgid)
            }
        };

        if let Some(pid) = matched {
            let zombie = if options.wnowait {
                zombies.get(&pid).cloned()
            } else {
                zombies.remove(&pid)
            };
            
            if !options.wnowait {
                // Remove from children list
                self.remove_child(parent, pid);
            }
            
            return Ok(zombie);
        }

        // No zombie found
        if options.nohang {
            return Ok(None);
        }

        // Register as waiter and block
        let wait_request = WaitRequest {
            target: target.clone(),
            options,
        };
        self.waiters.lock().insert(parent, wait_request);
        
        // Block the parent process - it will be woken when a child exits
        let _ = crate::sched::scheduler().block(parent);
        
        // After wakeup, try again (non-blocking this time)
        drop(zombies);
        let new_options = WaitOptions { nohang: true, ..WaitOptions::default() };
        self.wait(parent, target, new_options)
    }

    /// waitpid() implementation
    pub fn waitpid(&self, parent: ProcessId, pid: ProcessId, options: WaitOptions) -> Result<Option<ZombieInfo>, WaitError> {
        let target = if pid.0 == u64::MAX {
            WaitTarget::Any
        } else {
            WaitTarget::Pid(pid)
        };
        self.wait(parent, target, options)
    }

    /// Reparent children of a terminating process to init
    pub fn reparent_children(&self, dying: ProcessId, new_parent: ProcessId) {
        let mut children = self.children.lock();
        
        // Get dying process's children
        if let Some(kids) = children.remove(&dying) {
            // Add them to new parent
            let new_kids = children.entry(new_parent).or_insert_with(Vec::new);
            new_kids.extend(kids.iter());
            
            // Update zombie records
            let mut zombies = self.zombies.lock();
            for kid in &kids {
                if let Some(zombie) = zombies.get_mut(kid) {
                    zombie.parent = new_parent;
                }
            }
        }
    }

    /// Clean up when a process is reaped
    pub fn cleanup_process(&self, pid: ProcessId) {
        self.children.lock().remove(&pid);
        self.zombies.lock().remove(&pid);
        self.waiters.lock().remove(&pid);
    }

    /// Get zombie count (for debugging)
    pub fn zombie_count(&self) -> usize {
        self.zombies.lock().len()
    }
}

/// Global wait manager
pub static WAIT_MANAGER: WaitManager = WaitManager::new();

/// Exit the current process
pub fn exit(pid: ProcessId, exit_code: i32) -> Result<(), ExitError> {
    // Don't allow init to exit
    if pid.0 == 1 {
        return Err(ExitError::CannotExit);
    }

    // Get parent
    let parent = WAIT_MANAGER.get_parent(pid)
        .unwrap_or(ProcessId::new(1)); // Orphan goes to init

    // Get resource usage
    // Note: Full resource tracking (CPU time, memory, I/O) requires
    // integration with scheduler and memory manager statistics.
    // Currently returns zeroed usage; proper tracking is a future enhancement.
    let rusage = ResourceUsage::default();

    // Create zombie
    WAIT_MANAGER.do_exit(pid, parent, exit_code, rusage);

    // Reparent children to init
    WAIT_MANAGER.reparent_children(pid, ProcessId::new(1));

    // Clean up signal state
    crate::process::signal::SIGNAL_MANAGER.cleanup_process(pid);

    // Close file descriptors (handled by VFS if implemented)
    // For now, just log that we would close FDs
    #[cfg(feature = "vfs")]
    crate::fs::vfs::close_all_fds(pid);

    // Free process memory via scheduler termination
    // The scheduler's terminate() handles memory cleanup
    let _ = crate::sched::scheduler().terminate(pid);

    Ok(())
}

/// Exit due to signal
pub fn exit_signal(pid: ProcessId, signal: u8, core_dump: bool) -> Result<(), ExitError> {
    if pid.0 == 1 {
        return Err(ExitError::CannotExit);
    }

    let parent = WAIT_MANAGER.get_parent(pid)
        .unwrap_or(ProcessId::new(1));

    let rusage = ResourceUsage::default();
    WAIT_MANAGER.do_signal_exit(pid, parent, signal, core_dump, rusage);
    WAIT_MANAGER.reparent_children(pid, ProcessId::new(1));
    crate::process::signal::SIGNAL_MANAGER.cleanup_process(pid);

    Ok(())
}

/// Wait for any child
pub fn wait(parent: ProcessId) -> Result<ZombieInfo, WaitError> {
    WAIT_MANAGER.wait(parent, WaitTarget::Any, WaitOptions::new())?
        .ok_or(WaitError::WouldBlock)
}

/// Wait for specific child
pub fn waitpid(parent: ProcessId, pid: ProcessId, options: WaitOptions) -> Result<Option<ZombieInfo>, WaitError> {
    WAIT_MANAGER.waitpid(parent, pid, options)
}
