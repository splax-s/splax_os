//! # Signal Handling
//!
//! POSIX-style signals for asynchronous process notification.
//!
//! ## Signal Model
//!
//! Signals are asynchronous notifications sent to processes:
//! - Can be sent from kernel or other processes
//! - Handled when returning from kernel mode
//! - Can be masked/blocked by the process
//!
//! ## Standard Signals
//!
//! ```text
//! Signal     | Default Action | Description
//! -----------|----------------|---------------------------
//! SIGKILL    | Terminate      | Force kill (cannot be caught)
//! SIGTERM    | Terminate      | Graceful termination request
//! SIGINT     | Terminate      | Interrupt from keyboard
//! SIGSTOP    | Stop           | Pause execution (cannot be caught)
//! SIGCONT    | Continue       | Resume if stopped
//! SIGSEGV    | Core dump      | Segmentation fault
//! SIGCHLD    | Ignore         | Child process terminated
//! SIGUSR1/2  | Terminate      | User-defined signals
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use crate::sched::ProcessId;

/// Signal number type
pub type Signal = u8;

// Standard signal numbers (POSIX-compatible)
/// Hangup
pub const SIGHUP: Signal = 1;
/// Interrupt (Ctrl+C)
pub const SIGINT: Signal = 2;
/// Quit (Ctrl+\)
pub const SIGQUIT: Signal = 3;
/// Illegal instruction
pub const SIGILL: Signal = 4;
/// Trace/breakpoint trap
pub const SIGTRAP: Signal = 5;
/// Abort
pub const SIGABRT: Signal = 6;
/// Bus error
pub const SIGBUS: Signal = 7;
/// Floating point exception
pub const SIGFPE: Signal = 8;
/// Kill (cannot be caught)
pub const SIGKILL: Signal = 9;
/// User-defined signal 1
pub const SIGUSR1: Signal = 10;
/// Segmentation fault
pub const SIGSEGV: Signal = 11;
/// User-defined signal 2
pub const SIGUSR2: Signal = 12;
/// Broken pipe
pub const SIGPIPE: Signal = 13;
/// Alarm clock
pub const SIGALRM: Signal = 14;
/// Termination
pub const SIGTERM: Signal = 15;
/// Stack fault
pub const SIGSTKFLT: Signal = 16;
/// Child status changed
pub const SIGCHLD: Signal = 17;
/// Continue if stopped
pub const SIGCONT: Signal = 18;
/// Stop (cannot be caught)
pub const SIGSTOP: Signal = 19;
/// Keyboard stop
pub const SIGTSTP: Signal = 20;
/// Background read from tty
pub const SIGTTIN: Signal = 21;
/// Background write to tty
pub const SIGTTOU: Signal = 22;
/// Urgent data available
pub const SIGURG: Signal = 23;
/// CPU time limit exceeded
pub const SIGXCPU: Signal = 24;
/// File size limit exceeded
pub const SIGXFSZ: Signal = 25;
/// Virtual timer expired
pub const SIGVTALRM: Signal = 26;
/// Profiling timer expired
pub const SIGPROF: Signal = 27;
/// Window size changed
pub const SIGWINCH: Signal = 28;
/// I/O possible
pub const SIGIO: Signal = 29;
/// Power failure
pub const SIGPWR: Signal = 30;
/// Bad system call
pub const SIGSYS: Signal = 31;

/// Maximum signal number
pub const NSIG: usize = 32;

/// Real-time signals start here
pub const SIGRTMIN: Signal = 32;
/// Real-time signals end here  
pub const SIGRTMAX: Signal = 64;

/// Default action for a signal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    /// Terminate the process
    Terminate,
    /// Terminate and generate core dump
    CoreDump,
    /// Stop the process
    Stop,
    /// Continue if stopped
    Continue,
    /// Ignore the signal
    Ignore,
}

