//! USB HID (Human Interface Device) Keyboard Driver
//!
//! This module implements USB HID keyboard support including:
//! - HID report parsing
//! - Keyboard scancode translation
//! - Key event handling
//! - LED control (Caps Lock, Num Lock, Scroll Lock)

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

/// HID Usage Page codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UsagePage {
    GenericDesktop = 0x01,
    SimulationControls = 0x02,
    VRControls = 0x03,
    SportControls = 0x04,
    GameControls = 0x05,
    GenericDeviceControls = 0x06,
    Keyboard = 0x07,
    Led = 0x08,
    Button = 0x09,
    Ordinal = 0x0A,
    Telephony = 0x0B,
    Consumer = 0x0C,
    Digitizer = 0x0D,
    Unicode = 0x10,
    AlphanumericDisplay = 0x14,
    MedicalInstruments = 0x40,
    MonitorPages = 0x80,
    PowerPages = 0x84,
    BarCodeScanner = 0x8C,
    Scale = 0x8D,
    MagneticStripeReader = 0x8E,
    Camera = 0x90,
    Arcade = 0x91,
}

/// HID keyboard modifiers
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardModifiers {
    pub left_ctrl: bool,
    pub left_shift: bool,
    pub left_alt: bool,
    pub left_gui: bool,
    pub right_ctrl: bool,
    pub right_shift: bool,
    pub right_alt: bool,
    pub right_gui: bool,
}

impl KeyboardModifiers {
    /// Create from modifier byte
    pub fn from_byte(byte: u8) -> Self {
        Self {
            left_ctrl: (byte & 0x01) != 0,
            left_shift: (byte & 0x02) != 0,
            left_alt: (byte & 0x04) != 0,
            left_gui: (byte & 0x08) != 0,
            right_ctrl: (byte & 0x10) != 0,
            right_shift: (byte & 0x20) != 0,
            right_alt: (byte & 0x40) != 0,
            right_gui: (byte & 0x80) != 0,
        }
    }

    /// Check if any Ctrl key is pressed
    pub fn ctrl(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }

    /// Check if any Shift key is pressed
    pub fn shift(&self) -> bool {
        self.left_shift || self.right_shift
    }

    /// Check if any Alt key is pressed
    pub fn alt(&self) -> bool {
        self.left_alt || self.right_alt
    }

    /// Check if any GUI (Windows/Command) key is pressed
    pub fn gui(&self) -> bool {
        self.left_gui || self.right_gui
    }
}

/// Keyboard LED state
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardLeds {
    pub num_lock: bool,
    pub caps_lock: bool,
    pub scroll_lock: bool,
    pub compose: bool,
    pub kana: bool,
}

impl KeyboardLeds {
    /// Convert to LED report byte
    pub fn to_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.num_lock {
            byte |= 0x01;
        }
        if self.caps_lock {
            byte |= 0x02;
        }
        if self.scroll_lock {
            byte |= 0x04;
        }
        if self.compose {
            byte |= 0x08;
        }
        if self.kana {
            byte |= 0x10;
        }
        byte
    }
}

/// Key event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEvent {
    Pressed(u8),
    Released(u8),
}

