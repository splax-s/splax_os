//! # PS/2 Keyboard Driver
//!
//! Converts PS/2 scancodes (Set 1) to ASCII characters.
//! Supports extended keys (arrow keys, etc.) via 0xE0 prefix.
//!
//! ## Lock-Free Ring Buffer (Linux-style)
//! The keyboard interrupt handler NEVER blocks. Keypresses are stored
//! atomically in a ring buffer and consumed by the shell main loop.
//! This ensures fast, reliable typing like Linux.

use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

/// Special key codes (non-character keys)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKey {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
}

// ============================================================================
// Lock-Free Keyboard Ring Buffer (Linux-style)
// ============================================================================

const KEYBOARD_BUFFER_SIZE: usize = 256;

/// A lock-free ring buffer for keyboard events
/// Uses atomic head/tail pointers - interrupt writes, main loop reads
pub struct KeyboardRingBuffer {
    /// The buffer storing key events (packed as usize for atomic access)
    buffer: [AtomicUsize; KEYBOARD_BUFFER_SIZE],
    /// Write position (only written by interrupt handler)
    head: AtomicUsize,
    /// Read position (only written by consumer)
    tail: AtomicUsize,
}

impl KeyboardRingBuffer {
    /// Create a new empty keyboard buffer
    pub const fn new() -> Self {
        // const initializer for atomic array
        const ZERO: AtomicUsize = AtomicUsize::new(0);
        Self {
            buffer: [ZERO; KEYBOARD_BUFFER_SIZE],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }
    
    /// Pack a KeyEvent into a usize for atomic storage
    /// Format: [valid:1][ctrl:1][alt:1][is_special:1][data:28]
    #[inline]
    fn pack_event(event: &KeyEvent) -> usize {
        let mut packed: usize = 1; // valid bit
        if event.ctrl { packed |= 2; }
        if event.alt { packed |= 4; }
        
        if let Some(special) = event.special {
            packed |= 8; // is_special flag
            let special_id = match special {
                SpecialKey::ArrowUp => 0,
                SpecialKey::ArrowDown => 1,
                SpecialKey::ArrowLeft => 2,
                SpecialKey::ArrowRight => 3,
                SpecialKey::Home => 4,
                SpecialKey::End => 5,
                SpecialKey::PageUp => 6,
                SpecialKey::PageDown => 7,
                SpecialKey::Insert => 8,
                SpecialKey::Delete => 9,
                SpecialKey::F1 => 10,
                SpecialKey::F2 => 11,
                SpecialKey::F3 => 12,
                SpecialKey::F4 => 13,
                SpecialKey::F5 => 14,
                SpecialKey::F6 => 15,
                SpecialKey::F7 => 16,
                SpecialKey::F8 => 17,
                SpecialKey::F9 => 18,
                SpecialKey::F10 => 19,
                SpecialKey::F11 => 20,
                SpecialKey::F12 => 21,
            };
            packed |= special_id << 4;
        } else if let Some(c) = event.character {
            packed |= (c as usize) << 4;
        }
        
        packed
    }
    
    /// Unpack a KeyEvent from a usize
    #[inline]
    fn unpack_event(packed: usize) -> Option<KeyEvent> {
        if packed == 0 || (packed & 1) == 0 {
            return None; // Not valid
        }
        
        let ctrl = (packed & 2) != 0;
        let alt = (packed & 4) != 0;
        let is_special = (packed & 8) != 0;
        let data = packed >> 4;
        
        if is_special {
            let special = match data {
                0 => SpecialKey::ArrowUp,
                1 => SpecialKey::ArrowDown,
                2 => SpecialKey::ArrowLeft,
                3 => SpecialKey::ArrowRight,
                4 => SpecialKey::Home,
                5 => SpecialKey::End,
                6 => SpecialKey::PageUp,
                7 => SpecialKey::PageDown,
                8 => SpecialKey::Insert,
                9 => SpecialKey::Delete,
                10 => SpecialKey::F1,
                11 => SpecialKey::F2,
                12 => SpecialKey::F3,
                13 => SpecialKey::F4,
                14 => SpecialKey::F5,
                15 => SpecialKey::F6,
                16 => SpecialKey::F7,
                17 => SpecialKey::F8,
                18 => SpecialKey::F9,
                19 => SpecialKey::F10,
                20 => SpecialKey::F11,
                21 => SpecialKey::F12,
                _ => return None,
            };
            Some(KeyEvent { character: None, special: Some(special), ctrl, alt })
        } else {
            let c = char::from_u32(data as u32)?;
            Some(KeyEvent { character: Some(c), special: None, ctrl, alt })
        }
    }
    
    /// Push a key event into the buffer (called from interrupt - NEVER blocks)
    /// Returns true if event was stored, false if buffer was full (overwrites oldest)
    #[inline]
    pub fn push(&self, event: KeyEvent) -> bool {
        let packed = Self::pack_event(&event);
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let next_head = (head + 1) % KEYBOARD_BUFFER_SIZE;
        
        if next_head == tail {
            // Buffer full - overwrite oldest (Linux behavior: newer input priority)
            self.buffer[head].store(packed, Ordering::Release);
            self.head.store(next_head, Ordering::Release);
            self.tail.store((tail + 1) % KEYBOARD_BUFFER_SIZE, Ordering::Release);
        } else {
            self.buffer[head].store(packed, Ordering::Release);
            self.head.store(next_head, Ordering::Release);
        }
        true
    }
    
    /// Pop a key event from the buffer (called from main loop)
    /// Returns None if buffer is empty
    #[inline]
    pub fn pop(&self) -> Option<KeyEvent> {
        loop {
            let tail = self.tail.load(Ordering::Relaxed);
            let head = self.head.load(Ordering::Acquire);
            
            if tail == head {
                return None; // Buffer empty
            }
            
            let packed = self.buffer[tail].swap(0, Ordering::Acquire);
            self.tail.store((tail + 1) % KEYBOARD_BUFFER_SIZE, Ordering::Release);
            
            if let Some(event) = Self::unpack_event(packed) {
                return Some(event);
            }
            // Slot was cleared by race - try next
        }
    }
    
    /// Check if there are events waiting
    #[inline]
    pub fn has_events(&self) -> bool {
        self.head.load(Ordering::Relaxed) != self.tail.load(Ordering::Relaxed)
    }
    
    /// Get number of pending events (approximate)
    #[inline]
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        if head >= tail {
            head - tail
        } else {
            KEYBOARD_BUFFER_SIZE - tail + head
        }
    }
}

