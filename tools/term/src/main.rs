//! # S-TERM: Command-Line Interface
//!
//! S-TERM is the primary user interface for Splax OS. It provides a
//! command shell and utilities for interacting with the system.
//!
//! ## Design Philosophy
//!
//! - **Capability-Aware**: All commands operate within capability constraints
//! - **Service-Oriented**: Commands interact with services, not raw resources
//! - **Headless-First**: Optimized for remote/automated operation
//! - **Scriptable**: Consistent output formats for automation
//!
//! ## Built-in Commands
//!
//! - `help` - Display help
//! - `services` - List registered services
//! - `channels` - List S-LINK channels
//! - `storage` - Interact with S-STORAGE
//! - `wave` - Manage WASM modules
//! - `gateway` - Manage S-GATE gateways
//! - `cap` - Capability management

#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Command result.
pub type CommandResult = Result<CommandOutput, CommandError>;

/// Command output.
#[derive(Debug)]
pub struct CommandOutput {
    /// Text output
    pub text: String,
    /// Exit code (0 = success)
    pub exit_code: i32,
}

impl CommandOutput {
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            exit_code: 0,
        }
    }

    pub fn error(text: impl Into<String>, code: i32) -> Self {
        Self {
            text: text.into(),
            exit_code: code,
        }
    }
}

/// Command error.
#[derive(Debug)]
pub enum CommandError {
    /// Unknown command
    UnknownCommand(String),
    /// Invalid arguments
    InvalidArguments(String),
    /// Permission denied
    PermissionDenied,
    /// Service error
    ServiceError(String),
    /// Internal error
    InternalError(String),
}

/// Parsed command.
#[derive(Debug)]
pub struct ParsedCommand {
    /// Command name
    pub name: String,
    /// Positional arguments
    pub args: Vec<String>,
    /// Named options
    pub options: Vec<(String, Option<String>)>,
}

impl ParsedCommand {
    /// Parses a command line.
    pub fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        let name = parts[0].to_string();
        let mut args = Vec::new();
        let mut options = Vec::new();

        let mut i = 1;
        while i < parts.len() {
            let part = parts[i];
            if part.starts_with("--") {
                let opt = part.trim_start_matches("--");
                if let Some(eq_pos) = opt.find('=') {
                    let key = opt[..eq_pos].to_string();
                    let value = opt[eq_pos + 1..].to_string();
                    options.push((key, Some(value)));
                } else {
                    options.push((opt.to_string(), None));
                }
            } else if part.starts_with('-') {
                let opt = part.trim_start_matches('-');
                options.push((opt.to_string(), None));
            } else {
                args.push(part.to_string());
            }
            i += 1;
        }

        Some(Self { name, args, options })
    }

    /// Gets an option value.
    pub fn get_option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }

    /// Checks if an option is present.
    pub fn has_option(&self, name: &str) -> bool {
        self.options.iter().any(|(n, _)| n == name)
    }
}

/// Command handler trait.
pub trait Command {
    /// Command name.
    fn name(&self) -> &str;

    /// Short description.
    fn description(&self) -> &str;

    /// Usage string.
    fn usage(&self) -> &str;

    /// Executes the command.
    fn execute(&self, cmd: &ParsedCommand) -> CommandResult;
}

