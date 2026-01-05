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

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

pub mod emulator;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

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
                 pkg         - S-PKG package manager\n\
                 vim         - Vi IMproved text editor\n\
                 run         - Run installed package binaries\n\
                 \n\
                 Use 'help <command>' for detailed information.\n\
                 Use '<command> --help' for usage.",
            ))
        } else {
            let cmd_name = &cmd.args[0];
            match cmd_name.as_str() {
                "pkg" => Ok(CommandOutput::success(
                    "pkg - S-PKG package manager\n\
                     \n\
                     Usage: pkg <command> [arguments]\n\
                     \n\
                     Commands:\n\
                       install <pkg>    Install a package\n\
                       remove <pkg>     Remove an installed package\n\
                       search <query>   Search for packages\n\
                       list             List installed packages\n\
                       list --all       List all available packages\n\
                       info <pkg>       Show package information\n\
                       update           Update package repositories\n\
                       upgrade          Upgrade installed packages\n\
                     \n\
                     Examples:\n\
                       pkg install vim\n\
                       pkg search editor\n\
                       pkg info git",
                )),
                "vim" => Ok(CommandOutput::success(
                    "vim - Vi IMproved text editor\n\
                     \n\
                     Usage: vim [file]\n\
                     \n\
                     Start the vim editor, optionally opening a file.\n\
                     \n\
                     Examples:\n\
                       vim            Start vim with empty buffer\n\
                       vim file.txt   Edit file.txt",
                )),
                "run" => Ok(CommandOutput::success(
                    "run - Run installed package binaries\n\
                     \n\
                     Usage: run <binary> [arguments...]\n\
                     \n\
                     Execute an installed binary from a package.\n\
                     \n\
                     Examples:\n\
                       run grep pattern file.txt\n\
                       run python script.py\n\
                       run htop",
                )),
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

// =============================================================================
// Package Manager Command
// =============================================================================

/// Installed package tracking for the shell.
struct InstalledPackages {
    packages: Vec<InstalledPkg>,
}

struct InstalledPkg {
    name: String,
    version: String,
    installed_at: u64,
}

impl InstalledPackages {
    fn new() -> Self {
        Self { packages: Vec::new() }
    }

    fn is_installed(&self, name: &str) -> bool {
        self.packages.iter().any(|p| p.name == name)
    }

    fn install(&mut self, name: &str, version: &str) {
        if !self.is_installed(name) {
            self.packages.push(InstalledPkg {
                name: name.to_string(),
                version: version.to_string(),
                installed_at: 0, // Would be system time
            });
        }
    }

    fn remove(&mut self, name: &str) -> bool {
        let len_before = self.packages.len();
        self.packages.retain(|p| p.name != name);
        self.packages.len() < len_before
    }

    fn list(&self) -> &[InstalledPkg] {
        &self.packages
    }
}

/// Package manager command.
struct PkgCommand;

impl Command for PkgCommand {
    fn name(&self) -> &str {
        "pkg"
    }

    fn description(&self) -> &str {
        "S-PKG package manager - install, remove, and manage packages"
    }

    fn usage(&self) -> &str {
        "pkg [install|remove|search|list|info|update|upgrade] [package]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let action = cmd.args.first().map(|s| s.as_str()).unwrap_or("help");

        match action {
            "install" | "i" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: pkg install <package> [package...]".into(),
                    ));
                }
                
                let mut output = String::new();
                for pkg_name in &cmd.args[1..] {
                    // Simulate package installation
                    let (name, version, desc) = get_package_info(pkg_name);
                    if name.is_empty() {
                        output.push_str(&alloc::format!(
                            "Error: Package '{}' not found in repository\n",
                            pkg_name
                        ));
                        continue;
                    }
                    
                    output.push_str(&alloc::format!(
                        "Installing {} v{}...\n",
                        name, version
                    ));
                    output.push_str(&alloc::format!(
                        "  Description: {}\n",
                        desc
                    ));
                    output.push_str("  Resolving dependencies... done\n");
                    output.push_str("  Downloading package... done\n");
                    output.push_str("  Verifying signature... done\n");
                    output.push_str("  Extracting files... done\n");
                    output.push_str("  Running post-install hooks... done\n");
                    output.push_str(&alloc::format!(
                        "[OK] Successfully installed {}\n\n",
                        name
                    ));
                }
                
                Ok(CommandOutput::success(output))
            }
            
            "remove" | "uninstall" | "r" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: pkg remove <package>".into(),
                    ));
                }
                
                let pkg_name = &cmd.args[1];
                Ok(CommandOutput::success(alloc::format!(
                    "Removing {}...\n\
                     Running pre-remove hooks... done\n\
                     Removing files... done\n\
                     Updating database... done\n\
                     [OK] Successfully removed {}",
                    pkg_name, pkg_name
                )))
            }
            
            "search" | "s" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: pkg search <query>".into(),
                    ));
                }
                
                let query = &cmd.args[1];
                let results = search_packages(query);
                
                if results.is_empty() {
                    return Ok(CommandOutput::success(alloc::format!(
                        "No packages found matching '{}'",
                        query
                    )));
                }
                
                let mut output = String::from("Search Results:\n\n");
                output.push_str("NAME            VERSION    DESCRIPTION\n");
                output.push_str("─".repeat(60).as_str());
                output.push('\n');
                
                for (name, version, desc) in results {
                    output.push_str(&alloc::format!(
                        "{:<15} {:<10} {}\n",
                        name,
                        version,
                        if desc.len() > 35 {
                            alloc::format!("{}...", &desc[..32])
                        } else {
                            desc
                        }
                    ));
                }
                
                Ok(CommandOutput::success(output))
            }
            
            "list" | "ls" => {
                let show_all = cmd.has_option("all") || cmd.has_option("a");
                
                if show_all {
                    // Show all available packages
                    let packages = list_all_packages();
                    
                    let mut output = String::from("Available Packages:\n\n");
                    output.push_str("NAME            VERSION    SIZE       DESCRIPTION\n");
                    output.push_str("─".repeat(70).as_str());
                    output.push('\n');
                    
                    for (name, version, size, desc) in packages {
                        output.push_str(&alloc::format!(
                            "{:<15} {:<10} {:<10} {}\n",
                            name,
                            version,
                            format_size(size),
                            if desc.len() > 30 {
                                alloc::format!("{}...", &desc[..27])
                            } else {
                                desc
                            }
                        ));
                    }
                    
                    output.push_str(&alloc::format!(
                        "\nTotal: {} packages available",
                        list_all_packages().len()
                    ));
                    
                    Ok(CommandOutput::success(output))
                } else {
                    // Show installed packages
                    Ok(CommandOutput::success(
                        "Installed Packages:\n\n\
                         NAME            VERSION    INSTALLED    SIZE\n\
                         ──────────────────────────────────────────────────\n\
                         coreutils       0.1.0      2025-01-01   5.0 MB\n\
                         vim             9.1.0      2025-01-01   15.0 MB\n\
                         grep            3.11.0     2025-01-01   800 KB\n\
                         \n\
                         Total: 3 packages installed (20.8 MB)\n\
                         \n\
                         Use 'pkg list --all' to see all available packages."
                    ))
                }
            }
            
            "info" => {
                if cmd.args.len() < 2 {
                    return Err(CommandError::InvalidArguments(
                        "Usage: pkg info <package>".into(),
                    ));
                }
                
                let pkg_name = &cmd.args[1];
                let info = get_package_details(pkg_name);
                
                if info.is_empty() {
                    return Ok(CommandOutput::error(
                        alloc::format!("Package '{}' not found", pkg_name),
                        1,
                    ));
                }
                
                Ok(CommandOutput::success(info))
            }
            
            "update" => {
                Ok(CommandOutput::success(
                    "Updating package repositories...\n\
                     \n\
                     Repository: splax-core\n\
                     URL: https://pkg.splax.dev/core\n\
                     Status: Updated (156 packages)\n\
                     \n\
                     Repository: splax-extra\n\
                     URL: https://pkg.splax.dev/extra\n\
                     Status: Updated (423 packages)\n\
                     \n\
                     [OK] All repositories are up to date."
                ))
            }
            
            "upgrade" => {
                Ok(CommandOutput::success(
                    "Checking for upgrades...\n\
                     \n\
                     Packages to upgrade:\n\
                     \n\
                     vim           9.0.0 -> 9.1.0\n\
                     curl          8.4.0 -> 8.5.0\n\
                     \n\
                     Total download size: 18.5 MB\n\
                     \n\
                     Upgrading packages...\n\
                     [1/2] Upgrading vim... done\n\
                     [2/2] Upgrading curl... done\n\
                     \n\
                     [OK] 2 packages upgraded successfully."
                ))
            }
            
            "help" | _ => {
                Ok(CommandOutput::success(
                    "S-PKG - Splax Package Manager\n\
                     \n\
                     Usage: pkg <command> [arguments]\n\
                     \n\
                     Commands:\n\
                       install <pkg>    Install a package\n\
                       remove <pkg>     Remove an installed package\n\
                       search <query>   Search for packages\n\
                       list             List installed packages\n\
                       list --all       List all available packages\n\
                       info <pkg>       Show package information\n\
                       update           Update package repositories\n\
                       upgrade          Upgrade installed packages\n\
                     \n\
                     Examples:\n\
                       pkg install vim\n\
                       pkg search editor\n\
                       pkg info git\n\
                       pkg remove nano"
                ))
            }
        }
    }
}

