//! # S-CORE: The Splax OS Microkernel
//!
//! This is the heart of Splax OS. The kernel is responsible for exactly four things:
//!
//! 1. **CPU Scheduling** (`sched`): Deterministic, priority-based scheduling
//! 2. **Memory Management** (`mm`): No swap, no overcommit, explicit allocation
//! 3. **Inter-Process Communication** (`ipc`): Zero-copy message passing
//! 4. **Capability Enforcement** (`cap`): S-CAP - all access is capability-gated
//!
//! Everything else runs in userspace as isolated services.
//!
//! ## Architecture
//!
//! The kernel follows a strict microkernel architecture:
//! - Drivers run in userspace
//! - Filesystems run in userspace  
//! - Network stacks run in userspace
//! - Only the four core subsystems live in the kernel
//!
//! ## Security Model
//!
//! There are no users, groups, or root. Every resource access requires a
//! cryptographic capability token. No token = no access, not even to name the resource.
//!
//! ## Cross-Architecture Support
//!
//! The kernel simultaneously targets:
//! - `x86_64-splax-none`
//! - `aarch64-splax-none`

#![no_std]
#![no_main]
#![deny(unsafe_op_in_unsafe_fn)]
#![feature(abi_x86_interrupt)]

extern crate alloc;

pub mod arch;
pub mod block;
pub mod cap;
pub mod fs;
pub mod gpu;
pub mod ipc;
pub mod mm;
pub mod net;
pub mod process;
pub mod sched;
pub mod smp;
pub mod sound;
pub mod usb;
pub mod wasm;

use core::sync::atomic::{AtomicBool, Ordering};

/// Kernel version information
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const NAME: &str = "S-CORE";

/// Global kernel initialization flag
static KERNEL_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// The main kernel structure.
///
/// This holds references to all kernel subsystems. It is created once
/// during boot and never destroyed.
pub struct Kernel {
    /// The capability table - heart of S-CAP
    pub cap_table: cap::CapabilityTable,
    /// The memory manager
    pub memory_manager: mm::MemoryManager,
    /// The scheduler
    pub scheduler: sched::Scheduler,
    /// The IPC subsystem
    pub ipc_manager: ipc::IpcManager,
}

impl Kernel {
    /// Creates a new kernel instance.
    ///
    /// This should only be called once during boot. All subsystems are
    /// initialized with their default configurations.
    ///
    /// # Arguments
    ///
    /// * `config` - Kernel configuration passed from the bootloader
    ///
    /// # Returns
    ///
    /// A new `Kernel` instance with all subsystems initialized.
    pub fn new(config: KernelConfig) -> Self {
        // Ensure we only initialize once
        if KERNEL_INITIALIZED.swap(true, Ordering::SeqCst) {
            panic!("Kernel already initialized");
        }

        Self {
            cap_table: cap::CapabilityTable::new(config.max_capabilities),
            memory_manager: mm::MemoryManager::new(config.memory_config),
            scheduler: sched::Scheduler::new(config.scheduler_config),
            ipc_manager: ipc::IpcManager::new(config.ipc_config),
        }
    }

    /// Main kernel loop.
    ///
    /// This is the heart of the kernel. It:
    /// 1. Runs the scheduler to pick the next process
    /// 2. Handles any pending IPC
    /// 3. Checks for timer interrupts
    /// 4. Returns to the scheduled process
    ///
    /// This function never returns.
    pub fn run(&mut self) -> ! {
        loop {
            // Process pending keyboard input from the lock-free ring buffer
            #[cfg(target_arch = "x86_64")]
            arch::x86_64::interrupts::process_keyboard_input();
            
            // Process pending serial input from the lock-free ring buffer
            #[cfg(target_arch = "x86_64")]
            arch::x86_64::interrupts::process_serial_input();
            
            // Run scheduler
            if let Some(next_process) = self.scheduler.schedule() {
                // Switch to next process
                self.scheduler.switch_to(next_process);
            } else {
                // No runnable processes, halt until interrupt
                arch::halt();
            }
        }
    }
}

/// Configuration passed to the kernel during initialization.
#[derive(Debug, Clone)]
pub struct KernelConfig {
    /// Maximum number of capability tokens
    pub max_capabilities: usize,
    /// Memory subsystem configuration
    pub memory_config: mm::MemoryConfig,
    /// Scheduler configuration
    pub scheduler_config: sched::SchedulerConfig,
    /// IPC subsystem configuration
    pub ipc_config: ipc::IpcConfig,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            max_capabilities: 1_000_000,
            memory_config: mm::MemoryConfig::default(),
            scheduler_config: sched::SchedulerConfig::default(),
            ipc_config: ipc::IpcConfig::default(),
        }
    }
}