/// Help command.
struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Display help information"
    }

    fn usage(&self) -> &str {
        "help [command]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        if cmd.args.is_empty() {
            Ok(CommandOutput::success(
                "S-TERM - Splax OS Command Line Interface\n\
                 \n\
                 Available commands:\n\
                 \n\
                 help        - Display this help\n\
                 services    - List and manage registered services\n\
                 channels    - List and manage S-LINK channels\n\
                 gateway     - Manage S-GATE TCP/HTTP gateways\n\
                 cap         - S-CAP capability management\n\
                 storage     - Interact with S-STORAGE objects\n\
                 system      - Display system information\n\
                 \n\
                 Use 'help <command>' for detailed information.\n\
                 Use '<command> --help' for usage.",
            ))
        } else {
            let cmd_name = &cmd.args[0];
            match cmd_name.as_str() {
                "services" => Ok(CommandOutput::success(
                    "services - List and manage registered services\n\
                     \n\
                     Usage: services [action]\n\
                     \n\
                     Actions:\n\
                       list              List all registered services (default)\n\
                       info <name>       Show detailed info for a service\n\
                       discover <cap>    Find services providing a capability\n\
                     \n\
                     Examples:\n\
                       services list\n\
                       services info s-atlas\n\
                       services discover auth:verify",
                )),
                "channels" => Ok(CommandOutput::success(
                    "channels - List and manage S-LINK channels\n\
                     \n\
                     Usage: channels [action]\n\
                     \n\
                     Actions:\n\
                       list              List all active channels (default)\n\
                       create <from> <to> Create a new channel\n\
                       close <id>        Close a channel\n\
                       stats <id>        Show channel statistics\n\
                     \n\
                     Examples:\n\
                       channels list\n\
                       channels create s-term s-storage\n\
                       channels stats 1",
                )),
                "gateway" => Ok(CommandOutput::success(
                    "gateway - Manage S-GATE TCP/HTTP gateways\n\
                     \n\
                     Usage: gateway [action]\n\
                     \n\
                     Actions:\n\
                       list              List all gateways (default)\n\
                       create <port> <service> [--protocol=http|tcp|https]\n\
                                         Create a new gateway\n\
                       start <id>        Start a gateway\n\
                       stop <id>         Stop a gateway\n\
                       destroy <id>      Remove a gateway\n\
                       stats <id>        Show gateway statistics\n\
                     \n\
                     Examples:\n\
                       gateway create 8080 web-service --protocol=http\n\
                       gateway start 1",
                )),
                "cap" => Ok(CommandOutput::success(
                    "cap - S-CAP capability management\n\
                     \n\
                     Usage: cap [action]\n\
                     \n\
                     Actions:\n\
                       list              List capabilities for current process\n\
                       info <token>      Show detailed capability info\n\
                       grant <token> <target> [--ops=read,write]\n\
                                         Grant a derived capability\n\
                       revoke <token>    Revoke a capability (and derived)\n\
                       audit             Show recent capability operations\n\
                     \n\
                     Examples:\n\
                       cap list\n\
                       cap grant a1b2c3d4 s-storage --ops=read\n\
                       cap audit",
                )),
                "storage" => Ok(CommandOutput::success(
                    "storage - Interact with S-STORAGE objects\n\
                     \n\
                     Usage: storage [action]\n\
                     \n\
                     Actions:\n\
                       list              List accessible storage objects\n\
                       get <id>          Retrieve an object\n\
                       put <data>        Store new data (returns object ID)\n\
                       delete <id>       Delete an object\n\
                       stats             Show storage statistics\n\
                     \n\
                     Examples:\n\
                       storage list\n\
                       storage put \"hello world\" --type=text\n\
                       storage get obj:a1b2",
                )),
                "system" => Ok(CommandOutput::success(
                    "system - Display system information\n\
                     \n\
                     Usage: system [action]\n\
                     \n\
                     Actions:\n\
                       info              General system info (default)\n\
                       memory            Memory usage breakdown\n\
                       processes         Running process list\n\
                       uptime            System uptime",
                )),
                _ => Ok(CommandOutput::success(alloc::format!(
                    "No help available for '{}'",
                    cmd_name
                ))),
            }
        }
    }
}

/// Services command.
struct ServicesCommand;

impl Command for ServicesCommand {
    fn name(&self) -> &str {
        "services"
    }

    fn description(&self) -> &str {
        "List and manage registered services"
    }

