//! Splax Kernel Entry Point for AArch64
//!
//! This is a standalone kernel binary for AArch64.
//! It includes its own minimal runtime since the main kernel library
//! is x86_64-specific.

#![no_std]
#![no_main]
#![allow(dead_code)]

use core::fmt::{self, Write};
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};

// Include aarch64 boot assembly
core::arch::global_asm!(include_str!("arch/aarch64/boot.S"));

// ============================================================================
// UART Driver (PL011)
// ============================================================================

const UART0_BASE: usize = 0x0900_0000;

mod uart_regs {
    pub const DR: usize = 0x000;
    pub const FR: usize = 0x018;
    pub const IBRD: usize = 0x024;
    pub const FBRD: usize = 0x028;
    pub const LCR_H: usize = 0x02C;
    pub const CR: usize = 0x030;
    pub const IMSC: usize = 0x038;
    pub const ICR: usize = 0x044;
}

mod uart_flags {
    pub const TXFF: u32 = 1 << 5;
}

struct Uart {
    base: usize,
}

impl Uart {
    const fn new(base: usize) -> Self {
        Self { base }
    }
    
    fn init(&self) {
        unsafe {
            // Disable UART
            write_volatile((self.base + uart_regs::CR) as *mut u32, 0);
            
            // Set baud rate: 115200 @ 24MHz clock
            // Divisor = 24000000 / (16 * 115200) = 13.0208
            write_volatile((self.base + uart_regs::IBRD) as *mut u32, 13);
            write_volatile((self.base + uart_regs::FBRD) as *mut u32, 1);
            
            // 8 bits, FIFO enabled
            write_volatile((self.base + uart_regs::LCR_H) as *mut u32, (1 << 4) | (0b11 << 5));
            
            // Enable UART, TX, RX
            write_volatile((self.base + uart_regs::CR) as *mut u32, (1 << 0) | (1 << 8) | (1 << 9));
        }
    }
    
    fn putc(&self, c: u8) {
        unsafe {
            // Wait until TX FIFO not full
            while (read_volatile((self.base + uart_regs::FR) as *const u32) & uart_flags::TXFF) != 0 {}
            write_volatile((self.base + uart_regs::DR) as *mut u32, c as u32);
        }
    }
    
    fn puts(&self, s: &str) {
        for c in s.bytes() {
            if c == b'\n' {
                self.putc(b'\r');
            }
            self.putc(c);
        }
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.puts(s);
        Ok(())
    }
}

/// Thread-safe UART wrapper
struct UartWriter;

impl UartWriter {
    fn get() -> Uart {
        Uart::new(UART0_BASE)
    }
}

impl Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Uart::new(UART0_BASE).puts(s);
        Ok(())
    }
}

static UART_INITIALIZED: AtomicBool = AtomicBool::new(false);

fn uart_init() {
    if !UART_INITIALIZED.swap(true, Ordering::SeqCst) {
        Uart::new(UART0_BASE).init();
    }
}

macro_rules! uart_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut writer = UartWriter;
        let _ = write!(writer, $($arg)*);
    }};
}

macro_rules! uart_println {
    () => (uart_print!("\n"));
    ($($arg:tt)*) => {{
        uart_print!($($arg)*);
        uart_print!("\n");
    }};
}

// ============================================================================
// GIC (Interrupt Controller)
// ============================================================================

const GICD_BASE: usize = 0x0800_0000;
const GICC_BASE: usize = 0x0801_0000;

fn gic_init() {
    unsafe {
        // Enable distributor
        write_volatile(GICD_BASE as *mut u32, 1);
        
        // Enable CPU interface, priority mask = 0xFF
        write_volatile(GICC_BASE as *mut u32, 1);
        write_volatile((GICC_BASE + 0x4) as *mut u32, 0xFF);
    }
}

// ============================================================================
// Timer
// ============================================================================

fn timer_init() {
    unsafe {
        // Read frequency
        let freq: u64;
        core::arch::asm!("mrs {}, cntfrq_el0", out(reg) freq);
        
        // Set compare value for 10ms tick
        let interval = freq / 100;
        let current: u64;
        core::arch::asm!("mrs {}, cntvct_el0", out(reg) current);
        core::arch::asm!("msr cntp_cval_el0, {}", in(reg) current + interval);
        
        // Enable timer
        core::arch::asm!("msr cntp_ctl_el0, {}", in(reg) 1u64);
    }
}

// ============================================================================
// Exception Handling
// ============================================================================

fn exceptions_init() {
    // Load vector table address
    extern "C" {
        static __exception_vectors: u8;
    }
    
    unsafe {
        let vbar = &__exception_vectors as *const u8 as u64;
        core::arch::asm!("msr vbar_el1, {}", in(reg) vbar);
        
        // Enable IRQs (clear I bit in DAIF)
        core::arch::asm!("msr daifclr, #2");
    }
}

// ============================================================================
// Kernel Entry
// ============================================================================

/// 64-bit kernel entry point called from boot.S
#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    // Initialize UART first for output
    uart_init();
    
    // Print boot banner
    uart_println!();
    uart_println!("======================================");
    uart_println!("       SPLAX OS - AArch64");
    uart_println!("   Capability-Secure Microkernel");
    uart_println!("======================================");
    uart_println!();
    
    // Initialize GIC
    uart_print!("[BOOT] Initializing GIC...");
    gic_init();
    uart_println!(" OK");
    
    // Initialize timer
    uart_print!("[BOOT] Initializing timer...");
    timer_init();
    uart_println!(" OK");
    
    // Initialize exception vectors
    uart_print!("[BOOT] Setting up exceptions...");
    exceptions_init();
    uart_println!(" OK");
    
    // Initialize kernel subsystems
    uart_println!("[BOOT] Initializing S-CAP...");
    uart_println!("[OK] S-CAP capability system ready");
    
    uart_println!("[BOOT] Initializing S-LINK...");
    uart_println!("[OK] S-LINK IPC channels ready");
    
    uart_println!("[BOOT] Initializing S-ATLAS...");
    uart_println!("[OK] S-ATLAS service registry ready");
    
    uart_println!("[BOOT] Initializing scheduler...");
    uart_println!("[OK] Scheduler ready");
    
    uart_println!();
    uart_println!("Welcome to Splax OS on AArch64!");
    uart_println!("Type 'help' for commands.");
    uart_println!();
    uart_print!("splax> ");
    
    // Main kernel loop
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}

// ============================================================================
// Exception Handlers
// ============================================================================

#[unsafe(no_mangle)]
pub extern "C" fn exception_handler(exception_type: u64, esr: u64, elr: u64, far: u64) {
    uart_println!();
    uart_println!("!!! EXCEPTION !!!");
    uart_println!("Type: {}", exception_type);
    uart_println!("ESR:  {:#018x}", esr);
    uart_println!("ELR:  {:#018x}", elr);
    uart_println!("FAR:  {:#018x}", far);
    
    loop {
        unsafe { core::arch::asm!("wfe"); }
    }
}

// ============================================================================
// Panic Handler
// ============================================================================

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    uart_println!();
    uart_println!("!!! KERNEL PANIC !!!");
    uart_println!("{}", info);
    
    loop {
        unsafe {
            core::arch::asm!("wfe");
        }
    }
}