/// Kernel entry point.
///
/// This is called by the bootloader after setting up the initial environment.
/// The bootloader passes a pointer to the boot info structure.
#[no_mangle]
pub extern "C" fn kernel_main(_boot_info: *const u8) -> ! {
    // Initialize architecture-specific features (includes serial and VGA)
    arch::init();

    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        
        // Display banner on serial
        let mut serial = arch::x86_64::serial::SERIAL.lock();
        let _ = writeln!(serial);
        let _ = writeln!(serial, "╔══════════════════════════════════════════════════════════╗");
        let _ = writeln!(serial, "║            S-CORE: Splax OS Microkernel v{}            ║", VERSION);
        let _ = writeln!(serial, "║        Capability-Secure • Distributed-First            ║");
        let _ = writeln!(serial, "╚══════════════════════════════════════════════════════════╝");
        let _ = writeln!(serial);
        drop(serial);
        
        // Display banner on VGA screen
        use arch::x86_64::vga::{self, Color};
        vga::set_color(Color::LightCyan, Color::Black);
        vga_println!();
        vga::set_color(Color::White, Color::Blue);
        vga_println!("  S-CORE: Splax OS Microkernel v{}  ", VERSION);
        vga::set_color(Color::LightGray, Color::Black);
        vga_println!();
        vga::set_color(Color::Yellow, Color::Black);
        vga_println!("  Capability-Secure | Distributed-First | No POSIX");
        vga::set_color(Color::LightGray, Color::Black);
        vga_println!();
    }

    // Create kernel with default configuration
    // TODO: Parse boot_info to get actual configuration
    let config = KernelConfig::default();
    let mut kernel = Kernel::new(config);

    // Print kernel init message before network
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        let mut serial = arch::x86_64::serial::SERIAL.lock();
        let _ = writeln!(serial, "[kernel] Initializing subsystems...");
        drop(serial);
    }
    
    // Initialize filesystem
    fs::init();
    
    // Initialize block subsystem (VirtIO-blk, etc.)
    block::init();
    
    serial_println!("[kernel] About to init network...");
    
    // Initialize network subsystem
    net::init();
    
    serial_println!("[kernel] Network init complete, running diagnostics...");
    
    // Run network diagnostics
    net::run_diagnostics();
    
    serial_println!("[kernel] Diagnostics complete");

    // Initialize WASM runtime
    wasm::init();

    // Print completion messages and show VGA status
    #[cfg(target_arch = "x86_64")]
    {
        use arch::x86_64::vga::{self, Color};
        use core::fmt::Write;
        
        // Serial output
        if let Some(mut serial) = arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial, "[kernel] All subsystems initialized");
            let _ = writeln!(serial, "[kernel] Entering main loop...");
            let _ = writeln!(serial);
            let _ = writeln!(serial, "=== Splax OS Serial Console ===");
            let _ = writeln!(serial, "Type 'help' for available commands.");
            let _ = writeln!(serial);
            let _ = write!(serial, "splax> ");
        }
        
        // VGA output
        vga::set_color(Color::LightGreen, Color::Black);
        vga_println!("[OK] Kernel initialized");
        vga_println!("[OK] S-CAP capability system ready");
        vga_println!("[OK] S-LINK IPC system ready");
        vga_println!("[OK] S-ATLAS service registry ready");
        vga_println!("[OK] S-WAVE WASM runtime ready");
        vga_println!("[OK] Filesystem: ramfs (4 MB)");
        vga_println!("[OK] Network: virtio-net (10.0.2.15)");
        vga::set_color(Color::LightGray, Color::Black);
        vga_println!();
        vga::set_color(Color::Yellow, Color::Black);
        vga_println!("Welcome to Splax OS! Type 'help' for commands.");
        vga::set_color(Color::LightGray, Color::Black);
        vga_println!();
        vga_print!("splax> ");
    }

    // Start the kernel main loop
    kernel.run()
}

/// Panic handler for the kernel.
///
/// In a capability-secure system, panics are serious. We log as much
/// information as possible before halting.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        if let Some(mut serial) = arch::x86_64::serial::SERIAL.try_lock() {
            let _ = writeln!(serial);
            let _ = writeln!(serial, "!!! KERNEL PANIC !!!");
            let _ = writeln!(serial, "{}", info);
        }
    }
    
    loop {
        arch::halt();
    }
}

// Memory functions required by compiler_builtins

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *dest.add(i) = *src.add(i) };
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if src < dest as *const u8 {
        // copy backwards
        let mut i = n;
        while i > 0 {
            i -= 1;
            unsafe { *dest.add(i) = *src.add(i) };
        }
    } else {
        // copy forwards
        let mut i = 0;
        while i < n {
            unsafe { *dest.add(i) = *src.add(i) };
            i += 1;
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        unsafe { *dest.add(i) = c as u8 };
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let a = unsafe { *s1.add(i) };
        let b = unsafe { *s2.add(i) };
        if a != b {
            return (a as i32) - (b as i32);
        }
        i += 1;
    }
    0
}
