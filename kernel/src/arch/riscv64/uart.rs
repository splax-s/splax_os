//! # RISC-V UART Driver (16550)
//!
//! Basic UART driver for the NS16550A compatible UART.
//! Used for early console output via SBI or direct MMIO.

use core::ptr::{read_volatile, write_volatile};
use spin::Mutex;
use super::sbi;

/// UART base address (QEMU virt machine)
const UART_BASE: usize = 0x1000_0000;

/// UART register offsets
mod regs {
    pub const RBR: usize = 0x0;  // Receive Buffer Register (read)
    pub const THR: usize = 0x0;  // Transmit Holding Register (write)
    pub const IER: usize = 0x1;  // Interrupt Enable Register
    pub const IIR: usize = 0x2;  // Interrupt Identification Register (read)
    pub const FCR: usize = 0x2;  // FIFO Control Register (write)
    pub const LCR: usize = 0x3;  // Line Control Register
    pub const MCR: usize = 0x4;  // Modem Control Register
    pub const LSR: usize = 0x5;  // Line Status Register
    pub const MSR: usize = 0x6;  // Modem Status Register
    pub const SCR: usize = 0x7;  // Scratch Register
    pub const DLL: usize = 0x0;  // Divisor Latch Low (when DLAB=1)
    pub const DLH: usize = 0x1;  // Divisor Latch High (when DLAB=1)
}

/// Line Status Register bits
mod lsr {
    pub const DATA_READY: u8 = 1 << 0;
    pub const OVERRUN: u8 = 1 << 1;
    pub const PARITY_ERROR: u8 = 1 << 2;
    pub const FRAMING_ERROR: u8 = 1 << 3;
    pub const BREAK: u8 = 1 << 4;
    pub const THR_EMPTY: u8 = 1 << 5;
    pub const TRANSMITTER_EMPTY: u8 = 1 << 6;
    pub const FIFO_ERROR: u8 = 1 << 7;
}

/// Line Control Register bits
mod lcr {
    pub const WORD_8: u8 = 0x03;
    pub const DLAB: u8 = 0x80;
}

/// FIFO Control Register bits
mod fcr {
    pub const ENABLE: u8 = 0x01;
    pub const CLEAR_RX: u8 = 0x02;
    pub const CLEAR_TX: u8 = 0x04;
    pub const TRIGGER_14: u8 = 0xC0;
}

/// Interrupt Enable Register bits
mod ier {
    pub const RX_AVAILABLE: u8 = 0x01;
    pub const TX_EMPTY: u8 = 0x02;
    pub const LINE_STATUS: u8 = 0x04;
    pub const MODEM_STATUS: u8 = 0x08;
}

/// Whether to use SBI for console output
static USE_SBI: Mutex<bool> = Mutex::new(true);

/// Initialize UART
pub fn init() {
    // Try to initialize MMIO UART
    if init_mmio() {
        *USE_SBI.lock() = false;
    }
}

/// Initialize UART via MMIO
fn init_mmio() -> bool {
    // Check if UART is present by reading scratch register
    unsafe {
        let addr = UART_BASE + regs::SCR;
        write_volatile(addr as *mut u8, 0xAB);
        if read_volatile(addr as *const u8) != 0xAB {
            return false;
        }
    }
    
    unsafe {
        // Disable interrupts
        write_reg(regs::IER, 0x00);
        
        // Set baud rate divisor (115200 @ 1.8432MHz)
        write_reg(regs::LCR, lcr::DLAB);
        write_reg(regs::DLL, 0x01);  // Divisor low
        write_reg(regs::DLH, 0x00);  // Divisor high
        
        // 8 data bits, no parity, 1 stop bit
        write_reg(regs::LCR, lcr::WORD_8);
        
        // Enable and reset FIFOs
        write_reg(regs::FCR, fcr::ENABLE | fcr::CLEAR_RX | fcr::CLEAR_TX | fcr::TRIGGER_14);
        
        // Enable receive interrupt
        write_reg(regs::IER, ier::RX_AVAILABLE);
    }
    
    true
}