/// Get package info from the built-in registry.
fn get_package_info(name: &str) -> (String, String, String) {
    // Built-in package database
    match name.to_lowercase().as_str() {
        "vim" => (
            "vim".to_string(),
            "9.1.0".to_string(),
            "Vi IMproved - enhanced vi text editor with syntax highlighting".to_string(),
        ),
        "nano" => (
            "nano".to_string(),
            "7.2.0".to_string(),
            "Simple, easy-to-use terminal text editor".to_string(),
        ),
        "git" => (
            "git".to_string(),
            "2.43.0".to_string(),
            "Distributed version control system".to_string(),
        ),
        "grep" => (
            "grep".to_string(),
            "3.11.0".to_string(),
            "Search files for lines matching a pattern".to_string(),
        ),
        "curl" => (
            "curl".to_string(),
            "8.5.0".to_string(),
            "Command line tool for transferring data with URLs".to_string(),
        ),
        "wget" => (
            "wget".to_string(),
            "1.21.4".to_string(),
            "Network downloader".to_string(),
        ),
        "python" => (
            "python".to_string(),
            "3.12.1".to_string(),
            "Python programming language interpreter".to_string(),
        ),
        "node" => (
            "node".to_string(),
            "21.5.0".to_string(),
            "JavaScript runtime built on V8".to_string(),
        ),
        "htop" => (
            "htop".to_string(),
            "3.3.0".to_string(),
            "Interactive process viewer".to_string(),
        ),
        "less" => (
            "less".to_string(),
            "643.0.0".to_string(),
            "File pager with backward navigation".to_string(),
        ),
        "jq" => (
            "jq".to_string(),
            "1.7.1".to_string(),
            "Command-line JSON processor".to_string(),
        ),
        "coreutils" => (
            "coreutils".to_string(),
            "0.1.0".to_string(),
            "Core utilities: cat, ls, cp, mv, rm, mkdir, etc.".to_string(),
        ),
        "bash" => (
            "bash".to_string(),
            "5.2.21".to_string(),
            "Bourne Again SHell".to_string(),
        ),
        "zsh" => (
            "zsh".to_string(),
            "5.9.0".to_string(),
            "Z shell - extended Bourne shell".to_string(),
        ),
        "ssh" => (
            "ssh".to_string(),
            "9.5.0".to_string(),
            "Secure shell client and server".to_string(),
        ),
        "tar" => (
            "tar".to_string(),
            "1.35.0".to_string(),
            "Tape archive utility".to_string(),
        ),
        "gzip" => (
            "gzip".to_string(),
            "1.13.0".to_string(),
            "GNU compression utility".to_string(),
        ),
        _ => (String::new(), String::new(), String::new()),
    }
}

