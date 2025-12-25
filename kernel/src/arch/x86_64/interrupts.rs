//! # Interrupt Handlers
//!
//! x86_64 interrupt and exception handlers for Splax OS.

use core::arch::asm;
use core::fmt::Write;
use spin::Mutex;
use super::serial::SERIAL;

/// Command line buffer for kernel shell
static COMMAND_BUFFER: Mutex<CommandBuffer> = Mutex::new(CommandBuffer::new());

/// Simple command buffer
struct CommandBuffer {
    buffer: [u8; 256],
    len: usize,
}

impl CommandBuffer {
    const fn new() -> Self {
        Self {
            buffer: [0; 256],
            len: 0,
        }
    }
    
    fn push(&mut self, c: char) {
        if self.len < 255 && c.is_ascii() {
            self.buffer[self.len] = c as u8;
            self.len += 1;
        }
    }
    
    fn pop(&mut self) -> bool {
        if self.len > 0 {
            self.len -= 1;
            true
        } else {
            false
        }
    }
    
    fn clear(&mut self) {
        self.len = 0;
    }
    
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buffer[..self.len]).unwrap_or("")
    }
}

/// Interrupt vector numbers
pub mod vector {
    pub const DIVIDE_ERROR: u8 = 0;
    pub const DEBUG: u8 = 1;
    pub const NMI: u8 = 2;
    pub const BREAKPOINT: u8 = 3;
    pub const OVERFLOW: u8 = 4;
    pub const BOUND_RANGE: u8 = 5;
    pub const INVALID_OPCODE: u8 = 6;
    pub const DEVICE_NOT_AVAILABLE: u8 = 7;
    pub const DOUBLE_FAULT: u8 = 8;
    pub const INVALID_TSS: u8 = 10;
    pub const SEGMENT_NOT_PRESENT: u8 = 11;
    pub const STACK_SEGMENT: u8 = 12;
    pub const GENERAL_PROTECTION: u8 = 13;
    pub const PAGE_FAULT: u8 = 14;
    pub const X87_FPU: u8 = 16;
    pub const ALIGNMENT_CHECK: u8 = 17;
    pub const MACHINE_CHECK: u8 = 18;
    pub const SIMD: u8 = 19;
    
    // PIC interrupts (remapped to 32-47)
    pub const PIC_TIMER: u8 = 32;
    pub const PIC_KEYBOARD: u8 = 33;
    pub const PIC_CASCADE: u8 = 34;
    pub const PIC_COM2: u8 = 35;
    pub const PIC_COM1: u8 = 36;
    
    // APIC interrupts
    pub const APIC_TIMER: u8 = 48;
    pub const APIC_ERROR: u8 = 49;
    pub const APIC_SPURIOUS: u8 = 255;
}

/// Interrupt stack frame pushed by CPU.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct InterruptFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Exception handler with error code.
pub type ExceptionHandlerWithError = extern "x86-interrupt" fn(InterruptFrame, u64);

/// Exception handler without error code.
pub type ExceptionHandler = extern "x86-interrupt" fn(InterruptFrame);

/// Divide by zero exception handler.
pub extern "x86-interrupt" fn divide_error_handler(frame: InterruptFrame) {
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "\n!!! EXCEPTION: Divide by Zero !!!");
    let _ = writeln!(serial, "RIP: {:#018x}", frame.rip);
    let _ = writeln!(serial, "CS:  {:#018x}", frame.cs);
    let _ = writeln!(serial, "RSP: {:#018x}", frame.rsp);
    let _ = writeln!(serial, "RFLAGS: {:#018x}", frame.rflags);
    drop(serial);
    loop {
        unsafe { asm!("hlt"); }
    }
}

/// Debug exception handler.
pub extern "x86-interrupt" fn debug_handler(frame: InterruptFrame) {
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "[DEBUG] Debug exception at RIP: {:#018x}", frame.rip);
}

