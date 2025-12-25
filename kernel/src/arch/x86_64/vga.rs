//! # VGA Text Mode Driver
//!
//! Provides text output to the VGA text mode buffer at 0xB8000.
//! This allows visible output on the QEMU display.

use core::fmt;
use spin::Mutex;

/// VGA text buffer address
const VGA_BUFFER: usize = 0xB8000;

/// VGA text mode dimensions
const VGA_WIDTH: usize = 80;
const VGA_HEIGHT: usize = 25;

/// VGA color codes
#[allow(dead_code)]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A VGA color code combining foreground and background colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    /// Create a new color code from foreground and background colors
    pub const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// A character with its color on the VGA buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_char: u8,
    color_code: ColorCode,
}

/// Represents the VGA text buffer
#[repr(transparent)]
struct Buffer {
    chars: [[ScreenChar; VGA_WIDTH]; VGA_HEIGHT],
}

/// VGA text mode writer
pub struct Writer {
    column: usize,
    row: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    /// Create a new writer with default colors (light gray on black)
    /// 
    /// # Safety
    /// 
    /// This function is unsafe because it creates a mutable reference to
    /// the VGA buffer at a fixed memory address. Only one Writer should
    /// exist at a time.
    pub unsafe fn new() -> Writer {
        Writer {
            column: 0,
            row: 0,
            color_code: ColorCode::new(Color::LightGray, Color::Black),
            buffer: unsafe { &mut *(VGA_BUFFER as *mut Buffer) },
        }
    }

    /// Set the current color code
    pub fn set_color(&mut self, foreground: Color, background: Color) {
        self.color_code = ColorCode::new(foreground, background);
    }

    /// Clear the screen
    pub fn clear(&mut self) {
        let blank = ScreenChar {
            ascii_char: b' ',
            color_code: self.color_code,
        };
        for row in 0..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                // SAFETY: Using volatile write to prevent optimization
                unsafe {
                    core::ptr::write_volatile(&mut self.buffer.chars[row][col], blank);
                }
            }
        }
        self.column = 0;
        self.row = 0;
    }

    /// Write a single byte to the screen
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.column = 0,
            b'\t' => {
                // Tab to next 8-column boundary
                let spaces = 8 - (self.column % 8);
                for _ in 0..spaces {
                    self.write_byte(b' ');
                }
            }
            byte => {
                if self.column >= VGA_WIDTH {
                    self.new_line();
                }

                let row = self.row;
                let col = self.column;

                let screen_char = ScreenChar {
                    ascii_char: byte,
                    color_code: self.color_code,
                };

                // SAFETY: Using volatile write to prevent optimization
                unsafe {
                    core::ptr::write_volatile(&mut self.buffer.chars[row][col], screen_char);
                }

                self.column += 1;
            }
        }
    }

    /// Write a string to the screen
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // Printable ASCII or newline/tab
                0x20..=0x7e | b'\n' | b'\r' | b'\t' => self.write_byte(byte),
                // Non-printable, show a placeholder
                _ => self.write_byte(0xfe),
            }
        }
    }

    /// Move to the next line, scrolling if necessary
    fn new_line(&mut self) {
        self.column = 0;
        if self.row < VGA_HEIGHT - 1 {
            self.row += 1;
        } else {
            self.scroll();
        }
    }

    /// Scroll the screen up by one line
    fn scroll(&mut self) {
        // Move all lines up by one
        for row in 1..VGA_HEIGHT {
            for col in 0..VGA_WIDTH {
                let char_to_move = unsafe {
                    core::ptr::read_volatile(&self.buffer.chars[row][col])
                };
                unsafe {
                    core::ptr::write_volatile(&mut self.buffer.chars[row - 1][col], char_to_move);
                }
            }
        }
        
        // Clear the last line
        let blank = ScreenChar {
            ascii_char: b' ',
            color_code: self.color_code,
        };
        for col in 0..VGA_WIDTH {
            unsafe {
                core::ptr::write_volatile(&mut self.buffer.chars[VGA_HEIGHT - 1][col], blank);
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// SAFETY: The VGA buffer is at a fixed address and we control access through the mutex
unsafe impl Send for Writer {}

/// Global VGA writer instance
pub static VGA_WRITER: Mutex<Option<Writer>> = Mutex::new(None);

/// Initialize the VGA text mode driver
pub fn init() {
    let mut writer = VGA_WRITER.lock();
    // SAFETY: We're initializing once during kernel boot
    *writer = Some(unsafe { Writer::new() });
    
    if let Some(ref mut w) = *writer {
        w.clear();
        w.set_color(Color::LightCyan, Color::Black);
    }
}

/// Print to the VGA display
#[macro_export]
macro_rules! vga_print {
    ($($arg:tt)*) => ($crate::arch::x86_64::vga::_print(format_args!($($arg)*)));
}

/// Print with newline to the VGA display
#[macro_export]
macro_rules! vga_println {
    () => ($crate::vga_print!("\n"));
    ($($arg:tt)*) => ($crate::vga_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    if let Some(ref mut writer) = *VGA_WRITER.lock() {
        writer.write_fmt(args).unwrap();
    }
}

/// Set VGA text color
pub fn set_color(foreground: Color, background: Color) {
    if let Some(ref mut writer) = *VGA_WRITER.lock() {
        writer.set_color(foreground, background);
    }
}

/// Clear the VGA screen
pub fn clear() {
    if let Some(ref mut writer) = *VGA_WRITER.lock() {
        writer.clear();
    }
}

/// Handle backspace - move cursor back and erase character
pub fn backspace() {
    if let Some(ref mut writer) = *VGA_WRITER.lock() {
        if writer.column > 0 {
            writer.column -= 1;
            // Write a space to erase the character
            let blank = ScreenChar {
                ascii_char: b' ',
                color_code: writer.color_code,
            };
            unsafe {
                core::ptr::write_volatile(
                    &mut writer.buffer.chars[writer.row][writer.column],
                    blank
                );
            }
        }
    }
}