/// Search packages.
fn search_packages(query: &str) -> Vec<(String, String, String)> {
    let query_lower = query.to_lowercase();
    let all_packages = [
        ("vim", "9.1.0", "Vi IMproved - enhanced vi text editor"),
        ("nano", "7.2.0", "Simple terminal text editor"),
        ("ed", "1.19.0", "Standard Unix line editor"),
        ("git", "2.43.0", "Distributed version control system"),
        ("grep", "3.11.0", "Search files for patterns"),
        ("sed", "4.9.0", "Stream editor"),
        ("awk", "5.3.0", "Pattern processing language"),
        ("curl", "8.5.0", "Transfer data with URLs"),
        ("wget", "1.21.4", "Network downloader"),
        ("python", "3.12.1", "Python interpreter"),
        ("node", "21.5.0", "JavaScript runtime"),
        ("htop", "3.3.0", "Process viewer"),
        ("less", "643.0.0", "File pager"),
        ("jq", "1.7.1", "JSON processor"),
        ("ssh", "9.5.0", "Secure shell"),
        ("bash", "5.2.21", "Bourne Again SHell"),
        ("zsh", "5.9.0", "Z shell"),
    ];
    
    all_packages.iter()
        .filter(|(name, _, desc)| {
            name.contains(&query_lower) || desc.to_lowercase().contains(&query_lower)
        })
        .map(|(n, v, d)| (n.to_string(), v.to_string(), d.to_string()))
        .collect()
}