/// Get the default action for a signal
pub fn default_action(sig: Signal) -> SignalAction {
    match sig {
        SIGKILL | SIGTERM | SIGINT | SIGQUIT | SIGHUP | 
        SIGPIPE | SIGALRM | SIGUSR1 | SIGUSR2 | SIGPWR => SignalAction::Terminate,
        
        SIGSEGV | SIGILL | SIGBUS | SIGFPE | 
        SIGABRT | SIGTRAP | SIGSYS | SIGXCPU | SIGXFSZ => SignalAction::CoreDump,
        
        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => SignalAction::Stop,
        
        SIGCONT => SignalAction::Continue,
        
        SIGCHLD | SIGURG | SIGWINCH | SIGIO => SignalAction::Ignore,
        
        _ => SignalAction::Terminate, // Default for unknown signals
    }
}

/// Signal handler type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalHandler {
    /// Use default action
    Default,
    /// Ignore the signal
    Ignore,
    /// User-defined handler at this address
    Handler(u64),
}

/// Signal disposition for a process
#[derive(Debug, Clone)]
pub struct SignalDisposition {
    /// Handler for each signal
    handlers: [SignalHandler; NSIG],
    /// Signals to block while handler runs
    mask_on_handle: [SignalSet; NSIG],
    /// Flags for each signal
    flags: [SignalFlags; NSIG],
}

impl Default for SignalDisposition {
    fn default() -> Self {
        Self {
            handlers: [SignalHandler::Default; NSIG],
            mask_on_handle: [SignalSet::empty(); NSIG],
            flags: [SignalFlags::empty(); NSIG],
        }
    }
}

impl SignalDisposition {
    /// Create new signal disposition with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set handler for a signal
    pub fn set_handler(&mut self, sig: Signal, handler: SignalHandler) -> Result<(), SignalError> {
        if sig == 0 || sig as usize >= NSIG {
            return Err(SignalError::InvalidSignal);
        }
        
        // SIGKILL and SIGSTOP cannot be caught or ignored
        if sig == SIGKILL || sig == SIGSTOP {
            return Err(SignalError::UncatchableSignal);
        }
        
        self.handlers[sig as usize] = handler;
        Ok(())
    }

    /// Get handler for a signal
    pub fn get_handler(&self, sig: Signal) -> SignalHandler {
        if sig as usize >= NSIG {
            SignalHandler::Default
        } else {
            self.handlers[sig as usize]
        }
    }

    /// Set mask to apply while handling a signal
    pub fn set_mask_on_handle(&mut self, sig: Signal, mask: SignalSet) {
        if (sig as usize) < NSIG {
            self.mask_on_handle[sig as usize] = mask;
        }
    }
}

bitflags::bitflags! {
    /// Signal flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct SignalFlags: u32 {
        /// Don't revert to default when delivered
        const NODEFER = 1 << 0;
        /// Restart syscalls if interrupted
        const RESTART = 1 << 1;
        /// Reset handler to default after delivery
        const RESETHAND = 1 << 2;
        /// Use alternate signal stack
        const ONSTACK = 1 << 3;
        /// Don't generate SIGCHLD when child stops
        const NOCLDSTOP = 1 << 4;
        /// Don't create zombie when child exits
        const NOCLDWAIT = 1 << 5;
        /// Use 3-argument signal handler
        const SIGINFO = 1 << 6;
    }
}

/// A set of signals (bitmask)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SignalSet(u64);

impl SignalSet {
    /// Empty signal set
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Full signal set (all signals)
    pub const fn full() -> Self {
        Self(!0)
    }

    /// Check if set contains a signal
    pub fn contains(&self, sig: Signal) -> bool {
        if sig == 0 || sig > 64 {
            return false;
        }
        self.0 & (1 << (sig - 1)) != 0
    }

    /// Add a signal to the set
    pub fn add(&mut self, sig: Signal) {
        if sig > 0 && sig <= 64 {
            self.0 |= 1 << (sig - 1);
        }
    }

    /// Remove a signal from the set
    pub fn remove(&mut self, sig: Signal) {
        if sig > 0 && sig <= 64 {
            self.0 &= !(1 << (sig - 1));
        }
    }