/// Write to UART register
#[inline]
unsafe fn write_reg(offset: usize, value: u8) {
    write_volatile((UART_BASE + offset) as *mut u8, value);
}

/// Read from UART register
#[inline]
unsafe fn read_reg(offset: usize) -> u8 {
    read_volatile((UART_BASE + offset) as *const u8)
}

/// Send a byte
pub fn putchar(ch: u8) {
    if *USE_SBI.lock() {
        sbi::console_putchar(ch);
    } else {
        // Wait for transmitter to be ready
        unsafe {
            while (read_reg(regs::LSR) & lsr::THR_EMPTY) == 0 {
                core::hint::spin_loop();
            }
            write_reg(regs::THR, ch);
        }
    }
}

/// Receive a byte (non-blocking)
pub fn getchar() -> Option<u8> {
    if *USE_SBI.lock() {
        sbi::console_getchar()
    } else {
        unsafe {
            if (read_reg(regs::LSR) & lsr::DATA_READY) != 0 {
                Some(read_reg(regs::RBR))
            } else {
                None
            }
        }
    }
}

/// Check if data is available
pub fn has_data() -> bool {
    if *USE_SBI.lock() {
        false  // SBI doesn't provide this
    } else {
        unsafe { (read_reg(regs::LSR) & lsr::DATA_READY) != 0 }
    }
}

/// Print a string
pub fn puts(s: &str) {
    for byte in s.bytes() {
        if byte == b'\n' {
            putchar(b'\r');
        }
        putchar(byte);
    }
}

/// Input buffer for shell/console
const INPUT_BUFFER_SIZE: usize = 256;
static INPUT_BUFFER: Mutex<InputBuffer> = Mutex::new(InputBuffer::new());

struct InputBuffer {
    data: [u8; INPUT_BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl InputBuffer {
    const fn new() -> Self {
        Self {
            data: [0; INPUT_BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }
    
    fn push(&mut self, ch: u8) -> bool {
        let next = (self.head + 1) % INPUT_BUFFER_SIZE;
        if next == self.tail {
            return false; // Buffer full
        }
        self.data[self.head] = ch;
        self.head = next;
        true
    }
    
    fn pop(&mut self) -> Option<u8> {
        if self.tail == self.head {
            return None; // Buffer empty
        }
        let ch = self.data[self.tail];
        self.tail = (self.tail + 1) % INPUT_BUFFER_SIZE;
        Some(ch)
    }
    
    fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

/// Handle UART interrupt
pub fn handle_interrupt() {
    // Read all available characters
    while let Some(ch) = getchar() {
        // Echo back
        if ch == b'\r' {
            putchar(b'\n');
        } else {
            putchar(ch);
        }
        
        // Add to input buffer for shell/console
        let mut buf = INPUT_BUFFER.lock();
        if !buf.push(ch) {
            // Buffer full - drop the character
        }
    }
}

/// Read a character from the input buffer (for shell)
pub fn read_input() -> Option<u8> {
    INPUT_BUFFER.lock().pop()
}

/// Check if there's input available
pub fn has_input() -> bool {
    !INPUT_BUFFER.lock().is_empty()
}

/// Implement core::fmt::Write for UART
pub struct UartWriter;

impl core::fmt::Write for UartWriter {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        puts(s);
        Ok(())
    }
}

/// Print macro
#[macro_export]
macro_rules! print_riscv {
    ($($arg:tt)*) => ({
        use core::fmt::Write;
        let _ = write!($crate::arch::riscv64::uart::UartWriter, $($arg)*);
    });
}

/// Println macro
#[macro_export]
macro_rules! println_riscv {
    () => ($crate::print_riscv!("\n"));
    ($($arg:tt)*) => ($crate::print_riscv!("{}\n", format_args!($($arg)*)));
}
