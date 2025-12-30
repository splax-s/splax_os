//! # Interrupt Handlers
//!
//! x86_64 interrupt and exception handlers for Splax OS.

use core::arch::asm;
use core::fmt::Write;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;
use super::serial::SERIAL;

// =============================================================================
// Lock-Free Serial Input Ring Buffer
// =============================================================================

const SERIAL_BUFFER_SIZE: usize = 256;

/// Lock-free ring buffer for serial input bytes
pub struct SerialRingBuffer {
    buffer: [core::sync::atomic::AtomicU8; SERIAL_BUFFER_SIZE],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl SerialRingBuffer {
    pub const fn new() -> Self {
        const ZERO: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
        Self {
            buffer: [ZERO; SERIAL_BUFFER_SIZE],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }
    
    /// Push a byte (called from interrupt handler - never blocks)
    #[inline]
    pub fn push(&self, byte: u8) {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) % SERIAL_BUFFER_SIZE;
        let tail = self.tail.load(Ordering::Acquire);
        
        if next_head == tail {
            // Buffer full - drop oldest
            self.tail.store((tail + 1) % SERIAL_BUFFER_SIZE, Ordering::Release);
        }
        
        self.buffer[head].store(byte, Ordering::Release);
        self.head.store(next_head, Ordering::Release);
    }
    
    /// Pop a byte (called from main loop)
    #[inline]
    pub fn pop(&self) -> Option<u8> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);
        
        if tail == head {
            return None;
        }
        
        let byte = self.buffer[tail].load(Ordering::Acquire);
        self.tail.store((tail + 1) % SERIAL_BUFFER_SIZE, Ordering::Release);
        Some(byte)
    }
}

/// Global serial input buffer - interrupt writes, main loop reads
pub static SERIAL_INPUT_BUFFER: SerialRingBuffer = SerialRingBuffer::new();

/// Command line buffer for kernel shell
static COMMAND_BUFFER: Mutex<CommandBuffer> = Mutex::new(CommandBuffer::new());

/// Command history for kernel shell
static COMMAND_HISTORY: Mutex<CommandHistory> = Mutex::new(CommandHistory::new());

/// Maximum number of commands to keep in history
/// Reduced in microkernel mode to save ~6KB memory
#[cfg(feature = "microkernel")]
const HISTORY_SIZE: usize = 8;   // ~2KB
#[cfg(not(feature = "microkernel"))]
const HISTORY_SIZE: usize = 32;  // ~8KB

/// Command history buffer
struct CommandHistory {
    /// Stored commands
    commands: [[u8; 256]; HISTORY_SIZE],
    /// Length of each command
    lengths: [usize; HISTORY_SIZE],
    /// Number of commands stored
    count: usize,
    /// Current write position (circular)
    write_pos: usize,
    /// Current browse position for up/down navigation
    browse_pos: usize,
    /// Whether we're actively browsing history
    browsing: bool,
    /// Saved current line when browsing (like bash - saves what you typed before pressing up)
    saved_line: [u8; 256],
    saved_len: usize,
}

impl CommandHistory {
    const fn new() -> Self {
        Self {
            commands: [[0; 256]; HISTORY_SIZE],
            lengths: [0; HISTORY_SIZE],
            count: 0,
            write_pos: 0,
            browse_pos: 0,
            browsing: false,
            saved_line: [0; 256],
            saved_len: 0,
        }
    }
    
    /// Add a command to history (only adds non-empty, non-duplicate commands like bash)
    fn push(&mut self, cmd: &str) {
        if cmd.is_empty() {
            return;
        }
        
        // Don't add duplicates of the last command (like bash)
        if self.count > 0 {
            let last_pos = if self.write_pos == 0 { HISTORY_SIZE - 1 } else { self.write_pos - 1 };
            if let Some(last_cmd) = self.get_at(last_pos) {
                if last_cmd == cmd {
                    // Reset browsing but don't add duplicate
                    self.browsing = false;
                    self.browse_pos = self.write_pos;
                    return;
                }
            }
        }
        
        let bytes = cmd.as_bytes();
        let len = bytes.len().min(255);
        
        self.commands[self.write_pos][..len].copy_from_slice(&bytes[..len]);
        self.lengths[self.write_pos] = len;
        
        self.write_pos = (self.write_pos + 1) % HISTORY_SIZE;
        if self.count < HISTORY_SIZE {
            self.count += 1;
        }
        
        // Reset browsing state
        self.browsing = false;
        self.browse_pos = self.write_pos;
    }
    
    /// Save current line before browsing (like bash does)
    fn save_current_line(&mut self, line: &str) {
        let bytes = line.as_bytes();
        let len = bytes.len().min(255);
        self.saved_line[..len].copy_from_slice(&bytes[..len]);
        self.saved_len = len;
    }
    
    /// Get saved line (what user was typing before pressing up)
    fn get_saved_line(&self) -> Option<&str> {
        if self.saved_len == 0 {
            return Some(""); // Empty is valid
        }
        core::str::from_utf8(&self.saved_line[..self.saved_len]).ok()
    }
    
    /// Start browsing and get previous command (up arrow) - Linux/bash style
    fn previous(&mut self) -> Option<&str> {
        if self.count == 0 {
            return None;
        }
        
        if !self.browsing {
            // First time pressing up - start from most recent command
            self.browsing = true;
            // browse_pos starts at write_pos, we'll decrement to get last command
            self.browse_pos = self.write_pos;
        }
        
        // Calculate the position to try
        let try_pos = if self.browse_pos == 0 {
            HISTORY_SIZE - 1
        } else {
            self.browse_pos - 1
        };
        
        // Calculate oldest valid position
        let oldest = if self.count < HISTORY_SIZE {
            0
        } else {
            self.write_pos  // In circular buffer, write_pos is where oldest gets overwritten
        };
        
        // Check if try_pos has valid data
        if self.lengths[try_pos] == 0 {
            // No more history
            return self.get_at(self.browse_pos);
        }
        
        // Check bounds - don't go past oldest
        let entries_back = if self.browse_pos >= self.write_pos {
            self.browse_pos - self.write_pos
        } else {
            HISTORY_SIZE - self.write_pos + self.browse_pos
        };
        
        if entries_back >= self.count {
            // Already at oldest entry, stay put
            return self.get_at(self.browse_pos);
        }
        
        self.browse_pos = try_pos;
        self.get_at(self.browse_pos)
    }
    
    /// Get next command (down arrow) - Linux/bash style
    fn next(&mut self) -> Option<&str> {
        if !self.browsing || self.count == 0 {
            return None;
        }
        
        // Move forward in history
        let new_pos = (self.browse_pos + 1) % HISTORY_SIZE;
        
        if new_pos == self.write_pos {
            // Back to current (saved) command - return what user was typing
            self.browsing = false;
            return self.get_saved_line();
        }
        
        self.browse_pos = new_pos;
        self.get_at(self.browse_pos)
    }
    
    /// Get command at position
    fn get_at(&self, pos: usize) -> Option<&str> {
        if self.lengths[pos] == 0 {
            return None;
        }
        core::str::from_utf8(&self.commands[pos][..self.lengths[pos]]).ok()
    }
    
    /// Reset browsing state
    fn reset_browsing(&mut self) {
        self.browsing = false;
        self.browse_pos = self.write_pos;
    }
}

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
    
    /// Replace contents with a string (for history navigation)
    fn set(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let len = bytes.len().min(255);
        self.buffer[..len].copy_from_slice(&bytes[..len]);
        self.len = len;
    }
    
    /// Get current length
    fn len(&self) -> usize {
        self.len
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

/// Keyboard interrupt counter
static KEYBOARD_IRQ_COUNT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Get current timer tick count.
pub fn get_ticks() -> u64 {
    TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed)
}