    /// Union with another set
    pub fn union(&self, other: &SignalSet) -> SignalSet {
        SignalSet(self.0 | other.0)
    }

    /// Intersection with another set
    pub fn intersection(&self, other: &SignalSet) -> SignalSet {
        SignalSet(self.0 & other.0)
    }

    /// Check if set is empty
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Get the first pending signal
    pub fn first(&self) -> Option<Signal> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0.trailing_zeros() as Signal + 1)
        }
    }

    /// Iterate over signals in the set
    pub fn iter(&self) -> impl Iterator<Item = Signal> + '_ {
        (1..=64).filter(move |&sig| self.contains(sig))
    }
}

/// Pending signal information
#[derive(Debug, Clone)]
pub struct PendingSignal {
    /// Signal number
    pub signal: Signal,
    /// Signal info (sender, etc.)
    pub info: SignalInfo,
}

/// Information about a signal
#[derive(Debug, Clone, Default)]
pub struct SignalInfo {
    /// Signal number
    pub signo: Signal,
    /// Error number (if applicable)
    pub errno: i32,
    /// Signal code (why the signal was sent)
    pub code: SignalCode,
    /// Sending process ID
    pub sender_pid: Option<ProcessId>,
    /// Sending user ID (not used in Splax - capability-based)
    pub sender_uid: u32,
    /// Extra data (signal-specific)
    pub value: u64,
    /// Fault address (for SIGSEGV, SIGBUS)
    pub fault_addr: Option<u64>,
}

/// Why a signal was sent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignalCode {
    /// Sent by kill() or similar
    #[default]
    User,
    /// Sent by kernel
    Kernel,
    /// Timer expired
    Timer,
    /// Signal generated by async I/O
    AsyncIO,
    /// Sent by sigqueue()
    Queue,
    /// Fault - memory access violation
    FaultMapr,
    /// Fault - invalid permissions
    FaultAccerr,
    /// Fault - address not mapped
    FaultSegv,
    /// Child exited
    ChildExited,
    /// Child was killed
    ChildKilled,
    /// Child was stopped
    ChildStopped,
    /// Child continued
    ChildContinued,
}

/// Per-process signal state
#[derive(Debug, Clone)]
pub struct SignalState {
    /// Pending signals (not yet delivered)
    pending: Vec<PendingSignal>,
    /// Currently blocked signals
    blocked: SignalSet,
    /// Signal disposition (handlers)
    disposition: SignalDisposition,
    /// Alternate signal stack
    alt_stack: Option<AltStack>,
}

impl Default for SignalState {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalState {
    /// Create new signal state
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            blocked: SignalSet::empty(),
            disposition: SignalDisposition::new(),
            alt_stack: None,
        }
    }

    /// Queue a signal for delivery
    pub fn queue(&mut self, signal: Signal, info: SignalInfo) {
        // Don't queue if already pending (standard signals are not queued)
        if signal < SIGRTMIN && self.pending.iter().any(|p| p.signal == signal) {
            return;
        }
        
        self.pending.push(PendingSignal { signal, info });
    }

    /// Get the next deliverable signal
    pub fn dequeue(&mut self) -> Option<PendingSignal> {
        // Find first signal that isn't blocked
        let idx = self.pending.iter().position(|p| !self.blocked.contains(p.signal))?;
        Some(self.pending.remove(idx))
    }

    /// Check if there are pending deliverable signals
    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|p| !self.blocked.contains(p.signal))
    }

    /// Get pending signal set
    pub fn pending_set(&self) -> SignalSet {
        let mut set = SignalSet::empty();
        for p in &self.pending {
            set.add(p.signal);
        }
        set
    }

    /// Block signals
    pub fn block(&mut self, set: &SignalSet) {
        // Cannot block SIGKILL or SIGSTOP
        let mut new_blocked = self.blocked.union(set);
        new_blocked.remove(SIGKILL);
        new_blocked.remove(SIGSTOP);
        self.blocked = new_blocked;
    }

    /// Unblock signals
    pub fn unblock(&mut self, set: &SignalSet) {
        for sig in set.iter() {
            self.blocked.remove(sig);
        }
    }

    /// Set blocked mask
    pub fn set_blocked(&mut self, set: SignalSet) {
        let mut new_blocked = set;
        new_blocked.remove(SIGKILL);
        new_blocked.remove(SIGSTOP);
        self.blocked = new_blocked;
    }

    /// Get blocked mask
    pub fn get_blocked(&self) -> SignalSet {
        self.blocked
    }

    /// Get signal disposition
    pub fn disposition(&self) -> &SignalDisposition {
        &self.disposition
    }

    /// Get mutable signal disposition
    pub fn disposition_mut(&mut self) -> &mut SignalDisposition {
        &mut self.disposition
    }
}