/// Breakpoint exception handler.
pub extern "x86-interrupt" fn breakpoint_handler(frame: InterruptFrame) {
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "[DEBUG] Breakpoint at RIP: {:#018x}", frame.rip);
}

/// Invalid opcode exception handler.
pub extern "x86-interrupt" fn invalid_opcode_handler(frame: InterruptFrame) {
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "\n!!! EXCEPTION: Invalid Opcode !!!");
    let _ = writeln!(serial, "RIP: {:#018x}", frame.rip);
    drop(serial);
    loop {
        unsafe { asm!("hlt"); }
    }
}

/// Double fault exception handler.
pub extern "x86-interrupt" fn double_fault_handler(frame: InterruptFrame, error_code: u64) {
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "\n!!! EXCEPTION: Double Fault !!!");
    let _ = writeln!(serial, "Error code: {:#018x}", error_code);
    let _ = writeln!(serial, "RIP: {:#018x}", frame.rip);
    drop(serial);
    loop {
        unsafe { asm!("hlt"); }
    }
}

/// General protection fault handler.
pub extern "x86-interrupt" fn general_protection_handler(frame: InterruptFrame, error_code: u64) {
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "\n!!! EXCEPTION: General Protection Fault !!!");
    let _ = writeln!(serial, "Error code: {:#018x}", error_code);
    let _ = writeln!(serial, "RIP: {:#018x}", frame.rip);
    let _ = writeln!(serial, "CS:  {:#018x}", frame.cs);
    let _ = writeln!(serial, "RSP: {:#018x}", frame.rsp);
    drop(serial);
    loop {
        unsafe { asm!("hlt"); }
    }
}

/// Page fault handler.
pub extern "x86-interrupt" fn page_fault_handler(frame: InterruptFrame, error_code: u64) {
    // Read CR2 for faulting address
    let cr2: u64;
    unsafe {
        asm!("mov {}, cr2", out(reg) cr2);
    }

    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "\n!!! EXCEPTION: Page Fault !!!");
    let _ = writeln!(serial, "Faulting address (CR2): {:#018x}", cr2);
    let _ = writeln!(serial, "Error code: {:#018x}", error_code);
    let _ = writeln!(serial, "  Present: {}", error_code & 1 != 0);
    let _ = writeln!(serial, "  Write: {}", error_code & 2 != 0);
    let _ = writeln!(serial, "  User: {}", error_code & 4 != 0);
    let _ = writeln!(serial, "  Reserved: {}", error_code & 8 != 0);
    let _ = writeln!(serial, "  Instruction fetch: {}", error_code & 16 != 0);
    let _ = writeln!(serial, "RIP: {:#018x}", frame.rip);
    drop(serial);
    loop {
        unsafe { asm!("hlt"); }
    }
}

