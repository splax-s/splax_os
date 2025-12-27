//! # Serial Port Driver for x86_64
//!
//! Simple UART driver for early boot debugging output.

use core::fmt::{self, Write};
use spin::Mutex;

/// COM1 port address.
const COM1: u16 = 0x3F8;

/// Serial port registers (offsets from base).
mod registers {
    pub const DATA: u16 = 0;
    pub const INT_ENABLE: u16 = 1;
    pub const FIFO_CTRL: u16 = 2;
    pub const LINE_CTRL: u16 = 3;
    pub const MODEM_CTRL: u16 = 4;
    pub const LINE_STATUS: u16 = 5;
}

/// Line status register bits.
mod line_status {
    pub const DATA_READY: u8 = 0x01;
    pub const OUTPUT_EMPTY: u8 = 0x20;
}

/// Serial port wrapper.
pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    /// Creates a new serial port.
    pub const fn new(base: u16) -> Self {
        Self { base }
    }

    /// Initializes the serial port.
    pub fn init(&self) {
        unsafe {
            // Disable interrupts
            outb(self.base + registers::INT_ENABLE, 0x00);
            
            // Enable DLAB (set baud rate divisor)
            outb(self.base + registers::LINE_CTRL, 0x80);
            
            // Set baud rate to 115200 (divisor 1)
            outb(self.base + registers::DATA, 0x01);
            outb(self.base + registers::INT_ENABLE, 0x00);
            
            // 8 bits, no parity, one stop bit
            outb(self.base + registers::LINE_CTRL, 0x03);
            
            // Enable FIFO, clear them, 14-byte threshold
            outb(self.base + registers::FIFO_CTRL, 0xC7);
            
            // IRQs enabled, RTS/DSR set
            outb(self.base + registers::MODEM_CTRL, 0x0B);
            
            // Enable receive data available interrupt
            outb(self.base + registers::INT_ENABLE, 0x01);
        }
    }

    /// Checks if the transmit buffer is empty.
    fn is_transmit_empty(&self) -> bool {
        unsafe { (inb(self.base + registers::LINE_STATUS) & line_status::OUTPUT_EMPTY) != 0 }
    }
    
    /// Checks if data is available to read.
    pub fn has_data(&self) -> bool {
        unsafe { (inb(self.base + registers::LINE_STATUS) & line_status::DATA_READY) != 0 }
    }
    
    /// Reads a byte from the serial port (non-blocking).
    pub fn read_byte(&self) -> Option<u8> {
        if self.has_data() {
            Some(unsafe { inb(self.base + registers::DATA) })
        } else {
            None
        }
    }

    /// Writes a byte to the serial port.
    pub fn write_byte(&self, byte: u8) {
        while !self.is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(self.base + registers::DATA, byte);
        }
    }

    /// Writes a string to the serial port.
    pub fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        SerialPort::write_str(self, s);
        Ok(())
    }
}

/// Global serial port instance.
pub static SERIAL: Mutex<SerialPort> = Mutex::new(SerialPort::new(COM1));

/// Initializes the serial port.
pub fn init() {
    SERIAL.lock().init();
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    SERIAL.lock().write_fmt(args).unwrap();
}

/// Print to the serial port
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::arch::x86_64::serial::_print(format_args!($($arg)*)));
}

/// Print with newline to the serial port
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

/// Reads a byte from an I/O port.
#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        core::arch::asm!(
            "in al, dx",
            out("al") value,
            in("dx") port,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

/// Writes a byte to an I/O port.
#[inline(always)]
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}