/// Alternate signal stack
#[derive(Debug, Clone, Copy)]
pub struct AltStack {
    /// Stack base address
    pub base: u64,
    /// Stack size
    pub size: usize,
    /// Flags
    pub flags: AltStackFlags,
}

bitflags::bitflags! {
    /// Alternate stack flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AltStackFlags: u32 {
        /// Stack is disabled
        const DISABLE = 1 << 0;
        /// Currently executing on alt stack
        const ONSTACK = 1 << 1;
    }
}

/// Signal errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalError {
    /// Invalid signal number
    InvalidSignal,
    /// Cannot catch or ignore this signal
    UncatchableSignal,
    /// Process not found
    ProcessNotFound,
    /// Permission denied
    PermissionDenied,
}

/// Global signal manager
pub struct SignalManager {
    /// Per-process signal state
    states: Mutex<BTreeMap<ProcessId, SignalState>>,
    /// Signal counter (for debugging)
    signals_sent: AtomicU64,
}

impl SignalManager {
    /// Create new signal manager
    pub const fn new() -> Self {
        Self {
            states: Mutex::new(BTreeMap::new()),
            signals_sent: AtomicU64::new(0),
        }
    }

    /// Initialize signal state for a new process
    pub fn init_process(&self, pid: ProcessId) {
        self.states.lock().insert(pid, SignalState::new());
    }

    /// Cleanup signal state for a terminated process
    pub fn cleanup_process(&self, pid: ProcessId) {
        self.states.lock().remove(&pid);
    }

    /// Send a signal to a process
    pub fn send(&self, target: ProcessId, signal: Signal, info: SignalInfo) -> Result<(), SignalError> {
        if signal == 0 || signal as usize >= NSIG {
            return Err(SignalError::InvalidSignal);
        }

        let mut states = self.states.lock();
        let state = states.get_mut(&target).ok_or(SignalError::ProcessNotFound)?;
        
        state.queue(signal, info);
        self.signals_sent.fetch_add(1, Ordering::Relaxed);

        // Wake up process if it's blocked so it can handle the signal
        drop(states); // Release lock before calling scheduler
        let _ = crate::sched::scheduler().wake(target);

        Ok(())
    }

    /// Send signal from one process to another
    pub fn kill(&self, sender: ProcessId, target: ProcessId, signal: Signal) -> Result<(), SignalError> {
        // Signal 0 is used to check if process exists
        if signal == 0 {
            return if self.states.lock().contains_key(&target) {
                Ok(())
            } else {
                Err(SignalError::ProcessNotFound)
            };
        }

        let info = SignalInfo {
            signo: signal,
            errno: 0,
            code: SignalCode::User,
            sender_pid: Some(sender),
            sender_uid: 0,
            value: 0,
            fault_addr: None,
        };

        self.send(target, signal, info)
    }

    /// Check if process has pending signals
    pub fn has_pending(&self, pid: ProcessId) -> bool {
        self.states.lock()
            .get(&pid)
            .map(|s| s.has_pending())
            .unwrap_or(false)
    }