/// USB HID keyboard scancode to ASCII mapping
/// Based on USB HID Usage Tables for Keyboard/Keypad Page (0x07)
pub fn scancode_to_char(scancode: u8, shift: bool) -> Option<char> {
    match scancode {
        // Letters A-Z (0x04-0x1D)
        0x04 => Some(if shift { 'A' } else { 'a' }),
        0x05 => Some(if shift { 'B' } else { 'b' }),
        0x06 => Some(if shift { 'C' } else { 'c' }),
        0x07 => Some(if shift { 'D' } else { 'd' }),
        0x08 => Some(if shift { 'E' } else { 'e' }),
        0x09 => Some(if shift { 'F' } else { 'f' }),
        0x0A => Some(if shift { 'G' } else { 'g' }),
        0x0B => Some(if shift { 'H' } else { 'h' }),
        0x0C => Some(if shift { 'I' } else { 'i' }),
        0x0D => Some(if shift { 'J' } else { 'j' }),
        0x0E => Some(if shift { 'K' } else { 'k' }),
        0x0F => Some(if shift { 'L' } else { 'l' }),
        0x10 => Some(if shift { 'M' } else { 'm' }),
        0x11 => Some(if shift { 'N' } else { 'n' }),
        0x12 => Some(if shift { 'O' } else { 'o' }),
        0x13 => Some(if shift { 'P' } else { 'p' }),
        0x14 => Some(if shift { 'Q' } else { 'q' }),
        0x15 => Some(if shift { 'R' } else { 'r' }),
        0x16 => Some(if shift { 'S' } else { 's' }),
        0x17 => Some(if shift { 'T' } else { 't' }),
        0x18 => Some(if shift { 'U' } else { 'u' }),
        0x19 => Some(if shift { 'V' } else { 'v' }),
        0x1A => Some(if shift { 'W' } else { 'w' }),
        0x1B => Some(if shift { 'X' } else { 'x' }),
        0x1C => Some(if shift { 'Y' } else { 'y' }),
        0x1D => Some(if shift { 'Z' } else { 'z' }),
        
        // Numbers 1-0 (0x1E-0x27)
        0x1E => Some(if shift { '!' } else { '1' }),
        0x1F => Some(if shift { '@' } else { '2' }),
        0x20 => Some(if shift { '#' } else { '3' }),
        0x21 => Some(if shift { '$' } else { '4' }),
        0x22 => Some(if shift { '%' } else { '5' }),
        0x23 => Some(if shift { '^' } else { '6' }),
        0x24 => Some(if shift { '&' } else { '7' }),
        0x25 => Some(if shift { '*' } else { '8' }),
        0x26 => Some(if shift { '(' } else { '9' }),
        0x27 => Some(if shift { ')' } else { '0' }),
        
        // Enter, Escape, Backspace, Tab, Space
        0x28 => Some('\n'),  // Enter
        0x29 => Some('\x1B'), // Escape
        0x2A => Some('\x08'), // Backspace
        0x2B => Some('\t'),   // Tab
        0x2C => Some(' '),    // Space
        
        // Punctuation
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),
        
        // Keypad
        0x54 => Some('/'),  // Keypad /
        0x55 => Some('*'),  // Keypad *
        0x56 => Some('-'),  // Keypad -
        0x57 => Some('+'),  // Keypad +
        0x58 => Some('\n'), // Keypad Enter
        0x59 => Some('1'),  // Keypad 1
        0x5A => Some('2'),  // Keypad 2
        0x5B => Some('3'),  // Keypad 3
        0x5C => Some('4'),  // Keypad 4
        0x5D => Some('5'),  // Keypad 5
        0x5E => Some('6'),  // Keypad 6
        0x5F => Some('7'),  // Keypad 7
        0x60 => Some('8'),  // Keypad 8
        0x61 => Some('9'),  // Keypad 9
        0x62 => Some('0'),  // Keypad 0
        0x63 => Some('.'),  // Keypad .
        
        _ => None,
    }
}

/// Get scancode name (for debugging)
pub fn scancode_name(scancode: u8) -> &'static str {
    match scancode {
        0x04..=0x1D => "Letter",
        0x1E..=0x27 => "Number",
        0x28 => "Enter",
        0x29 => "Escape",
        0x2A => "Backspace",
        0x2B => "Tab",
        0x2C => "Space",
        0x2D => "Minus",
        0x2E => "Equal",
        0x2F => "LeftBracket",
        0x30 => "RightBracket",
        0x31 => "Backslash",
        0x33 => "Semicolon",
        0x34 => "Quote",
        0x35 => "Grave",
        0x36 => "Comma",
        0x37 => "Period",
        0x38 => "Slash",
        0x39 => "CapsLock",
        0x3A..=0x45 => "F-Key",
        0x46 => "PrintScreen",
        0x47 => "ScrollLock",
        0x48 => "Pause",
        0x49 => "Insert",
        0x4A => "Home",
        0x4B => "PageUp",
        0x4C => "Delete",
        0x4D => "End",
        0x4E => "PageDown",
        0x4F => "RightArrow",
        0x50 => "LeftArrow",
        0x51 => "DownArrow",
        0x52 => "UpArrow",
        0x53 => "NumLock",
        0x54..=0x63 => "Keypad",
        0xE0 => "LeftCtrl",
        0xE1 => "LeftShift",
        0xE2 => "LeftAlt",
        0xE3 => "LeftGUI",
        0xE4 => "RightCtrl",
        0xE5 => "RightShift",
        0xE6 => "RightAlt",
        0xE7 => "RightGUI",
        _ => "Unknown",
    }
}