/// List all available packages.
fn list_all_packages() -> Vec<(String, String, usize, String)> {
    vec![
        ("vim".to_string(), "9.1.0".to_string(), 15_000_000, "Vi IMproved text editor".to_string()),
        ("nano".to_string(), "7.2.0".to_string(), 2_500_000, "Simple text editor".to_string()),
        ("git".to_string(), "2.43.0".to_string(), 30_000_000, "Version control system".to_string()),
        ("grep".to_string(), "3.11.0".to_string(), 800_000, "Pattern search".to_string()),
        ("curl".to_string(), "8.5.0".to_string(), 3_000_000, "URL transfer tool".to_string()),
        ("wget".to_string(), "1.21.4".to_string(), 1_500_000, "Network downloader".to_string()),
        ("python".to_string(), "3.12.1".to_string(), 50_000_000, "Python interpreter".to_string()),
        ("node".to_string(), "21.5.0".to_string(), 80_000_000, "JavaScript runtime".to_string()),
        ("htop".to_string(), "3.3.0".to_string(), 500_000, "Process viewer".to_string()),
        ("less".to_string(), "643.0.0".to_string(), 500_000, "File pager".to_string()),
        ("jq".to_string(), "1.7.1".to_string(), 1_000_000, "JSON processor".to_string()),
        ("coreutils".to_string(), "0.1.0".to_string(), 5_000_000, "Core utilities".to_string()),
        ("bash".to_string(), "5.2.21".to_string(), 2_000_000, "Bourne Again SHell".to_string()),
        ("zsh".to_string(), "5.9.0".to_string(), 3_000_000, "Z shell".to_string()),
        ("ssh".to_string(), "9.5.0".to_string(), 4_000_000, "Secure shell".to_string()),
        ("tar".to_string(), "1.35.0".to_string(), 800_000, "Archive utility".to_string()),
        ("gzip".to_string(), "1.13.0".to_string(), 300_000, "Compression utility".to_string()),
    ]
}

/// Format file size.
fn format_size(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        alloc::format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        alloc::format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        alloc::format!("{} B", bytes)
    }
}

/// Get detailed package information.
fn get_package_details(name: &str) -> String {
    let (pkg_name, version, desc) = get_package_info(name);
    
    if pkg_name.is_empty() {
        return String::new();
    }
    
    match name.to_lowercase().as_str() {
        "vim" => alloc::format!(
            "Package: {}\n\
             Version: {}\n\
             Description: {}\n\
             \n\
             Homepage: https://www.vim.org\n\
             License: Vim License\n\
             Size: 15.0 MB\n\
             \n\
             Provides:\n\
               vim     - Main editor executable\n\
               vi      - Vi compatibility mode\n\
               vimdiff - Side-by-side diff viewer\n\
               view    - Read-only mode\n\
             \n\
             Dependencies: none\n\
             \n\
             Status: Not installed\n\
             \n\
             Use 'pkg install vim' to install.",
            pkg_name, version, desc
        ),
        "git" => alloc::format!(
            "Package: {}\n\
             Version: {}\n\
             Description: {}\n\
             \n\
             Homepage: https://git-scm.com\n\
             License: GPL-2.0\n\
             Size: 30.0 MB\n\
             \n\
             Provides:\n\
               git - Version control system\n\
             \n\
             Dependencies: curl, openssl\n\
             \n\
             Status: Not installed\n\
             \n\
             Use 'pkg install git' to install.",
            pkg_name, version, desc
        ),
        _ => alloc::format!(
            "Package: {}\n\
             Version: {}\n\
             Description: {}\n\
             \n\
             License: MIT OR Apache-2.0\n\
             Status: Not installed\n\
             \n\
             Use 'pkg install {}' to install.",
            pkg_name, version, desc, pkg_name
        ),
    }
}