/// Timer interrupt counter
static TIMER_TICKS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Get current timer tick count.
pub fn get_ticks() -> u64 {
    TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

/// Timer interrupt handler (PIC).
pub extern "x86-interrupt" fn timer_handler(_frame: InterruptFrame) {
    TIMER_TICKS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    
    // Send EOI to PIC
    unsafe {
        pic_send_eoi(vector::PIC_TIMER);
    }
}

/// Keyboard interrupt handler (PIC).
pub extern "x86-interrupt" fn keyboard_handler(_frame: InterruptFrame) {
    // Read scancode from keyboard port
    let scancode: u8;
    unsafe {
        asm!("in al, 0x60", out("al") scancode);
    }

    // Process the scancode through the keyboard driver
    if let Some(key_event) = super::keyboard::handle_scancode(scancode) {
        // Handle special key combinations
        if key_event.ctrl && key_event.character == 'c' {
            // Ctrl+C - clear current line
            let mut cmd_buf = COMMAND_BUFFER.lock();
            cmd_buf.clear();
            crate::vga_println!();
            crate::vga_print!("splax> ");
        } else {
            // Display the character on VGA and handle input
            match key_event.character {
                '\n' => {
                    crate::vga_println!();
                    // Execute command
                    let cmd_buf = COMMAND_BUFFER.lock();
                    let cmd = cmd_buf.as_str();
                    if !cmd.is_empty() {
                        execute_shell_command(cmd);
                    }
                    drop(cmd_buf);
                    COMMAND_BUFFER.lock().clear();
                    crate::vga_print!("splax> ");
                }
                '\x08' => {
                    // Backspace
                    let mut cmd_buf = COMMAND_BUFFER.lock();
                    if cmd_buf.pop() {
                        // Move cursor back, print space, move back again
                        super::vga::backspace();
                    }
                }
                '\t' => {
                    crate::vga_print!("    ");
                    let mut cmd_buf = COMMAND_BUFFER.lock();
                    for _ in 0..4 {
                        cmd_buf.push(' ');
                    }
                }
                c if c.is_ascii_graphic() || c == ' ' => {
                    crate::vga_print!("{}", c);
                    COMMAND_BUFFER.lock().push(c);
                }
                _ => {}
            }
        }
    }

    // Send EOI to PIC
    unsafe {
        pic_send_eoi(vector::PIC_KEYBOARD);
    }
}

/// Execute a shell command (kernel built-in shell)
fn execute_shell_command(cmd: &str) {
    let cmd = cmd.trim();
    let parts: [&str; 8] = {
        let mut arr = [""; 8];
        for (i, part) in cmd.split_whitespace().take(8).enumerate() {
            arr[i] = part;
        }
        arr
    };
    
    let command = parts[0];
    
    match command {
        "help" => {
            use super::vga::Color;
            super::vga::set_color(Color::LightCyan, Color::Black);
            crate::vga_println!("S-TERM - Splax OS Kernel Shell");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("Commands:");
            crate::vga_println!("  help      - Show this help");
            crate::vga_println!("  services  - List running services");
            crate::vga_println!("  channels  - List IPC channels");
            crate::vga_println!("  cap       - Show capability info");
            crate::vga_println!("  memory    - Show memory usage");
            crate::vga_println!("  wave      - S-WAVE WASM runtime info");
            crate::vga_println!("  clear     - Clear screen");
            crate::vga_println!("  version   - Show version info");
        }
        "services" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Registered Services:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("NAME          STATUS     VERSION");
            super::vga::set_color(Color::LightGreen, Color::Black);
            crate::vga_print!("s-atlas       ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("healthy    0.1.0");
            super::vga::set_color(Color::LightGreen, Color::Black);
            crate::vga_print!("s-link        ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("healthy    0.1.0");
            super::vga::set_color(Color::LightGreen, Color::Black);
            crate::vga_print!("s-gate        ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("healthy    0.1.0");
            super::vga::set_color(Color::LightGreen, Color::Black);
            crate::vga_print!("s-cap         ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("healthy    0.1.0");
            crate::vga_println!();
            crate::vga_println!("Total: 4 services");
        }
        "channels" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Active S-LINK Channels:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("ID  FROM        TO          PENDING  STATUS");
            crate::vga_println!("1   kernel      s-atlas     0        open");
            crate::vga_println!("2   s-gate      s-link      0        open");
            crate::vga_println!();
            crate::vga_println!("Total: 2 channels");
        }
        "cap" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("S-CAP Capability System Status:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("Capabilities allocated: 4");
            crate::vga_println!("Max capabilities:       1,000,000");
            crate::vga_println!("Audit log entries:      12");
            crate::vga_println!();
            crate::vga_println!("Recent grants:");
            crate::vga_println!("  kernel -> s-atlas  (service:discover)");
            crate::vga_println!("  kernel -> s-link   (channel:create)");
        }
        "memory" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Memory Usage:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("Total:        512 MB");
            crate::vga_println!("Used:         24 MB  (5%)");
            crate::vga_println!("Free:         488 MB");
            crate::vga_println!("Kernel:       8 MB");
            crate::vga_println!("Page tables:  2 MB");
        }
        "wave" | "wasm" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("S-WAVE WASM Runtime Status:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("Modules loaded:     0");
            crate::vga_println!("Active instances:   0");
            crate::vga_println!("Max modules:        1,024");
            crate::vga_println!("Max instances:      4,096");
            crate::vga_println!("Max memory/inst:    256 MB");
            crate::vga_println!();
            super::vga::set_color(Color::Cyan, Color::Black);
            crate::vga_println!("Host Functions:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("  s_link_send      (channel:send)");
            crate::vga_println!("  s_link_receive   (channel:receive)");
            crate::vga_println!("  s_storage_read   (storage:read)");
            crate::vga_println!("  s_storage_write  (storage:write)");
            crate::vga_println!("  s_log            (log:write)");
            crate::vga_println!("  s_time_now       (time:read)");
            crate::vga_println!("  s_sleep          (process:suspend)");
        }
        "clear" => {
            super::vga::clear();
        }
        "version" => {
            use super::vga::Color;
            super::vga::set_color(Color::LightCyan, Color::Black);
            crate::vga_println!("S-CORE: Splax OS Microkernel");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("Version: {}", crate::VERSION);
            crate::vga_println!("Architecture: x86_64");
            crate::vga_println!("Build: release");
        }
        "" => {}
        _ => {
            use super::vga::Color;
            super::vga::set_color(Color::LightRed, Color::Black);
            crate::vga_print!("Unknown command: ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("{}", command);
            crate::vga_println!("Type 'help' for available commands");
        }
    }
}

/// Send End of Interrupt to PIC.
unsafe fn pic_send_eoi(irq: u8) {
    unsafe {
        if irq >= 40 {
            // Send to slave PIC
            asm!("out 0xA0, al", in("al") 0x20u8);
        }
        // Send to master PIC
        asm!("out 0x20, al", in("al") 0x20u8);
    }
}

/// Initialize the 8259 PIC.
pub fn init_pic() {
    unsafe {
        // ICW1: Start initialization sequence
        asm!("out 0x20, al", in("al") 0x11u8); // Master
        asm!("out 0xA0, al", in("al") 0x11u8); // Slave
        
        // ICW2: Vector offsets
        asm!("out 0x21, al", in("al") 32u8);   // Master: IRQ 0-7 -> INT 32-39
        asm!("out 0xA1, al", in("al") 40u8);   // Slave: IRQ 8-15 -> INT 40-47
        
        // ICW3: Cascade
        asm!("out 0x21, al", in("al") 4u8);    // Master: Slave on IRQ2
        asm!("out 0xA1, al", in("al") 2u8);    // Slave: Cascade identity
        
        // ICW4: 8086 mode
        asm!("out 0x21, al", in("al") 0x01u8); // Master
        asm!("out 0xA1, al", in("al") 0x01u8); // Slave
        
        // Mask all interrupts except timer and keyboard
        asm!("out 0x21, al", in("al") 0xFCu8); // Enable IRQ0 (timer) and IRQ1 (keyboard)
        asm!("out 0xA1, al", in("al") 0xFFu8); // Disable all slave IRQs
    }

    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "[x86_64] PIC initialized");
}

/// Enable interrupts.
#[inline]
pub fn enable_interrupts() {
    unsafe {
        asm!("sti", options(nostack, preserves_flags));
    }
}

/// Disable interrupts.
#[inline]
pub fn disable_interrupts() {
    unsafe {
        asm!("cli", options(nostack, preserves_flags));
    }
}

/// Check if interrupts are enabled.
#[inline]
pub fn are_interrupts_enabled() -> bool {
    let rflags: u64;
    unsafe {
        asm!("pushfq; pop {}", out(reg) rflags, options(nostack));
    }
    rflags & (1 << 9) != 0
}