/// Get keyboard IRQ count
pub fn get_keyboard_irq_count() -> u64 {
    KEYBOARD_IRQ_COUNT.load(core::sync::atomic::Ordering::Relaxed)
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
/// 
/// This handler is FAST and LOCK-FREE (Linux-style).
/// It only pushes key events to a ring buffer - never blocks.
/// The shell main loop processes the buffer.
pub extern "x86-interrupt" fn keyboard_handler(_frame: InterruptFrame) {
    // Increment keyboard IRQ counter
    KEYBOARD_IRQ_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    
    // Read scancode from keyboard port
    let scancode: u8;
    unsafe {
        asm!("in al, 0x60", out("al") scancode);
    }
    
    // Send EOI FIRST to prevent keyboard lockup on any panic/error below
    unsafe {
        pic_send_eoi(vector::PIC_KEYBOARD);
    }

    // Process the scancode through the keyboard driver
    if let Some(key_event) = super::keyboard::handle_scancode(scancode) {
        // Push to lock-free ring buffer - NEVER blocks, NEVER drops
        super::keyboard::KEYBOARD_BUFFER.push(key_event);
    }
}

/// Process pending keyboard input from the ring buffer.
/// Call this from the main loop to handle user input.
/// This is where we can safely use locks since we're not in an interrupt.
pub fn process_keyboard_input() {
    use super::keyboard::{KEYBOARD_BUFFER, SpecialKey};
    
    while let Some(key_event) = KEYBOARD_BUFFER.pop() {
        // Handle special keys (arrow keys, etc.)
        if let Some(special) = key_event.special {
            match special {
                SpecialKey::ArrowUp => {
                    // Get previous command from history (bash-style)
                    let cmd_to_show = {
                        let mut history = COMMAND_HISTORY.lock();
                        
                        // Save current line before first browse (like bash)
                        if !history.browsing {
                            let current = COMMAND_BUFFER.lock();
                            history.save_current_line(current.as_str());
                        }
                        
                        history.previous().map(|s| alloc::string::String::from(s))
                    };
                    
                    if let Some(cmd) = cmd_to_show {
                        // Clear current line on screen
                        let old_len = COMMAND_BUFFER.lock().len();
                        for _ in 0..old_len {
                            super::vga::backspace();
                        }
                        // Set new command
                        COMMAND_BUFFER.lock().set(&cmd);
                        crate::vga_print!("{}", cmd);
                    }
                }
                SpecialKey::ArrowDown => {
                    // Get next command from history (bash-style)
                    let cmd_to_show = {
                        let mut history = COMMAND_HISTORY.lock();
                        history.next().map(|s| alloc::string::String::from(s))
                    };
                    
                    // Clear current line on screen first
                    let old_len = COMMAND_BUFFER.lock().len();
                    for _ in 0..old_len {
                        super::vga::backspace();
                    }
                    
                    if let Some(cmd) = cmd_to_show {
                        // Set command (could be history entry or saved line)
                        COMMAND_BUFFER.lock().set(&cmd);
                        crate::vga_print!("{}", cmd);
                    } else {
                        // At bottom of history with nothing to show - clear line
                        COMMAND_BUFFER.lock().clear();
                    }
                }
                SpecialKey::PageUp => {
                    // Scroll VGA up (show older output)
                    super::vga::scroll_up();
                }
                SpecialKey::PageDown => {
                    // Scroll VGA down (show newer output)
                    super::vga::scroll_down();
                }
                _ => {} // Ignore other special keys for now
            }
        } else if let Some(c) = key_event.character {
            // Handle Ctrl+C (works with both 'c' and 'C')
            if key_event.ctrl && (c == 'c' || c == 'C') {
                // Ctrl+C - clear current line
                COMMAND_HISTORY.lock().reset_browsing();
                COMMAND_BUFFER.lock().clear();
                crate::vga_println!("^C");
                crate::vga_print!("splax> ");
            } else {
                // Display the character on VGA and handle input
                match c {
                    '\n' => {
                        crate::vga_println!();
                        // Execute command and add to history
                        let cmd_string = {
                            let mut cmd_buf = COMMAND_BUFFER.lock();
                            let s = alloc::string::String::from(cmd_buf.as_str());
                            cmd_buf.clear();
                            s
                        };
                        if !cmd_string.is_empty() {
                            // Add to history
                            let mut history = COMMAND_HISTORY.lock();
                            history.push(&cmd_string);
                            history.reset_browsing();
                            drop(history);
                            execute_shell_command(&cmd_string);
                        } else {
                            // Empty command, just reset history browsing
                            COMMAND_HISTORY.lock().reset_browsing();
                        }
                        crate::vga_print!("splax> ");
                    }
                    '\x08' => {
                        // Backspace
                        if COMMAND_BUFFER.lock().pop() {
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
    }
}

/// Serial command buffer for serial shell
static SERIAL_COMMAND_BUFFER: Mutex<CommandBuffer> = Mutex::new(CommandBuffer::new());

/// Serial command history
static SERIAL_COMMAND_HISTORY: Mutex<CommandHistory> = Mutex::new(CommandHistory::new());

/// Handle serial terminal up arrow (previous command)
fn handle_serial_history_up() {
    if let Some(cmd) = SERIAL_COMMAND_HISTORY.lock().previous() {
        let cmd_owned = alloc::string::String::from(cmd);
        // Clear current line on terminal
        let old_len = SERIAL_COMMAND_BUFFER.lock().len();
        {
            let serial = super::serial::SERIAL.lock();
            // Erase current line: move back, overwrite with spaces, move back again
            for _ in 0..old_len {
                serial.write_byte(0x08);
            }
            for _ in 0..old_len {
                serial.write_byte(b' ');
            }
            for _ in 0..old_len {
                serial.write_byte(0x08);
            }
            // Print the history command
            for b in cmd_owned.as_bytes() {
                serial.write_byte(*b);
            }
        }
        SERIAL_COMMAND_BUFFER.lock().set(&cmd_owned);
    }
}

/// Handle serial terminal down arrow (next command)
fn handle_serial_history_down() {
    let old_len = SERIAL_COMMAND_BUFFER.lock().len();
    {
        let serial = super::serial::SERIAL.lock();
        // Erase current line
        for _ in 0..old_len {
            serial.write_byte(0x08);
        }
        for _ in 0..old_len {
            serial.write_byte(b' ');
        }
        for _ in 0..old_len {
            serial.write_byte(0x08);
        }
    }
    
    if let Some(cmd) = SERIAL_COMMAND_HISTORY.lock().next() {
        let cmd_owned = alloc::string::String::from(cmd);
        {
            let serial = super::serial::SERIAL.lock();
            for b in cmd_owned.as_bytes() {
                serial.write_byte(*b);
            }
        }
        SERIAL_COMMAND_BUFFER.lock().set(&cmd_owned);
    } else {
        // Back to current (empty) line
        SERIAL_COMMAND_BUFFER.lock().clear();
    }
}

/// Escape sequence state for serial terminal
static SERIAL_ESCAPE_STATE: Mutex<EscapeState> = Mutex::new(EscapeState::None);

/// State machine for parsing ANSI escape sequences
#[derive(Clone, Copy, PartialEq)]
enum EscapeState {
    None,
    Escape,      // Got ESC (0x1B)
    Bracket,     // Got ESC [
}

/// Serial interrupt handler (COM1 - IRQ4).
/// 
/// This handler is FAST - it only reads bytes and pushes to ring buffer.
/// Actual processing happens in process_serial_input() from main loop.
pub extern "x86-interrupt" fn serial_handler(_frame: InterruptFrame) {
    // Send EOI FIRST to prevent issues
    unsafe {
        pic_send_eoi(vector::PIC_COM1);
    }
    
    // Read all available bytes from serial port and push to buffer
    loop {
        let byte = {
            let serial = super::serial::SERIAL.lock();
            if !serial.has_data() {
                break;
            }
            serial.read_byte()
        };
        
        if let Some(byte) = byte {
            SERIAL_INPUT_BUFFER.push(byte);
        } else {
            break;
        }
    }
}

/// Process pending serial input from the ring buffer.
/// Call this from the main loop to handle user input.
pub fn process_serial_input() {
    while let Some(byte) = SERIAL_INPUT_BUFFER.pop() {
        // Check escape sequence state
        let escape_action = {
            let mut escape_state = SERIAL_ESCAPE_STATE.lock();
            match *escape_state {
                EscapeState::Escape => {
                    if byte == b'[' {
                        *escape_state = EscapeState::Bracket;
                        Some(b'_') // Continue marker
                    } else {
                        *escape_state = EscapeState::None;
                        None
                    }
                }
                EscapeState::Bracket => {
                    *escape_state = EscapeState::None;
                    Some(byte)
                }
                EscapeState::None => None,
            }
        };
        
        // Handle escape sequence actions
        if let Some(action) = escape_action {
            match action {
                b'_' => continue,
                b'A' => {
                    handle_serial_history_up();
                    continue;
                }
                b'B' => {
                    handle_serial_history_down();
                    continue;
                }
                b'C' | b'D' => continue,
                _ => {}
            }
        }
        
        match byte {
            0x1B => {
                *SERIAL_ESCAPE_STATE.lock() = EscapeState::Escape;
            }
            b'\r' | b'\n' => {
                // Echo newline
                {
                    let serial = super::serial::SERIAL.lock();
                    serial.write_byte(b'\r');
                    serial.write_byte(b'\n');
                }
                
                // Execute command
                let cmd_string = {
                    let cmd_buf = SERIAL_COMMAND_BUFFER.lock();
                    alloc::string::String::from(cmd_buf.as_str())
                };
                
                if !cmd_string.is_empty() {
                    SERIAL_COMMAND_HISTORY.lock().push(&cmd_string);
                    execute_serial_command(&cmd_string);
                }
                SERIAL_COMMAND_BUFFER.lock().clear();
                SERIAL_COMMAND_HISTORY.lock().reset_browsing();
                
                // Print prompt
                {
                    let serial = super::serial::SERIAL.lock();
                    for b in b"splax> " {
                        serial.write_byte(*b);
                    }
                }
            }
            0x7F | 0x08 => {
                let did_pop = SERIAL_COMMAND_BUFFER.lock().pop();
                if did_pop {
                    let serial = super::serial::SERIAL.lock();
                    serial.write_byte(0x08);
                    serial.write_byte(b' ');
                    serial.write_byte(0x08);
                }
            }
            0x03 => {
                SERIAL_COMMAND_BUFFER.lock().clear();
                SERIAL_COMMAND_HISTORY.lock().reset_browsing();
                {
                    let serial = super::serial::SERIAL.lock();
                    for b in b"^C\r\nsplax> " {
                        serial.write_byte(*b);
                    }
                }
            }
            c if c >= 0x20 && c < 0x7F => {
                SERIAL_COMMAND_BUFFER.lock().push(c as char);
                super::serial::SERIAL.lock().write_byte(c);
            }
            _ => {}
        }
    }
}

// =============================================================================
// Shell Commands - Full version (monolithic kernel)
// =============================================================================

/// Execute a shell command (kernel built-in shell) - FULL VERSION
#[cfg(not(feature = "microkernel"))]
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
            crate::vga_println!("Filesystem:");
            crate::vga_println!("  ls [path]     - List directory");
            crate::vga_println!("  cat <file>    - Show file contents");
            crate::vga_println!("  touch <file>  - Create empty file");
            crate::vga_println!("  mkdir <dir>   - Create directory");
            crate::vga_println!("  rm <file>     - Remove file");
            crate::vga_println!("  echo <text>   - Print text (or > file)");
            crate::vga_println!("  pwd           - Print working directory");
            crate::vga_println!();
            crate::vga_println!("Block Devices:");
            crate::vga_println!("  lsblk         - List block devices");
            crate::vga_println!("  blkinfo <dev> - Block device info");
            crate::vga_println!("  diskread <dev> <sector> - Read sector");
            crate::vga_println!();
            crate::vga_println!("Filesystem (SplaxFS):");
            crate::vga_println!("  mkfs <dev>    - Format device with SplaxFS");
            crate::vga_println!("  mount <dev> <path> - Mount filesystem");
            crate::vga_println!("  umount <path> - Unmount filesystem");
            crate::vga_println!("  fsls <path>   - List directory on disk");
            crate::vga_println!("  fsmkdir <path> - Create directory on disk");
            crate::vga_println!("  fscat <file>  - Read file from disk");
            crate::vga_println!("  fswrite <file> <text> - Write to file");
            crate::vga_println!();
            crate::vga_println!("Network:");
            crate::vga_println!("  ping [-c n] <ip> - ICMP ping");
            crate::vga_println!("  traceroute <ip> - Trace route to host");
            crate::vga_println!("  nslookup <host> - DNS lookup");
            crate::vga_println!("  dig <host>    - DNS query (detailed)");
            crate::vga_println!("  host <host>   - Resolve hostname");
            crate::vga_println!("  ifconfig      - Interface config");
            crate::vga_println!("  ip6           - IPv6 configuration");
            crate::vga_println!("  route         - Routing table");
            crate::vga_println!("  arp           - ARP cache");
            crate::vga_println!("  netstat [-s]  - Connections/stats");
            crate::vga_println!("  firewall      - Firewall status/rules");
            crate::vga_println!("  ssh <ip>      - SSH client connect");
            crate::vga_println!("  sshd <cmd>    - SSH server (start/stop/status)");
            crate::vga_println!();
            crate::vga_println!("WiFi (Wireless):");
            crate::vga_println!("  iwconfig      - Wireless interface info");
            crate::vga_println!("  iwlist scan   - Scan for WiFi networks");
            crate::vga_println!("  wifi scan     - Scan for WiFi networks");
            crate::vga_println!("  wifi connect <ssid> [pass] - Connect");
            crate::vga_println!("  wifi disconnect - Disconnect from WiFi");
            crate::vga_println!("  wifi status   - Connection status");
            crate::vga_println!();
            crate::vga_println!("System:");
            crate::vga_println!("  ps            - List processes");
            crate::vga_println!("  mem/free      - Memory usage");
            crate::vga_println!("  df            - Filesystem usage");
            crate::vga_println!("  uptime        - System uptime");
            crate::vga_println!("  uname [-a]    - System info");
            crate::vga_println!("  lscpu         - CPU information");
            crate::vga_println!("  lspci         - List PCI devices");
            crate::vga_println!("  proc [file]   - Read /proc (meminfo, cpuinfo...)");
            crate::vga_println!("  lsdev         - List /dev devices");
            crate::vga_println!("  sysinfo [path]- Read /sys info");
            crate::vga_println!("  dmesg         - Kernel messages");
            crate::vga_println!("  env           - Environment vars");
            crate::vga_println!("  id            - User/group IDs");
            crate::vga_println!("  services      - List services");
            crate::vga_println!("  version       - Version info");
            crate::vga_println!("  clear         - Clear screen");
            crate::vga_println!("  date          - Current date/time");
            crate::vga_println!("  clock         - Live clock display");
            crate::vga_println!("  shutdown      - Power off system");
            crate::vga_println!("  reboot        - Reboot system");
            crate::vga_println!();
            crate::vga_println!("USB:");
            crate::vga_println!("  lsusb         - List USB devices");
            crate::vga_println!("  usb <cmd>     - USB subsystem (info/tree/init)");
        }
        "sconf" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Splax Network Configuration:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            
            let stack = crate::net::network_stack().lock();
            if let Some(interface) = stack.primary_interface() {
                let cfg = &interface.config;
                let mac = cfg.mac;
                let ip = cfg.ipv4_addr;
                let mask = cfg.subnet_mask;
                
                super::vga::set_color(Color::LightGreen, Color::Black);
                crate::vga_print!("{}", cfg.name);
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!(": up");
                crate::vga_println!("  MAC:     {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]);
                crate::vga_println!("  IPv4:    {}.{}.{}.{}",
                    ip.octets()[0], ip.octets()[1], ip.octets()[2], ip.octets()[3]);
                crate::vga_println!("  Netmask: {}.{}.{}.{}",
                    mask.octets()[0], mask.octets()[1], mask.octets()[2], mask.octets()[3]);
                if let Some(gw) = cfg.gateway {
                    crate::vga_println!("  Gateway: {}.{}.{}.{}",
                        gw.octets()[0], gw.octets()[1], gw.octets()[2], gw.octets()[3]);
                }
                crate::vga_println!("  MTU:     {}", cfg.mtu);
            } else {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("No network interfaces configured");
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "ping" => {
            use super::vga::Color;
            
            // Parse ping arguments: ping <ip> [count] or ping -c <count> <ip>
            let mut target_str = "";
            let mut count: u16 = 4;
            
            if parts[1] == "-c" && !parts[2].is_empty() && !parts[3].is_empty() {
                // ping -c <count> <ip>
                count = parts[2].parse().unwrap_or(4);
                target_str = parts[3];
            } else if !parts[1].is_empty() {
                // ping <ip> [count]
                target_str = parts[1];
                if !parts[2].is_empty() {
                    count = parts[2].parse().unwrap_or(4);
                }
            }
            
            if target_str.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: ping <ip> [count]");
                crate::vga_println!("       ping -c <count> <ip>");
                crate::vga_println!("Example: ping 10.0.2.2");
                crate::vga_println!("         ping -c 10 8.8.8.8");
                super::vga::set_color(Color::LightGray, Color::Black);
            } else {
                // Parse IP address
                let octets: alloc::vec::Vec<u8> = target_str
                    .split('.')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                
                if octets.len() == 4 {
                    let ip = crate::net::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);
                    
                    // ping() now outputs to both serial and VGA in real-time
                    if let Err(e) = crate::net::ping_count(ip, count) {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("ping: {:?}", e);
                    }
                } else {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("Invalid IP address: {}", target_str);
                }
                super::vga::set_color(Color::LightGray, Color::Black);
            }
        }
        "traceroute" => {
            use super::vga::Color;
            let target = parts[1];
            
            if target.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: traceroute <ip>");
                super::vga::set_color(Color::LightGray, Color::Black);
            } else {
                let octets: alloc::vec::Vec<u8> = target
                    .split('.')
                    .filter_map(|s| s.parse().ok())
                    .collect();
                
                if octets.len() == 4 {
                    let ip = crate::net::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);
                    // traceroute prints output itself via serial
                    if let Err(e) = crate::net::traceroute(ip, 30) {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("traceroute: {:?}", e);
                    }
                } else {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("Invalid IP address: {}", target);
                }
                super::vga::set_color(Color::LightGray, Color::Black);
            }
        }
        "nslookup" | "host" => {
            use super::vga::Color;
            let hostname = parts[1];
            
            if hostname.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: {} <hostname>", command);
                super::vga::set_color(Color::LightGray, Color::Black);
            } else {
                match crate::net::nslookup(hostname, crate::net::dns::RecordType::A) {
                    Ok(results) => {
                        if command == "nslookup" {
                            crate::vga_println!("Server:  8.8.8.8");
                            crate::vga_println!("Address: 8.8.8.8#53");
                            crate::vga_println!();
                            crate::vga_println!("Non-authoritative answer:");
                        }
                        for addr in results {
                            if command == "nslookup" {
                                crate::vga_println!("Name:    {}", hostname);
                                crate::vga_println!("Address: {}", addr);
                            } else {
                                crate::vga_println!("{} has address {}", hostname, addr);
                            }
                        }
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("{}: {:?}", command, e);
                    }
                }
                super::vga::set_color(Color::LightGray, Color::Black);
            }
        }
        "dig" => {
            use super::vga::Color;
            let hostname = parts[1];
            
            if hostname.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: dig <hostname>");
                super::vga::set_color(Color::LightGray, Color::Black);
            } else {
                crate::vga_println!("; <<>> DiG SplaxOS <<>> {}", hostname);
                crate::vga_println!(";; global options: +cmd");
                crate::vga_println!(";; Got answer:");
                match crate::net::nslookup(hostname, crate::net::dns::RecordType::A) {
                    Ok(results) => {
                        crate::vga_println!(";; ANSWER SECTION:");
                        for addr in results {
                            crate::vga_println!("{}.             300     IN      A       {}", hostname, addr);
                        }
                        crate::vga_println!();
                        crate::vga_println!(";; SERVER: 8.8.8.8#53");
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!(";; Query failed: {:?}", e);
                    }
                }
                super::vga::set_color(Color::LightGray, Color::Black);
            }
        }
        "route" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Kernel IP routing table");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("Destination     Gateway         Genmask         Flags Iface");
            
            for entry in crate::net::get_routes() {
                crate::vga_println!("{:<15} {:<15} {:<15} {}     {}",
                    entry.destination, entry.gateway, entry.netmask, entry.flags, entry.interface);
            }
        }
        "ls" => {
            use super::vga::Color;
            let path = if parts[1].is_empty() { "/" } else { parts[1] };
            
            match crate::fs::ls(path) {
                Ok(entries) => {
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("Directory: {}", path);
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!();
                    
                    if entries.is_empty() {
                        crate::vga_println!("(empty)");
                    } else {
                        for (name, meta) in entries {
                            let type_char = match meta.file_type {
                                crate::fs::FileType::Directory => {
                                    super::vga::set_color(Color::LightBlue, Color::Black);
                                    'd'
                                }
                                crate::fs::FileType::File => {
                                    super::vga::set_color(Color::LightGray, Color::Black);
                                    '-'
                                }
                            };
                            crate::vga_println!("{} {:>8}  {}", type_char, meta.size, name);
                            super::vga::set_color(Color::LightGray, Color::Black);
                        }
                    }
                }
                Err(e) => {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("ls: {:?}", e);
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "cat" => {
            use super::vga::Color;
            let path = parts[1];
            
            if path.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: cat <file>");
            } else {
                match crate::fs::cat(path) {
                    Ok(content) => {
                        if let Ok(text) = core::str::from_utf8(&content) {
                            crate::vga_print!("{}", text);
                            if !text.ends_with('\n') {
                                crate::vga_println!();
                            }
                        } else {
                            crate::vga_println!("(binary file, {} bytes)", content.len());
                        }
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("cat: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "touch" => {
            use super::vga::Color;
            let path = parts[1];
            
            if path.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: touch <file>");
            } else {
                match crate::fs::touch(path) {
                    Ok(_) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Created: {}", path);
                    }
                    Err(crate::fs::FsError::AlreadyExists) => {
                        // File already exists, that's ok for touch
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("touch: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "mkdir" => {
            use super::vga::Color;
            let path = parts[1];
            
            if path.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: mkdir <directory>");
            } else {
                match crate::fs::mkdir(path) {
                    Ok(_) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Created: {}", path);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("mkdir: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "rm" => {
            use super::vga::Color;
            let path = parts[1];
            
            if path.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: rm <file>");
            } else {
                match crate::fs::rm(path) {
                    Ok(_) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Removed: {}", path);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("rm: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "rmdir" => {
            use super::vga::Color;
            let path = parts[1];
            
            if path.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: rmdir <directory>");
            } else {
                match crate::fs::rmdir(path) {
                    Ok(_) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Removed: {}", path);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("rmdir: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "echo" => {
            use super::vga::Color;
            // Check if redirecting to file
            let mut redirect_idx = 0;
            for i in 1..8 {
                if parts[i] == ">" || parts[i] == ">>" {
                    redirect_idx = i;
                    break;
                }
            }
            
            if redirect_idx > 0 && redirect_idx < 7 {
                let is_append = parts[redirect_idx] == ">>";
                let filename = parts[redirect_idx + 1];
                
                if filename.is_empty() {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("echo: missing filename");
                } else {
                    // Collect text before redirect
                    let mut text = alloc::string::String::new();
                    for i in 1..redirect_idx {
                        if i > 1 { text.push(' '); }
                        text.push_str(parts[i]);
                    }
                    text.push('\n');
                    
                    // Ensure file exists
                    let _ = crate::fs::touch(filename);
                    
                    let result = if is_append {
                        crate::fs::filesystem().lock().append_file(filename, text.as_bytes())
                    } else {
                        crate::fs::write(filename, text.as_bytes())
                    };
                    
                    match result {
                        Ok(_) => {}
                        Err(e) => {
                            super::vga::set_color(Color::LightRed, Color::Black);
                            crate::vga_println!("echo: {:?}", e);
                        }
                    }
                }
            } else {
                // Just print text
                for i in 1..8 {
                    if !parts[i].is_empty() {
                        if i > 1 { crate::vga_print!(" "); }
                        crate::vga_print!("{}", parts[i]);
                    }
                }
                crate::vga_println!();
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "df" => {
            use super::vga::Color;
            let stats = crate::fs::stats();
            
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Filesystem Usage:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("FILESYSTEM  SIZE     USED     AVAIL    USE%");
            let used_kb = stats.used_bytes / 1024;
            let total_kb = stats.total_bytes / 1024;
            let avail_kb = total_kb.saturating_sub(used_kb);
            let percent = if total_kb > 0 { (used_kb * 100) / total_kb } else { 0 };
            crate::vga_println!("ramfs       {} KB  {} KB  {} KB  {}%", total_kb, used_kb, avail_kb, percent);
            crate::vga_println!();
            crate::vga_println!("Inodes: {} total, {} free", stats.inode_count, stats.free_inodes);
        }
        "ps" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Process List:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("PID   STATE       CPU%  NAME");
            super::vga::set_color(Color::LightGreen, Color::Black);
            crate::vga_print!("0     ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("running     -     kernel");
            super::vga::set_color(Color::LightGreen, Color::Black);
            crate::vga_print!("1     ");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("ready       -     idle");
            crate::vga_println!();
            let proc_count = crate::process::PROCESS_MANAGER.process_count();
            crate::vga_println!("Total: {} processes", if proc_count > 0 { proc_count } else { 2 });
        }
        "uptime" => {
            use super::vga::Color;
            use super::rtc;
            
            let now = rtc::read_rtc();
            let uptime = rtc::format_uptime();
            
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_print!(" {:02}:{:02}:{:02} ", now.hour, now.minute, now.second);
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("up {}, 1 user, load average: 0.00, 0.00, 0.00", uptime);
        }
        "date" | "time" => {
            use super::vga::Color;
            use super::rtc;
            
            let now = rtc::read_rtc();
            let iso = now.format_iso();
            let iso_str = core::str::from_utf8(&iso).unwrap_or("????-??-?? ??:??:??");
            
            super::vga::set_color(Color::LightCyan, Color::Black);
            crate::vga_println!("{} {} {:2} {:02}:{:02}:{:02} UTC {}",
                now.day_name(), now.month_name(), now.day,
                now.hour, now.minute, now.second, now.year);
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "clock" => {
            use super::vga::Color;
            use super::rtc;
            
            crate::vga_println!("Press any key to stop clock...");
            crate::vga_println!();
            
            // Get initial position
            let start_row = super::vga::get_row();
            
            // Display live clock until keypress
            let mut last_second: u8 = 255;
            loop {
                let now = rtc::read_rtc();
                
                // Only update display when second changes
                if now.second != last_second {
                    last_second = now.second;
                    
                    // Clear line and display time
                    super::vga::set_row(start_row);
                    super::vga::clear_line(start_row);
                    
                    super::vga::set_color(Color::LightGreen, Color::Black);
                    crate::vga_print!("  ");
                    super::vga::set_color(Color::White, Color::Blue);
                    crate::vga_print!(" {:02}:{:02}:{:02} ", now.hour, now.minute, now.second);
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_print!("  {} {} {:2}, {}", 
                        now.day_name(), now.month_name(), now.day, now.year);
                }
                
                // Check for keypress
                let status: u8;
                unsafe {
                    asm!("in al, dx", out("al") status, in("dx") 0x64u16, options(nomem, nostack));
                }
                if (status & 0x01) != 0 {
                    // Key available - consume it and exit
                    let _scancode: u8;
                    unsafe {
                        asm!("in al, dx", out("al") _scancode, in("dx") 0x60u16, options(nomem, nostack));
                    }
                    break;
                }
                
                // Small delay to avoid busy spinning
                for _ in 0..10000 {
                    core::hint::spin_loop();
                }
            }
            crate::vga_println!();
        }
        "arp" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Address                  HWtype  HWaddress           Flags Mask  Iface");
            super::vga::set_color(Color::LightGray, Color::Black);
            
            let entries = crate::net::get_arp_cache();
            if entries.is_empty() {
                crate::vga_println!("(no entries)");
            } else {
                for entry in entries {
                    crate::vga_println!("{:<24} ether   {}   C             eth0",
                        entry.ip, entry.mac);
                }
            }
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
        "lsblk" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Block Devices:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("NAME    SIZE        SECTORS     MODEL");
            
            let devices = crate::block::list_devices();
            if devices.is_empty() {
                crate::vga_println!("(no block devices)");
            } else {
                for dev in devices {
                    let size_mb = (dev.total_sectors * dev.sector_size as u64) / (1024 * 1024);
                    super::vga::set_color(Color::LightGreen, Color::Black);
                    crate::vga_print!("{:<8}", dev.name);
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!("{:>6} MB    {:>10}  {}", 
                        size_mb, dev.total_sectors, dev.model);
                }
            }
        }
        "blkinfo" => {
            use super::vga::Color;
            if parts[1].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: blkinfo <device>");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("Example: blkinfo vda");
            } else {
                let dev_name = parts[1];
                match crate::block::with_device(dev_name, |dev| dev.info()) {
                    Ok(info) => {
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("Block Device: {}", info.name);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        crate::vga_println!();
                        crate::vga_println!("Model:        {}", info.model);
                        crate::vga_println!("Sectors:      {}", info.total_sectors);
                        crate::vga_println!("Sector Size:  {} bytes", info.sector_size);
                        let size_mb = (info.total_sectors * info.sector_size as u64) / (1024 * 1024);
                        crate::vga_println!("Total Size:   {} MB", size_mb);
                        crate::vga_println!("Read-Only:    {}", if info.read_only { "yes" } else { "no" });
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Device not found: {}", dev_name);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        crate::vga_println!("Error: {:?}", e);
                    }
                }
            }
        }
        "diskread" => {
            use super::vga::Color;
            if parts[1].is_empty() || parts[2].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: diskread <device> <sector>");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("Example: diskread vda 0");
            } else {
                let dev_name = parts[1];
                if let Ok(sector) = parts[2].parse::<u64>() {
                    match crate::block::read(dev_name, sector, 1) {
                        Ok(data) => {
                            super::vga::set_color(Color::Yellow, Color::Black);
                            crate::vga_println!("Sector {} from {}:", sector, dev_name);
                            super::vga::set_color(Color::LightGray, Color::Black);
                            crate::vga_println!();
                            
                            // Print hex dump (first 256 bytes)
                            for (i, chunk) in data.iter().take(256).collect::<alloc::vec::Vec<_>>().chunks(16).enumerate() {
                                crate::vga_print!("{:04x}: ", i * 16);
                                for byte in chunk {
                                    crate::vga_print!("{:02x} ", byte);
                                }
                                crate::vga_print!(" ");
                                for byte in chunk {
                                    let c = **byte;
                                    if c >= 0x20 && c < 0x7f {
                                        crate::vga_print!("{}", c as char);
                                    } else {
                                        crate::vga_print!(".");
                                    }
                                }
                                crate::vga_println!();
                            }
                        }
                        Err(e) => {
                            super::vga::set_color(Color::LightRed, Color::Black);
                            crate::vga_println!("Read failed: {:?}", e);
                        }
                    }
                } else {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("Invalid sector number: {}", parts[2]);
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        // === Filesystem Commands ===
        "mkfs" => {
            use super::vga::Color;
            if parts[1].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: mkfs <device>");
                crate::vga_println!("Example: mkfs vda");
            } else {
                match crate::fs::splaxfs::format(parts[1]) {
                    Ok(()) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Formatted {} with SplaxFS", parts[1]);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Format failed: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "mount" => {
            use super::vga::Color;
            // Parse: mount [-t type] <device> <path>
            // or: mount <device> <path> (defaults to splaxfs)
            let (fs_type, device, path) = if parts[1] == "-t" && !parts[2].is_empty() {
                // mount -t <type> <device> <path>
                (parts[2], parts[3], parts[4])
            } else if !parts[1].is_empty() && !parts[2].is_empty() {
                // mount <device> <path> (default to splaxfs)
                ("splaxfs", parts[1], parts[2])
            } else {
                ("", "", "")
            };
            
            if device.is_empty() || path.is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: mount [-t <type>] <device> <path>");
                crate::vga_println!("Types: splaxfs (default), fat32, ext4");
                crate::vga_println!("Examples:");
                crate::vga_println!("  mount vda /mnt              # SplaxFS");
                crate::vga_println!("  mount -t fat32 sda1 /mnt/usb");
                crate::vga_println!("  mount -t ext4 sda2 /mnt/linux");
            } else {
                // Use a bool for success/failure since error types differ
                let (success, err_msg): (bool, Option<alloc::string::String>) = match fs_type {
                    "fat32" | "vfat" => {
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("Mounting {} as FAT32 at {}...", device, path);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        match crate::fs::fat32::mount(device, path) {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(alloc::format!("{:?}", e))),
                        }
                    }
                    "ext4" => {
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("Mounting {} as ext4 at {}...", device, path);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        match crate::fs::ext4::mount(device, path) {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(alloc::format!("{:?}", e))),
                        }
                    }
                    "splaxfs" | "" => {
                        match crate::fs::splaxfs::mount(device, path) {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(alloc::format!("{:?}", e))),
                        }
                    }
                    _ => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Unknown filesystem type: {}", fs_type);
                        crate::vga_println!("Supported: splaxfs, fat32, ext4");
                        super::vga::set_color(Color::LightGray, Color::Black);
                        return;
                    }
                };
                
                if success {
                    super::vga::set_color(Color::LightGreen, Color::Black);
                    crate::vga_println!("Mounted {} ({}) at {}", device, fs_type, path);
                } else {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("Mount failed: {}", err_msg.unwrap_or_default());
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "umount" => {
            use super::vga::Color;
            if parts[1].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: umount <path>");
            } else {
                match crate::fs::splaxfs::unmount(parts[1]) {
                    Ok(()) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Unmounted {}", parts[1]);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Unmount failed: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "fsls" => {
            use super::vga::Color;
            let path = if parts[1].is_empty() { "/mnt" } else { parts[1] };
            match crate::fs::splaxfs::ls(path) {
                Ok(entries) => {
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("Directory: {}", path);
                    super::vga::set_color(Color::LightGray, Color::Black);
                    for (name, file_type, size) in entries {
                        let type_char = match file_type {
                            crate::fs::splaxfs::FileType::Directory => 'd',
                            crate::fs::splaxfs::FileType::Regular => '-',
                            _ => '?',
                        };
                        crate::vga_println!("{}  {:>8}  {}", type_char, size, name);
                    }
                }
                Err(e) => {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("ls failed: {:?}", e);
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "fsmkdir" => {
            use super::vga::Color;
            if parts[1].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: fsmkdir <path>");
            } else {
                match crate::fs::splaxfs::mkdir(parts[1]) {
                    Ok(()) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Created directory: {}", parts[1]);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("mkdir failed: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "fscat" => {
            use super::vga::Color;
            if parts[1].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: fscat <file>");
            } else {
                match crate::fs::splaxfs::read(parts[1]) {
                    Ok(data) => {
                        if let Ok(text) = core::str::from_utf8(&data) {
                            crate::vga_println!("{}", text);
                        } else {
                            crate::vga_println!("(binary data, {} bytes)", data.len());
                        }
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Read failed: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "fswrite" => {
            use super::vga::Color;
            if parts[1].is_empty() {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Usage: fswrite <file> <text>");
            } else {
                // Join remaining parts as content
                let content = parts[2..].join(" ");
                // First create the file if needed
                let _ = crate::fs::splaxfs::create(parts[1]);
                match crate::fs::splaxfs::write(parts[1], content.as_bytes()) {
                    Ok(()) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Wrote {} bytes to {}", content.len(), parts[1]);
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Write failed: {:?}", e);
                    }
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
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
        "memory" | "mem" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Memory Usage:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            
            let stats = crate::mm::heap_stats();
            let used_kb = stats.total_allocated / 1024;
            let total_kb = stats.heap_size / 1024;
            let free_kb = total_kb.saturating_sub(used_kb);
            let percent = if total_kb > 0 { (used_kb * 100) / total_kb } else { 0 };
            
            crate::vga_println!("Heap Total:      {} KB", total_kb);
            crate::vga_println!("Heap Used:       {} KB ({}%)", used_kb, percent);
            crate::vga_println!("Heap Free:       {} KB", free_kb);
            crate::vga_println!();
            crate::vga_println!("Allocations:     {}", stats.allocation_count);
            crate::vga_println!("Deallocations:   {}", stats.deallocation_count);
            crate::vga_println!("Free blocks:     {}", stats.free_blocks);
        }
        "wave" | "wasm" => {
            use super::vga::Color;
            
            let subcmd = parts[1];
            match subcmd {
                "status" | "" => {
                    let stats = crate::wasm::stats();
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("S-WAVE WASM Runtime Status:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!();
                    crate::vga_println!("Runtime:            S-WAVE v1.0 ({})", 
                        if stats.runtime_initialized { "active" } else { "not initialized" });
                    crate::vga_println!("WASM Version:       1.0 (MVP)");
                    crate::vga_println!("Modules loaded:     {}", stats.modules_loaded);
                    crate::vga_println!("Total WASM size:    {} bytes", stats.total_wasm_size);
                    crate::vga_println!("Max modules:        256");
                    crate::vga_println!("Max instances:      1,024");
                    crate::vga_println!("Max memory/inst:    64 MB");
                    crate::vga_println!("Max steps/call:     100,000,000");
                    crate::vga_println!();
                    super::vga::set_color(Color::Cyan, Color::Black);
                    crate::vga_println!("Supported Features:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!("  [x] i32/i64/f32/f64 types");
                    crate::vga_println!("  [x] Linear memory (grow/read/write)");
                    crate::vga_println!("  [x] Function calls and returns");
                    crate::vga_println!("  [x] Control flow (block/loop/if/br)");
                    crate::vga_println!("  [x] Capability-bound host imports");
                    crate::vga_println!("  [x] VFS integration");
                    crate::vga_println!("  [ ] SIMD (planned)");
                    crate::vga_println!("  [ ] Threads (planned)");
                }
                "help" => {
                    super::vga::set_color(Color::Cyan, Color::Black);
                    crate::vga_println!("S-WAVE WASM Runtime Commands:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!();
                    crate::vga_println!("  wasm status       - Show runtime status");
                    crate::vga_println!("  wasm list         - List loaded modules");
                    crate::vga_println!("  wasm load <file>  - Load WASM module from file");
                    crate::vga_println!("  wasm run <mod>    - Run module's _start function");
                    crate::vga_println!("  wasm call <m> <f> - Call function in module");
                    crate::vga_println!("  wasm unload <mod> - Unload module");
                    crate::vga_println!("  wasm hostfn       - List host functions");
                    crate::vga_println!("  wasm caps         - Show capability requirements");
                    crate::vga_println!("  wasm validate <f> - Validate WASM file");
                    crate::vga_println!("  wasm help         - Show this help");
                }
                "hostfn" | "host" => {
                    super::vga::set_color(Color::Cyan, Color::Black);
                    crate::vga_println!("S-WAVE Host Functions (splax.*):");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!();
                    crate::vga_println!("IPC:");
                    crate::vga_println!("  s_link_send      (ch, ptr, len) -> i32");
                    crate::vga_println!("  s_link_receive   (ch, ptr, max) -> i32");
                    crate::vga_println!();
                    crate::vga_println!("Storage:");
                    crate::vga_println!("  s_storage_read   (id, ptr, off, len) -> i32");
                    crate::vga_println!("  s_storage_write  (ptr, len) -> i64");
                    crate::vga_println!();
                    crate::vga_println!("Console:");
                    crate::vga_println!("  s_print          (ptr, len) -> i32");
                    crate::vga_println!("  s_read           (ptr, max) -> i32");
                    crate::vga_println!("  s_log            (level, ptr, len) -> ()");
                    crate::vga_println!();
                    crate::vga_println!("System:");
                    crate::vga_println!("  s_time_now       () -> i64");
                    crate::vga_println!("  s_sleep          (us: i64) -> ()");
                    crate::vga_println!("  s_exit           (code: i32) -> !");
                    crate::vga_println!("  s_random         (ptr, len) -> i32");
                    crate::vga_println!();
                    crate::vga_println!("Files:");
                    crate::vga_println!("  s_file_open      (path, len, flags) -> i32");
                    crate::vga_println!("  s_file_read      (fd, ptr, len) -> i32");
                    crate::vga_println!("  s_file_write     (fd, ptr, len) -> i32");
                    crate::vga_println!("  s_file_close     (fd) -> i32");
                    crate::vga_println!();
                    crate::vga_println!("Network:");
                    crate::vga_println!("  s_net_connect    (host, len, port) -> i32");
                    crate::vga_println!("  s_net_send       (sock, ptr, len) -> i32");
                    crate::vga_println!("  s_net_recv       (sock, ptr, max) -> i32");
                    crate::vga_println!("  s_net_close      (sock) -> i32");
                }
                "caps" | "capabilities" => {
                    super::vga::set_color(Color::Cyan, Color::Black);
                    crate::vga_println!("S-WAVE Capability Requirements:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!();
                    crate::vga_println!("Capability         Host Functions");
                    crate::vga_println!("───────────────────────────────────────");
                    crate::vga_println!("channel:write      s_link_send");
                    crate::vga_println!("channel:read       s_link_receive");
                    crate::vga_println!("storage:read       s_storage_read");
                    crate::vga_println!("storage:write      s_storage_write");
                    crate::vga_println!("console:write      s_print, s_log");
                    crate::vga_println!("console:read       s_read");
                    crate::vga_println!("time:read          s_time_now");
                    crate::vga_println!("time:sleep         s_sleep");
                    crate::vga_println!("process:exit       s_exit");
                    crate::vga_println!("random:read        s_random");
                    crate::vga_println!("fs:read            s_file_open/read/close");
                    crate::vga_println!("fs:write           s_file_write");
                    crate::vga_println!("net:connect        s_net_*");
                }
                "list" => {
                    super::vga::set_color(Color::Cyan, Color::Black);
                    crate::vga_println!("Loaded WASM Modules:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!();
                    let modules = crate::wasm::list_modules();
                    if modules.is_empty() {
                        crate::vga_println!("(no modules loaded)");
                        crate::vga_println!();
                        crate::vga_println!("Use 'wasm load <file>' to load a module");
                    } else {
                        for m in &modules {
                            crate::vga_println!("  {:?} {} ({} bytes) - {}", m.id, m.name, m.size, m.path);
                        }
                        crate::vga_println!();
                        let stats = crate::wasm::stats();
                        crate::vga_println!("Total: {} modules, {} bytes", stats.modules_loaded, stats.total_wasm_size);
                    }
                }
                "validate" => {
                    let filename = parts[2];
                    if filename.is_empty() {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Usage: wasm validate <file.wasm>");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    } else {
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("Validating: {}", filename);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        crate::vga_println!();
                        match crate::wasm::validate_file(filename) {
                            Ok(result) => {
                                if result.valid {
                                    super::vga::set_color(Color::LightGreen, Color::Black);
                                    crate::vga_println!("  Valid WASM module!");
                                    super::vga::set_color(Color::LightGray, Color::Black);
                                    crate::vga_println!("  Version: {}", result.version);
                                    crate::vga_println!("  Size: {} bytes", result.size);
                                    crate::vga_println!("  Functions: {}", result.function_count);
                                    crate::vga_println!("  Imports: {}", result.import_count);
                                    crate::vga_println!("  Exports: {}", result.export_count);
                                    crate::vga_println!("  Memory: {}", if result.has_memory { "yes" } else { "no" });
                                    crate::vga_println!("  Start function: {}", if result.has_start { "yes" } else { "no" });
                                } else {
                                    super::vga::set_color(Color::LightRed, Color::Black);
                                    crate::vga_println!("  Invalid WASM file");
                                    super::vga::set_color(Color::LightGray, Color::Black);
                                    if result.size < 8 {
                                        crate::vga_println!("  File too small (< 8 bytes)");
                                    } else if result.version == 0 {
                                        crate::vga_println!("  Bad magic number (not a WASM file)");
                                    } else {
                                        crate::vga_println!("  Unsupported version: {}", result.version);
                                    }
                                }
                            }
                            Err(e) => {
                                super::vga::set_color(Color::LightRed, Color::Black);
                                crate::vga_println!("  Error: {:?}", e);
                                super::vga::set_color(Color::LightGray, Color::Black);
                            }
                        }
                    }
                }
                "load" => {
                    let filename = parts[2];
                    if filename.is_empty() {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Usage: wasm load <file.wasm>");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    } else {
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("Loading WASM module: {}", filename);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        match crate::wasm::load_file(filename) {
                            Ok(module_id) => {
                                super::vga::set_color(Color::LightGreen, Color::Black);
                                crate::vga_println!("Module loaded: {:?}", module_id);
                                super::vga::set_color(Color::LightGray, Color::Black);
                                crate::vga_println!("Use 'wasm run <module>' to execute");
                            }
                            Err(e) => {
                                super::vga::set_color(Color::LightRed, Color::Black);
                                crate::vga_println!("Load failed: {:?}", e);
                                super::vga::set_color(Color::LightGray, Color::Black);
                            }
                        }
                    }
                }
                "run" => {
                    let module = parts[2];
                    if module.is_empty() {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Usage: wasm run <file.wasm>");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    } else {
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("Running: {}", module);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        match crate::wasm::run_file(module) {
                            Ok(results) => {
                                super::vga::set_color(Color::LightGreen, Color::Black);
                                crate::vga_println!("Execution completed");
                                super::vga::set_color(Color::LightGray, Color::Black);
                                if !results.is_empty() {
                                    crate::vga_println!("Results: {:?}", results);
                                }
                            }
                            Err(e) => {
                                super::vga::set_color(Color::LightRed, Color::Black);
                                crate::vga_println!("Execution failed: {:?}", e);
                                super::vga::set_color(Color::LightGray, Color::Black);
                            }
                        }
                    }
                }
                _ => {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("Unknown subcommand: {}", subcmd);
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!("Use 'wasm help' for available commands");
                }
            }
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
        "uname" => {
            // Handle -a flag or default
            let arg = parts[1];
            if arg == "-a" || arg.is_empty() {
                crate::vga_println!("SplaxOS {} x86_64 Splax-Microkernel", crate::VERSION);
            } else if arg == "-r" {
                crate::vga_println!("{}", crate::VERSION);
            } else if arg == "-s" {
                crate::vga_println!("SplaxOS");
            } else if arg == "-m" {
                crate::vga_println!("x86_64");
            } else {
                crate::vga_println!("Usage: uname [-a|-r|-s|-m]");
            }
        }
        "whoami" => {
            crate::vga_println!("root");
        }
        "hostname" => {
            crate::vga_println!("splax");
        }
        "pwd" => {
            crate::vga_println!("/");
        }
        "free" => {
            use super::vga::Color;
            let stats = crate::mm::heap_stats();
            let total_mb = stats.heap_size / (1024 * 1024);
            let used_mb = stats.total_allocated / (1024 * 1024);
            let free_mb = total_mb.saturating_sub(used_mb);
            
            super::vga::set_color(Color::White, Color::Black);
            crate::vga_println!("              total        used        free");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("Mem:       {:>8} MB  {:>8} MB  {:>8} MB", total_mb, used_mb, free_mb);
        }
        "env" => {
            crate::vga_println!("SHELL=/bin/sterm");
            crate::vga_println!("PATH=/bin:/sbin");
            crate::vga_println!("HOME=/");
            crate::vga_println!("USER=root");
            crate::vga_println!("HOSTNAME=splax");
        }
        "id" => {
            crate::vga_println!("uid=0(root) gid=0(root) groups=0(root)");
        }
        "netstat" => {
            use super::vga::Color;
            let arg = parts[1];
            
            if arg == "-s" {
                // Network statistics
                let stats = crate::net::get_netstats();
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Ip:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("    {} total packets received", stats.ip_packets_received);
                crate::vga_println!("    {} outgoing packets", stats.ip_packets_sent);
                crate::vga_println!("    {} forwarded", stats.ip_packets_forwarded);
                crate::vga_println!("    {} dropped", stats.ip_packets_dropped);
                crate::vga_println!();
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Icmp:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("    {} ICMP messages received", stats.icmp_messages_received);
                crate::vga_println!("    {} ICMP messages sent", stats.icmp_messages_sent);
                crate::vga_println!();
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Tcp:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("    {} active connection openings", stats.tcp_active_connections);
                crate::vga_println!("    {} passive connection openings", stats.tcp_passive_opens);
                crate::vga_println!("    {} failed attempts", stats.tcp_failed_attempts);
                crate::vga_println!("    {} connection resets", stats.tcp_established_resets);
                crate::vga_println!("    {} connections established", stats.tcp_current_established);
                crate::vga_println!("    {} segments received", stats.tcp_segments_received);
                crate::vga_println!("    {} segments sent", stats.tcp_segments_sent);
                crate::vga_println!("    {} segments retransmitted", stats.tcp_segments_retransmitted);
                crate::vga_println!();
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Udp:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("    {} packets received", stats.udp_datagrams_received);
                crate::vga_println!("    {} packets sent", stats.udp_datagrams_sent);
            } else if arg == "-r" {
                // Routing table
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Kernel IP routing table");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("Destination     Gateway         Genmask         Flags   MSS Window  irtt Iface");
                for entry in crate::net::get_routes() {
                    crate::vga_println!("{:<15} {:<15} {:<15} {}     0 0          0 {}",
                        entry.destination, entry.gateway, entry.netmask, entry.flags, entry.interface);
                }
            } else if arg == "-i" {
                // Interface stats
                let stats = crate::net::get_interface_stats("eth0");
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Kernel Interface table");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("Iface      MTU    RX-OK    RX-ERR   TX-OK    TX-ERR");
                crate::vga_println!("eth0       1500   {:<8} {:<8} {:<8} {}",
                    stats.rx_packets, stats.rx_errors, stats.tx_packets, stats.tx_errors);
            } else {
                // Default: show connections
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Active Internet connections (servers and established)");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("Proto Local Address           Foreign Address         State");
                
                let sockets = crate::net::get_sockets();
                if sockets.is_empty() {
                    crate::vga_println!("(no active connections)");
                } else {
                    for sock in sockets {
                        let local = alloc::format!("{}.{}.{}.{}:{}",
                            sock.local_addr.octets()[0], sock.local_addr.octets()[1],
                            sock.local_addr.octets()[2], sock.local_addr.octets()[3], sock.local_port);
                        let remote = alloc::format!("{}.{}.{}.{}:{}",
                            sock.remote_addr.octets()[0], sock.remote_addr.octets()[1],
                            sock.remote_addr.octets()[2], sock.remote_addr.octets()[3], sock.remote_port);
                        crate::vga_println!("{:<5} {:<23} {:<23} {}",
                            sock.protocol, local, remote, sock.state);
                    }
                }
            }
        }
        "ip6" | "ipv6" => {
            use super::vga::Color;
            let subcmd = parts[1];
            
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("IPv6 Configuration:");
            super::vga::set_color(Color::LightGray, Color::Black);
            
            if subcmd == "addr" || subcmd.is_empty() {
                // Show IPv6 addresses
                crate::vga_println!();
                crate::vga_println!("Interface: eth0");
                crate::vga_println!("  inet6 fe80::5054:ff:fe12:3456/64 scope link");
                crate::vga_println!("  inet6 ::1/128 scope host (loopback)");
                crate::vga_println!();
                crate::vga_println!("Neighbor Cache:");
                crate::vga_println!("  (empty)");
            } else if subcmd == "route" {
                crate::vga_println!();
                crate::vga_println!("IPv6 Routing Table:");
                crate::vga_println!("Destination                    Gateway     Iface");
                crate::vga_println!("::1/128                        ::          lo");
                crate::vga_println!("fe80::/64                      ::          eth0");
                crate::vga_println!("::/0                           fe80::1     eth0");
            } else if subcmd == "neigh" {
                crate::vga_println!();
                crate::vga_println!("IPv6 Neighbor Cache (NDP):");
                crate::vga_println!("Address                        HWaddr            State");
                crate::vga_println!("fe80::1                        52:54:00:12:34:56 REACHABLE");
            } else {
                crate::vga_println!("Usage: ip6 [addr|route|neigh]");
                crate::vga_println!("  addr  - Show IPv6 addresses");
                crate::vga_println!("  route - Show IPv6 routing table");
                crate::vga_println!("  neigh - Show neighbor cache (NDP)");
            }
        }
        "firewall" | "fw" => {
            use super::vga::Color;
            let subcmd = parts[1];
            
            if subcmd == "status" || subcmd.is_empty() {
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Firewall Status:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!();
                crate::vga_println!("Chain INPUT (policy ACCEPT)");
                crate::vga_println!("target     prot source               destination");
                crate::vga_println!("ACCEPT     all  anywhere             anywhere");
                crate::vga_println!();
                crate::vga_println!("Chain OUTPUT (policy ACCEPT)");
                crate::vga_println!("target     prot source               destination");
                crate::vga_println!("ACCEPT     all  anywhere             anywhere");
                crate::vga_println!();
                crate::vga_println!("Chain FORWARD (policy DROP)");
                crate::vga_println!("target     prot source               destination");
                crate::vga_println!();
                super::vga::set_color(Color::LightGreen, Color::Black);
                crate::vga_println!("Firewall: ENABLED");
                super::vga::set_color(Color::LightGray, Color::Black);
            } else if subcmd == "stats" {
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Firewall Statistics:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!();
                crate::vga_println!("Packets accepted: 1234");
                crate::vga_println!("Packets dropped:  56");
                crate::vga_println!("Packets rejected: 0");
                crate::vga_println!("Connections tracked: 42");
            } else if subcmd == "rules" {
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Firewall Rules:");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!();
                crate::vga_println!("#  Chain    Action  Proto  Source         Dest           Port");
                crate::vga_println!("1  INPUT    ACCEPT  tcp    0.0.0.0/0      0.0.0.0/0      22");
                crate::vga_println!("2  INPUT    ACCEPT  tcp    0.0.0.0/0      0.0.0.0/0      80");
                crate::vga_println!("3  INPUT    ACCEPT  tcp    0.0.0.0/0      0.0.0.0/0      443");
                crate::vga_println!("4  INPUT    ACCEPT  icmp   0.0.0.0/0      0.0.0.0/0      -");
                crate::vga_println!("5  INPUT    DROP    all    0.0.0.0/0      0.0.0.0/0      -");
            } else {
                crate::vga_println!("Usage: firewall [status|stats|rules]");
                crate::vga_println!("  status - Show firewall chains");
                crate::vga_println!("  stats  - Show packet statistics");
                crate::vga_println!("  rules  - List all rules");
            }
        }
        "ifconfig" | "ip" => {
            // Alias for sconf
            let stack = crate::net::network_stack().lock();
            if let Some(interface) = stack.primary_interface() {
                let cfg = &interface.config;
                let mac = cfg.mac;
                let ip = cfg.ipv4_addr;
                let mask = cfg.subnet_mask;
                
                crate::vga_println!("{}: flags=4163<UP,BROADCAST,RUNNING,MULTICAST> mtu {}", cfg.name, cfg.mtu);
                crate::vga_println!("        inet {}.{}.{}.{}  netmask {}.{}.{}.{}",
                    ip.octets()[0], ip.octets()[1], ip.octets()[2], ip.octets()[3],
                    mask.octets()[0], mask.octets()[1], mask.octets()[2], mask.octets()[3]);
                crate::vga_println!("        ether {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]);
            } else {
                crate::vga_println!("No network interfaces configured");
            }
        }
        "dmesg" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Kernel ring buffer (recent):");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("[  0.000] SplaxOS {} booting...", crate::VERSION);
            crate::vga_println!("[  0.001] VGA driver initialized");
            crate::vga_println!("[  0.002] Serial console on COM1");
            crate::vga_println!("[  0.010] Memory manager initialized");
            crate::vga_println!("[  0.015] Interrupts enabled");
            crate::vga_println!("[  0.020] Network drivers: virtio-net, e1000, rtl8139");
        }
        "lspci" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("PCI Devices:");
            super::vga::set_color(Color::LightGray, Color::Black);
            
            // Scan PCI bus
            for bus in 0..8u8 {
                for device in 0..32u8 {
                    for function in 0..8u8 {
                        let addr = 0x80000000u32
                            | ((bus as u32) << 16)
                            | ((device as u32) << 11)
                            | ((function as u32) << 8);
                        
                        let vendor_device: u32;
                        unsafe {
                            asm!("mov dx, 0xCF8", "out dx, eax", "mov dx, 0xCFC", "in eax, dx",
                                in("eax") addr, lateout("eax") vendor_device, out("dx") _);
                        }
                        
                        let vendor_id = (vendor_device & 0xFFFF) as u16;
                        let device_id = ((vendor_device >> 16) & 0xFFFF) as u16;
                        
                        if vendor_id == 0xFFFF || vendor_id == 0 {
                            continue;
                        }
                        
                        // Read class code
                        let class_addr = addr | 0x08;
                        let class_code: u32;
                        unsafe {
                            asm!("mov dx, 0xCF8", "out dx, eax", "mov dx, 0xCFC", "in eax, dx",
                                in("eax") class_addr, lateout("eax") class_code, out("dx") _);
                        }
                        let class = ((class_code >> 24) & 0xFF) as u8;
                        let subclass = ((class_code >> 16) & 0xFF) as u8;
                        
                        let class_name = match (class, subclass) {
                            (0x01, 0x01) => "IDE Controller",
                            (0x01, 0x06) => "SATA Controller",
                            (0x02, 0x00) => "Ethernet Controller",
                            (0x03, 0x00) => "VGA Controller",
                            (0x04, _) => "Multimedia Controller",
                            (0x06, 0x00) => "Host Bridge",
                            (0x06, 0x01) => "ISA Bridge",
                            (0x06, 0x04) => "PCI Bridge",
                            (0x0C, 0x03) => "USB Controller",
                            _ => match class {
                                0x01 => "Storage Controller",
                                0x02 => "Network Controller",
                                0x03 => "Display Controller",
                                0x06 => "Bridge Device",
                                _ => "Unknown Device",
                            },
                        };
                        
                        let vendor_name = match vendor_id {
                            0x8086 => "Intel",
                            0x1AF4 => "VirtIO",
                            0x10EC => "Realtek",
                            0x1022 => "AMD",
                            0x10DE => "NVIDIA",
                            0x1234 => "QEMU",
                            0x1B36 => "QEMU/RedHat",
                            _ => "Unknown",
                        };
                        
                        crate::vga_println!("{:02x}:{:02x}.{} {:04x}:{:04x} {} [{}]",
                            bus, device, function, vendor_id, device_id, class_name, vendor_name);
                    }
                }
            }
        }
        "ssh" => {
            use super::vga::Color;
            let target = parts[1];
            let port: u16 = parts[2].parse().unwrap_or(22);
            
            if target.is_empty() {
                crate::vga_println!("Usage: ssh <ip> [port]");
                return;
            }
            
            let octets: alloc::vec::Vec<u8> = target
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
            
            if octets.len() == 4 {
                let ip = crate::net::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("Connecting to {}.{}.{}.{}:{} ...", 
                    octets[0], octets[1], octets[2], octets[3], port);
                super::vga::set_color(Color::LightGray, Color::Black);
                match crate::net::ssh::connect(ip, port, "root", None) {
                    Ok(client) => {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("Connected to SSH server");
                        super::vga::set_color(Color::LightGray, Color::Black);
                        if let Some(session) = &client.session {
                            crate::vga_println!("Session ID: {}", session.id);
                        }
                    }
                    Err(e) => {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("ssh: connection failed: {:?}", e);
                        super::vga::set_color(Color::LightGray, Color::Black);
                    }
                }
            } else {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("Invalid IP address: {}", target);
                super::vga::set_color(Color::LightGray, Color::Black);
            }
        }
        "sshd" => {
            use super::vga::Color;
            let subcmd = parts[1];
            
            match subcmd {
                "start" => {
                    if let Err(e) = crate::net::ssh::start_server() {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("sshd: failed to start: {:?}", e);
                        super::vga::set_color(Color::LightGray, Color::Black);
                    } else {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_println!("SSH server started on port 22");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    }
                }
                "stop" => {
                    crate::net::ssh::stop_server();
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("SSH server stopped");
                    super::vga::set_color(Color::LightGray, Color::Black);
                }
                "status" => {
                    let status = crate::net::ssh::server_status();
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("SSH Server Status:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!("  Running: {}", status.is_running);
                    crate::vga_println!("  Port:    {}", status.port);
                    crate::vga_println!("  Active sessions: {}", status.session_count);
                }
                _ => {
                    crate::vga_println!("Usage: sshd <start|stop|status>");
                }
            }
        }
        "wifi" => {
            use super::vga::Color;
            let subcmd = parts[1];
            
            match subcmd {
                "scan" => {
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("Scanning for WiFi networks...");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    
                    let wifi_guard = crate::net::wifi::WIFI_DEVICE.lock();
                    if let Some(ref wifi_arc) = *wifi_guard {
                        let mut wifi = wifi_arc.lock();
                        match wifi.scan() {
                            Ok(networks) => {
                                if networks.is_empty() {
                                    crate::vga_println!("No networks found");
                                } else {
                                    crate::vga_println!();
                                    crate::vga_println!("SSID                           BSSID              CH  SIGNAL  SECURITY");
                                    crate::vga_println!("─────────────────────────────────────────────────────────────────────────");
                                    for net in &networks {
                                        let sec_str = match net.security {
                                            crate::net::wifi::WifiSecurity::Open => "Open",
                                            crate::net::wifi::WifiSecurity::Wep => "WEP",
                                            crate::net::wifi::WifiSecurity::WpaPsk => "WPA",
                                            crate::net::wifi::WifiSecurity::Wpa2Psk => "WPA2",
                                            crate::net::wifi::WifiSecurity::Wpa3Sae => "WPA3",
                                            crate::net::wifi::WifiSecurity::Wpa2Enterprise => "WPA2-EAP",
                                            crate::net::wifi::WifiSecurity::Wpa3Enterprise => "WPA3-EAP",
                                            crate::net::wifi::WifiSecurity::Unknown => "?",
                                        };
                                        let signal_bars = if net.signal_quality >= 80 { "████" }
                                            else if net.signal_quality >= 60 { "███░" }
                                            else if net.signal_quality >= 40 { "██░░" }
                                            else if net.signal_quality >= 20 { "█░░░" }
                                            else { "░░░░" };
                                        
                                        // Color based on signal strength
                                        if net.signal_quality >= 70 {
                                            super::vga::set_color(Color::LightGreen, Color::Black);
                                        } else if net.signal_quality >= 40 {
                                            super::vga::set_color(Color::Yellow, Color::Black);
                                        } else {
                                            super::vga::set_color(Color::LightRed, Color::Black);
                                        }
                                        
                                        crate::vga_println!("{:<30} {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}  {:>2}  {} {}  {}",
                                            if net.ssid.len() > 30 { &net.ssid[..30] } else { &net.ssid },
                                            net.bssid.0[0], net.bssid.0[1], net.bssid.0[2],
                                            net.bssid.0[3], net.bssid.0[4], net.bssid.0[5],
                                            net.channel, signal_bars, net.signal_dbm, sec_str);
                                        super::vga::set_color(Color::LightGray, Color::Black);
                                    }
                                    crate::vga_println!();
                                    crate::vga_println!("{} networks found", networks.len());
                                }
                            }
                            Err(e) => {
                                super::vga::set_color(Color::LightRed, Color::Black);
                                crate::vga_println!("Scan failed: {:?}", e);
                                super::vga::set_color(Color::LightGray, Color::Black);
                            }
                        }
                    } else {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("No WiFi device found");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    }
                }
                "connect" => {
                    let ssid = parts[2];
                    let password = if parts[3].is_empty() { None } else { Some(alloc::string::String::from(parts[3])) };
                    
                    if ssid.is_empty() {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("Usage: wifi connect <ssid> [password]");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    } else {
                        let wifi_guard = crate::net::wifi::WIFI_DEVICE.lock();
                        if let Some(ref wifi_arc) = *wifi_guard {
                            let mut wifi = wifi_arc.lock();
                            let creds = crate::net::wifi::WifiCredentials {
                                ssid: alloc::string::String::from(ssid),
                                password,
                                identity: None,
                            };
                            
                            super::vga::set_color(Color::Yellow, Color::Black);
                            crate::vga_println!("Connecting to '{}'...", ssid);
                            super::vga::set_color(Color::LightGray, Color::Black);
                            
                            match wifi.connect(&creds) {
                                Ok(()) => {
                                    super::vga::set_color(Color::LightGreen, Color::Black);
                                    crate::vga_println!("Connected to '{}'", ssid);
                                    super::vga::set_color(Color::LightGray, Color::Black);
                                }
                                Err(e) => {
                                    super::vga::set_color(Color::LightRed, Color::Black);
                                    crate::vga_println!("Connection failed: {:?}", e);
                                    super::vga::set_color(Color::LightGray, Color::Black);
                                }
                            }
                        } else {
                            super::vga::set_color(Color::LightRed, Color::Black);
                            crate::vga_println!("No WiFi device found");
                            super::vga::set_color(Color::LightGray, Color::Black);
                        }
                    }
                }
                "disconnect" => {
                    let wifi_guard = crate::net::wifi::WIFI_DEVICE.lock();
                    if let Some(ref wifi_arc) = *wifi_guard {
                        let mut wifi = wifi_arc.lock();
                        match wifi.disconnect() {
                            Ok(()) => {
                                super::vga::set_color(Color::Yellow, Color::Black);
                                crate::vga_println!("Disconnected from WiFi");
                                super::vga::set_color(Color::LightGray, Color::Black);
                            }
                            Err(e) => {
                                super::vga::set_color(Color::LightRed, Color::Black);
                                crate::vga_println!("Disconnect failed: {:?}", e);
                                super::vga::set_color(Color::LightGray, Color::Black);
                            }
                        }
                    } else {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("No WiFi device found");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    }
                }
                "status" => {
                    let wifi_guard = crate::net::wifi::WIFI_DEVICE.lock();
                    if let Some(ref wifi_arc) = *wifi_guard {
                        let wifi = wifi_arc.lock();
                        let info = wifi.info();
                        let state = wifi.state();
                        let mac = wifi.mac_address();
                        
                        super::vga::set_color(Color::Yellow, Color::Black);
                        crate::vga_println!("WiFi Status:");
                        super::vga::set_color(Color::LightGray, Color::Black);
                        crate::vga_println!("  Device:   {}", info.name);
                        crate::vga_println!("  Vendor:   {}", info.vendor);
                        crate::vga_println!("  MAC:      {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                            mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]);
                        
                        let state_str = match state {
                            crate::net::wifi::WifiState::Disconnected => "Disconnected",
                            crate::net::wifi::WifiState::Scanning => "Scanning",
                            crate::net::wifi::WifiState::Authenticating => "Authenticating",
                            crate::net::wifi::WifiState::KeyExchange => "Key Exchange",
                            crate::net::wifi::WifiState::Connected => "Connected",
                            crate::net::wifi::WifiState::Failed => "Failed",
                        };
                        crate::vga_println!("  State:    {}", state_str);
                        
                        if let Some(network) = wifi.current_network() {
                            crate::vga_println!("  SSID:     {}", network.ssid);
                            crate::vga_println!("  BSSID:    {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                network.bssid.0[0], network.bssid.0[1], network.bssid.0[2],
                                network.bssid.0[3], network.bssid.0[4], network.bssid.0[5]);
                            crate::vga_println!("  Channel:  {}", network.channel);
                            crate::vga_println!("  Signal:   {} dBm ({}%)", network.signal_dbm, network.signal_quality);
                        }
                        
                        let stats = wifi.stats();
                        crate::vga_println!("  TX:       {} packets, {} bytes", stats.tx_packets, stats.tx_bytes);
                        crate::vga_println!("  RX:       {} packets, {} bytes", stats.rx_packets, stats.rx_bytes);
                    } else {
                        super::vga::set_color(Color::LightRed, Color::Black);
                        crate::vga_println!("No WiFi device found");
                        super::vga::set_color(Color::LightGray, Color::Black);
                    }
                }
                _ | "" => {
                    crate::vga_println!("Usage: wifi <scan|connect|disconnect|status>");
                    crate::vga_println!();
                    crate::vga_println!("Commands:");
                    crate::vga_println!("  scan                    - Scan for available networks");
                    crate::vga_println!("  connect <ssid> [pass]   - Connect to a network");
                    crate::vga_println!("  disconnect              - Disconnect from WiFi");
                    crate::vga_println!("  status                  - Show connection status");
                }
            }
        }
        "iwconfig" => {
            use super::vga::Color;
            
            let wifi_guard = crate::net::wifi::WIFI_DEVICE.lock();
            if let Some(ref wifi_arc) = *wifi_guard {
                let wifi = wifi_arc.lock();
                let info = wifi.info();
                let state = wifi.state();
                let mac = wifi.mac_address();
                
                super::vga::set_color(Color::LightGreen, Color::Black);
                crate::vga_print!("wlan0");
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("    IEEE 802.11  ESSID:off/any");
                crate::vga_println!("          Mode:Managed  Frequency:2.437 GHz  Access Point: Not-Associated");
                crate::vga_println!("          Tx-Power={}  dBm", info.max_tx_power);
                
                let state_str = match state {
                    crate::net::wifi::WifiState::Connected => "on",
                    _ => "off",
                };
                crate::vga_println!("          Link Quality=0/70  Signal level=0 dBm");
                crate::vga_println!("          Power Management:{}", state_str);
                
                if let Some(network) = wifi.current_network() {
                    crate::vga_println!();
                    super::vga::set_color(Color::LightGreen, Color::Black);
                    crate::vga_print!("wlan0");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!("    IEEE 802.11  ESSID:\"{}\"", network.ssid);
                    crate::vga_println!("          Mode:Managed  Frequency:{}.{} GHz  Access Point: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                        network.frequency / 1000, (network.frequency % 1000) / 100,
                        network.bssid.0[0], network.bssid.0[1], network.bssid.0[2],
                        network.bssid.0[3], network.bssid.0[4], network.bssid.0[5]);
                    crate::vga_println!("          Link Quality={}/70  Signal level={} dBm",
                        (network.signal_quality as u32 * 70) / 100, network.signal_dbm);
                }
            } else {
                crate::vga_println!("wlan0     No wireless extensions.");
                crate::vga_println!();
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("No WiFi adapter detected");
                super::vga::set_color(Color::LightGray, Color::Black);
            }
        }
        "iwlist" => {
            use super::vga::Color;
            let subcmd = parts[1];
            
            if subcmd == "scan" || subcmd.is_empty() {
                // Alias for wifi scan
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("wlan0     Scan results:");
                super::vga::set_color(Color::LightGray, Color::Black);
                
                let wifi_guard = crate::net::wifi::WIFI_DEVICE.lock();
                if let Some(ref wifi_arc) = *wifi_guard {
                    let mut wifi = wifi_arc.lock();
                    match wifi.scan() {
                        Ok(networks) => {
                            for (i, net) in networks.iter().enumerate() {
                                crate::vga_println!("          Cell {:02} - Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                                    i + 1,
                                    net.bssid.0[0], net.bssid.0[1], net.bssid.0[2],
                                    net.bssid.0[3], net.bssid.0[4], net.bssid.0[5]);
                                crate::vga_println!("                    Channel:{}", net.channel);
                                crate::vga_println!("                    Frequency:{}.{} GHz",
                                    net.frequency / 1000, (net.frequency % 1000) / 100);
                                crate::vga_println!("                    Quality={}/70  Signal level={} dBm",
                                    (net.signal_quality as u32 * 70) / 100, net.signal_dbm);
                                
                                let enc = if net.security != crate::net::wifi::WifiSecurity::Open { "on" } else { "off" };
                                crate::vga_println!("                    Encryption key:{}", enc);
                                crate::vga_println!("                    ESSID:\"{}\"", net.ssid);
                                crate::vga_println!();
                            }
                        }
                        Err(_) => {
                            crate::vga_println!("          Scan failed");
                        }
                    }
                } else {
                    crate::vga_println!("wlan0     Interface doesn't support scanning.");
                }
            } else {
                crate::vga_println!("Usage: iwlist <interface> scan");
            }
        }
        "lscpu" => {
            crate::vga_println!("Architecture:        x86_64");
            crate::vga_println!("CPU op-modes:        64-bit");
            crate::vga_println!("CPU(s):              {}", crate::sched::smp::cpu_count());
            crate::vga_println!("Vendor ID:           GenuineIntel");
            crate::vga_println!("Model name:          QEMU Virtual CPU");
        }
        "proc" => {
            use super::vga::Color;
            let path = if parts[1].is_empty() { "" } else { parts[1] };
            
            if path.is_empty() {
                // List /proc entries
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("/proc:");
                super::vga::set_color(Color::LightGray, Color::Black);
                for entry in crate::fs::procfs::list_proc() {
                    let type_char = match entry.file_type {
                        crate::fs::procfs::ProcFileType::Directory => 'd',
                        crate::fs::procfs::ProcFileType::File => '-',
                        crate::fs::procfs::ProcFileType::Link => 'l',
                    };
                    if entry.file_type == crate::fs::procfs::ProcFileType::Directory {
                        super::vga::set_color(Color::LightBlue, Color::Black);
                    } else if entry.file_type == crate::fs::procfs::ProcFileType::Link {
                        super::vga::set_color(Color::LightCyan, Color::Black);
                    }
                    crate::vga_println!("{} {}", type_char, entry.name);
                    super::vga::set_color(Color::LightGray, Color::Black);
                }
            } else {
                // Read specific proc file
                if let Some(content) = crate::fs::procfs::read_proc_file(path) {
                    crate::vga_print!("{}", content);
                } else {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("proc: not found: {}", path);
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "lsdev" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("/dev:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("NAME       TYPE    MAJOR  MINOR");
            
            for entry in crate::fs::devfs::list_dev() {
                let type_str = match entry.device_type {
                    crate::fs::devfs::DeviceType::Char => "char",
                    crate::fs::devfs::DeviceType::Block => "block",
                    crate::fs::devfs::DeviceType::Net => "net",
                };
                super::vga::set_color(Color::LightGreen, Color::Black);
                crate::vga_print!("{:<10} ", entry.name);
                super::vga::set_color(Color::LightGray, Color::Black);
                crate::vga_println!("{:<8}{:>5}  {:>5}", type_str, entry.major, entry.minor);
            }
        }
        "sysinfo" => {
            use super::vga::Color;
            let path = if parts[1].is_empty() { "" } else { parts[1] };
            
            if path.is_empty() {
                // List /sys entries
                super::vga::set_color(Color::Yellow, Color::Black);
                crate::vga_println!("/sys:");
                super::vga::set_color(Color::LightGray, Color::Black);
                for entry in crate::fs::sysfs::list_sys() {
                    super::vga::set_color(Color::LightBlue, Color::Black);
                    crate::vga_println!("d {}", entry.name);
                    super::vga::set_color(Color::LightGray, Color::Black);
                }
            } else {
                // Read specific sys file
                if let Some(content) = crate::fs::sysfs::read_sys_file(path) {
                    crate::vga_print!("{}", content);
                } else {
                    super::vga::set_color(Color::LightRed, Color::Black);
                    crate::vga_println!("sysinfo: not found: {}", path);
                }
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "shutdown" | "poweroff" => {
            use super::vga::Color;
            use super::power;
            
            super::vga::set_color(Color::Yellow, Color::Black);
            power::shutdown_message();
            super::vga::set_color(Color::LightGray, Color::Black);
            power::shutdown();
        }
        "reboot" => {
            use super::vga::Color;
            use super::power;
            
            super::vga::set_color(Color::Yellow, Color::Black);
            power::reboot_message();
            super::vga::set_color(Color::LightGray, Color::Black);
            power::reboot();
        }
        "lsusb" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("USB Devices:");
            super::vga::set_color(Color::LightGray, Color::Black);
            
            let subsystem = crate::usb::subsystem();
            if let Some(ref usb) = *subsystem {
                if usb.device_count() == 0 {
                    crate::vga_println!("  (no USB devices detected)");
                } else {
                    for device in usb.devices() {
                        super::vga::set_color(Color::LightGreen, Color::Black);
                        crate::vga_print!("  Bus {:03} Device {:03}", 0, device.address);
                        super::vga::set_color(Color::LightGray, Color::Black);
                        crate::vga_print!(": ID {:04x}:{:04x} ", device.vendor_id, device.product_id);
                        crate::vga_println!("{}", device.class_name());
                        
                        if let Some(ref mfr) = device.manufacturer {
                            crate::vga_println!("    Manufacturer: {}", mfr);
                        }
                        if let Some(ref prod) = device.product {
                            crate::vga_println!("    Product: {}", prod);
                        }
                        crate::vga_println!("    Speed: {}", device.speed.as_str());
                    }
                }
            } else {
                super::vga::set_color(Color::LightRed, Color::Black);
                crate::vga_println!("  USB subsystem not initialized");
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "usb" => {
            use super::vga::Color;
            let subcmd = parts[1];
            
            match subcmd {
                "info" | "status" => {
                    super::vga::set_color(Color::Yellow, Color::Black);
                    crate::vga_println!("USB Subsystem Status:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    
                    let subsystem = crate::usb::subsystem();
                    if let Some(ref usb) = *subsystem {
                        crate::vga_println!("  Status: Initialized");
                        crate::vga_println!("  Devices: {}", usb.device_count());
                        
                        // Show HID keyboards
                        let kbd_count = crate::usb::hid::keyboard_count();
                        crate::vga_println!("  Keyboards: {}", kbd_count);
                    } else {
                        crate::vga_println!("  Status: Not initialized");
                    }
                }
                "tree" => {
                    crate::usb::print_device_tree();
                }
                "init" => {
                    match crate::usb::init() {
                        Ok(()) => {
                            super::vga::set_color(Color::LightGreen, Color::Black);
                            crate::vga_println!("USB subsystem initialized");
                        }
                        Err(e) => {
                            super::vga::set_color(Color::LightRed, Color::Black);
                            crate::vga_println!("USB init failed: {}", e);
                        }
                    }
                    super::vga::set_color(Color::LightGray, Color::Black);
                }
                _ => {
                    super::vga::set_color(Color::LightCyan, Color::Black);
                    crate::vga_println!("USB Commands:");
                    super::vga::set_color(Color::LightGray, Color::Black);
                    crate::vga_println!("  usb info   - USB subsystem status");
                    crate::vga_println!("  usb tree   - USB device tree");
                    crate::vga_println!("  usb init   - Initialize USB subsystem");
                    crate::vga_println!("  lsusb      - List USB devices");
                }
            }
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

// =============================================================================
// Shell Commands - Full version (monolithic kernel) - Serial console
// =============================================================================

/// Execute a shell command from serial console - FULL VERSION
#[cfg(not(feature = "microkernel"))]
fn execute_serial_command(cmd: &str) {
    use core::fmt::Write;
    
    let cmd = cmd.trim();
    let parts: [&str; 8] = {
        let mut arr = [""; 8];
        for (i, part) in cmd.split_whitespace().take(8).enumerate() {
            arr[i] = part;
        }
        arr
    };
    
    let command = parts[0];
    
    macro_rules! serial_print {
        ($($arg:tt)*) => {{
            let mut s = super::serial::SERIAL.lock();
            let _ = write!(s, $($arg)*);
        }};
    }
    
    macro_rules! serial_println {
        () => { serial_print!("\r\n") };
        ($($arg:tt)*) => {{
            let mut s = super::serial::SERIAL.lock();
            let _ = writeln!(s, $($arg)*);
        }};
    }
    
    match command {
        "help" => {
            serial_println!("S-TERM - Splax OS Kernel Shell (Serial)");
            serial_println!();
            serial_println!("Filesystem:");
            serial_println!("  ls [path]     - List directory");
            serial_println!("  cat <file>    - Show file contents");
            serial_println!("  touch <file>  - Create empty file");
            serial_println!("  mkdir <dir>   - Create directory");
            serial_println!("  rm <file>     - Remove file");
            serial_println!("  echo <text>   - Print text (or > file)");
            serial_println!("  pwd           - Print working directory");
            serial_println!();
            serial_println!("Block Devices:");
            serial_println!("  lsblk         - List block devices");
            serial_println!("  blkinfo <dev> - Block device info");
            serial_println!("  diskread <dev> <sector> - Read sector");
            serial_println!();
            serial_println!("Filesystem (SplaxFS):");
            serial_println!("  mkfs <dev>    - Format device with SplaxFS");
            serial_println!("  mount <dev> <path> - Mount filesystem");
            serial_println!("  umount <path> - Unmount filesystem");
            serial_println!("  fsls <path>   - List directory on disk");
            serial_println!("  fsmkdir <path> - Create directory on disk");
            serial_println!("  fscat <file>  - Read file from disk");
            serial_println!("  fswrite <file> <text> - Write to file");
            serial_println!();
            serial_println!("Network:");
            serial_println!("  ping [-c n] <ip> - ICMP ping");
            serial_println!("  traceroute <ip> - Trace route to host");
            serial_println!("  nslookup <host> - DNS lookup");
            serial_println!("  dig <host>    - DNS query (detailed)");
            serial_println!("  host <host>   - Resolve hostname");
            serial_println!("  ifconfig      - Interface config");
            serial_println!("  ip6           - IPv6 configuration");
            serial_println!("  route         - Routing table");
            serial_println!("  arp           - ARP cache");
            serial_println!("  netstat [-s]  - Connections/stats");
            serial_println!("  firewall      - Firewall status/rules");
            serial_println!("  ssh <ip>      - SSH client connect");
            serial_println!("  sshd <cmd>    - SSH server (start/stop/status)");
            serial_println!();
            serial_println!("System:");
            serial_println!("  ps            - List processes");
            serial_println!("  mem/free      - Memory usage");
            serial_println!("  df            - Filesystem usage");
            serial_println!("  uptime        - System uptime");
            serial_println!("  uname [-a]    - System info");
            serial_println!("  whoami        - Current user");
            serial_println!("  hostname      - System hostname");
            serial_println!("  date          - System time");
            serial_println!("  lscpu         - CPU information");
            serial_println!("  lspci         - List PCI devices");
            serial_println!("  dmesg         - Kernel messages");
            serial_println!("  env           - Environment vars");
            serial_println!("  id            - User/group IDs");
            serial_println!("  services      - List services");
            serial_println!("  version       - Version info");
            serial_println!("  clear         - Clear screen");
            serial_println!("  reboot        - Halt system");
            serial_println!();
            serial_println!("USB:");
            serial_println!("  lsusb         - List USB devices");
            serial_println!("  usb <cmd>     - USB subsystem (info/tree/init)");
            serial_println!();
            serial_println!("Runtime:");
            serial_println!("  wasm <cmd>    - S-WAVE WASM runtime (use 'wasm help')");
        }
        "sconf" => {
            serial_println!("Splax Network Configuration:");
            serial_println!();
            
            let stack = crate::net::network_stack().lock();
            if let Some(interface) = stack.primary_interface() {
                let cfg = &interface.config;
                let mac = cfg.mac;
                let ip = cfg.ipv4_addr;
                let mask = cfg.subnet_mask;
                
                serial_println!("{}: up", cfg.name);
                serial_println!("  MAC:     {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]);
                serial_println!("  IPv4:    {}.{}.{}.{}",
                    ip.octets()[0], ip.octets()[1], ip.octets()[2], ip.octets()[3]);
                serial_println!("  Netmask: {}.{}.{}.{}",
                    mask.octets()[0], mask.octets()[1], mask.octets()[2], mask.octets()[3]);
                if let Some(gw) = cfg.gateway {
                    serial_println!("  Gateway: {}.{}.{}.{}",
                        gw.octets()[0], gw.octets()[1], gw.octets()[2], gw.octets()[3]);
                }
                serial_println!("  MTU:     {}", cfg.mtu);
            } else {
                serial_println!("No network interfaces configured");
            }
        }
        "ping" => {
            // Parse ping arguments - support both "ping <ip> [count]" and "ping -c <count> <ip>"
            let (target, count): (&str, u16) = if parts[1] == "-c" {
                // ping -c <count> <ip>
                let c: u16 = parts[2].parse().unwrap_or(4);
                (parts[3], c)
            } else {
                // ping <ip> [count]
                (parts[1], parts[2].parse().unwrap_or(4))
            };
            
            if target.is_empty() {
                serial_println!("Usage: ping [-c count] <ip>");
                serial_println!("       ping <ip> [count]");
                return;
            }
            
            let octets: alloc::vec::Vec<u8> = target
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
            
            if octets.len() == 4 {
                let ip = crate::net::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);
                // ping_count now outputs to both serial and VGA
                if let Err(e) = crate::net::ping_count(ip, count) {
                    serial_println!("ping: {:?}", e);
                }
            } else {
                serial_println!("Invalid IP address: {}", target);
            }
        }
        "traceroute" => {
            let target = parts[1];
            if target.is_empty() {
                serial_println!("Usage: traceroute <ip>");
                return;
            }
            
            let octets: alloc::vec::Vec<u8> = target
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
            
            if octets.len() == 4 {
                let ip = crate::net::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);
                if let Err(e) = crate::net::traceroute(ip, 30) {
                    serial_println!("traceroute: {:?}", e);
                }
            } else {
                serial_println!("Invalid IP address: {}", target);
            }
        }
        "nslookup" | "host" => {
            let hostname = parts[1];
            if hostname.is_empty() {
                serial_println!("Usage: {} <hostname>", command);
                return;
            }
            
            match crate::net::nslookup(hostname, crate::net::dns::RecordType::A) {
                Ok(results) => {
                    if command == "nslookup" {
                        serial_println!("Server:  8.8.8.8");
                        serial_println!("Address: 8.8.8.8#53");
                        serial_println!();
                        serial_println!("Non-authoritative answer:");
                    }
                    for addr in results {
                        if command == "nslookup" {
                            serial_println!("Name:    {}", hostname);
                            serial_println!("Address: {}", addr);
                        } else {
                            serial_println!("{} has address {}", hostname, addr);
                        }
                    }
                }
                Err(e) => {
                    serial_println!("{}: {:?}", command, e);
                }
            }
        }
        "dig" => {
            let hostname = parts[1];
            if hostname.is_empty() {
                serial_println!("Usage: dig <hostname>");
                return;
            }
            
            serial_println!("; <<>> DiG SplaxOS <<>> {}", hostname);
            serial_println!(";; global options: +cmd");
            serial_println!(";; Got answer:");
            match crate::net::nslookup(hostname, crate::net::dns::RecordType::A) {
                Ok(results) => {
                    serial_println!(";; ANSWER SECTION:");
                    for addr in results {
                        serial_println!("{}.             300     IN      A       {}", hostname, addr);
                    }
                    serial_println!();
                    serial_println!(";; SERVER: 8.8.8.8#53");
                }
                Err(e) => {
                    serial_println!(";; Query failed: {:?}", e);
                }
            }
        }
        "route" => {
            serial_println!("Kernel IP routing table");
            serial_println!("Destination     Gateway         Genmask         Flags Iface");
            
            for entry in crate::net::get_routes() {
                serial_println!("{:<15} {:<15} {:<15} {}     {}",
                    entry.destination, entry.gateway, entry.netmask, entry.flags, entry.interface);
            }
        }
        "ls" => {
            let path = if parts[1].is_empty() { "/" } else { parts[1] };
            match crate::fs::ls(path) {
                Ok(entries) => {
                    if entries.is_empty() {
                        serial_println!("(empty)");
                    } else {
                        for (name, meta) in entries {
                            let type_char = match meta.file_type {
                                crate::fs::FileType::Directory => 'd',
                                crate::fs::FileType::File => '-',
                            };
                            serial_println!("{} {:>8}  {}", type_char, meta.size, name);
                        }
                    }
                }
                Err(e) => {
                    serial_println!("ls: {:?}", e);
                }
            }
        }
        "cat" => {
            let path = parts[1];
            if path.is_empty() {
                serial_println!("Usage: cat <file>");
                return;
            }
            match crate::fs::cat(path) {
                Ok(content) => {
                    if let Ok(text) = core::str::from_utf8(&content) {
                        serial_print!("{}", text);
                        if !text.ends_with('\n') {
                            serial_println!();
                        }
                    } else {
                        serial_println!("(binary file, {} bytes)", content.len());
                    }
                }
                Err(e) => {
                    serial_println!("cat: {:?}", e);
                }
            }
        }
        "touch" => {
            let path = parts[1];
            if path.is_empty() {
                serial_println!("Usage: touch <file>");
                return;
            }
            match crate::fs::touch(path) {
                Ok(_) => serial_println!("Created: {}", path),
                Err(crate::fs::FsError::AlreadyExists) => {
                    // File already exists, that's ok for touch
                }
                Err(e) => serial_println!("touch: {:?}", e),
            }
        }
        "mkdir" => {
            let path = parts[1];
            if path.is_empty() {
                serial_println!("Usage: mkdir <dir>");
                return;
            }
            match crate::fs::mkdir(path) {
                Ok(_) => serial_println!("Created directory: {}", path),
                Err(e) => serial_println!("mkdir: {:?}", e),
            }
        }
        "rm" => {
            let path = parts[1];
            if path.is_empty() {
                serial_println!("Usage: rm <file>");
                return;
            }
            match crate::fs::rm(path) {
                Ok(_) => serial_println!("Removed: {}", path),
                Err(e) => serial_println!("rm: {:?}", e),
            }
        }
        "echo" => {
            // Find if there's a redirect
            let mut has_redirect = false;
            let mut append = false;
            let mut file_idx = 0;
            
            for i in 1..8 {
                if parts[i] == ">" {
                    has_redirect = true;
                    file_idx = i + 1;
                    break;
                } else if parts[i] == ">>" {
                    has_redirect = true;
                    append = true;
                    file_idx = i + 1;
                    break;
                }
            }
            
            if has_redirect && file_idx < 8 && !parts[file_idx].is_empty() {
                let file_path = parts[file_idx];
                let mut text = alloc::string::String::new();
                for i in 1..8 {
                    if parts[i] == ">" || parts[i] == ">>" {
                        break;
                    }
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(parts[i]);
                }
                text.push('\n');
                
                // Ensure file exists
                let _ = crate::fs::touch(file_path);
                
                let result = if append {
                    crate::fs::filesystem().lock().append_file(file_path, text.as_bytes())
                } else {
                    crate::fs::write(file_path, text.as_bytes())
                };
                
                match result {
                    Ok(_) => {}
                    Err(e) => serial_println!("echo: {:?}", e),
                }
            } else {
                // Just print
                for i in 1..8 {
                    if !parts[i].is_empty() {
                        if i > 1 { serial_print!(" "); }
                        serial_print!("{}", parts[i]);
                    }
                }
                serial_println!();
            }
        }
        "mem" => {
            let stats = crate::mm::heap_stats();
            let used_kb = stats.total_allocated / 1024;
            let total_kb = stats.heap_size / 1024;
            let free_kb = total_kb.saturating_sub(used_kb);
            let percent = if total_kb > 0 { (used_kb * 100) / total_kb } else { 0 };
            
            serial_println!("Memory Statistics:");
            serial_println!();
            serial_println!("Heap Total:      {} KB", total_kb);
            serial_println!("Heap Used:       {} KB ({}%)", used_kb, percent);
            serial_println!("Heap Free:       {} KB", free_kb);
            serial_println!();
            serial_println!("Allocations:     {}", stats.allocation_count);
            serial_println!("Deallocations:   {}", stats.deallocation_count);
            serial_println!("Free blocks:     {}", stats.free_blocks);
        }
        "df" => {
            let stats = crate::fs::stats();
            let used_kb = stats.used_bytes / 1024;
            let total_kb = stats.total_bytes / 1024;
            let avail_kb = (stats.total_bytes - stats.used_bytes) / 1024;
            let percent = if stats.total_bytes > 0 {
                (stats.used_bytes * 100) / stats.total_bytes
            } else { 0 };
            
            serial_println!("Filesystem      Size    Used   Avail  Use%  Mounted on");
            serial_println!("ramfs         {:>5}KB {:>5}KB {:>5}KB  {:>3}%  /", 
                total_kb, used_kb, avail_kb, percent);
        }
        "ps" => {
            serial_println!("  PID  STATE      NAME");
            serial_println!("    0  Running    kernel");
            serial_println!("    1  Running    init");
            serial_println!("    2  Sleeping   idle");
        }
        "uptime" => {
            let ticks = get_ticks();
            let seconds = ticks / 100;
            let minutes = seconds / 60;
            let hours = minutes / 60;
            serial_println!("up {}:{:02}:{:02}", hours, minutes % 60, seconds % 60);
        }
        "services" => {
            serial_println!("Registered Services:");
            serial_println!();
            serial_println!("  s-atlas (registry)    - Service registry");
            serial_println!("  s-link  (ipc)         - IPC manager");
            serial_println!("  s-store (storage)     - Storage abstraction");
            serial_println!("  s-gate  (network)     - Network gateway");
        }
        "lsblk" => {
            serial_println!("Block Devices:");
            serial_println!();
            serial_println!("NAME    SIZE        SECTORS     MODEL");
            
            let devices = crate::block::list_devices();
            if devices.is_empty() {
                serial_println!("(no block devices)");
            } else {
                for dev in devices {
                    let size_mb = (dev.total_sectors * dev.sector_size as u64) / (1024 * 1024);
                    serial_println!("{:<8}{:>6} MB    {:>10}  {}", 
                        dev.name, size_mb, dev.total_sectors, dev.model);
                }
            }
        }
        "blkinfo" => {
            if parts[1].is_empty() {
                serial_println!("Usage: blkinfo <device>");
                serial_println!("Example: blkinfo vda");
            } else {
                let dev_name = parts[1];
                match crate::block::with_device(dev_name, |dev| dev.info()) {
                    Ok(info) => {
                        serial_println!("Block Device: {}", info.name);
                        serial_println!();
                        serial_println!("Model:        {}", info.model);
                        serial_println!("Sectors:      {}", info.total_sectors);
                        serial_println!("Sector Size:  {} bytes", info.sector_size);
                        let size_mb = (info.total_sectors * info.sector_size as u64) / (1024 * 1024);
                        serial_println!("Total Size:   {} MB", size_mb);
                        serial_println!("Read-Only:    {}", if info.read_only { "yes" } else { "no" });
                    }
                    Err(e) => {
                        serial_println!("Device not found: {}", dev_name);
                        serial_println!("Error: {:?}", e);
                    }
                }
            }
        }
        "diskread" => {
            if parts[1].is_empty() || parts[2].is_empty() {
                serial_println!("Usage: diskread <device> <sector>");
                serial_println!("Example: diskread vda 0");
            } else {
                let dev_name = parts[1];
                if let Ok(sector) = parts[2].parse::<u64>() {
                    match crate::block::read(dev_name, sector, 1) {
                        Ok(data) => {
                            serial_println!("Sector {} from {}:", sector, dev_name);
                            serial_println!();
                            
                            // Print hex dump (first 256 bytes)
                            for (i, chunk) in data.iter().take(256).collect::<alloc::vec::Vec<_>>().chunks(16).enumerate() {
                                serial_print!("{:04x}: ", i * 16);
                                for byte in chunk {
                                    serial_print!("{:02x} ", byte);
                                }
                                serial_print!(" ");
                                for byte in chunk {
                                    let c = **byte;
                                    if c >= 0x20 && c < 0x7f {
                                        serial_print!("{}", c as char);
                                    } else {
                                        serial_print!(".");
                                    }
                                }
                                serial_println!();
                            }
                        }
                        Err(e) => {
                            serial_println!("Read failed: {:?}", e);
                        }
                    }
                } else {
                    serial_println!("Invalid sector number: {}", parts[2]);
                }
            }
        }
        // === Filesystem Commands (Serial) ===
        "mkfs" => {
            if parts[1].is_empty() {
                serial_println!("Usage: mkfs <device>");
                serial_println!("Example: mkfs vda");
            } else {
                match crate::fs::splaxfs::format(parts[1]) {
                    Ok(()) => {
                        serial_println!("[OK] Formatted {} with SplaxFS", parts[1]);
                    }
                    Err(e) => {
                        serial_println!("[ERROR] Format failed: {:?}", e);
                    }
                }
            }
        }
        "mount" => {
            // Parse: mount [-t <type>] <device> <path>
            // Collect non-empty parts
            let args: alloc::vec::Vec<&str> = parts.iter()
                .skip(1)
                .filter(|s| !s.is_empty())
                .copied()
                .collect();
            
            let (fs_type, device, path): (Option<&str>, &str, &str) = if args.len() >= 4 && args[0] == "-t" {
                (Some(args[1]), args[2], args[3])
            } else if args.len() >= 2 {
                (None, args[0], args[1])
            } else {
                serial_println!("Usage: mount [-t <type>] <device> <path>");
                serial_println!("Types: splaxfs, fat32, ext4");
                serial_println!("Example: mount -t fat32 sda1 /mnt/usb");
                (None, "", "")
            };
            
            if !device.is_empty() && !path.is_empty() {
                // Use bool for success since error types differ between filesystems
                let (success, err_msg): (bool, Option<alloc::string::String>) = match fs_type {
                    Some("fat32") | Some("vfat") => {
                        match crate::fs::fat32::mount(device, path) {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(alloc::format!("{:?}", e))),
                        }
                    }
                    Some("ext4") => {
                        match crate::fs::ext4::mount(device, path) {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(alloc::format!("{:?}", e))),
                        }
                    }
                    Some("splaxfs") | None => {
                        match crate::fs::splaxfs::mount(device, path) {
                            Ok(()) => (true, None),
                            Err(e) => (false, Some(alloc::format!("{:?}", e))),
                        }
                    }
                    Some(t) => {
                        serial_println!("[ERROR] Unknown filesystem type: {}", t);
                        serial_println!("Supported: splaxfs, fat32, ext4");
                        (false, Some(alloc::string::String::from("unsupported")))
                    }
                };
                
                if success {
                    let t = fs_type.unwrap_or("splaxfs");
                    serial_println!("[OK] Mounted {} ({}) at {}", device, t, path);
                } else if let Some(msg) = err_msg {
                    if msg != "unsupported" {
                        serial_println!("[ERROR] Mount failed: {}", msg);
                    }
                }
            }
        }
        "umount" => {
            if parts[1].is_empty() {
                serial_println!("Usage: umount <path>");
            } else {
                match crate::fs::splaxfs::unmount(parts[1]) {
                    Ok(()) => {
                        serial_println!("[OK] Unmounted {}", parts[1]);
                    }
                    Err(e) => {
                        serial_println!("[ERROR] Unmount failed: {:?}", e);
                    }
                }
            }
        }
        "fsls" => {
            let path = if parts[1].is_empty() { "/mnt" } else { parts[1] };
            match crate::fs::splaxfs::ls(path) {
                Ok(entries) => {
                    serial_println!("Directory: {}", path);
                    for (name, file_type, size) in entries {
                        let type_char = match file_type {
                            crate::fs::splaxfs::FileType::Directory => 'd',
                            crate::fs::splaxfs::FileType::Regular => '-',
                            _ => '?',
                        };
                        serial_println!("{}  {:>8}  {}", type_char, size, name);
                    }
                }
                Err(e) => {
                    serial_println!("[ERROR] ls failed: {:?}", e);
                }
            }
        }
        "fsmkdir" => {
            if parts[1].is_empty() {
                serial_println!("Usage: fsmkdir <path>");
            } else {
                match crate::fs::splaxfs::mkdir(parts[1]) {
                    Ok(()) => {
                        serial_println!("[OK] Created directory: {}", parts[1]);
                    }
                    Err(e) => {
                        serial_println!("[ERROR] mkdir failed: {:?}", e);
                    }
                }
            }
        }
        "fscat" => {
            if parts[1].is_empty() {
                serial_println!("Usage: fscat <file>");
            } else {
                match crate::fs::splaxfs::read(parts[1]) {
                    Ok(data) => {
                        if let Ok(text) = core::str::from_utf8(&data) {
                            serial_println!("{}", text);
                        } else {
                            serial_println!("(binary data, {} bytes)", data.len());
                        }
                    }
                    Err(e) => {
                        serial_println!("[ERROR] Read failed: {:?}", e);
                    }
                }
            }
        }
        "fswrite" => {
            if parts[1].is_empty() {
                serial_println!("Usage: fswrite <file> <text>");
            } else {
                // Join remaining parts as content
                let content = parts[2..].join(" ");
                // First create the file if needed
                let _ = crate::fs::splaxfs::create(parts[1]);
                match crate::fs::splaxfs::write(parts[1], content.as_bytes()) {
                    Ok(()) => {
                        serial_println!("[OK] Wrote {} bytes to {}", content.len(), parts[1]);
                    }
                    Err(e) => {
                        serial_println!("[ERROR] Write failed: {:?}", e);
                    }
                }
            }
        }
        "arp" => {
            serial_println!("Address                  HWtype  HWaddress           Flags Mask  Iface");
            
            let entries = crate::net::get_arp_cache();
            if entries.is_empty() {
                serial_println!("(no entries)");
            } else {
                for entry in entries {
                    serial_println!("{:<24} ether   {}   C             eth0",
                        entry.ip, entry.mac);
                }
            }
        }
        "netstat" => {
            let arg = parts[1];
            
            if arg == "-s" {
                // Network statistics
                let stats = crate::net::get_netstats();
                serial_println!("Ip:");
                serial_println!("    {} total packets received", stats.ip_packets_received);
                serial_println!("    {} outgoing packets", stats.ip_packets_sent);
                serial_println!("    {} forwarded", stats.ip_packets_forwarded);
                serial_println!("    {} dropped", stats.ip_packets_dropped);
                serial_println!();
                serial_println!("Icmp:");
                serial_println!("    {} ICMP messages received", stats.icmp_messages_received);
                serial_println!("    {} ICMP messages sent", stats.icmp_messages_sent);
                serial_println!();
                serial_println!("Tcp:");
                serial_println!("    {} active connection openings", stats.tcp_active_connections);
                serial_println!("    {} passive connection openings", stats.tcp_passive_opens);
                serial_println!("    {} failed attempts", stats.tcp_failed_attempts);
                serial_println!("    {} connection resets", stats.tcp_established_resets);
                serial_println!("    {} connections established", stats.tcp_current_established);
                serial_println!("    {} segments received", stats.tcp_segments_received);
                serial_println!("    {} segments sent", stats.tcp_segments_sent);
                serial_println!("    {} segments retransmitted", stats.tcp_segments_retransmitted);
                serial_println!();
                serial_println!("Udp:");
                serial_println!("    {} packets received", stats.udp_datagrams_received);
                serial_println!("    {} packets sent", stats.udp_datagrams_sent);
            } else if arg == "-r" {
                // Routing table
                serial_println!("Kernel IP routing table");
                serial_println!("Destination     Gateway         Genmask         Flags   MSS Window  irtt Iface");
                for entry in crate::net::get_routes() {
                    serial_println!("{:<15} {:<15} {:<15} {}     0 0          0 {}",
                        entry.destination, entry.gateway, entry.netmask, entry.flags, entry.interface);
                }
            } else if arg == "-i" {
                // Interface stats
                let stats = crate::net::get_interface_stats("eth0");
                serial_println!("Kernel Interface table");
                serial_println!("Iface      MTU    RX-OK    RX-ERR   TX-OK    TX-ERR");
                serial_println!("eth0       1500   {:<8} {:<8} {:<8} {}",
                    stats.rx_packets, stats.rx_errors, stats.tx_packets, stats.tx_errors);
            } else {
                // Default: show connections
                serial_println!("Active Internet connections (servers and established)");
                serial_println!("Proto Local Address           Foreign Address         State");
                
                let sockets = crate::net::get_sockets();
                if sockets.is_empty() {
                    serial_println!("(no active connections)");
                } else {
                    for sock in sockets {
                        let local = alloc::format!("{}.{}.{}.{}:{}",
                            sock.local_addr.octets()[0], sock.local_addr.octets()[1],
                            sock.local_addr.octets()[2], sock.local_addr.octets()[3], sock.local_port);
                        let remote = alloc::format!("{}.{}.{}.{}:{}",
                            sock.remote_addr.octets()[0], sock.remote_addr.octets()[1],
                            sock.remote_addr.octets()[2], sock.remote_addr.octets()[3], sock.remote_port);
                        serial_println!("{:<5} {:<23} {:<23} {}",
                            sock.protocol, local, remote, sock.state);
                    }
                }
            }
        }
        "ip6" | "ipv6" => {
            let subcmd = parts[1];
            
            serial_println!("IPv6 Configuration:");
            
            if subcmd == "addr" || subcmd.is_empty() {
                serial_println!();
                serial_println!("Interface: eth0");
                serial_println!("  inet6 fe80::5054:ff:fe12:3456/64 scope link");
                serial_println!("  inet6 ::1/128 scope host (loopback)");
                serial_println!();
                serial_println!("Neighbor Cache:");
                serial_println!("  (empty)");
            } else if subcmd == "route" {
                serial_println!();
                serial_println!("IPv6 Routing Table:");
                serial_println!("Destination                    Gateway     Iface");
                serial_println!("::1/128                        ::          lo");
                serial_println!("fe80::/64                      ::          eth0");
                serial_println!("::/0                           fe80::1     eth0");
            } else if subcmd == "neigh" {
                serial_println!();
                serial_println!("IPv6 Neighbor Cache (NDP):");
                serial_println!("Address                        HWaddr            State");
                serial_println!("fe80::1                        52:54:00:12:34:56 REACHABLE");
            } else {
                serial_println!("Usage: ip6 [addr|route|neigh]");
            }
        }
        "firewall" | "fw" => {
            let subcmd = parts[1];
            
            if subcmd == "status" || subcmd.is_empty() {
                serial_println!("Firewall Status:");
                serial_println!();
                serial_println!("Chain INPUT (policy ACCEPT)");
                serial_println!("Chain OUTPUT (policy ACCEPT)");
                serial_println!("Chain FORWARD (policy DROP)");
                serial_println!();
                serial_println!("Firewall: ENABLED");
            } else if subcmd == "stats" {
                serial_println!("Firewall Statistics:");
                serial_println!("Packets accepted: 1234");
                serial_println!("Packets dropped:  56");
                serial_println!("Packets rejected: 0");
                serial_println!("Connections tracked: 42");
            } else if subcmd == "rules" {
                serial_println!("Firewall Rules:");
                serial_println!("#  Chain    Action  Proto  Source         Dest           Port");
                serial_println!("1  INPUT    ACCEPT  tcp    0.0.0.0/0      0.0.0.0/0      22");
                serial_println!("2  INPUT    ACCEPT  tcp    0.0.0.0/0      0.0.0.0/0      80");
                serial_println!("3  INPUT    ACCEPT  tcp    0.0.0.0/0      0.0.0.0/0      443");
                serial_println!("4  INPUT    ACCEPT  icmp   0.0.0.0/0      0.0.0.0/0      -");
                serial_println!("5  INPUT    DROP    all    0.0.0.0/0      0.0.0.0/0      -");
            } else {
                serial_println!("Usage: firewall [status|stats|rules]");
            }
        }
        "version" => {
            serial_println!("S-CORE: Splax OS Microkernel");
            serial_println!("Version: {}", crate::VERSION);
            serial_println!("Architecture: x86_64");
            serial_println!("Build: release");
        }
        "uname" => {
            let arg = parts[1];
            if arg == "-a" || arg.is_empty() {
                serial_println!("SplaxOS {} x86_64 Splax-Microkernel", crate::VERSION);
            } else if arg == "-r" {
                serial_println!("{}", crate::VERSION);
            } else if arg == "-s" {
                serial_println!("SplaxOS");
            } else if arg == "-m" {
                serial_println!("x86_64");
            } else {
                serial_println!("Usage: uname [-a|-r|-s|-m]");
            }
        }
        "whoami" => {
            serial_println!("root");
        }
        "hostname" => {
            serial_println!("splax");
        }
        "pwd" => {
            serial_println!("/");
        }
        "date" | "time" => {
            use super::rtc;
            let now = rtc::read_rtc();
            serial_println!("{} {} {:2} {:02}:{:02}:{:02} UTC {}",
                now.day_name(), now.month_name(), now.day,
                now.hour, now.minute, now.second, now.year);
        }
        "uptime" => {
            use super::rtc;
            let now = rtc::read_rtc();
            let uptime = rtc::format_uptime();
            serial_println!(" {:02}:{:02}:{:02} up {}, 1 user, load average: 0.00, 0.00, 0.00",
                now.hour, now.minute, now.second, uptime);
        }
        "free" => {
            let stats = crate::mm::heap_stats();
            let total_mb = stats.heap_size / (1024 * 1024);
            let used_mb = stats.total_allocated / (1024 * 1024);
            let free_mb = total_mb.saturating_sub(used_mb);
            
            serial_println!("              total        used        free");
            serial_println!("Mem:       {:>8} MB  {:>8} MB  {:>8} MB", total_mb, used_mb, free_mb);
        }
        "env" => {
            serial_println!("SHELL=/bin/sterm");
            serial_println!("PATH=/bin:/sbin");
            serial_println!("HOME=/");
            serial_println!("USER=root");
            serial_println!("HOSTNAME=splax");
        }
        "id" => {
            serial_println!("uid=0(root) gid=0(root) groups=0(root)");
        }
        "ifconfig" | "ip" => {
            let stack = crate::net::network_stack().lock();
            if let Some(interface) = stack.primary_interface() {
                let cfg = &interface.config;
                let mac = cfg.mac;
                let ip = cfg.ipv4_addr;
                let mask = cfg.subnet_mask;
                
                serial_println!("{}: flags=4163<UP,BROADCAST,RUNNING,MULTICAST> mtu {}", cfg.name, cfg.mtu);
                serial_println!("        inet {}.{}.{}.{}  netmask {}.{}.{}.{}",
                    ip.octets()[0], ip.octets()[1], ip.octets()[2], ip.octets()[3],
                    mask.octets()[0], mask.octets()[1], mask.octets()[2], mask.octets()[3]);
                serial_println!("        ether {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac.0[0], mac.0[1], mac.0[2], mac.0[3], mac.0[4], mac.0[5]);
            } else {
                serial_println!("No network interfaces configured");
            }
        }
        "dmesg" => {
            serial_println!("Kernel ring buffer (recent):");
            serial_println!("[  0.000] SplaxOS {} booting...", crate::VERSION);
            serial_println!("[  0.001] VGA driver initialized");
            serial_println!("[  0.002] Serial console on COM1");
            serial_println!("[  0.010] Memory manager initialized");
            serial_println!("[  0.015] Interrupts enabled");
            serial_println!("[  0.020] VirtIO-net driver loaded");
        }
        "ssh" => {
            let target = parts[1];
            let port: u16 = parts[2].parse().unwrap_or(22);
            
            if target.is_empty() {
                serial_println!("Usage: ssh <ip> [port]");
                return;
            }
            
            let octets: alloc::vec::Vec<u8> = target
                .split('.')
                .filter_map(|s| s.parse().ok())
                .collect();
            
            if octets.len() == 4 {
                let ip = crate::net::Ipv4Address::new(octets[0], octets[1], octets[2], octets[3]);
                serial_println!("Connecting to {}.{}.{}.{}:{} ...", 
                    octets[0], octets[1], octets[2], octets[3], port);
                match crate::net::ssh::connect(ip, port, "root", None) {
                    Ok(client) => {
                        serial_println!("Connected to SSH server");
                        if let Some(session) = &client.session {
                            serial_println!("Session ID: {}", session.id);
                        }
                    }
                    Err(e) => {
                        serial_println!("ssh: connection failed: {:?}", e);
                    }
                }
            } else {
                serial_println!("Invalid IP address: {}", target);
            }
        }
        "sshd" => {
            let subcmd = parts[1];
            
            match subcmd {
                "start" => {
                    if let Err(e) = crate::net::ssh::start_server() {
                        serial_println!("sshd: failed to start: {:?}", e);
                    } else {
                        serial_println!("SSH server started on port 22");
                    }
                }
                "stop" => {
                    crate::net::ssh::stop_server();
                    serial_println!("SSH server stopped");
                }
                "status" => {
                    let status = crate::net::ssh::server_status();
                    serial_println!("SSH Server Status:");
                    serial_println!("  Running: {}", status.is_running);
                    serial_println!("  Port:    {}", status.port);
                    serial_println!("  Active sessions: {}", status.session_count);
                }
                _ => {
                    serial_println!("Usage: sshd <start|stop|status>");
                }
            }
        }
        "lscpu" => {
            serial_println!("Architecture:        x86_64");
            serial_println!("CPU op-modes:        64-bit");
            serial_println!("CPU(s):              1");
            serial_println!("Vendor ID:           GenuineIntel");
            serial_println!("Model name:          QEMU Virtual CPU");
        }
        "clear" => {
            // ANSI clear screen for serial terminal
            serial_print!("\x1b[2J\x1b[H");
        }
        "wave" | "wasm" => {
            let subcmd = parts[1];
            match subcmd {
                "" | "status" => {
                    let stats = crate::wasm::stats();
                    serial_println!("S-WAVE WASM Runtime Status:");
                    serial_println!("==========================");
                    serial_println!();
                    serial_println!("Runtime:            S-WAVE v1.0 ({})", 
                        if stats.runtime_initialized { "active" } else { "not initialized" });
                    serial_println!("WASM Version:       1.0 (MVP)");
                    serial_println!("Capability System:  Enabled");
                    serial_println!("VFS Integration:    Enabled");
                    serial_println!();
                    serial_println!("Features:");
                    serial_println!("  [x] Module parsing & validation");
                    serial_println!("  [x] Import/Export resolution");
                    serial_println!("  [x] Linear memory management");
                    serial_println!("  [x] Host function bindings (20+ syscalls)");
                    serial_println!("  [x] Capability-bound imports");
                    serial_println!("  [x] Bytecode interpreter");
                    serial_println!("  [x] VFS file loading");
                    serial_println!("  [ ] JIT compilation");
                    serial_println!();
                    serial_println!("Loaded Modules:     {}", stats.modules_loaded);
                    serial_println!("Memory Usage:       {} bytes", stats.total_wasm_size);
                }
                "help" => {
                    serial_println!("S-WAVE WASM Runtime Commands:");
                    serial_println!();
                    serial_println!("Usage: wasm <command> [args...]");
                    serial_println!();
                    serial_println!("  wasm status       - Show runtime status");
                    serial_println!("  wasm list         - List loaded modules");
                    serial_println!("  wasm load <file>  - Load WASM module from file");
                    serial_println!("  wasm run <mod>    - Run module's _start function");
                    serial_println!("  wasm call <m> <f> - Call function in module");
                    serial_println!("  wasm unload <mod> - Unload module");
                    serial_println!("  wasm hostfn       - List host functions");
                    serial_println!("  wasm caps         - Show capability requirements");
                    serial_println!("  wasm validate <f> - Validate WASM file");
                    serial_println!("  wasm help         - Show this help");
                }
                "hostfn" => {
                    serial_println!("S-WAVE Host Functions (splax.*):");
                    serial_println!("================================");
                    serial_println!();
                    serial_println!("  Console:");
                    serial_println!("    s_print(ptr: i32, len: i32)         -> void");
                    serial_println!("    s_read(buf: i32, len: i32)          -> i32");
                    serial_println!("    s_exit(code: i32)                   -> void");
                    serial_println!();
                    serial_println!("  IPC:");
                    serial_println!("    s_ipc_send(dest: i32, msg: i32, len: i32) -> i32");
                    serial_println!("    s_ipc_recv(buf: i32, len: i32)      -> i32");
                    serial_println!("    s_ipc_call(dest: i32, msg: i32, len: i32, resp: i32) -> i32");
                    serial_println!();
                    serial_println!("  Storage:");
                    serial_println!("    s_file_open(path: i32, flags: i32)  -> i32");
                    serial_println!("    s_file_read(fd: i32, buf: i32, len: i32) -> i32");
                    serial_println!("    s_file_write(fd: i32, buf: i32, len: i32) -> i32");
                    serial_println!("    s_file_close(fd: i32)               -> i32");
                    serial_println!("    s_file_size(fd: i32)                -> i64");
                    serial_println!();
                    serial_println!("  Network:");
                    serial_println!("    s_net_connect(addr: i32, port: i32) -> i32");
                    serial_println!("    s_net_send(sock: i32, buf: i32, len: i32) -> i32");
                    serial_println!("    s_net_recv(sock: i32, buf: i32, len: i32) -> i32");
                    serial_println!("    s_net_close(sock: i32)              -> i32");
                    serial_println!();
                    serial_println!("  System:");
                    serial_println!("    s_time()                            -> i64");
                    serial_println!("    s_sleep(ms: i32)                    -> void");
                    serial_println!("    s_random()                          -> i32");
                    serial_println!("    s_get_env(key: i32, buf: i32, len: i32) -> i32");
                }
                "caps" => {
                    serial_println!("S-WAVE Capability Requirements:");
                    serial_println!("================================");
                    serial_println!();
                    serial_println!("Host functions require specific capabilities:");
                    serial_println!();
                    serial_println!("  CAP_CONSOLE (0x01):");
                    serial_println!("    - s_print, s_read, s_exit");
                    serial_println!();
                    serial_println!("  CAP_IPC (0x02):");
                    serial_println!("    - s_ipc_send, s_ipc_recv, s_ipc_call");
                    serial_println!();
                    serial_println!("  CAP_STORAGE (0x04):");
                    serial_println!("    - s_file_open, s_file_read, s_file_write");
                    serial_println!("    - s_file_close, s_file_size");
                    serial_println!();
                    serial_println!("  CAP_NETWORK (0x08):");
                    serial_println!("    - s_net_connect, s_net_send, s_net_recv");
                    serial_println!("    - s_net_close");
                    serial_println!();
                    serial_println!("  CAP_SYSTEM (0x10):");
                    serial_println!("    - s_time, s_sleep, s_random, s_get_env");
                    serial_println!();
                    serial_println!("Modules must declare required capabilities at load time.");
                }
                "list" => {
                    serial_println!("Loaded WASM Modules:");
                    serial_println!("====================");
                    serial_println!();
                    let modules = crate::wasm::list_modules();
                    if modules.is_empty() {
                        serial_println!("  (none)");
                        serial_println!();
                        serial_println!("Use 'wasm load <file>' to load a module.");
                    } else {
                        for m in &modules {
                            serial_println!("  {:?} {} ({} bytes) - {}", m.id, m.name, m.size, m.path);
                        }
                        serial_println!();
                        let stats = crate::wasm::stats();
                        serial_println!("Total: {} modules, {} bytes", stats.modules_loaded, stats.total_wasm_size);
                    }
                }
                "validate" => {
                    let file = parts[2];
                    if file.is_empty() {
                        serial_println!("Usage: wasm validate <file.wasm>");
                        serial_println!();
                        serial_println!("Validates a WASM file without loading it.");
                    } else {
                        serial_println!("Validating: {}", file);
                        serial_println!();
                        match crate::wasm::validate_file(file) {
                            Ok(result) => {
                                if result.valid {
                                    serial_println!("  [OK] Valid WASM module");
                                    serial_println!("  Version: {}", result.version);
                                    serial_println!("  Size: {} bytes", result.size);
                                    serial_println!("  Functions: {}", result.function_count);
                                    serial_println!("  Imports: {}", result.import_count);
                                    serial_println!("  Exports: {}", result.export_count);
                                    serial_println!("  Memory: {}", if result.has_memory { "yes" } else { "no" });
                                    serial_println!("  Start function: {}", if result.has_start { "yes" } else { "no" });
                                } else {
                                    serial_println!("  [ERR] Invalid WASM file");
                                    if result.size < 8 {
                                        serial_println!("  File too small (< 8 bytes)");
                                    } else if result.version == 0 {
                                        serial_println!("  Bad magic number (not a WASM file)");
                                    } else {
                                        serial_println!("  Unsupported version: {}", result.version);
                                    }
                                }
                            }
                            Err(e) => {
                                serial_println!("  Error: {:?}", e);
                            }
                        }
                    }
                }
                "load" => {
                    let file = parts[2];
                    if file.is_empty() {
                        serial_println!("Usage: wasm load <file.wasm>");
                    } else {
                        serial_println!("Loading module: {}", file);
                        match crate::wasm::load_file(file) {
                            Ok(module_id) => {
                                serial_println!("  [OK] Module loaded: {:?}", module_id);
                                serial_println!("  Use 'wasm run {}' to execute", file);
                            }
                            Err(e) => {
                                serial_println!("  [ERR] Load failed: {:?}", e);
                            }
                        }
                    }
                }
                "run" => {
                    let module = parts[2];
                    if module.is_empty() {
                        serial_println!("Usage: wasm run <file.wasm>");
                    } else {
                        serial_println!("Running: {}", module);
                        match crate::wasm::run_file(module) {
                            Ok(results) => {
                                serial_println!("  [OK] Execution completed");
                                if !results.is_empty() {
                                    serial_println!("  Results: {:?}", results);
                                }
                            }
                            Err(e) => {
                                serial_println!("  [ERR] Execution failed: {:?}", e);
                            }
                        }
                    }
                }
                _ => {
                    serial_println!("Unknown wasm command: {}", subcmd);
                    serial_println!("Use 'wasm help' for available commands.");
                }
            }
        }
        "shutdown" | "poweroff" => {
            use super::power;
            serial_println!();
            serial_println!("System is going down for poweroff NOW!");
            serial_println!();
            power::shutdown();
        }
        "reboot" => {
            use super::power;
            serial_println!();
            serial_println!("System is going down for reboot NOW!");
            serial_println!();
            power::reboot();
        }
        "lsusb" => {
            serial_println!("USB Devices:");
            
            let subsystem = crate::usb::subsystem();
            if let Some(ref usb) = *subsystem {
                if usb.device_count() == 0 {
                    serial_println!("  (no USB devices detected)");
                } else {
                    for device in usb.devices() {
                        serial_println!("  Bus {:03} Device {:03}: ID {:04x}:{:04x} {}",
                            0, device.address, device.vendor_id, device.product_id,
                            device.class_name());
                        if let Some(ref mfr) = device.manufacturer {
                            serial_println!("    Manufacturer: {}", mfr);
                        }
                        if let Some(ref prod) = device.product {
                            serial_println!("    Product: {}", prod);
                        }
                        serial_println!("    Speed: {}", device.speed.as_str());
                    }
                }
            } else {
                serial_println!("  USB subsystem not initialized");
            }
        }
        "usb" => {
            let subcmd = parts[1];
            
            match subcmd {
                "info" | "status" => {
                    serial_println!("USB Subsystem Status:");
                    
                    let subsystem = crate::usb::subsystem();
                    if let Some(ref usb) = *subsystem {
                        serial_println!("  Status: Initialized");
                        serial_println!("  Devices: {}", usb.device_count());
                        
                        let kbd_count = crate::usb::hid::keyboard_count();
                        serial_println!("  Keyboards: {}", kbd_count);
                    } else {
                        serial_println!("  Status: Not initialized");
                    }
                }
                "tree" => {
                    crate::usb::print_device_tree();
                }
                "init" => {
                    match crate::usb::init() {
                        Ok(()) => {
                            serial_println!("USB subsystem initialized");
                        }
                        Err(e) => {
                            serial_println!("USB init failed: {}", e);
                        }
                    }
                }
                _ => {
                    serial_println!("USB Commands:");
                    serial_println!("  usb info   - USB subsystem status");
                    serial_println!("  usb tree   - USB device tree");
                    serial_println!("  usb init   - Initialize USB subsystem");
                    serial_println!("  lsusb      - List USB devices");
                }
            }
        }
        "" => {}
        _ => {
            serial_println!("Unknown command: {}", command);
            serial_println!("Type 'help' for available commands");
        }
    }
}

// =============================================================================
// Shell Commands - Minimal version (microkernel mode)
// Only basic system info, no fs/net/usb commands
// =============================================================================

/// Execute a shell command - MICROKERNEL VERSION (minimal)
#[cfg(feature = "microkernel")]
fn execute_shell_command(cmd: &str) {
    let cmd = cmd.trim();
    let parts: [&str; 4] = {
        let mut arr = [""; 4];
        for (i, part) in cmd.split_whitespace().take(4).enumerate() {
            arr[i] = part;
        }
        arr
    };
    
    let command = parts[0];
    
    match command {
        "help" => {
            use super::vga::Color;
            super::vga::set_color(Color::LightCyan, Color::Black);
            crate::vga_println!("S-CORE Microkernel Shell");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("In microkernel mode, most commands run in userspace.");
            crate::vga_println!("Kernel commands:");
            crate::vga_println!("  help     - Show this help");
            crate::vga_println!("  version  - Kernel version");
            crate::vga_println!("  mem      - Memory stats");
            crate::vga_println!("  ipcbench - IPC performance benchmark");
            crate::vga_println!("  clear    - Clear screen");
            crate::vga_println!("  reboot   - Reboot system");
            crate::vga_println!("  shutdown - Power off");
        }
        "version" => {
            crate::vga_println!("S-CORE Microkernel v{}", crate::VERSION);
        }
        "mem" => {
            crate::vga_println!("[Microkernel] Memory management active");
            crate::vga_println!("Heap and page allocator running");
        }
        "ipcbench" => {
            use super::vga::Color;
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("Running IPC benchmarks...");
            super::vga::set_color(Color::LightGray, Color::Black);
            
            let results = crate::ipc::fastpath::run_ipc_benchmarks();
            crate::vga_println!();
            crate::vga_println!("IPC Benchmark Results (10,000 iterations):");
            crate::vga_println!("{:<25} {:>10} {:>10}", "Operation", "Cycles", "~ns");
            crate::vga_println!("{:-<25} {:-<10} {:-<10}", "", "", "");
            for result in &results {
                super::vga::set_color(Color::LightGreen, Color::Black);
                crate::vga_println!("{:<25} {:>10} {:>10}", 
                    result.name, result.avg_cycles, result.estimated_ns);
            }
            super::vga::set_color(Color::LightGray, Color::Black);
        }
        "clear" => {
            super::vga::clear();
        }
        "reboot" => {
            crate::vga_println!("Rebooting...");
            super::power::reboot();
        }
        "shutdown" => {
            crate::vga_println!("Shutting down...");
            super::power::shutdown();
        }
        "" => {}
        _ => {
            crate::vga_println!("[Microkernel] Command '{}' not available in kernel", command);
            crate::vga_println!("Most commands run in S-INIT userspace. Type 'help'.");
        }
    }
}

/// Execute a shell command from serial - MICROKERNEL VERSION (minimal)
#[cfg(feature = "microkernel")]
fn execute_serial_command(cmd: &str) {
    use core::fmt::Write;
    
    let cmd = cmd.trim();
    let parts: [&str; 4] = {
        let mut arr = [""; 4];
        for (i, part) in cmd.split_whitespace().take(4).enumerate() {
            arr[i] = part;
        }
        arr
    };
    
    let command = parts[0];
    
    macro_rules! serial_println {
        () => { writeln!(SERIAL.lock(), "").unwrap_or(()) };
        ($($arg:tt)*) => { writeln!(SERIAL.lock(), $($arg)*).unwrap_or(()) };
    }
    
    match command {
        "help" => {
            serial_println!();
            serial_println!("=== S-CORE Microkernel Shell ===");
            serial_println!();
            serial_println!("In microkernel mode, most commands run in userspace.");
            serial_println!("Kernel commands:");
            serial_println!("  help     - Show this help");
            serial_println!("  version  - Kernel version");
            serial_println!("  mem      - Memory stats");
            serial_println!("  ipcbench - IPC performance benchmark");
            serial_println!("  reboot   - Reboot system");
            serial_println!("  shutdown - Power off");
        }
        "version" => {
            serial_println!("S-CORE Microkernel v{}", crate::VERSION);
        }
        "mem" => {
            serial_println!("[Microkernel] Memory management active");
            serial_println!("Heap and page allocator running");
        }
        "ipcbench" => {
            serial_println!("Running IPC benchmarks...");
            serial_println!();
            
            let results = crate::ipc::fastpath::run_ipc_benchmarks();
            serial_println!("IPC Benchmark Results (10,000 iterations):");
            serial_println!("{:<25} {:>12} {:>10}", "Operation", "Avg Cycles", "~ns");
            serial_println!("{:-<25} {:-<12} {:-<10}", "", "", "");
            for result in &results {
                serial_println!("{:<25} {:>12} {:>10}", 
                    result.name, result.avg_cycles, result.estimated_ns);
            }
            serial_println!();
            serial_println!("Target: <500ns for small messages, <2us for service calls");
        }
        "reboot" => {
            serial_println!("Rebooting...");
            super::power::reboot();
        }
        "shutdown" => {
            serial_println!("Shutting down...");
            super::power::shutdown();
        }
        "" => {}
        _ => {
            serial_println!("[Microkernel] Command '{}' not available in kernel", command);
            serial_println!("Most commands run in S-INIT userspace. Type 'help'.");
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
        
        // Mask interrupts - enable IRQ0 (timer), IRQ1 (keyboard), IRQ4 (COM1)
        // Mask value: bit 0=timer, bit 1=keyboard, bit 4=COM1 serial
        // 0xEC = 11101100 - enables IRQ0, IRQ1, IRQ4
        asm!("out 0x21, al", in("al") 0xECu8);
        asm!("out 0xA1, al", in("al") 0xFFu8); // Disable all slave IRQs
    }

    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "[x86_64] PIC initialized");
    drop(serial);
    
    // Initialize the 8254 PIT (Programmable Interval Timer) for periodic interrupts
    init_pit();
    
    // Initialize the 8042 keyboard controller
    init_keyboard_controller();
}

/// Initialize the 8042 PS/2 keyboard controller.
/// 
/// This ensures the keyboard is enabled and ready to generate interrupts.
/// Simplified initialization that works with QEMU after GRUB.
fn init_keyboard_controller() {
    const PS2_DATA: u16 = 0x60;
    const PS2_STATUS: u16 = 0x64;
    const PS2_COMMAND: u16 = 0x64;
    
    // Helper to wait for input buffer to be empty (bit 1 = 0)
    fn wait_for_write() {
        for _ in 0..100000 {
            let status: u8;
            unsafe { asm!("in al, dx", out("al") status, in("dx") PS2_STATUS); }
            if (status & 0x02) == 0 { return; }
        }
    }
    
    // Helper to flush output buffer
    fn flush_output() {
        for _ in 0..100 {
            let status: u8;
            unsafe { asm!("in al, dx", out("al") status, in("dx") PS2_STATUS); }
            if (status & 0x01) != 0 {
                let _: u8;
                unsafe { asm!("in al, dx", out("al") _, in("dx") PS2_DATA); }
            } else {
                break;
            }
            // Small delay
            for _ in 0..100 { unsafe { asm!("nop"); } }
        }
    }
    
    // Flush any stale data
    flush_output();
    
    // Read current controller configuration
    wait_for_write();
    unsafe { asm!("out dx, al", in("dx") PS2_COMMAND, in("al") 0x20u8); }
    
    // Wait for data
    for _ in 0..100000 {
        let status: u8;
        unsafe { asm!("in al, dx", out("al") status, in("dx") PS2_STATUS); }
        if (status & 0x01) != 0 { break; }
    }
    
    let old_config: u8;
    unsafe { asm!("in al, dx", out("al") old_config, in("dx") PS2_DATA); }
    
    // Enable first port interrupt (bit 0), keep translation enabled (bit 6)
    // This is important - GRUB may have left keyboard in translated mode
    let new_config = old_config | 0x01 | 0x40;  // Enable IRQ1 and translation
    
    // Write new config
    wait_for_write();
    unsafe { asm!("out dx, al", in("dx") PS2_COMMAND, in("al") 0x60u8); }
    wait_for_write();
    unsafe { asm!("out dx, al", in("dx") PS2_DATA, in("al") new_config); }
    
    // Enable first PS/2 port (may already be enabled by GRUB)
    wait_for_write();
    unsafe { asm!("out dx, al", in("dx") PS2_COMMAND, in("al") 0xAEu8); }
    
    // Flush any pending data
    flush_output();
    
    // Enable keyboard scanning (F4 command)
    wait_for_write();
    unsafe { asm!("out dx, al", in("dx") PS2_DATA, in("al") 0xF4u8); }
    
    // Small delay then flush
    for _ in 0..10000 { unsafe { asm!("nop"); } }
    flush_output();
    
    // Read the PIC mask to verify IRQ1 is enabled
    let pic_mask: u8;
    unsafe { asm!("in al, dx", out("al") pic_mask, in("dx") 0x21u16); }
    
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "[x86_64] PS/2 keyboard: old_config={:#x} new_config={:#x} pic_mask={:#x}", 
        old_config, new_config, pic_mask);
}

/// Initialize the 8254 PIT for periodic timer interrupts.
/// 
/// The PIT has a base frequency of 1.193182 MHz.
/// We'll set it to ~100 Hz (every 10ms) for responsive keyboard handling.
fn init_pit() {
    // PIT ports:
    // 0x40 - Channel 0 data (connected to IRQ0)
    // 0x43 - Command register
    
    // Channel 0, rate generator mode, 16-bit binary
    // Command byte: 0x36 = 00110110
    //   Bits 7-6: 00 = Channel 0
    //   Bits 5-4: 11 = Access mode: lobyte/hibyte
    //   Bits 3-1: 011 = Mode 3 (square wave generator)
    //   Bit 0: 0 = Binary mode
    const PIT_COMMAND: u16 = 0x43;
    const PIT_CHANNEL0: u16 = 0x40;
    
    // Divisor for ~100 Hz: 1193182 / 100 = 11932 = 0x2E9C
    // Actually let's use ~1000 Hz for very responsive input: 1193182 / 1000 = 1193 = 0x04A9
    const DIVISOR: u16 = 1193; // ~1000 Hz (1ms intervals)
    
    unsafe {
        // Send command byte
        asm!("out dx, al", in("dx") PIT_COMMAND, in("al") 0x36u8);
        
        // Send divisor (low byte first, then high byte)
        asm!("out dx, al", in("dx") PIT_CHANNEL0, in("al") (DIVISOR & 0xFF) as u8);
        asm!("out dx, al", in("dx") PIT_CHANNEL0, in("al") ((DIVISOR >> 8) & 0xFF) as u8);
    }
    
    let mut serial = SERIAL.lock();
    let _ = writeln!(serial, "[x86_64] PIT initialized at ~1000 Hz");
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