// =============================================================================
// Vim Command
// =============================================================================

/// Vim text editor command.
struct VimCommand;

impl Command for VimCommand {
    fn name(&self) -> &str {
        "vim"
    }

    fn description(&self) -> &str {
        "Vi IMproved - text editor"
    }

    fn usage(&self) -> &str {
        "vim [file]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        let file = cmd.args.first().map(|s| s.as_str());

        match file {
            Some(filename) => {
                // Opening a file
                Ok(CommandOutput::success(alloc::format!(
                    "VIM - Vi IMproved 9.1 (Splax OS)\n\
                     \n\
                     ┌─────────────────────────────────────────────────────────────────┐\n\
                     │ {:<63} │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~              VIM - Vi IMproved                                │\n\
                     │ ~                                                               │\n\
                     │ ~              version 9.1.0                                    │\n\
                     │ ~              by Bram Moolenaar et al.                         │\n\
                     │ ~              Splax OS Port                                    │\n\
                     │ ~                                                               │\n\
                     │ ~              type :q<Enter> to exit                           │\n\
                     │ ~              type :help<Enter> for help                       │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     ├─────────────────────────────────────────────────────────────────┤\n\
                     │ \"{}\" [New File]                                                 │\n\
                     └─────────────────────────────────────────────────────────────────┘\n\
                     \n\
                     [Interactive vim session - use Ctrl+C to exit in this demo]\n\
                     \n\
                     Type ':q' + Enter to quit, ':wq' to save and quit.\n\
                     Press 'i' for insert mode, 'Esc' for normal mode.",
                    filename, filename
                )))
            }
            None => {
                // No file - show welcome screen
                Ok(CommandOutput::success(
                    "VIM - Vi IMproved 9.1 (Splax OS)\n\
                     \n\
                     ┌─────────────────────────────────────────────────────────────────┐\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~              VIM - Vi IMproved                                │\n\
                     │ ~                                                               │\n\
                     │ ~              version 9.1.0                                    │\n\
                     │ ~              by Bram Moolenaar et al.                         │\n\
                     │ ~              Splax OS Port                                    │\n\
                     │ ~                                                               │\n\
                     │ ~              type :q<Enter> to exit                           │\n\
                     │ ~              type :help<Enter> for help                       │\n\
                     │ ~              type i to start editing                          │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     │ ~                                                               │\n\
                     ├─────────────────────────────────────────────────────────────────┤\n\
                     │ NORMAL                                                    1,1  │\n\
                     └─────────────────────────────────────────────────────────────────┘\n\
                     \n\
                     [Interactive vim session - commands available]\n\
                     \n\
                     Basic commands:\n\
                       i       - Enter insert mode\n\
                       Esc     - Return to normal mode\n\
                       h/j/k/l - Move left/down/up/right\n\
                       :w      - Save file\n\
                       :q      - Quit\n\
                       :wq     - Save and quit\n\
                       dd      - Delete line\n\
                       yy      - Yank (copy) line\n\
                       p       - Paste\n\
                       /text   - Search for 'text'\n\
                       u       - Undo"
                ))
            }
        }
    }
}

// =============================================================================
// Run Command (Generic Binary Execution)
// =============================================================================

/// Run command - execute installed binaries.
struct RunCommand;

impl Command for RunCommand {
    fn name(&self) -> &str {
        "run"
    }

    fn description(&self) -> &str {
        "Run an installed package binary"
    }

    fn usage(&self) -> &str {
        "run <binary> [arguments...]"
    }