/// Standard HID keyboard report (8 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C, packed)]
pub struct KeyboardReport {
    /// Modifier keys (Ctrl, Shift, Alt, GUI)
    pub modifiers: u8,
    /// Reserved byte
    pub reserved: u8,
    /// Up to 6 simultaneous key presses
    pub keys: [u8; 6],
}

impl KeyboardReport {
    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        
        Some(Self {
            modifiers: data[0],
            reserved: data[1],
            keys: [data[2], data[3], data[4], data[5], data[6], data[7]],
        })
    }

    /// Get modifiers struct
    pub fn modifiers(&self) -> KeyboardModifiers {
        KeyboardModifiers::from_byte(self.modifiers)
    }

    /// Check if a key is pressed in this report
    pub fn key_pressed(&self, scancode: u8) -> bool {
        self.keys.iter().any(|&k| k == scancode)
    }

    /// Get pressed keys (non-zero entries)
    pub fn pressed_keys(&self) -> impl Iterator<Item = u8> + '_ {
        self.keys.iter().filter(|&&k| k != 0).copied()
    }

    /// Check for rollover error (all keys = 0x01)
    pub fn is_rollover_error(&self) -> bool {
        self.keys.iter().all(|&k| k == 0x01)
    }
}

/// USB HID Keyboard driver
pub struct UsbKeyboard {
    /// USB device address
    pub device_address: u8,
    /// Interface number
    pub interface: u8,
    /// Interrupt IN endpoint
    pub endpoint_in: u8,
    /// Current LED state
    pub leds: KeyboardLeds,
    /// Previous report (for detecting key changes)
    previous_report: KeyboardReport,
    /// Event queue
    event_queue: VecDeque<KeyEvent>,
    /// Character queue (translated key presses)
    char_queue: VecDeque<char>,
    /// Is keyboard attached
    attached: AtomicBool,
}

impl UsbKeyboard {
    /// Create a new USB keyboard driver
    pub fn new(device_address: u8, interface: u8, endpoint_in: u8) -> Self {
        Self {
            device_address,
            interface,
            endpoint_in,
            leds: KeyboardLeds::default(),
            previous_report: KeyboardReport::default(),
            event_queue: VecDeque::with_capacity(64),
            char_queue: VecDeque::with_capacity(64),
            attached: AtomicBool::new(true),
        }
    }

    /// Process a new keyboard report
    pub fn process_report(&mut self, report: KeyboardReport) {
        // Check for rollover error
        if report.is_rollover_error() {
            return;
        }

        let modifiers = report.modifiers();

        // Find released keys (in previous but not in current)
        for &prev_key in &self.previous_report.keys {
            if prev_key != 0 && !report.key_pressed(prev_key) {
                self.event_queue.push_back(KeyEvent::Released(prev_key));
            }
        }

        // Find pressed keys (in current but not in previous)
        for key in report.pressed_keys() {
            if !self.previous_report.key_pressed(key) {
                self.event_queue.push_back(KeyEvent::Pressed(key));

                // Translate to character
                if let Some(ch) = scancode_to_char(key, modifiers.shift()) {
                    // Handle Caps Lock for letters
                    let ch = if self.leds.caps_lock && ch.is_ascii_alphabetic() {
                        if modifiers.shift() {
                            ch.to_ascii_lowercase()
                        } else {
                            ch.to_ascii_uppercase()
                        }
                    } else {
                        ch
                    };
                    
                    self.char_queue.push_back(ch);
                }

                // Handle Caps Lock toggle
                if key == 0x39 {
                    self.leds.caps_lock = !self.leds.caps_lock;
                }
                // Handle Num Lock toggle
                if key == 0x53 {
                    self.leds.num_lock = !self.leds.num_lock;
                }
                // Handle Scroll Lock toggle
                if key == 0x47 {
                    self.leds.scroll_lock = !self.leds.scroll_lock;
                }
            }
        }

        self.previous_report = report;
    }