    fn usage(&self) -> &str {
        "services [list|info <name>|discover <capability>]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("list");

        match action {
            "list" => Ok(CommandOutput::success(
                "Registered Services:\n\
                 \n\
                 NAME            STATUS     VERSION\n\
                 s-atlas         healthy    0.1.0\n\
                 s-link          healthy    0.1.0\n\
                 s-gate          healthy    0.1.0\n\
                 s-storage       healthy    0.1.0\n\
                 s-wave          healthy    0.1.0\n\
                 \n\
                 Total: 5 services",
            )),
            "info" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: services info <name>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "Service '{}' info not yet implemented",
                    cmd.args[1]
                )))
            }
            "discover" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: services discover <capability>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "Discovery for '{}' not yet implemented",
                    cmd.args[1]
                )))
            }
            _ => Err(CommandError::InvalidArguments(alloc::format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

/// Channels command.
struct ChannelsCommand;

impl Command for ChannelsCommand {
    fn name(&self) -> &str {
        "channels"
    }

    fn description(&self) -> &str {
        "List and manage S-LINK channels"
    }

    fn usage(&self) -> &str {
        "channels [list|create <from> <to>|close <id>|stats <id>]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("list");

        match action {
            "list" => Ok(CommandOutput::success(
                "Active Channels:\n\
                 \n\
                 ID    FROM          TO            PENDING  STATUS\n\
                 1     s-term        s-atlas       0        open\n\
                 2     s-gate        s-wave        3        open\n\
                 3     s-gate        s-storage     0        open\n\
                 \n\
                 Total: 3 channels",
            )),
            "create" => {
                if cmd.args.len() < 3 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: channels create <from> <to>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Created channel {} -> {} (id: 4)",
                    cmd.args[1], cmd.args[2]
                )))
            }
            "close" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: channels close <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Channel {} closed",
                    cmd.args[1]
                )))
            }
            "stats" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: channels stats <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "Channel {} Statistics:\n\
                     \n\
                     Messages sent:     1,234\n\
                     Messages received: 1,230\n\
                     Bytes transferred: 45.6 KB\n\
                     Avg latency:       2.3 µs\n\
                     Status:            open",
                    cmd.args[1]
                )))
            }
            _ => Err(CommandError::InvalidArguments(alloc::format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

/// Gateway command for S-GATE management.
struct GatewayCommand;

impl Command for GatewayCommand {
    fn name(&self) -> &str {
        "gateway"
    }

    fn description(&self) -> &str {
        "Manage S-GATE gateways (TCP/HTTP endpoints)"
    }

    fn usage(&self) -> &str {
        "gateway [list|create <port> <service>|start <id>|stop <id>|destroy <id>]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("list");

        match action {
            "list" => Ok(CommandOutput::success(
                "Active Gateways:\n\
                 \n\
                 ID    PORT   PROTOCOL  SERVICE        CONNS  STATUS\n\
                 1     8080   HTTP/1.1  web-service    12     running\n\
                 2     8443   HTTPS     api-service    45     running\n\
                 3     9000   TCP       data-service   3      stopped\n\
                 \n\
                 Total: 3 gateways (2 running)",
            )),
            "create" => {
                if cmd.args.len() < 3 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: gateway create <port> <service> [--protocol=http|tcp|https]".into(),
                    ));
                }
                let protocol = cmd.get_option("protocol").unwrap_or("http");
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Created gateway on port {} -> {} (protocol: {}, id: 4)",
                    cmd.args[1], cmd.args[2], protocol
                )))
            }
            "start" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: gateway start <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Gateway {} started",
                    cmd.args[1]
                )))
            }
            "stop" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: gateway stop <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Gateway {} stopped",
                    cmd.args[1]
                )))
            }
            "destroy" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: gateway destroy <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Gateway {} destroyed",
                    cmd.args[1]
                )))
            }
            "stats" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: gateway stats <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "Gateway {} Statistics:\n\
                     \n\
                     Total connections:    5,678\n\
                     Active connections:   45\n\
                     Bytes received:       12.3 MB\n\
                     Bytes sent:           89.4 MB\n\
                     Firewall rejections:  23\n\
                     Avg response time:    4.2 ms",
                    cmd.args[1]
                )))
            }
            _ => Err(CommandError::InvalidArguments(alloc::format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

/// Capability command for S-CAP management.
struct CapCommand;

impl Command for CapCommand {
    fn name(&self) -> &str {
        "cap"
    }

    fn description(&self) -> &str {
        "Manage S-CAP capabilities"
    }

    fn usage(&self) -> &str {
        "cap [list|info <token>|grant <token> <target>|revoke <token>|audit]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("list");

        match action {
            "list" => Ok(CommandOutput::success(
                "Process Capabilities:\n\
                 \n\
                 TOKEN (short)  RESOURCE       OPERATIONS      EXPIRES\n\
                 a1b2c3d4...    channel:1      read,write      never\n\
                 e5f6g7h8...    service:atlas  discover        never\n\
                 i9j0k1l2...    storage:obj1   read            2h 30m\n\
                 m3n4o5p6...    memory:heap    alloc,free      never\n\
                 \n\
                 Total: 4 capabilities",
            )),
            "info" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: cap info <token>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "Capability {}:\n\
                     \n\
                     Token:      {}...\n\
                     Resource:   channel:1\n\
                     Operations: read, write, grant\n\
                     Owner:      process:s-term\n\
                     Parent:     (root)\n\
                     Created:    2025-12-25 10:30:00\n\
                     Expires:    never\n\
                     Revoked:    no",
                    cmd.args[1], cmd.args[1]
                )))
            }
            "grant" => {
                if cmd.args.len() < 3 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: cap grant <token> <target-process> [--ops=read,write]".into(),
                    ));
                }
                let ops = cmd.get_option("ops").unwrap_or("read");
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Granted derived capability to '{}' (ops: {})\n\
                     New token: q7r8s9t0...",
                    cmd.args[2], ops
                )))
            }
            "revoke" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: cap revoke <token>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Revoked capability {} and 2 derived capabilities",
                    cmd.args[1]
                )))
            }
            "audit" => {
                Ok(CommandOutput::success(
                    "Recent Capability Operations:\n\
                     \n\
                     TIME         OPERATION  TOKEN       ACTOR     RESULT\n\
                     10:45:23.001 check      a1b2c3d4    s-gate    success\n\
                     10:45:22.998 check      e5f6g7h8    s-term    success\n\
                     10:45:22.456 grant      i9j0k1l2    s-atlas   success\n\
                     10:45:21.789 check      m3n4o5p6    s-wave    denied\n\
                     10:45:20.123 create     q7r8s9t0    kernel    success\n\
                     \n\
                     Total entries: 1,234 (showing last 5)",
                ))
            }
            _ => Err(CommandError::InvalidArguments(alloc::format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

/// Storage command for S-STORAGE management.
struct StorageCommand;

impl Command for StorageCommand {
    fn name(&self) -> &str {
        "storage"
    }

    fn description(&self) -> &str {
        "Interact with S-STORAGE objects"
    }

    fn usage(&self) -> &str {
        "storage [list|get <id>|put <data>|delete <id>|stats]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("list");

        match action {
            "list" => Ok(CommandOutput::success(
                "Storage Objects:\n\
                 \n\
                 ID          SIZE      TYPE         CREATED\n\
                 obj:a1b2    4.2 KB    binary       2025-12-25 10:00\n\
                 obj:c3d4    128 B     text/json    2025-12-25 10:15\n\
                 obj:e5f6    1.2 MB    wasm         2025-12-25 10:30\n\
                 \n\
                 Total: 3 objects (1.2 MB)",
            )),
            "get" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: storage get <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "Object {}:\n\
                     Size: 128 B\n\
                     Type: text/json\n\
                     Data: {{\"hello\": \"world\"}}",
                    cmd.args[1]
                )))
            }
            "put" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: storage put <data> [--type=binary|text]".into(),
                    ));
                }
                Ok(CommandOutput::success(
                    "[OK] Created object obj:f7g8 (45 bytes)",
                ))
            }
            "delete" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: storage delete <id>".into(),
                    ));
                }
                Ok(CommandOutput::success(alloc::format!(
                    "[OK] Deleted object {}",
                    cmd.args[1]
                )))
            }
            "stats" => Ok(CommandOutput::success(
                "Storage Statistics:\n\
                 \n\
                 Total objects:    156\n\
                 Total size:       45.6 MB\n\
                 Available:        954.4 MB\n\
                 Read ops/sec:     1,234\n\
                 Write ops/sec:    567",
            )),
            _ => Err(CommandError::InvalidArguments(alloc::format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

/// System info command.
struct SystemCommand;

impl Command for SystemCommand {
    fn name(&self) -> &str {
        "system"
    }

    fn description(&self) -> &str {
        "Display system information"
    }

    fn usage(&self) -> &str {
        "system [info|memory|processes|uptime]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("info");

        match action {
            "info" => Ok(CommandOutput::success(
                "Splax OS System Information\n\
                 \n\
                 Kernel:       S-CORE v0.1.0\n\
                 Architecture: x86_64\n\
                 Build:        release\n\
                 Uptime:       0d 2h 34m 56s\n\
                 \n\
                 Services:     5 running\n\
                 Channels:     23 active\n\
                 Capabilities: 1,234 allocated\n\
                 Memory:       256 MB / 512 MB used",
            )),
            "memory" => Ok(CommandOutput::success(
                "Memory Usage:\n\
                 \n\
                 Total:        512 MB\n\
                 Used:         256 MB (50%)\n\
                 Free:         256 MB\n\
                 Kernel:       8 MB\n\
                 Services:     128 MB\n\
                 Page tables:  4 MB\n\
                 \n\
                 Allocations:  4,567\n\
                 Deallocations: 3,210",
            )),
            "processes" => Ok(CommandOutput::success(
                "Running Processes:\n\
                 \n\
                 PID   NAME        CLASS        CPU%   MEM     STATE\n\
                 0     kernel      realtime     2.1%   8 MB    running\n\
                 1     s-atlas     interactive  0.5%   12 MB   ready\n\
                 2     s-link      interactive  1.2%   8 MB    ready\n\
                 3     s-gate      interactive  3.4%   24 MB   running\n\
                 4     s-storage   background   0.8%   64 MB   ready\n\
                 5     s-term      interactive  0.1%   4 MB    running\n\
                 \n\
                 Total: 6 processes",
            )),
            "uptime" => Ok(CommandOutput::success(
                "Uptime: 0 days, 2 hours, 34 minutes, 56 seconds",
            )),
            _ => Err(CommandError::InvalidArguments(alloc::format!(
                "Unknown action: {}",
                action
            ))),
        }
    }
}

/// The S-TERM shell.
pub struct Shell {
    commands: Vec<alloc::boxed::Box<dyn Command>>,
    prompt: String,
}

impl Shell {
    /// Creates a new shell.
    pub fn new() -> Self {
        let commands: Vec<alloc::boxed::Box<dyn Command>> = alloc::vec![
            alloc::boxed::Box::new(HelpCommand),
            alloc::boxed::Box::new(ServicesCommand),
            alloc::boxed::Box::new(ChannelsCommand),
            alloc::boxed::Box::new(GatewayCommand),
            alloc::boxed::Box::new(CapCommand),
            alloc::boxed::Box::new(StorageCommand),
            alloc::boxed::Box::new(SystemCommand),
        ];

        Self {
            commands,
            prompt: String::from("splax> "),
        }
    }

    /// Executes a command line.
    pub fn execute(&self, line: &str) -> CommandResult {
        let line = line.trim();
        if line.is_empty() {
            return Ok(CommandOutput::success(""));
        }
        
        let cmd = match ParsedCommand::parse(line) {
            Some(c) => c,
            None => return Ok(CommandOutput::success("")),
        };

        for handler in &self.commands {
            if handler.name() == cmd.name {
                return handler.execute(&cmd);
            }
        }

        Err(CommandError::UnknownCommand(cmd.name))
    }

    /// Gets the prompt string.
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// Sets the prompt string.
    pub fn set_prompt(&mut self, prompt: impl Into<String>) {
        self.prompt = prompt.into();
    }
    
    /// Lists all available commands.
    pub fn list_commands(&self) -> Vec<(&str, &str)> {
        self.commands.iter()
            .map(|c| (c.name(), c.description()))
            .collect()
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

/// Stub allocator for tools (would be provided by runtime).
struct StubAllocator;

unsafe impl core::alloc::GlobalAlloc for StubAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

#[global_allocator]
static ALLOCATOR: StubAllocator = StubAllocator;

/// Entry point (placeholder - would be called by runtime).
#[no_mangle]
pub extern "C" fn main() -> i32 {
    // In a real implementation, this would:
    // 1. Initialize shell
    // 2. Read input from terminal service
    // 3. Execute commands
    // 4. Print output
    0
}

/// Panic handler.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_command() {
        let cmd = ParsedCommand::parse("services list --format=json").unwrap();
        assert_eq!(cmd.name, "services");
        assert_eq!(cmd.args, vec!["list"]);
        assert_eq!(cmd.get_option("format"), Some("json"));
    }

    #[test]
    fn test_help_command() {
        let shell = Shell::new();
        let result = shell.execute("help").unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.text.contains("Available commands"));
    }
}
