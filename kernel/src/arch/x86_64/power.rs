//! Power Management - Shutdown and Reboot
//!
//! Provides system shutdown and reboot functionality for both
//! virtual machines and physical hardware.

use core::arch::asm;

/// ACPI shutdown ports (QEMU specific)
const QEMU_SHUTDOWN_PORT: u16 = 0x604;
const QEMU_SHUTDOWN_VALUE: u16 = 0x2000;

/// Alternative ACPI ports for Bochs/older QEMU
const BOCHS_SHUTDOWN_PORT: u16 = 0xB004;
const BOCHS_SHUTDOWN_VALUE: u16 = 0x2000;

/// VirtualBox ACPI port
const VBOX_SHUTDOWN_PORT: u16 = 0x4004;
const VBOX_SHUTDOWN_VALUE: u16 = 0x3400;

/// 8042 keyboard controller port for reset
const KEYBOARD_CONTROLLER: u16 = 0x64;
const RESET_COMMAND: u8 = 0xFE;

/// CMOS shutdown reason port
const CMOS_ADDRESS: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

/// Output a word (16-bit) to a port
unsafe fn outw(port: u16, value: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
    }
}

/// Output a byte to a port
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

/// Input a byte from a port
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Attempt to shut down the system
/// 
/// This tries multiple methods to ensure compatibility with
/// various hypervisors and physical hardware.
pub fn shutdown() -> ! {
    // Try QEMU shutdown first
    unsafe {
        outw(QEMU_SHUTDOWN_PORT, QEMU_SHUTDOWN_VALUE);
    }
    
    // Small delay
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }
    
    // Try Bochs/older QEMU
    unsafe {
        outw(BOCHS_SHUTDOWN_PORT, BOCHS_SHUTDOWN_VALUE);
    }
    
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }
    
    // Try VirtualBox
    unsafe {
        outw(VBOX_SHUTDOWN_PORT, VBOX_SHUTDOWN_VALUE);
    }
    
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }
    
    // If we're still here, try the APM shutdown (older systems)
    // APM interface at segment 0x40, offset 0x67
    // This rarely works on modern systems but worth a try
    
    // As a last resort, halt with interrupts disabled
    unsafe {
        asm!("cli");
        loop {
            asm!("hlt");
        }
    }
}

/// Reboot the system
/// 
/// Uses multiple methods for compatibility:
/// 1. 8042 keyboard controller reset (most reliable)
/// 2. CMOS reset
/// 3. Triple fault (last resort)
pub fn reboot() -> ! {
    // Method 1: 8042 keyboard controller CPU reset
    // This is the most reliable method on x86
    unsafe {
        // Wait for keyboard controller to be ready
        let mut timeout = 10000;
        while timeout > 0 {
            if (inb(KEYBOARD_CONTROLLER) & 0x02) == 0 {
                break;
            }
            timeout -= 1;
            core::hint::spin_loop();
        }
        
        // Send reset command
        outb(KEYBOARD_CONTROLLER, RESET_COMMAND);
    }
    
    // Small delay
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }
    
    // Method 2: CMOS reset (some systems support this)
    unsafe {
        outb(CMOS_ADDRESS, 0x0F); // Shutdown status register
        outb(CMOS_DATA, 0x00);    // Reset reason: normal
        
        // Try reset vector
        outb(CMOS_ADDRESS, 0x0F);
        outb(CMOS_DATA, 0x0A);    // Shutdown status: JMP DWORD [40:67]
    }
    
    // Small delay
    for _ in 0..1000000 {
        core::hint::spin_loop();
    }
    
    // Method 3: Triple fault (guaranteed to work)
    // Load a null IDT and trigger an interrupt
    triple_fault()
}

/// Trigger a triple fault to force CPU reset
/// 
/// This works by loading a null IDT and then triggering an interrupt.
/// When the CPU can't handle the double fault (because IDT is null),
/// it triple faults and resets.
fn triple_fault() -> ! {
    #[repr(C, packed)]
    struct NullIdtPtr {
        limit: u16,
        base: u64,
    }
    
    let null_idt = NullIdtPtr {
        limit: 0,
        base: 0,
    };
    
    unsafe {
        // Load null IDT
        asm!(
            "lidt [{}]",
            in(reg) &null_idt,
            options(nostack)
        );
        
        // Trigger an interrupt (any will do)
        asm!("int 3", options(nostack, nomem));
        
        // Should never reach here, but just in case
        loop {
            asm!("hlt");
        }
    }
}

/// Check if ACPI shutdown is available (basic check)
pub fn is_acpi_available() -> bool {
    // This is a simplified check
    // Full ACPI detection would involve finding the RSDP and parsing ACPI tables
    // For now, we assume ACPI is available if we're running on x86
    true
}

/// Print shutdown message
pub fn shutdown_message() {
    crate::vga_println!();
    crate::vga_println!("System is going down for poweroff NOW!");
    crate::vga_println!();
}

/// Print reboot message
pub fn reboot_message() {
    crate::vga_println!();
    crate::vga_println!("System is going down for reboot NOW!");
    crate::vga_println!();
}