    /// Dequeue a pending signal for delivery
    pub fn dequeue(&self, pid: ProcessId) -> Option<PendingSignal> {
        self.states.lock()
            .get_mut(&pid)
            .and_then(|s| s.dequeue())
    }

    /// Get signal handler for a process
    pub fn get_handler(&self, pid: ProcessId, signal: Signal) -> SignalHandler {
        self.states.lock()
            .get(&pid)
            .map(|s| s.disposition().get_handler(signal))
            .unwrap_or(SignalHandler::Default)
    }

    /// Set signal handler for a process
    pub fn set_handler(&self, pid: ProcessId, signal: Signal, handler: SignalHandler) -> Result<(), SignalError> {
        let mut states = self.states.lock();
        let state = states.get_mut(&pid).ok_or(SignalError::ProcessNotFound)?;
        state.disposition_mut().set_handler(signal, handler)
    }

    /// Block signals for a process
    pub fn sigprocmask(&self, pid: ProcessId, how: SigMaskHow, set: &SignalSet) -> Result<SignalSet, SignalError> {
        let mut states = self.states.lock();
        let state = states.get_mut(&pid).ok_or(SignalError::ProcessNotFound)?;
        
        let old_mask = state.get_blocked();
        
        match how {
            SigMaskHow::Block => state.block(set),
            SigMaskHow::Unblock => state.unblock(set),
            SigMaskHow::SetMask => state.set_blocked(*set),
        }
        
        Ok(old_mask)
    }

    /// Get total signals sent
    pub fn total_signals(&self) -> u64 {
        self.signals_sent.load(Ordering::Relaxed)
    }
}

/// How to modify signal mask
#[derive(Debug, Clone, Copy)]
pub enum SigMaskHow {
    /// Add signals to blocked set
    Block,
    /// Remove signals from blocked set
    Unblock,
    /// Replace blocked set
    SetMask,
}

/// Global signal manager instance
pub static SIGNAL_MANAGER: SignalManager = SignalManager::new();

/// Convenience function: send a signal
pub fn kill(sender: ProcessId, target: ProcessId, signal: Signal) -> Result<(), SignalError> {
    SIGNAL_MANAGER.kill(sender, target, signal)
}

/// Convenience function: raise a signal to self
pub fn raise(pid: ProcessId, signal: Signal) -> Result<(), SignalError> {
    SIGNAL_MANAGER.kill(pid, pid, signal)
}

/// Get signal name
pub fn signal_name(sig: Signal) -> &'static str {
    match sig {
        SIGHUP => "SIGHUP",
        SIGINT => "SIGINT",
        SIGQUIT => "SIGQUIT",
        SIGILL => "SIGILL",
        SIGTRAP => "SIGTRAP",
        SIGABRT => "SIGABRT",
        SIGBUS => "SIGBUS",
        SIGFPE => "SIGFPE",
        SIGKILL => "SIGKILL",
        SIGUSR1 => "SIGUSR1",
        SIGSEGV => "SIGSEGV",
        SIGUSR2 => "SIGUSR2",
        SIGPIPE => "SIGPIPE",
        SIGALRM => "SIGALRM",
        SIGTERM => "SIGTERM",
        SIGSTKFLT => "SIGSTKFLT",
        SIGCHLD => "SIGCHLD",
        SIGCONT => "SIGCONT",
        SIGSTOP => "SIGSTOP",
        SIGTSTP => "SIGTSTP",
        SIGTTIN => "SIGTTIN",
        SIGTTOU => "SIGTTOU",
        SIGURG => "SIGURG",
        SIGXCPU => "SIGXCPU",
        SIGXFSZ => "SIGXFSZ",
        SIGVTALRM => "SIGVTALRM",
        SIGPROF => "SIGPROF",
        SIGWINCH => "SIGWINCH",
        SIGIO => "SIGIO",
        SIGPWR => "SIGPWR",
        SIGSYS => "SIGSYS",
        _ => "UNKNOWN",
    }
}
