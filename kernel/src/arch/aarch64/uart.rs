//! # PL011 UART Driver
//!
//! Serial console driver for ARM PL011 UART.
//! Used on QEMU virt machine and many ARM boards.

use core::fmt::{self, Write};
use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;

/// PL011 UART register offsets.
mod regs {
    /// Data register
    pub const DR: usize = 0x000;
    /// Flag register
    pub const FR: usize = 0x018;
    /// Integer baud rate register
    pub const IBRD: usize = 0x024;
    /// Fractional baud rate register
    pub const FBRD: usize = 0x028;
    /// Line control register
    pub const LCR_H: usize = 0x02C;
    /// Control register
    pub const CR: usize = 0x030;
    /// Interrupt FIFO level select
    pub const IFLS: usize = 0x034;
    /// Interrupt mask set/clear
    pub const IMSC: usize = 0x038;
    /// Raw interrupt status
    pub const RIS: usize = 0x03C;
    /// Masked interrupt status
    pub const MIS: usize = 0x040;
    /// Interrupt clear register
    pub const ICR: usize = 0x044;
}

/// Flag register bits.
mod flags {
    /// Receive FIFO empty
    pub const RXFE: u32 = 1 << 4;
    /// Transmit FIFO full
    pub const TXFF: u32 = 1 << 5;
    /// Receive FIFO full
    pub const RXFF: u32 = 1 << 6;
    /// Transmit FIFO empty
    pub const TXFE: u32 = 1 << 7;
    /// UART busy
    pub const BUSY: u32 = 1 << 3;
}

/// Control register bits.
mod ctrl {
    /// UART enable
    pub const UARTEN: u32 = 1 << 0;
    /// Transmit enable
    pub const TXE: u32 = 1 << 8;
    /// Receive enable
    pub const RXE: u32 = 1 << 9;
}

/// Line control register bits.
mod lcr {
    /// Enable FIFOs
    pub const FEN: u32 = 1 << 4;
    /// 8-bit word length
    pub const WLEN8: u32 = 0b11 << 5;
}

/// Interrupt bits.
mod irq {
    /// Receive interrupt
    pub const RX: u32 = 1 << 4;
    /// Transmit interrupt
    pub const TX: u32 = 1 << 5;
    /// Receive timeout interrupt
    pub const RT: u32 = 1 << 6;
}

/// PL011 UART driver.
pub struct Uart {
    base: *mut u8,
}

// SAFETY: UART is memory-mapped I/O, safe to send across threads with proper synchronization
unsafe impl Send for Uart {}
unsafe impl Sync for Uart {}

impl Uart {
    /// QEMU virt machine UART0 base address.
    pub const UART0_BASE: u64 = 0x0900_0000;
    
    /// Create a new UART instance.
    ///
    /// # Safety
    ///
    /// `base` must point to valid PL011 UART registers.
    pub const unsafe fn new(base: u64) -> Self {
        Self { base: base as *mut u8 }
    }
    
    /// Read a register.
    fn read(&self, offset: usize) -> u32 {
        unsafe {
            read_volatile(self.base.add(offset) as *const u32)
        }
    }
    
    /// Write a register.
    fn write(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile(self.base.add(offset) as *mut u32, value);
        }
    }
    
    /// Initialize the UART.
    pub fn init(&self) {
        // Disable UART
        self.write(regs::CR, 0);
        
        // Wait for any current TX to complete
        while self.read(regs::FR) & flags::BUSY != 0 {}
        
        // Clear all interrupts
        self.write(regs::ICR, 0x7FF);
        
        // Set baud rate (115200 @ 24MHz clock)
        // IBRD = 24000000 / (16 * 115200) = 13
        // FBRD = 0.02 * 64 + 0.5 = 1
        self.write(regs::IBRD, 13);
        self.write(regs::FBRD, 1);
        
        // 8N1, enable FIFOs
        self.write(regs::LCR_H, lcr::WLEN8 | lcr::FEN);
        
        // Enable receive interrupt
        self.write(regs::IMSC, irq::RX | irq::RT);
        
        // Enable UART, TX, RX
        self.write(regs::CR, ctrl::UARTEN | ctrl::TXE | ctrl::RXE);
    }
    
    /// Send a byte.
    pub fn putc(&self, c: u8) {
        // Wait for TX FIFO to have space
        while self.read(regs::FR) & flags::TXFF != 0 {}
        self.write(regs::DR, c as u32);
    }
    
    /// Try to receive a byte.
    pub fn getc(&self) -> Option<u8> {
        if self.read(regs::FR) & flags::RXFE != 0 {
            None
        } else {
            Some((self.read(regs::DR) & 0xFF) as u8)
        }
    }
    
    /// Check if data is available to read.
    pub fn is_rx_ready(&self) -> bool {
        self.read(regs::FR) & flags::RXFE == 0
    }
    
    /// Check if transmitter is ready.
    pub fn is_tx_ready(&self) -> bool {
        self.read(regs::FR) & flags::TXFF == 0
    }
    
    /// Clear all pending interrupts.
    pub fn clear_interrupts(&self) {
        self.write(regs::ICR, 0x7FF);
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.bytes() {
            if c == b'\n' {
                self.putc(b'\r');
            }
            self.putc(c);
        }
        Ok(())
    }
}

/// Global UART instance.
pub static UART: Mutex<Uart> = Mutex::new(unsafe { Uart::new(Uart::UART0_BASE) });

/// Initialize the UART.
pub fn init() {
    UART.lock().init();
}

/// Handle UART interrupt.
pub fn handle_uart_irq() {
    let uart = UART.lock();
    
    // Read and echo characters
    while let Some(c) = uart.getc() {
        // Echo back
        if c == b'\r' {
            uart.putc(b'\n');
        }
        uart.putc(c);
        
        // TODO: Send to input buffer for shell
    }
    
    // Clear interrupts
    uart.clear_interrupts();
}

/// Print macro for aarch64.
#[macro_export]
macro_rules! uart_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write;
        let mut uart = $crate::arch::aarch64::uart::UART.lock();
        let _ = write!(uart, $($arg)*);
    }};
}

/// Print with newline macro for aarch64.
#[macro_export]
macro_rules! uart_println {
    () => ($crate::uart_print!("\n"));
    ($($arg:tt)*) => {{
        $crate::uart_print!($($arg)*);
        $crate::uart_print!("\n");
    }};
}