    /// Get next key event
    pub fn next_event(&mut self) -> Option<KeyEvent> {
        self.event_queue.pop_front()
    }

    /// Get next character (translated from key press)
    pub fn next_char(&mut self) -> Option<char> {
        self.char_queue.pop_front()
    }

    /// Check if there are pending events
    pub fn has_events(&self) -> bool {
        !self.event_queue.is_empty()
    }

    /// Check if there are pending characters
    pub fn has_chars(&self) -> bool {
        !self.char_queue.is_empty()
    }

    /// Check if keyboard is attached
    pub fn is_attached(&self) -> bool {
        self.attached.load(Ordering::Relaxed)
    }

    /// Mark keyboard as detached
    pub fn detach(&self) {
        self.attached.store(false, Ordering::Relaxed);
    }
}

/// Global USB keyboard state
static USB_KEYBOARDS: Mutex<Vec<UsbKeyboard>> = Mutex::new(Vec::new());
static USB_KEYBOARD_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Initialize USB keyboard subsystem
pub fn init() {
    USB_KEYBOARD_INITIALIZED.store(true, Ordering::SeqCst);
}

/// Register a new USB keyboard
pub fn register_keyboard(keyboard: UsbKeyboard) {
    let mut keyboards = USB_KEYBOARDS.lock();
    keyboards.push(keyboard);
}

/// Get next character from any USB keyboard
pub fn get_char() -> Option<char> {
    let mut keyboards = USB_KEYBOARDS.lock();
    for keyboard in keyboards.iter_mut() {
        if let Some(ch) = keyboard.next_char() {
            return Some(ch);
        }
    }
    None
}

/// Check if any USB keyboard has pending input
pub fn has_input() -> bool {
    let keyboards = USB_KEYBOARDS.lock();
    keyboards.iter().any(|k| k.has_chars())
}

/// Get USB keyboard count
pub fn keyboard_count() -> usize {
    USB_KEYBOARDS.lock().len()
}

/// Poll all keyboards (call from USB interrupt handler)
/// 
/// This function should be called periodically (e.g., from a timer interrupt)
/// or when a USB interrupt indicates new data is available.
pub fn poll_keyboards() {
    let mut keyboards = USB_KEYBOARDS.lock();
    
    for keyboard in keyboards.iter_mut() {
        if !keyboard.is_attached() {
            continue;
        }
        
        // Check if we have a USB controller to poll
        // In a hardware scenario, we would:
        // 1. Read from the interrupt endpoint using the xHCI/EHCI controller
        // 2. Parse the 8-byte HID report
        // 3. Process the report to generate key events
        
        // Poll the interrupt IN endpoint for new data
        let mut report_buffer = [0u8; 8];
        
        // Use the USB subsystem to read from the interrupt endpoint
        if let Some(subsystem) = crate::usb::get_usb_subsystem() {
            let result = subsystem.interrupt_transfer_in(
                keyboard.device_address,
                keyboard.endpoint_in,
                &mut report_buffer,
            );
            
            match result {
                super::TransferResult::Success(len) if len >= 8 => {
                    // Parse and process the keyboard report
                    if let Some(report) = KeyboardReport::from_bytes(&report_buffer) {
                        keyboard.process_report(report);
                        
                        // Update LEDs if they changed
                        let _ = update_keyboard_leds(keyboard);
                    }
                }
                super::TransferResult::Timeout | super::TransferResult::NotResponding => {
                    // No new data available, this is normal
                }
                _ => {
                    // Other errors might indicate the device was disconnected
                }
            }
        }
    }
}

