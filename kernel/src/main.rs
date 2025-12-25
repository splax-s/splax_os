//! Splax Kernel Entry Point - 64-bit Rust entry
//!
//! This is called from boot.S after transitioning to long mode.

#![no_std]
#![no_main]

// Include boot assembly (contains multiboot header and 32->64 bit transition)
core::arch::global_asm!(include_str!("boot.S"));

/// 64-bit kernel entry point called from boot.S
#[unsafe(no_mangle)]
pub extern "C" fn kernel_entry(multiboot_info: u64) -> ! {
    // Initialize serial for output
    unsafe {
        init_serial();
        write_serial(b"\r\n");
        write_serial(b"Splax OS Kernel Starting...\r\n");
    }
    
    // Call the main kernel
    splax_kernel::kernel_main(multiboot_info as *const u8);
}

// Simple serial port output for early boot
const SERIAL_PORT: u16 = 0x3F8; // COM1

unsafe fn init_serial() {
    unsafe {
        outb(SERIAL_PORT + 1, 0x00); // Disable interrupts
        outb(SERIAL_PORT + 3, 0x80); // Enable DLAB
        outb(SERIAL_PORT + 0, 0x03); // Set divisor (lo byte) 38400 baud
        outb(SERIAL_PORT + 1, 0x00); // Set divisor (hi byte)
        outb(SERIAL_PORT + 3, 0x03); // 8 bits, no parity, one stop bit
        outb(SERIAL_PORT + 2, 0xC7); // Enable FIFO
        outb(SERIAL_PORT + 4, 0x0B); // IRQs enabled, RTS/DSR set
    }
}

unsafe fn write_serial(data: &[u8]) {
    for &byte in data {
        // Wait for transmit to be ready
        while (inb(SERIAL_PORT + 5) & 0x20) == 0 {}
        outb(SERIAL_PORT, byte);
    }
}

#[inline]
unsafe fn outb(port: u16, val: u8) {
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val);
    }
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") val, in("dx") port);
    }
    val
}
