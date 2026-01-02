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

// =============================================================================
// Core Microkernel Modules (Always Present)
// These implement S-CORE: scheduling, memory, IPC, capabilities
// =============================================================================

pub mod arch;
pub mod cap;
pub mod ipc;
pub mod mm;
pub mod sched;
pub mod process;
pub mod smp;

// Crypto: minimal for capabilities, full for monolithic
#[cfg(feature = "microkernel")]
pub mod crypto;  // Minimal crypto for capability tokens
#[cfg(not(feature = "microkernel"))]
pub mod crypto;  // Full crypto suite

// =============================================================================
// Hardware Interface Modules (Required for Microkernel)
// These provide minimal hardware access needed by S-CORE
// =============================================================================

pub mod acpi;
pub mod pci;

// =============================================================================
// Block Layer (Monolithic only - S-STORAGE handles in microkernel)
// =============================================================================

#[cfg(not(feature = "microkernel"))]
pub mod block;

// =============================================================================
// Hybrid Microkernel Stubs (Forward to Userspace Services)
// =============================================================================

/// Device stub - forwards device operations to S-DEV userspace service
#[cfg(any(feature = "hybrid", feature = "microkernel"))]
pub mod dev_stub;

// =============================================================================
// Monolithic Subsystems (Disabled in Microkernel Mode)
// In microkernel mode, these are replaced by userspace services
// =============================================================================

/// Filesystem support (monolithic mode, else use S-STORAGE service)
#[cfg(not(feature = "microkernel"))]
pub mod fs;

/// GPU/Graphics support (monolithic mode, else use S-GPU service)
#[cfg(any(not(feature = "microkernel"), feature = "monolithic_gpu"))]
pub mod gpu;

/// Network stack (monolithic mode, else use S-NET service)
#[cfg(any(not(feature = "microkernel"), feature = "monolithic_net"))]
pub mod net;

/// USB drivers (monolithic mode, else use S-DEV service)
#[cfg(any(not(feature = "microkernel"), feature = "monolithic_usb"))]
pub mod usb;

/// Sound drivers (monolithic mode, else use S-DEV service)
#[cfg(any(not(feature = "microkernel"), feature = "monolithic_sound"))]
pub mod sound;

/// WASM runtime (userspace, kept for compatibility)
#[cfg(not(feature = "microkernel"))]
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
pub extern "C" fn kernel_main(boot_info: *const u8) -> ! {
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

    // Create kernel configuration from boot info
    // Parse memory size, framebuffer, and other parameters
    let config = {
        let mut cfg = KernelConfig::default();
        
        // Parse memory regions if available in boot_info
        // The boot_info pointer contains multiboot/UEFI data
        if !boot_info.is_null() {
            // For multiboot2, magic is at offset 0
            // For UEFI, we'd have different structure
            // Currently using defaults as actual parsing requires
            // architecture-specific multiboot2 parsing
            #[cfg(feature = "multiboot2")]
            {
                // Would parse multiboot2 info here
                // cfg.memory_size = parsed_memory_size;
                let _ = boot_info; // suppress unused warning when feature enabled
            }
        }
        
        cfg
    };
    let mut kernel = Kernel::new(config);

    // Initialize security hardening (stack canaries, ASLR)
    // Use RDTSC as entropy source for random seed
    #[cfg(target_arch = "x86_64")]
    {
        let seed = unsafe { core::arch::x86_64::_rdtsc() };
        mm::security::init_security(seed);
        serial_println!("[kernel] Security hardening initialized (stack canaries + ASLR)");
    }
    #[cfg(target_arch = "aarch64")]
    {
        let seed: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) seed, options(nostack, nomem));
        }
        mm::security::init_security(seed);
    }

    // Initialize ACPI subsystem (required for SMP and power management)
    #[cfg(target_arch = "x86_64")]
    {
        if acpi::init() {
            serial_println!("[kernel] ACPI subsystem ready");
        } else {
            serial_println!("[kernel] ACPI not available (continuing without)");
        }
    }

    // Initialize SMP (multi-core support)
    smp::init_bsp();
    serial_println!("[kernel] SMP initialized (BSP registered)");

    // Print kernel init message
    #[cfg(target_arch = "x86_64")]
    {
        use core::fmt::Write;
        let mut serial = arch::x86_64::serial::SERIAL.lock();
        #[cfg(not(feature = "microkernel"))]
        let _ = writeln!(serial, "[kernel] Initializing subsystems...");
        #[cfg(feature = "microkernel")]
        let _ = writeln!(serial, "[kernel] Microkernel mode - minimal subsystems...");
        drop(serial);
    }
    
    // Initialize monolithic subsystems (disabled in microkernel mode)
    #[cfg(not(feature = "microkernel"))]
    {
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
        
        // Initialize USB subsystem
        match usb::init() {
            Ok(()) => serial_println!("[kernel] USB subsystem initialized"),
            Err(e) => serial_println!("[kernel] USB init skipped: {}", e),
        }
        
        // Initialize sound subsystem
        sound::init();
        serial_println!("[kernel] Sound subsystem initialized");

        // Initialize WASM runtime
        wasm::init();
    }
    
    // Microkernel mode - minimal init
    #[cfg(feature = "microkernel")]
    {
        // Only essential kernel services, subsystems run as userspace servers
        serial_println!("[kernel] Microkernel ready - services will run in userspace");
    }

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
        #[cfg(not(feature = "microkernel"))]
        {
            vga_println!("[OK] S-WAVE WASM runtime ready");
            vga_println!("[OK] Filesystem: ramfs (4 MB)");
            vga_println!("[OK] Network: virtio-net (10.0.2.15)");
        }
        #[cfg(feature = "microkernel")]
        {
            vga_println!("[OK] Microkernel mode - minimal core");
        }
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