/// Update keyboard LEDs based on current state
fn update_keyboard_leds(keyboard: &UsbKeyboard) -> Result<(), &'static str> {
    if let Some(subsystem) = crate::usb::get_usb_subsystem() {
        let led_byte = keyboard.leds.to_byte();
        
        // HID SET_REPORT request for output report (LEDs)
        // bmRequestType: 0x21 (Host to device, class, interface)
        // bRequest: 0x09 (SET_REPORT)
        // wValue: 0x0200 (Report Type: Output, Report ID: 0)
        // wIndex: interface number
        // wLength: 1
        let setup = super::SetupPacket::new(
            0x21,  // Host to device, class, interface
            0x09,  // SET_REPORT
            0x0200, // Output report, report ID 0
            keyboard.interface as u16,
            1,
        );
        
        let mut data = [led_byte];
        match subsystem.control_transfer_out(keyboard.device_address, setup, &mut data) {
            super::TransferResult::Success(_) => Ok(()),
            _ => Err("Failed to set keyboard LEDs"),
        }
    } else {
        Err("USB subsystem not available")
    }
}

/// HID Report Descriptor Parser (for boot protocol keyboards)
#[derive(Debug, Clone)]
pub struct HidReportDescriptor {
    /// Raw descriptor data
    pub data: Vec<u8>,
    /// Parsed items
    pub items: Vec<HidItem>,
}

/// HID Item types
#[derive(Debug, Clone, Copy)]
pub enum HidItemType {
    Main,
    Global,
    Local,
    Reserved,
}

/// HID Item
#[derive(Debug, Clone)]
pub struct HidItem {
    /// Item type
    pub item_type: HidItemType,
    /// Item tag
    pub tag: u8,
    /// Item data
    pub data: u32,
    /// Item size
    pub size: u8,
}

impl HidReportDescriptor {
    /// Parse HID report descriptor
    pub fn parse(data: &[u8]) -> Self {
        let mut items = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let prefix = data[offset];
            
            // Long item
            if prefix == 0xFE {
                if offset + 2 < data.len() {
                    let size = data[offset + 1];
                    offset += 3 + size as usize;
                }
                continue;
            }

            // Short item
            let size = match prefix & 0x03 {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 4,
                _ => 0,
            };

            let item_type = match (prefix >> 2) & 0x03 {
                0 => HidItemType::Main,
                1 => HidItemType::Global,
                2 => HidItemType::Local,
                _ => HidItemType::Reserved,
            };

            let tag = (prefix >> 4) & 0x0F;

            let item_data = if offset + 1 + size as usize <= data.len() {
                match size {
                    0 => 0,
                    1 => data[offset + 1] as u32,
                    2 => u16::from_le_bytes([data[offset + 1], data[offset + 2]]) as u32,
                    4 => u32::from_le_bytes([
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                        data[offset + 4],
                    ]),
                    _ => 0,
                }
            } else {
                0
            };

            items.push(HidItem {
                item_type,
                tag,
                data: item_data,
                size,
            });

            offset += 1 + size as usize;
        }

        Self {
            data: data.to_vec(),
            items,
        }
    }
}

/// Set keyboard boot protocol (for HID keyboards)
/// 
/// This sends a SET_PROTOCOL request to switch the keyboard to boot protocol mode.
/// Boot protocol uses a fixed 8-byte report format which is simpler to parse.
/// 
/// # Arguments
/// * `device_address` - USB device address (1-127)
/// * `interface` - HID interface number
pub fn set_boot_protocol(device_address: u8, interface: u8) -> Result<(), &'static str> {
    if let Some(subsystem) = crate::usb::get_usb_subsystem() {
        // HID SET_PROTOCOL request
        // bmRequestType: 0x21 (Host to device, class, interface)
        // bRequest: 0x0B (SET_PROTOCOL)
        // wValue: 0x0000 (Boot Protocol = 0, Report Protocol = 1)
        // wIndex: interface number
        // wLength: 0
        let setup = super::SetupPacket::new(
            0x21,  // Host to device, class, interface
            0x0B,  // SET_PROTOCOL
            0x0000, // Boot Protocol
            interface as u16,
            0,
        );
        
        match subsystem.control_transfer_out(device_address, setup, &mut []) {
            super::TransferResult::Success(_) => {
                crate::serial_println!("[hid] Set boot protocol on device {} interface {}", device_address, interface);
                Ok(())
            }
            super::TransferResult::Stall => Err("Device stalled SET_PROTOCOL request"),
            super::TransferResult::Timeout => Err("SET_PROTOCOL request timed out"),
            _ => Err("Failed to set boot protocol"),
        }
    } else {
        Err("USB subsystem not available")
    }
}