    fn execute(&self, cmd: &ParsedCommand) -> CommandResult {
        if cmd.args.is_empty() {
            return Err(CommandError::InvalidArguments(
                "Usage: run <binary> [arguments...]\n\n\
                 Example: run vim myfile.txt\n\
                 Example: run grep pattern file.txt".into(),
            ));
        }

        let binary = &cmd.args[0];
        let args: Vec<&str> = cmd.args[1..].iter().map(|s| s.as_str()).collect();

        // Check if binary is available
        let available_binaries = [
            "vim", "vi", "nano", "ed", "grep", "egrep", "fgrep",
            "sed", "awk", "curl", "wget", "git", "ls", "cat",
            "cp", "mv", "rm", "mkdir", "rmdir", "pwd", "echo",
            "head", "tail", "wc", "sort", "uniq", "less", "more",
            "tar", "gzip", "gunzip", "ssh", "scp", "python", "node",
            "htop", "tree", "jq", "bash", "zsh",
        ];

        if !available_binaries.contains(&binary.as_str()) {
            return Ok(CommandOutput::error(
                alloc::format!(
                    "Command '{}' not found.\n\n\
                     To install, use: pkg install <package>\n\
                     To search: pkg search {}",
                    binary, binary
                ),
                127,
            ));
        }

        // Simulate running the binary
        match binary.as_str() {
            "ls" => {
                if args.contains(&"-l") || args.contains(&"-la") {
                    Ok(CommandOutput::success(
                        "total 42\n\
                         drwxr-xr-x  5 root root 4096 Jan  1 00:00 bin\n\
                         drwxr-xr-x  3 root root 4096 Jan  1 00:00 boot\n\
                         drwxr-xr-x  2 root root 4096 Jan  1 00:00 dev\n\
                         drwxr-xr-x 10 root root 4096 Jan  1 00:00 etc\n\
                         drwxr-xr-x  3 root root 4096 Jan  1 00:00 home\n\
                         drwxr-xr-x  5 root root 4096 Jan  1 00:00 lib\n\
                         drwxr-xr-x  2 root root 4096 Jan  1 00:00 opt\n\
                         drwxr-xr-x  3 root root 4096 Jan  1 00:00 root\n\
                         drwxr-xr-x  2 root root 4096 Jan  1 00:00 srv\n\
                         drwxr-xr-x  2 root root 4096 Jan  1 00:00 tmp\n\
                         drwxr-xr-x  8 root root 4096 Jan  1 00:00 usr\n\
                         drwxr-xr-x  5 root root 4096 Jan  1 00:00 var"
                    ))
                } else {
                    Ok(CommandOutput::success(
                        "bin  boot  dev  etc  home  lib  opt  root  srv  tmp  usr  var"
                    ))
                }
            }
            "pwd" => Ok(CommandOutput::success("/home/user")),
            "echo" => Ok(CommandOutput::success(args.join(" "))),
            "cat" => {
                if args.is_empty() {
                    Err(CommandError::InvalidArguments("Usage: cat <file>".into()))
                } else {
                    Ok(CommandOutput::success(alloc::format!(
                        "[Contents of {}]\n\
                         Hello, World!\n\
                         This is a sample file.",
                        args[0]
                    )))
                }
            }
            "grep" => {
                if args.len() < 2 {
                    Err(CommandError::InvalidArguments("Usage: grep <pattern> <file>".into()))
                } else {
                    Ok(CommandOutput::success(alloc::format!(
                        "{}:1:This line contains the pattern '{}'",
                        args.get(1).unwrap_or(&"file"),
                        args[0]
                    )))
                }
            }
            "python" => {
                if args.is_empty() {
                    Ok(CommandOutput::success(
                        "Python 3.12.1 (Splax OS)\n\
                         Type \"help\", \"copyright\", \"credits\" or \"license\" for more information.\n\
                         >>> \n\
                         [Interactive Python REPL - use Ctrl+D to exit]"
                    ))
                } else {
                    Ok(CommandOutput::success(alloc::format!(
                        "[Running python {}]",
                        args.join(" ")
                    )))
                }
            }
            "node" => {
                if args.is_empty() {
                    Ok(CommandOutput::success(
                        "Welcome to Node.js v21.5.0.\n\
                         Type \".help\" for more information.\n\
                         > \n\
                         [Interactive Node.js REPL - use Ctrl+D to exit]"
                    ))
                } else {
                    Ok(CommandOutput::success(alloc::format!(
                        "[Running node {}]",
                        args.join(" ")
                    )))
                }
            }
            "htop" => Ok(CommandOutput::success(
                "htop 3.3.0 - Splax OS\n\
                 \n\
                 ┌───────────────────────────────────────────────────────────────────┐\n\
                 │  CPU[||||||||||                    25.0%]   Tasks: 6, 0 thr       │\n\
                 │  Mem[||||||||||||||||              50.0%]   Load: 0.45 0.23 0.12  │\n\
                 │  Swp[                               0.0%]   Uptime: 02:34:56      │\n\
                 ├───────────────────────────────────────────────────────────────────┤\n\
                 │  PID USER      PRI  NI  VIRT   RES   SHR S  CPU% MEM%   TIME+  C  │\n\
                 │    0 root       20   0  8192  8192  4096 R   2.1  1.6  0:12.34 0  │\n\
                 │    1 root       20   0 12288 12288  8192 S   0.5  2.4  0:05.67 0  │\n\
                 │    2 root       20   0  8192  8192  4096 S   1.2  1.6  0:08.90 0  │\n\
                 │    3 root       20   0 24576 24576 16384 R   3.4  4.8  0:15.23 0  │\n\
                 │    4 root       20   0 65536 65536 32768 S   0.8 12.8  0:22.45 0  │\n\
                 │    5 user       20   0  4096  4096  2048 R   0.1  0.8  0:01.11 0  │\n\
                 └───────────────────────────────────────────────────────────────────┘\n\
                 \n\
                 [Press q to quit, F1-F10 for options]"
            )),
            "tree" => Ok(CommandOutput::success(
                ".\n\
                 ├── bin\n\
                 │   ├── vim\n\
                 │   ├── grep\n\
                 │   └── ls\n\
                 ├── etc\n\
                 │   └── pkg.conf\n\
                 ├── home\n\
                 │   └── user\n\
                 └── var\n\
                     └── pkg\n\
                 \n\
                 5 directories, 4 files"
            )),
            "git" => {
                if args.is_empty() {
                    Ok(CommandOutput::success(
                        "usage: git [-v | --version] [-h | --help] [-C <path>]\n\
                         \n\
                         Common commands:\n\
                           clone      Clone a repository\n\
                           init       Create empty repository\n\
                           add        Add files to staging\n\
                           commit     Record changes\n\
                           push       Upload changes\n\
                           pull       Download changes\n\
                           status     Show working tree status\n\
                           log        Show commit logs\n\
                         \n\
                         See 'git help <command>' for more info."
                    ))
                } else {
                    match args[0] {
                        "status" => Ok(CommandOutput::success(
                            "On branch main\n\
                             Your branch is up to date with 'origin/main'.\n\
                             \n\
                             nothing to commit, working tree clean"
                        )),
                        "version" | "--version" => Ok(CommandOutput::success("git version 2.43.0")),
                        _ => Ok(CommandOutput::success(alloc::format!(
                            "[git {}]",
                            args.join(" ")
                        ))),
                    }
                }
            }
            _ => Ok(CommandOutput::success(alloc::format!(
                "[Running {} {}]",
                binary,
                args.join(" ")
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
            alloc::boxed::Box::new(PkgCommand),
            alloc::boxed::Box::new(VimCommand),
            alloc::boxed::Box::new(RunCommand),
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
#[cfg(not(test))]
struct StubAllocator;

#[cfg(not(test))]
unsafe impl core::alloc::GlobalAlloc for StubAllocator {
    unsafe fn alloc(&self, _layout: core::alloc::Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: core::alloc::Layout) {}
}

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: StubAllocator = StubAllocator;

/// Entry point (placeholder - would be called by runtime).
#[cfg(not(test))]
#[no_mangle]
pub extern "C" fn main() -> i32 {
    // Initialize shell instance
    let shell = Shell::new();
    
    // Main shell loop
    loop {
        // Display prompt (would go to terminal service)
        // Read line from stdin (terminal service provides input)
        // Parse and execute command
        // Display output
        
        // For now, return success (runtime provides event loop)
        break;
    }
    
    0
}

/// Panic handler.
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(test)]
mod tests {
    use alloc::vec;
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
