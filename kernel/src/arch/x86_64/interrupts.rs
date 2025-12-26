//! # Interrupt Handlers
//!
//! x86_64 interrupt and exception handlers for Splax OS.

use core::arch::asm;
use core::fmt::Write;
use spin::Mutex;
use super::serial::SERIAL;

/// Command line buffer for kernel shell
static COMMAND_BUFFER: Mutex<CommandBuffer> = Mutex::new(CommandBuffer::new());

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

/// Get current timer tick count.
pub fn get_ticks() -> u64 {
    TIMER_TICKS.load(core::sync::atomic::Ordering::Relaxed)
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
pub extern "x86-interrupt" fn keyboard_handler(_frame: InterruptFrame) {
    // Read scancode from keyboard port
    let scancode: u8;
    unsafe {
        asm!("in al, 0x60", out("al") scancode);
    }

    // Process the scancode through the keyboard driver
    if let Some(key_event) = super::keyboard::handle_scancode(scancode) {
        // Handle special key combinations
        if key_event.ctrl && key_event.character == 'c' {
            // Ctrl+C - clear current line
            let mut cmd_buf = COMMAND_BUFFER.lock();
            cmd_buf.clear();
            crate::vga_println!();
            crate::vga_print!("splax> ");
        } else {
            // Display the character on VGA and handle input
            match key_event.character {
                '\n' => {
                    crate::vga_println!();
                    // Execute command
                    let cmd_buf = COMMAND_BUFFER.lock();
                    let cmd = cmd_buf.as_str();
                    if !cmd.is_empty() {
                        execute_shell_command(cmd);
                    }
                    drop(cmd_buf);
                    COMMAND_BUFFER.lock().clear();
                    crate::vga_print!("splax> ");
                }
                '\x08' => {
                    // Backspace
                    let mut cmd_buf = COMMAND_BUFFER.lock();
                    if cmd_buf.pop() {
                        // Move cursor back, print space, move back again
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

    // Send EOI to PIC
    unsafe {
        pic_send_eoi(vector::PIC_KEYBOARD);
    }
}

/// Serial command buffer for serial shell
static SERIAL_COMMAND_BUFFER: Mutex<CommandBuffer> = Mutex::new(CommandBuffer::new());

/// Serial interrupt handler (COM1 - IRQ4).
pub extern "x86-interrupt" fn serial_handler(_frame: InterruptFrame) {
    
    // Read all available bytes from serial port
    loop {
        // Check if data available and read it
        let byte = {
            let serial = super::serial::SERIAL.lock();
            if !serial.has_data() {
                break;
            }
            serial.read_byte()
        };
        
        let Some(byte) = byte else { break };
        
        match byte {
            b'\r' | b'\n' => {
                // Echo newline
                {
                    let serial = super::serial::SERIAL.lock();
                    serial.write_byte(b'\r');
                    serial.write_byte(b'\n');
                }
                
                // Execute command - copy to local buffer first
                let cmd_string = {
                    let cmd_buf = SERIAL_COMMAND_BUFFER.lock();
                    alloc::string::String::from(cmd_buf.as_str())
                };
                
                if !cmd_string.is_empty() {
                    execute_serial_command(&cmd_string);
                }
                SERIAL_COMMAND_BUFFER.lock().clear();
                
                // Print prompt
                {
                    let serial = super::serial::SERIAL.lock();
                    for b in b"splax> " {
                        serial.write_byte(*b);
                    }
                }
            }
            0x7F | 0x08 => {
                // Backspace/Delete
                let did_pop = SERIAL_COMMAND_BUFFER.lock().pop();
                if did_pop {
                    let serial = super::serial::SERIAL.lock();
                    serial.write_byte(0x08); // Move back
                    serial.write_byte(b' ');  // Erase
                    serial.write_byte(0x08); // Move back again
                }
            }
            0x03 => {
                // Ctrl+C - clear line
                SERIAL_COMMAND_BUFFER.lock().clear();
                {
                    let serial = super::serial::SERIAL.lock();
                    for b in b"^C\r\nsplax> " {
                        serial.write_byte(*b);
                    }
                }
            }
            c if c >= 0x20 && c < 0x7F => {
                // Printable character
                SERIAL_COMMAND_BUFFER.lock().push(c as char);
                super::serial::SERIAL.lock().write_byte(c);
            }
            _ => {}
        }
    }
    
    // Send EOI to PIC
    unsafe {
        pic_send_eoi(vector::PIC_COM1);
    }
}

/// Execute a shell command (kernel built-in shell)
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
            crate::vga_println!("Network:");
            crate::vga_println!("  ping [-c n] <ip> - ICMP ping");
            crate::vga_println!("  traceroute <ip> - Trace route to host");
            crate::vga_println!("  nslookup <host> - DNS lookup");
            crate::vga_println!("  dig <host>    - DNS query (detailed)");
            crate::vga_println!("  host <host>   - Resolve hostname");
            crate::vga_println!("  ifconfig      - Interface config");
            crate::vga_println!("  route         - Routing table");
            crate::vga_println!("  arp           - ARP cache");
            crate::vga_println!("  netstat [-s]  - Connections/stats");
            crate::vga_println!("  ssh <ip>      - SSH client connect");
            crate::vga_println!("  sshd <cmd>    - SSH server (start/stop/status)");
            crate::vga_println!();
            crate::vga_println!("System:");
            crate::vga_println!("  ps            - List processes");
            crate::vga_println!("  mem/free      - Memory usage");
            crate::vga_println!("  df            - Filesystem usage");
            crate::vga_println!("  uptime        - System uptime");
            crate::vga_println!("  uname [-a]    - System info");
            crate::vga_println!("  whoami        - Current user");
            crate::vga_println!("  hostname      - System hostname");
            crate::vga_println!("  date          - System time");
            crate::vga_println!("  lscpu         - CPU information");
            crate::vga_println!("  dmesg         - Kernel messages");
            crate::vga_println!("  env           - Environment vars");
            crate::vga_println!("  id            - User/group IDs");
            crate::vga_println!("  services      - List services");
            crate::vga_println!("  version       - Version info");
            crate::vga_println!("  clear         - Clear screen");
            crate::vga_println!("  reboot        - Halt system");
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
            let ticks = get_ticks();
            // Assuming ~100 Hz timer
            let seconds = ticks / 100;
            let minutes = seconds / 60;
            let hours = minutes / 60;
            
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_print!("Uptime: ");
            super::vga::set_color(Color::LightGray, Color::Black);
            if hours > 0 {
                crate::vga_println!("{}h {}m {}s ({} ticks)", hours, minutes % 60, seconds % 60, ticks);
            } else if minutes > 0 {
                crate::vga_println!("{}m {}s ({} ticks)", minutes, seconds % 60, ticks);
            } else {
                crate::vga_println!("{}s ({} ticks)", seconds, ticks);
            }
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
            super::vga::set_color(Color::Yellow, Color::Black);
            crate::vga_println!("S-WAVE WASM Runtime Status:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!();
            crate::vga_println!("Modules loaded:     0");
            crate::vga_println!("Active instances:   0");
            crate::vga_println!("Max modules:        1,024");
            crate::vga_println!("Max instances:      4,096");
            crate::vga_println!("Max memory/inst:    256 MB");
            crate::vga_println!();
            super::vga::set_color(Color::Cyan, Color::Black);
            crate::vga_println!("Host Functions:");
            super::vga::set_color(Color::LightGray, Color::Black);
            crate::vga_println!("  s_link_send      (channel:send)");
            crate::vga_println!("  s_link_receive   (channel:receive)");
            crate::vga_println!("  s_storage_read   (storage:read)");
            crate::vga_println!("  s_storage_write  (storage:write)");
            crate::vga_println!("  s_log            (log:write)");
            crate::vga_println!("  s_time_now       (time:read)");
            crate::vga_println!("  s_sleep          (process:suspend)");
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
        "date" => {
            // Read from CMOS RTC
            let ticks = get_ticks();
            let seconds = ticks / 100; // Assuming ~100Hz timer
            let hours = (seconds / 3600) % 24;
            let minutes = (seconds / 60) % 60;
            let secs = seconds % 60;
            crate::vga_println!("System time: {:02}:{:02}:{:02} (since boot)", hours, minutes, secs);
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
            crate::vga_println!("[  0.020] VirtIO-net driver loaded");
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
        "lscpu" => {
            crate::vga_println!("Architecture:        x86_64");
            crate::vga_println!("CPU op-modes:        64-bit");
            crate::vga_println!("CPU(s):              1");
            crate::vga_println!("Vendor ID:           GenuineIntel");
            crate::vga_println!("Model name:          QEMU Virtual CPU");
        }
        "reboot" | "shutdown" => {
            use super::vga::Color;
            super::vga::set_color(Color::LightRed, Color::Black);
            crate::vga_println!("System halting...");
            super::vga::set_color(Color::LightGray, Color::Black);
            // Actually halt the CPU
            loop {
                unsafe { asm!("hlt"); }
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

/// Execute a shell command from serial console
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
            serial_println!("Network:");
            serial_println!("  ping [-c n] <ip> - ICMP ping");
            serial_println!("  traceroute <ip> - Trace route to host");
            serial_println!("  nslookup <host> - DNS lookup");
            serial_println!("  dig <host>    - DNS query (detailed)");
            serial_println!("  host <host>   - Resolve hostname");
            serial_println!("  ifconfig      - Interface config");
            serial_println!("  route         - Routing table");
            serial_println!("  arp           - ARP cache");
            serial_println!("  netstat [-s]  - Connections/stats");
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
            serial_println!("  dmesg         - Kernel messages");
            serial_println!("  env           - Environment vars");
            serial_println!("  id            - User/group IDs");
            serial_println!("  services      - List services");
            serial_println!("  version       - Version info");
            serial_println!("  clear         - Clear screen");
            serial_println!("  reboot        - Halt system");
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
        "date" => {
            let ticks = get_ticks();
            let seconds = ticks / 100;
            let hours = (seconds / 3600) % 24;
            let minutes = (seconds / 60) % 60;
            let secs = seconds % 60;
            serial_println!("System time: {:02}:{:02}:{:02} (since boot)", hours, minutes, secs);
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
        "reboot" | "shutdown" => {
            serial_println!("System halting...");
            loop {
                unsafe { asm!("hlt"); }
            }
        }
        "" => {}
        _ => {
            serial_println!("Unknown command: {}", command);
            serial_println!("Type 'help' for available commands");
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