/// Set keyboard idle rate
/// 
/// This sends a SET_IDLE request to configure how often the keyboard sends reports.
/// 
/// # Arguments
/// * `device_address` - USB device address (1-127)
/// * `interface` - HID interface number
/// * `idle_rate` - Idle rate in 4ms units (0 = report only on change, 1 = 4ms, etc.)
///                 Duration = idle_rate * 4ms. Max duration = 1020ms (0xFF * 4ms)
pub fn set_idle(device_address: u8, interface: u8, idle_rate: u8) -> Result<(), &'static str> {
    if let Some(subsystem) = crate::usb::get_usb_subsystem() {
        // HID SET_IDLE request
        // bmRequestType: 0x21 (Host to device, class, interface)
        // bRequest: 0x0A (SET_IDLE)
        // wValue: (idle_rate << 8) | report_id (report_id = 0 for all reports)
        // wIndex: interface number
        // wLength: 0
        let setup = super::SetupPacket::new(
            0x21,  // Host to device, class, interface
            0x0A,  // SET_IDLE
            (idle_rate as u16) << 8, // idle_rate in high byte, report ID 0 in low byte
            interface as u16,
            0,
        );
        
        match subsystem.control_transfer_out(device_address, setup, &mut []) {
            super::TransferResult::Success(_) => {
                crate::serial_println!("[hid] Set idle rate {} on device {} interface {}", idle_rate, device_address, interface);
                Ok(())
            }
            super::TransferResult::Stall => {
                // Some keyboards don't support SET_IDLE, which is acceptable
                crate::serial_println!("[hid] Device {} does not support SET_IDLE (stalled)", device_address);
                Ok(())
            }
            super::TransferResult::Timeout => Err("SET_IDLE request timed out"),
            _ => Err("Failed to set idle rate"),
        }
    } else {
        Err("USB subsystem not available")
    }
}

/// Set keyboard LEDs (Num Lock, Caps Lock, Scroll Lock, etc.)
/// 
/// This sends a SET_REPORT request with an output report containing the LED state.
/// 
/// # Arguments
/// * `device_address` - USB device address (1-127)
/// * `interface` - HID interface number
/// * `leds` - LED state to set
pub fn set_leds(device_address: u8, interface: u8, leds: KeyboardLeds) -> Result<(), &'static str> {
    if let Some(subsystem) = crate::usb::get_usb_subsystem() {
        let led_byte = leds.to_byte();
        
        // HID SET_REPORT request for output report (LEDs)
        // bmRequestType: 0x21 (Host to device, class, interface)
        // bRequest: 0x09 (SET_REPORT)
        // wValue: (report_type << 8) | report_id
        //         report_type: 1=Input, 2=Output, 3=Feature
        //         report_id: 0 for keyboards without report IDs
        // wIndex: interface number
        // wLength: 1 (single byte for LED state)
        let setup = super::SetupPacket::new(
            0x21,  // Host to device, class, interface
            0x09,  // SET_REPORT
            0x0200, // Output report (type 2), report ID 0
            interface as u16,
            1,
        );
        
        let mut data = [led_byte];
        match subsystem.control_transfer_out(device_address, setup, &mut data) {
            super::TransferResult::Success(_) => {
                crate::serial_println!("[hid] Set LEDs 0x{:02x} on device {} interface {}", led_byte, device_address, interface);
                Ok(())
            }
            super::TransferResult::Stall => Err("Device stalled SET_REPORT request"),
            super::TransferResult::Timeout => Err("SET_REPORT request timed out"),
            _ => Err("Failed to set keyboard LEDs"),
        }
    } else {
        Err("USB subsystem not available")
    }
}
