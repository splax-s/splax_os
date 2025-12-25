//! # PS/2 Keyboard Driver
//!
//! Converts PS/2 scancodes (Set 1) to ASCII characters.

use spin::Mutex;

/// Keyboard state
pub struct Keyboard {
    /// Shift key pressed
    shift: bool,
    /// Caps lock active
    caps_lock: bool,
    /// Ctrl key pressed
    ctrl: bool,
    /// Alt key pressed
    alt: bool,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self {
            shift: false,
            caps_lock: false,
            ctrl: false,
            alt: false,
        }
    }

    /// Process a scancode and return the corresponding character (if any)
    pub fn process_scancode(&mut self, scancode: u8) -> Option<KeyEvent> {
        // Check if this is a key release (bit 7 set)
        let released = scancode & 0x80 != 0;
        let code = scancode & 0x7F;

        match code {
            // Modifier keys
            0x2A | 0x36 => {
                // Left/Right Shift
                self.shift = !released;
                None
            }
            0x1D => {
                // Ctrl
                self.ctrl = !released;
                None
            }
            0x38 => {
                // Alt
                self.alt = !released;
                None
            }
            0x3A if !released => {
                // Caps Lock (toggle on press)
                self.caps_lock = !self.caps_lock;
                None
            }
            _ if released => None, // Ignore other key releases
            _ => {
                // Convert to character
                let ch = self.scancode_to_char(code);
                ch.map(|c| KeyEvent {
                    character: c,
                    ctrl: self.ctrl,
                    alt: self.alt,
                })
            }
        }
    }

    fn scancode_to_char(&self, code: u8) -> Option<char> {
        let shifted = self.shift ^ self.caps_lock;

        // PS/2 Scancode Set 1 mapping
        let ch = match code {
            // Number row
            0x02 => if self.shift { '!' } else { '1' },
            0x03 => if self.shift { '@' } else { '2' },
            0x04 => if self.shift { '#' } else { '3' },
            0x05 => if self.shift { '$' } else { '4' },
            0x06 => if self.shift { '%' } else { '5' },
            0x07 => if self.shift { '^' } else { '6' },
            0x08 => if self.shift { '&' } else { '7' },
            0x09 => if self.shift { '*' } else { '8' },
            0x0A => if self.shift { '(' } else { '9' },
            0x0B => if self.shift { ')' } else { '0' },
            0x0C => if self.shift { '_' } else { '-' },
            0x0D => if self.shift { '+' } else { '=' },

            // Top row (QWERTY)
            0x10 => if shifted { 'Q' } else { 'q' },
            0x11 => if shifted { 'W' } else { 'w' },
            0x12 => if shifted { 'E' } else { 'e' },
            0x13 => if shifted { 'R' } else { 'r' },
            0x14 => if shifted { 'T' } else { 't' },
            0x15 => if shifted { 'Y' } else { 'y' },
            0x16 => if shifted { 'U' } else { 'u' },
            0x17 => if shifted { 'I' } else { 'i' },
            0x18 => if shifted { 'O' } else { 'o' },
            0x19 => if shifted { 'P' } else { 'p' },
            0x1A => if self.shift { '{' } else { '[' },
            0x1B => if self.shift { '}' } else { ']' },

            // Home row (ASDF)
            0x1E => if shifted { 'A' } else { 'a' },
            0x1F => if shifted { 'S' } else { 's' },
            0x20 => if shifted { 'D' } else { 'd' },
            0x21 => if shifted { 'F' } else { 'f' },
            0x22 => if shifted { 'G' } else { 'g' },
            0x23 => if shifted { 'H' } else { 'h' },
            0x24 => if shifted { 'J' } else { 'j' },
            0x25 => if shifted { 'K' } else { 'k' },
            0x26 => if shifted { 'L' } else { 'l' },
            0x27 => if self.shift { ':' } else { ';' },
            0x28 => if self.shift { '"' } else { '\'' },
            0x29 => if self.shift { '~' } else { '`' },

            // Bottom row (ZXCV)
            0x2B => if self.shift { '|' } else { '\\' },
            0x2C => if shifted { 'Z' } else { 'z' },
            0x2D => if shifted { 'X' } else { 'x' },
            0x2E => if shifted { 'C' } else { 'c' },
            0x2F => if shifted { 'V' } else { 'v' },
            0x30 => if shifted { 'B' } else { 'b' },
            0x31 => if shifted { 'N' } else { 'n' },
            0x32 => if shifted { 'M' } else { 'm' },
            0x33 => if self.shift { '<' } else { ',' },
            0x34 => if self.shift { '>' } else { '.' },
            0x35 => if self.shift { '?' } else { '/' },

            // Special keys
            0x0E => '\x08', // Backspace
            0x0F => '\t',   // Tab
            0x1C => '\n',   // Enter
            0x39 => ' ',    // Space

            _ => return None,
        };

        Some(ch)
    }
}

/// A keyboard event with the character and modifier state
#[derive(Debug, Clone, Copy)]
pub struct KeyEvent {
    pub character: char,
    pub ctrl: bool,
    pub alt: bool,
}

/// Global keyboard instance
pub static KEYBOARD: Mutex<Keyboard> = Mutex::new(Keyboard::new());

/// Process a scancode from the keyboard interrupt
pub fn handle_scancode(scancode: u8) -> Option<KeyEvent> {
    KEYBOARD.lock().process_scancode(scancode)
}