/// Global keyboard input buffer - interrupt handler writes, shell reads
pub static KEYBOARD_BUFFER: KeyboardRingBuffer = KeyboardRingBuffer::new();

/// Check if Ctrl+C was pressed (consumes the event if found)
/// Returns true if Ctrl+C was detected
#[inline]
pub fn check_ctrl_c() -> bool {
    // Peek at keyboard buffer for Ctrl+C without removing other keys
    let tail = KEYBOARD_BUFFER.tail.load(core::sync::atomic::Ordering::Relaxed);
    let head = KEYBOARD_BUFFER.head.load(core::sync::atomic::Ordering::Acquire);
    
    let mut current = tail;
    while current != head {
        let packed = KEYBOARD_BUFFER.buffer[current].load(core::sync::atomic::Ordering::Relaxed);
        if let Some(event) = KeyboardRingBuffer::unpack_event(packed) {
            if event.ctrl && event.character == Some('c') {
                // Consume this event by clearing it
                KEYBOARD_BUFFER.buffer[current].store(0, core::sync::atomic::Ordering::Release);
                return true;
            }
        }
        current = (current + 1) % 256; // KEYBOARD_BUFFER_SIZE
    }
    false
}

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
    /// Extended scancode prefix received (0xE0)
    extended: bool,
}

impl Keyboard {
    pub const fn new() -> Self {
        Self {
            shift: false,
            caps_lock: false,
            ctrl: false,
            alt: false,
            extended: false,
        }
    }

    /// Process a scancode and return the corresponding character (if any)
    pub fn process_scancode(&mut self, scancode: u8) -> Option<KeyEvent> {
        // Check for extended scancode prefix
        if scancode == 0xE0 {
            self.extended = true;
            return None;
        }
        
        // Check if this is a key release (bit 7 set)
        let released = scancode & 0x80 != 0;
        let code = scancode & 0x7F;
        
        // Handle extended scancodes (arrow keys, etc.)
        if self.extended {
            self.extended = false;
            
            if released {
                return None; // Ignore extended key releases
            }
            
            // Extended key mappings
            let special = match code {
                0x48 => Some(SpecialKey::ArrowUp),
                0x50 => Some(SpecialKey::ArrowDown),
                0x4B => Some(SpecialKey::ArrowLeft),
                0x4D => Some(SpecialKey::ArrowRight),
                0x47 => Some(SpecialKey::Home),
                0x4F => Some(SpecialKey::End),
                0x49 => Some(SpecialKey::PageUp),
                0x51 => Some(SpecialKey::PageDown),
                0x52 => Some(SpecialKey::Insert),
                0x53 => Some(SpecialKey::Delete),
                _ => None,
            };
            
            return special.map(|s| KeyEvent {
                character: None,
                special: Some(s),
                ctrl: self.ctrl,
                alt: self.alt,
            });
        }

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
                    character: Some(c),
                    special: None,
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
    pub character: Option<char>,
    pub special: Option<SpecialKey>,
    pub ctrl: bool,
    pub alt: bool,
}

impl KeyEvent {
    /// Check if this is a character key event
    pub fn is_char(&self) -> bool {
        self.character.is_some()
    }
    
    /// Check if this is a special key event
    pub fn is_special(&self) -> bool {
        self.special.is_some()
    }
    
    /// Get the character if this is a character event
    pub fn char(&self) -> Option<char> {
        self.character
    }
}

/// Global keyboard instance
pub static KEYBOARD: Mutex<Keyboard> = Mutex::new(Keyboard::new());

/// Process a scancode from the keyboard interrupt
pub fn handle_scancode(scancode: u8) -> Option<KeyEvent> {
    KEYBOARD.lock().process_scancode(scancode)
}
