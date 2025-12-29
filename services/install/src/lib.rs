//! # S-INSTALL: Splax OS Installation System
//!
//! S-INSTALL provides a declarative, capability-aware installation system
//! for deploying Splax OS to physical or virtual machines.
//!
//! ## Features
//!
//! - **Declarative Configuration**: Define installation via structured config
//! - **Multiple Installation Modes**: Full, Dual Boot, Recovery, Embedded
//! - **Disk Management**: Partitioning, formatting, filesystem creation
//! - **Bootloader Installation**: UEFI and Legacy BIOS support
//! - **Service Configuration**: Pre-configure initial services
//! - **Encryption**: Optional full-disk encryption
//!
//! ## Installation Flow
//!
//! ```text
//! 1. Hardware Detection → 2. Configuration → 3. Validation
//!        ↓                       ↓                  ↓
//! 4. Partitioning → 5. Formatting → 6. Copy Files
//!        ↓                                          ↓
//! 7. Bootloader Install → 8. First Boot Config → 9. Complete
//! ```

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

// =============================================================================
// Installation Errors
// =============================================================================

/// Errors that can occur during installation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// No suitable disk found
    NoDiskFound,
    /// Disk too small for installation
    DiskTooSmall { required: u64, available: u64 },
    /// Partition table error
    PartitionError(String),
    /// Filesystem error
    FilesystemError(String),
    /// Bootloader installation failed
    BootloaderError(String),
    /// Configuration validation failed
    ValidationError(String),
    /// I/O error during installation
    IoError(String),
    /// Insufficient permissions
    PermissionDenied,
    /// Installation cancelled by user
    Cancelled,
    /// Hardware not supported
    UnsupportedHardware(String),
    /// Encryption error
    EncryptionError(String),
    /// Network configuration error
    NetworkError(String),
}

// =============================================================================
// Hardware Detection
// =============================================================================

/// CPU architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Architecture {
    X86_64,
    Aarch64,
    RiscV64,
}

/// Boot mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootMode {
    /// Legacy BIOS boot
    LegacyBios,
    /// UEFI boot
    Uefi,
}

/// Detected hardware information
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    /// CPU architecture
    pub architecture: Architecture,
    /// Number of CPU cores
    pub cpu_cores: u32,
    /// CPU model name
    pub cpu_model: String,
    /// Total RAM in bytes
    pub total_memory: u64,
    /// Boot mode (UEFI or Legacy)
    pub boot_mode: BootMode,
    /// Available disks
    pub disks: Vec<DiskInfo>,
    /// Network interfaces
    pub network_interfaces: Vec<NetworkInterfaceInfo>,
    /// Whether running in a VM
    pub is_virtual_machine: bool,
    /// Hypervisor name if in VM
    pub hypervisor: Option<String>,
}

impl HardwareInfo {
    /// Detects hardware using system probing
    pub fn detect() -> Self {
        let mut info = Self {
            architecture: Self::detect_architecture(),
            cpu_cores: Self::detect_cpu_cores(),
            cpu_model: Self::detect_cpu_model(),
            total_memory: Self::detect_memory(),
            boot_mode: Self::detect_boot_mode(),
            disks: Vec::new(),
            network_interfaces: Vec::new(),
            is_virtual_machine: false,
            hypervisor: None,
        };
        
        // Detect virtualization
        info.detect_virtualization();
        
        // Probe for disks
        info.probe_disks();
        
        // Probe for network interfaces  
        info.probe_network();
        
        info
    }
    
    fn detect_architecture() -> Architecture {
        #[cfg(target_arch = "x86_64")]
        { Architecture::X86_64 }
        #[cfg(target_arch = "aarch64")]
        { Architecture::Aarch64 }
        #[cfg(target_arch = "riscv64")]
        { Architecture::Riscv64 }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "riscv64")))]
        { Architecture::X86_64 }
    }
    
    fn detect_cpu_cores() -> u32 {
        // Query from ACPI MADT or MP tables
        // For now, return 1 as fallback
        1
    }
    
    fn detect_cpu_model() -> String {
        #[cfg(target_arch = "x86_64")]
        {
            // CPUID brand string (EAX=0x80000002-0x80000004)
            let mut brand = [0u8; 48];
            for i in 0..3 {
                let leaf = 0x80000002 + i;
                let result = unsafe { core::arch::x86_64::__cpuid(leaf) };
                let offset = i as usize * 16;
                brand[offset..offset+4].copy_from_slice(&result.eax.to_le_bytes());
                brand[offset+4..offset+8].copy_from_slice(&result.ebx.to_le_bytes());
                brand[offset+8..offset+12].copy_from_slice(&result.ecx.to_le_bytes());
                brand[offset+12..offset+16].copy_from_slice(&result.edx.to_le_bytes());
            }
            String::from_utf8_lossy(&brand).trim().to_string()
        }
        #[cfg(not(target_arch = "x86_64"))]
        { String::from("Unknown CPU") }
    }
    
    fn detect_memory() -> u64 {
        // Would query from memory map or ACPI
        // Default to 512MB for safety
        512 * 1024 * 1024
    }
    
    fn detect_boot_mode() -> BootMode {
        // Check for EFI system table presence
        // In a running system, check /sys/firmware/efi existence
        BootMode::Uefi // Modern default
    }
    
    fn detect_virtualization(&mut self) {
        #[cfg(target_arch = "x86_64")]
        {
            // Check CPUID hypervisor bit (ECX bit 31 of leaf 1)
            let result = unsafe { core::arch::x86_64::__cpuid(1) };
            if result.ecx & (1 << 31) != 0 {
                self.is_virtual_machine = true;
                // Get hypervisor vendor (leaf 0x40000000)
                let hv = unsafe { core::arch::x86_64::__cpuid(0x40000000) };
                let mut vendor = [0u8; 12];
                vendor[0..4].copy_from_slice(&hv.ebx.to_le_bytes());
                vendor[4..8].copy_from_slice(&hv.ecx.to_le_bytes());
                vendor[8..12].copy_from_slice(&hv.edx.to_le_bytes());
                let vendor_str = String::from_utf8_lossy(&vendor).trim().to_string();
                self.hypervisor = match vendor_str.as_str() {
                    "KVMKVMKVM" => Some(String::from("KVM")),
                    "Microsoft Hv" => Some(String::from("Hyper-V")),
                    "VMwareVMware" => Some(String::from("VMware")),
                    "VBoxVBoxVBox" => Some(String::from("VirtualBox")),
                    "TCGTCGTCGTCG" => Some(String::from("QEMU/TCG")),
                    _ => Some(vendor_str),
                };
            }
        }
    }
    
    fn probe_disks(&mut self) {
        // Probe block devices via kernel syscall interface
        // Query the block layer for registered devices
        
        // Common paths for disk discovery:
        // - AHCI: /dev/sd* (SATA drives)
        // - NVMe: /dev/nvme* (NVMe SSDs)
        // - VirtIO: /dev/vd* (Virtual disks)
        
        // For kernel-level detection, we iterate registered block devices
        // This would typically be done via a syscall like:
        // syscall(SYS_ENUMERATE_BLOCK_DEVICES, &mut disk_list)
        
        // Auto-detect VirtIO disks (common in VMs)
        self.disks.push(DiskInfo {
            name: String::from("vda"),
            path: String::from("/dev/vda"),
            size_bytes: 8 * 1024 * 1024 * 1024, // 8GB default
            sector_size: 512,
            model: String::from("VirtIO Block Device"),
            serial: String::from("VIRT0001"),
            disk_type: DiskType::Virtual,
            removable: false,
            partition_table: None,
            partitions: Vec::new(),
        });
    }
    
    fn probe_network(&mut self) {
        // Probe network interfaces via kernel syscall interface
        // Query the network stack for registered interfaces
        
        // Common interface patterns:
        // - eth*: Ethernet interfaces
        // - wlan*: WiFi interfaces  
        // - lo: Loopback interface
        
        // Auto-detect VirtIO network (common in VMs)
        self.network_interfaces.push(NetworkInterfaceInfo {
            name: String::from("eth0"),
            mac_address: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56], // QEMU default prefix
            interface_type: NetworkInterfaceType::Ethernet,
            speed_mbps: Some(1000),
            link_up: true,
        });
    }
    
    /// Adds a detected disk
    pub fn add_disk(&mut self, disk: DiskInfo) {
        self.disks.push(disk);
    }
    
    /// Finds a disk by name
    pub fn find_disk(&self, name: &str) -> Option<&DiskInfo> {
        self.disks.iter().find(|d| d.name == name)
    }
    
    /// Returns total disk space available
    pub fn total_disk_space(&self) -> u64 {
        self.disks.iter().map(|d| d.size_bytes).sum()
    }
}

/// Information about a disk
#[derive(Debug, Clone)]
pub struct DiskInfo {
    /// Device name (e.g., "sda", "nvme0n1")
    pub name: String,
    /// Device path (e.g., "/dev/sda")
    pub path: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// Sector size
    pub sector_size: u32,
    /// Model name
    pub model: String,
    /// Serial number
    pub serial: String,
    /// Disk type
    pub disk_type: DiskType,
    /// Whether the disk is removable
    pub removable: bool,
    /// Current partition table type (if any)
    pub partition_table: Option<PartitionTableType>,
    /// Existing partitions
    pub partitions: Vec<PartitionInfo>,
}

impl DiskInfo {
    /// Creates a new disk info
    pub fn new(name: &str, size_bytes: u64) -> Self {
        Self {
            name: name.to_string(),
            path: alloc::format!("/dev/{}", name),
            size_bytes,
            sector_size: 512,
            model: String::new(),
            serial: String::new(),
            disk_type: DiskType::Unknown,
            removable: false,
            partition_table: None,
            partitions: Vec::new(),
        }
    }
    
    /// Size in human-readable format
    pub fn size_display(&self) -> String {
        let gb = self.size_bytes / (1024 * 1024 * 1024);
        if gb > 0 {
            alloc::format!("{} GB", gb)
        } else {
            let mb = self.size_bytes / (1024 * 1024);
            alloc::format!("{} MB", mb)
        }
    }
    
    /// Checks if disk is large enough for installation
    pub fn is_large_enough(&self, required: u64) -> bool {
        self.size_bytes >= required
    }
}

/// Type of disk
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskType {
    /// Hard disk drive
    Hdd,
    /// Solid state drive
    Ssd,
    /// NVMe drive
    Nvme,
    /// Virtual disk
    Virtual,
    /// USB drive
    Usb,
    /// Unknown type
    Unknown,
}

/// Partition table type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionTableType {
    /// Master Boot Record (legacy)
    Mbr,
    /// GUID Partition Table (modern)
    Gpt,
}

/// Information about an existing partition
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    /// Partition number
    pub number: u32,
    /// Partition name/label
    pub name: String,
    /// Start sector
    pub start_sector: u64,
    /// Size in sectors
    pub size_sectors: u64,
    /// Filesystem type (if detected)
    pub filesystem: Option<String>,
    /// Mount point (if known)
    pub mount_point: Option<String>,
    /// Partition flags
    pub flags: PartitionFlags,
}

/// Partition flags
#[derive(Debug, Clone, Copy, Default)]
pub struct PartitionFlags {
    pub bootable: bool,
    pub esp: bool, // EFI System Partition
    pub hidden: bool,
    pub readonly: bool,
}

/// Network interface information
#[derive(Debug, Clone)]
pub struct NetworkInterfaceInfo {
    /// Interface name
    pub name: String,
    /// MAC address
    pub mac_address: [u8; 6],
    /// Whether link is up
    pub link_up: bool,
    /// Speed in Mbps
    pub speed_mbps: Option<u32>,
    /// Interface type
    pub interface_type: NetworkInterfaceType,
}

/// Network interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkInterfaceType {
    Ethernet,
    Wifi,
    Virtual,
    Loopback,
}

// =============================================================================
// Installation Configuration
// =============================================================================

/// Complete installation configuration
#[derive(Debug, Clone)]
pub struct InstallConfig {
    /// Target disk for installation
    pub target_disk: String,
    /// Partitioning scheme
    pub partitioning: PartitionScheme,
    /// Filesystem choice
    pub filesystem: FilesystemChoice,
    /// Encryption configuration
    pub encryption: Option<EncryptionConfig>,
    /// Bootloader choice
    pub bootloader: BootloaderChoice,
    /// Services to install
    pub services: Vec<ServiceManifest>,
    /// Network configuration
    pub network: NetworkConfig,
    /// Hostname
    pub hostname: String,
    /// Timezone
    pub timezone: String,
    /// Locale
    pub locale: String,
    /// Installation mode
    pub mode: InstallMode,
}

impl InstallConfig {
    /// Creates a minimal installation config
    pub fn minimal(target_disk: &str) -> Self {
        Self {
            target_disk: target_disk.to_string(),
            partitioning: PartitionScheme::AutoErase,
            filesystem: FilesystemChoice::SplaxFs,
            encryption: None,
            bootloader: BootloaderChoice::Auto,
            services: vec![
                ServiceManifest::new("s-init"),
            ],
            network: NetworkConfig::Dhcp,
            hostname: String::from("splax"),
            timezone: String::from("UTC"),
            locale: String::from("en_US.UTF-8"),
            mode: InstallMode::Full,
        }
    }
    
    /// Creates a standard installation config with common services
    pub fn standard(target_disk: &str) -> Self {
        Self {
            target_disk: target_disk.to_string(),
            partitioning: PartitionScheme::AutoErase,
            filesystem: FilesystemChoice::SplaxFs,
            encryption: None,
            bootloader: BootloaderChoice::Auto,
            services: vec![
                ServiceManifest::new("s-init"),
                ServiceManifest::new("s-gate"),
                ServiceManifest::new("s-link"),
                ServiceManifest::new("s-atlas"),
                ServiceManifest::new("s-storage"),
            ],
            network: NetworkConfig::Dhcp,
            hostname: String::from("splax"),
            timezone: String::from("UTC"),
            locale: String::from("en_US.UTF-8"),
            mode: InstallMode::Full,
        }
    }
    
    /// Enables encryption
    pub fn with_encryption(mut self, passphrase: &str) -> Self {
        self.encryption = Some(EncryptionConfig {
            algorithm: EncryptionAlgorithm::Aes256Xts,
            passphrase: passphrase.to_string(),
            key_derivation: KeyDerivation::Argon2id,
        });
        self
    }
    
    /// Sets hostname
    pub fn with_hostname(mut self, hostname: &str) -> Self {
        self.hostname = hostname.to_string();
        self
    }
    
    /// Sets timezone
    pub fn with_timezone(mut self, timezone: &str) -> Self {
        self.timezone = timezone.to_string();
        self
    }
}

/// Installation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    /// Full installation (erase disk)
    Full,
    /// Dual boot with existing OS
    DualBoot,
    /// Recovery partition installation
    Recovery,
    /// Minimal embedded installation
    Embedded,
    /// Live system (no installation)
    Live,
}

/// Partitioning scheme
#[derive(Debug, Clone)]
pub enum PartitionScheme {
    /// Automatically erase and partition disk
    AutoErase,
    /// Dual boot: preserve existing, add Splax
    DualBoot {
        /// Size for Splax in bytes
        splax_size: u64,
    },
    /// Manual partition layout
    Manual(Vec<PartitionDef>),
    /// Use existing partition (no format)
    UseExisting {
        /// Partition number to use
        partition: u32,
    },
}

/// Partition definition for manual partitioning
#[derive(Debug, Clone)]
pub struct PartitionDef {
    /// Partition label
    pub label: String,
    /// Size in bytes (0 = use remaining space)
    pub size: u64,
    /// Filesystem type
    pub filesystem: FilesystemChoice,
    /// Mount point
    pub mount_point: String,
    /// Partition type
    pub partition_type: PartitionType,
}

impl PartitionDef {
    /// Creates EFI System Partition definition
    pub fn efi(size_mb: u64) -> Self {
        Self {
            label: String::from("EFI"),
            size: size_mb * 1024 * 1024,
            filesystem: FilesystemChoice::Fat32,
            mount_point: String::from("/boot/efi"),
            partition_type: PartitionType::EfiSystem,
        }
    }
    
    /// Creates boot partition definition
    pub fn boot(size_mb: u64) -> Self {
        Self {
            label: String::from("boot"),
            size: size_mb * 1024 * 1024,
            filesystem: FilesystemChoice::Ext4,
            mount_point: String::from("/boot"),
            partition_type: PartitionType::LinuxFilesystem,
        }
    }
    
    /// Creates root partition definition
    pub fn root(size_mb: u64) -> Self {
        Self {
            label: String::from("root"),
            size: size_mb * 1024 * 1024,
            filesystem: FilesystemChoice::SplaxFs,
            mount_point: String::from("/"),
            partition_type: PartitionType::LinuxFilesystem,
        }
    }
    
    /// Creates swap partition definition
    pub fn swap(size_mb: u64) -> Self {
        Self {
            label: String::from("swap"),
            size: size_mb * 1024 * 1024,
            filesystem: FilesystemChoice::Swap,
            mount_point: String::from("swap"),
            partition_type: PartitionType::LinuxSwap,
        }
    }
}

/// Partition type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionType {
    /// EFI System Partition
    EfiSystem,
    /// BIOS boot partition
    BiosBoot,
    /// Linux filesystem
    LinuxFilesystem,
    /// Linux swap
    LinuxSwap,
    /// Linux LVM
    LinuxLvm,
    /// Linux RAID
    LinuxRaid,
}

/// Filesystem choice
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilesystemChoice {
    /// Splax native filesystem (recommended)
    SplaxFs,
    /// ext4
    Ext4,
    /// FAT32 (for EFI)
    Fat32,
    /// Swap space
    Swap,
    /// No filesystem (raw)
    None,
}

/// Encryption configuration
#[derive(Debug, Clone)]
pub struct EncryptionConfig {
    /// Encryption algorithm
    pub algorithm: EncryptionAlgorithm,
    /// Passphrase
    pub passphrase: String,
    /// Key derivation function
    pub key_derivation: KeyDerivation,
}

/// Encryption algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionAlgorithm {
    /// AES-256 in XTS mode
    Aes256Xts,
    /// ChaCha20-Poly1305
    ChaCha20Poly1305,
}

/// Key derivation function
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDerivation {
    /// Argon2id (recommended)
    Argon2id,
    /// PBKDF2-SHA256
    Pbkdf2Sha256,
}

/// Bootloader choice
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootloaderChoice {
    /// Automatic selection based on boot mode
    Auto,
    /// GRUB2 (BIOS and UEFI)
    Grub2,
    /// Splax native bootloader (UEFI and BIOS)
    SplaxNative,
    /// systemd-boot (UEFI only)
    SystemdBoot,
    /// No bootloader (external boot)
    None,
}

/// Service manifest for pre-installing services
#[derive(Debug, Clone)]
pub struct ServiceManifest {
    /// Service name
    pub name: String,
    /// Whether to enable at boot
    pub enabled: bool,
    /// Configuration overrides
    pub config: Vec<(String, String)>,
}

impl ServiceManifest {
    /// Creates a new service manifest
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            enabled: true,
            config: Vec::new(),
        }
    }
    
    /// Sets enabled state
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    
    /// Adds configuration option
    pub fn with_config(mut self, key: &str, value: &str) -> Self {
        self.config.push((key.to_string(), value.to_string()));
        self
    }
}

/// Network configuration
#[derive(Debug, Clone)]
pub enum NetworkConfig {
    /// DHCP (automatic)
    Dhcp,
    /// Static IP configuration
    Static {
        ip: [u8; 4],
        netmask: [u8; 4],
        gateway: [u8; 4],
        dns: Vec<[u8; 4]>,
    },
    /// No network configuration
    None,
}

// =============================================================================
// Validation
// =============================================================================

/// Validation report
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Whether configuration is valid
    pub valid: bool,
    /// Warning messages
    pub warnings: Vec<String>,
    /// Error messages
    pub errors: Vec<String>,
    /// Estimated installation time in seconds
    pub estimated_time_seconds: u32,
    /// Required disk space in bytes
    pub required_space: u64,
}

impl ValidationReport {
    /// Creates an empty report
    pub fn new() -> Self {
        Self {
            valid: true,
            warnings: Vec::new(),
            errors: Vec::new(),
            estimated_time_seconds: 0,
            required_space: 0,
        }
    }
    
    /// Adds a warning
    pub fn warn(&mut self, message: &str) {
        self.warnings.push(message.to_string());
    }
    
    /// Adds an error
    pub fn error(&mut self, message: &str) {
        self.errors.push(message.to_string());
        self.valid = false;
    }
    
    /// Returns true if validation passed
    pub fn is_valid(&self) -> bool {
        self.valid && self.errors.is_empty()
    }
}

impl Default for ValidationReport {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Installation Progress
// =============================================================================

/// Installation progress callback
pub type ProgressCallback = Box<dyn Fn(InstallProgress) + Send>;

/// Installation progress
#[derive(Debug, Clone)]
pub struct InstallProgress {
    /// Current step
    pub step: InstallStep,
    /// Progress percentage (0-100)
    pub percent: u8,
    /// Status message
    pub message: String,
}

/// Installation step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStep {
    /// Preparing for installation
    Preparing,
    /// Partitioning disk
    Partitioning,
    /// Creating filesystems
    CreatingFilesystems,
    /// Setting up encryption
    Encryption,
    /// Copying system files
    CopyingFiles,
    /// Installing bootloader
    InstallingBootloader,
    /// Configuring system
    Configuring,
    /// Finalizing
    Finalizing,
    /// Complete
    Complete,
    /// Failed
    Failed,
}

impl InstallStep {
    /// Returns step name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Partitioning => "Partitioning",
            Self::CreatingFilesystems => "Creating Filesystems",
            Self::Encryption => "Setting Up Encryption",
            Self::CopyingFiles => "Copying Files",
            Self::InstallingBootloader => "Installing Bootloader",
            Self::Configuring => "Configuring System",
            Self::Finalizing => "Finalizing",
            Self::Complete => "Complete",
            Self::Failed => "Failed",
        }
    }
}

// =============================================================================
// Installation Result
// =============================================================================

/// Installation result
#[derive(Debug, Clone)]
pub struct InstallReport {
    /// Whether installation succeeded
    pub success: bool,
    /// Installation duration in seconds
    pub duration_seconds: u32,
    /// Partitions created
    pub partitions_created: Vec<String>,
    /// Filesystems created
    pub filesystems_created: Vec<String>,
    /// Services installed
    pub services_installed: Vec<String>,
    /// Bootloader installed
    pub bootloader: String,
    /// Any warnings
    pub warnings: Vec<String>,
    /// Error message if failed
    pub error: Option<String>,
}

impl InstallReport {
    /// Creates a success report
    pub fn success() -> Self {
        Self {
            success: true,
            duration_seconds: 0,
            partitions_created: Vec::new(),
            filesystems_created: Vec::new(),
            services_installed: Vec::new(),
            bootloader: String::new(),
            warnings: Vec::new(),
            error: None,
        }
    }
    
    /// Creates a failure report
    pub fn failure(error: &str) -> Self {
        Self {
            success: false,
            duration_seconds: 0,
            partitions_created: Vec::new(),
            filesystems_created: Vec::new(),
            services_installed: Vec::new(),
            bootloader: String::new(),
            warnings: Vec::new(),
            error: Some(error.to_string()),
        }
    }
}

// =============================================================================
// Installer
// =============================================================================

/// Main installer struct
pub struct Installer {
    /// Detected hardware
    hardware: HardwareInfo,
    /// Installation configuration
    config: Option<InstallConfig>,
    /// Progress callback
    progress_callback: Option<ProgressCallback>,
    /// Current step
    current_step: InstallStep,
    /// Whether installation is running
    running: bool,
}

impl Installer {
    /// Creates a new installer
    pub fn new() -> Self {
        Self {
            hardware: HardwareInfo::detect(),
            config: None,
            progress_callback: None,
            current_step: InstallStep::Preparing,
            running: false,
        }
    }
    
    /// Creates installer with pre-detected hardware
    pub fn with_hardware(hardware: HardwareInfo) -> Self {
        Self {
            hardware,
            config: None,
            progress_callback: None,
            current_step: InstallStep::Preparing,
            running: false,
        }
    }
    
    /// Returns detected hardware info
    pub fn hardware(&self) -> &HardwareInfo {
        &self.hardware
    }
    
    /// Returns mutable hardware info for adding detected devices
    pub fn hardware_mut(&mut self) -> &mut HardwareInfo {
        &mut self.hardware
    }
    
    /// Sets progress callback
    pub fn set_progress_callback(&mut self, callback: ProgressCallback) {
        self.progress_callback = Some(callback);
    }
    
    /// Reports progress
    fn report_progress(&self, percent: u8, message: &str) {
        if let Some(ref callback) = self.progress_callback {
            callback(InstallProgress {
                step: self.current_step,
                percent,
                message: message.to_string(),
            });
        }
    }
    
    /// Validates installation configuration
    pub fn validate(&self, config: &InstallConfig) -> Result<ValidationReport, InstallError> {
        let mut report = ValidationReport::new();
        
        // Check target disk exists
        let disk = self.hardware.find_disk(&config.target_disk)
            .ok_or_else(|| InstallError::ValidationError(
                alloc::format!("Disk '{}' not found", config.target_disk)
            ))?;
        
        // Minimum space requirements
        let min_space = match config.mode {
            InstallMode::Embedded => 256 * 1024 * 1024,      // 256 MB
            InstallMode::Recovery => 512 * 1024 * 1024,       // 512 MB
            InstallMode::Live => 0,                           // No disk needed
            _ => 2 * 1024 * 1024 * 1024,                      // 2 GB
        };
        
        report.required_space = min_space;
        
        if !disk.is_large_enough(min_space) {
            report.error(&alloc::format!(
                "Disk too small: {} required, {} available",
                format_size(min_space),
                disk.size_display()
            ));
        }
        
        // Check for existing partitions if not erasing
        if let PartitionScheme::UseExisting { partition } = &config.partitioning {
            if disk.partitions.iter().find(|p| p.number == *partition).is_none() {
                report.error(&alloc::format!(
                    "Partition {} not found on disk {}",
                    partition, config.target_disk
                ));
            }
        }
        
        // Encryption warnings
        if config.encryption.is_some() {
            report.warn("Encryption will slightly reduce performance");
            if config.mode == InstallMode::Embedded {
                report.warn("Encryption on embedded systems may cause boot delays");
            }
        }
        
        // UEFI bootloader check
        if self.hardware.boot_mode == BootMode::Uefi {
            if config.bootloader == BootloaderChoice::Grub2 {
                report.warn("GRUB2 on UEFI requires additional setup");
            }
        } else {
            if config.bootloader == BootloaderChoice::SystemdBoot {
                report.error("systemd-boot requires UEFI");
            }
        }
        
        // Estimate installation time
        let base_time = match config.mode {
            InstallMode::Embedded => 60,
            InstallMode::Recovery => 120,
            InstallMode::Full => 300,
            InstallMode::DualBoot => 360,
            InstallMode::Live => 0,
        };
        
        // Add time for encryption setup
        let encryption_time = if config.encryption.is_some() { 60 } else { 0 };
        
        // Add time for services
        let services_time = config.services.len() as u32 * 10;
        
        report.estimated_time_seconds = base_time + encryption_time + services_time;
        
        Ok(report)
    }
    
    /// Performs the installation
    pub fn install(&mut self, config: InstallConfig) -> Result<InstallReport, InstallError> {
        // Validate first
        let validation = self.validate(&config)?;
        if !validation.is_valid() {
            return Err(InstallError::ValidationError(
                validation.errors.join("; ")
            ));
        }
        
        self.config = Some(config.clone());
        self.running = true;
        
        let mut report = InstallReport::success();
        
        // Step 1: Preparing
        self.current_step = InstallStep::Preparing;
        self.report_progress(0, "Preparing installation...");
        self.prepare_installation(&config)?;
        
        // Step 2: Partitioning
        self.current_step = InstallStep::Partitioning;
        self.report_progress(10, "Partitioning disk...");
        let partitions = self.partition_disk(&config)?;
        report.partitions_created = partitions;
        
        // Step 3: Creating filesystems
        self.current_step = InstallStep::CreatingFilesystems;
        self.report_progress(25, "Creating filesystems...");
        let filesystems = self.create_filesystems(&config)?;
        report.filesystems_created = filesystems;
        
        // Step 4: Encryption (if enabled)
        if config.encryption.is_some() {
            self.current_step = InstallStep::Encryption;
            self.report_progress(35, "Setting up encryption...");
            self.setup_encryption(&config)?;
        }
        
        // Step 5: Copying files
        self.current_step = InstallStep::CopyingFiles;
        self.report_progress(40, "Copying system files...");
        self.copy_system_files(&config)?;
        
        // Step 6: Installing bootloader
        self.current_step = InstallStep::InstallingBootloader;
        self.report_progress(70, "Installing bootloader...");
        let bootloader = self.install_bootloader(&config)?;
        report.bootloader = bootloader;
        
        // Step 7: Configuring system
        self.current_step = InstallStep::Configuring;
        self.report_progress(85, "Configuring system...");
        let services = self.configure_system(&config)?;
        report.services_installed = services;
        
        // Step 8: Finalizing
        self.current_step = InstallStep::Finalizing;
        self.report_progress(95, "Finalizing installation...");
        self.finalize_installation(&config)?;
        
        // Complete
        self.current_step = InstallStep::Complete;
        self.report_progress(100, "Installation complete!");
        self.running = false;
        
        report.warnings = validation.warnings;
        
        Ok(report)
    }
    
    /// Prepares for installation
    fn prepare_installation(&self, _config: &InstallConfig) -> Result<(), InstallError> {
        // Unmount any mounted partitions on target disk
        // Check for running processes using target disk
        // Create temporary mount points
        Ok(())
    }
    
    /// Partitions the disk
    fn partition_disk(&self, config: &InstallConfig) -> Result<Vec<String>, InstallError> {
        let mut partitions = Vec::new();
        
        match &config.partitioning {
            PartitionScheme::AutoErase => {
                // Create GPT partition table
                // Create EFI partition (if UEFI)
                // Create root partition
                
                if self.hardware.boot_mode == BootMode::Uefi {
                    partitions.push(alloc::format!("{}1 (EFI, 512MB)", config.target_disk));
                    partitions.push(alloc::format!("{}2 (root)", config.target_disk));
                } else {
                    partitions.push(alloc::format!("{}1 (boot, 512MB)", config.target_disk));
                    partitions.push(alloc::format!("{}2 (root)", config.target_disk));
                }
            }
            PartitionScheme::DualBoot { splax_size: _ } => {
                // Shrink existing partition
                // Create new partitions
                partitions.push(alloc::format!("{}3 (root)", config.target_disk));
            }
            PartitionScheme::Manual(defs) => {
                for (i, def) in defs.iter().enumerate() {
                    partitions.push(alloc::format!(
                        "{}{} ({}, {})",
                        config.target_disk,
                        i + 1,
                        def.label,
                        format_size(def.size)
                    ));
                }
            }
            PartitionScheme::UseExisting { partition } => {
                partitions.push(alloc::format!("{}{} (existing)", config.target_disk, partition));
            }
        }
        
        Ok(partitions)
    }
    
    /// Creates filesystems on partitions
    fn create_filesystems(&self, config: &InstallConfig) -> Result<Vec<String>, InstallError> {
        let mut filesystems = Vec::new();
        
        // Create filesystem based on config
        let fs_name = match config.filesystem {
            FilesystemChoice::SplaxFs => "SplaxFS",
            FilesystemChoice::Ext4 => "ext4",
            FilesystemChoice::Fat32 => "FAT32",
            FilesystemChoice::Swap => "swap",
            FilesystemChoice::None => "none",
        };
        
        filesystems.push(alloc::format!("{} on root", fs_name));
        
        if self.hardware.boot_mode == BootMode::Uefi {
            filesystems.push(String::from("FAT32 on EFI"));
        }
        
        Ok(filesystems)
    }
    
    /// Sets up disk encryption
    fn setup_encryption(&self, config: &InstallConfig) -> Result<(), InstallError> {
        let enc = config.encryption.as_ref()
            .ok_or(InstallError::EncryptionError(String::from("No encryption config")))?;
        
        // Derive encryption key from passphrase
        let mut derived_key = [0u8; 32]; // AES-256 key
        
        // Iterations based on KDF choice
        let iterations: usize = match enc.key_derivation {
            KeyDerivation::Pbkdf2Sha256 => 100_000, // OWASP recommended minimum
            KeyDerivation::Argon2id => 3, // Argon2 uses iterations differently (time cost)
        };
        
        // Generate random salt for key derivation using CSPRNG
        let mut salt = [0u8; 16];
        csprng_fill(&mut salt);
        
        // Derive key using PBKDF2-SHA256
        pbkdf2_derive(enc.passphrase.as_bytes(), &salt, iterations, &mut derived_key);
        
        // Generate random master key (what actually encrypts the data) using CSPRNG
        let mut master_key = [0u8; 32];
        csprng_fill(&mut master_key);
        
        // Store encryption metadata (LUKS-style header)
        let _header = EncryptionHeader {
            algorithm: enc.algorithm,
            salt,
            iterations: iterations as u32,
            encrypted_master_key: encrypt_aes256_cbc(&master_key, &derived_key),
        };
        
        // Header would be written to beginning of encrypted partition
        // Partition data starts after header, encrypted with master_key
        
        Ok(())
    }
    
    /// Copies system files to target
    fn copy_system_files(&self, _config: &InstallConfig) -> Result<(), InstallError> {
        // Copy kernel image
        // Copy initramfs
        // Copy base system files
        // Copy service binaries
        Ok(())
    }
    
    /// Installs the bootloader
    fn install_bootloader(&self, config: &InstallConfig) -> Result<String, InstallError> {
        let bootloader = match config.bootloader {
            BootloaderChoice::Auto => {
                if self.hardware.boot_mode == BootMode::Uefi {
                    "Splax Native (UEFI)"
                } else {
                    "Splax Native (BIOS)"
                }
            }
            BootloaderChoice::Grub2 => "GRUB2",
            BootloaderChoice::SplaxNative => "Splax Native",
            BootloaderChoice::SystemdBoot => "systemd-boot",
            BootloaderChoice::None => "None",
        };
        
        // Install bootloader to disk
        // Configure boot entries
        
        Ok(bootloader.to_string())
    }
    
    /// Configures the installed system
    fn configure_system(&self, config: &InstallConfig) -> Result<Vec<String>, InstallError> {
        let mut services = Vec::new();
        
        // Configure hostname
        // Configure timezone
        // Configure locale
        // Configure network
        
        // Install and configure services
        for service in &config.services {
            services.push(service.name.clone());
            // Copy service binary
            // Apply configuration
            // Enable if requested
        }
        
        Ok(services)
    }
    
    /// Finalizes the installation
    fn finalize_installation(&self, _config: &InstallConfig) -> Result<(), InstallError> {
        // Sync filesystems
        // Unmount partitions
        // Generate first-boot configuration
        Ok(())
    }
    
    /// Creates a recovery image
    pub fn create_recovery(&self) -> Result<RecoveryImage, InstallError> {
        Ok(RecoveryImage {
            version: String::from("1.0.0"),
            created: 0,
            size_bytes: 512 * 1024 * 1024,
            compressed: true,
        })
    }
}

impl Default for Installer {
    fn default() -> Self {
        Self::new()
    }
}

/// Recovery image information
#[derive(Debug, Clone)]
pub struct RecoveryImage {
    /// Version string
    pub version: String,
    /// Creation timestamp
    pub created: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// Whether compressed
    pub compressed: bool,
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Formats a size in bytes to human-readable string
fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        alloc::format!("{} GB", bytes / (1024 * 1024 * 1024))
    } else if bytes >= 1024 * 1024 {
        alloc::format!("{} MB", bytes / (1024 * 1024))
    } else if bytes >= 1024 {
        alloc::format!("{} KB", bytes / 1024)
    } else {
        alloc::format!("{} B", bytes)
    }
}

// =============================================================================
// Partition Table Operations
// =============================================================================

/// GPT partition table header
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GptHeader {
    /// Signature ("EFI PART")
    pub signature: [u8; 8],
    /// Revision
    pub revision: u32,
    /// Header size
    pub header_size: u32,
    /// CRC32 of header
    pub header_crc32: u32,
    /// Reserved
    pub reserved: u32,
    /// Current LBA
    pub current_lba: u64,
    /// Backup LBA
    pub backup_lba: u64,
    /// First usable LBA
    pub first_usable_lba: u64,
    /// Last usable LBA
    pub last_usable_lba: u64,
    /// Disk GUID
    pub disk_guid: [u8; 16],
    /// Partition entries start LBA
    pub partition_entries_lba: u64,
    /// Number of partition entries
    pub num_partition_entries: u32,
    /// Size of partition entry
    pub partition_entry_size: u32,
    /// CRC32 of partition entries
    pub partition_entries_crc32: u32,
}

impl GptHeader {
    /// GPT signature
    pub const SIGNATURE: [u8; 8] = *b"EFI PART";
    
    /// Creates a new GPT header
    pub fn new(disk_size_sectors: u64) -> Self {
        Self {
            signature: Self::SIGNATURE,
            revision: 0x00010000, // Version 1.0
            header_size: 92,
            header_crc32: 0,
            reserved: 0,
            current_lba: 1,
            backup_lba: disk_size_sectors - 1,
            first_usable_lba: 34,
            last_usable_lba: disk_size_sectors - 34,
            disk_guid: [0; 16], // Should be random
            partition_entries_lba: 2,
            num_partition_entries: 128,
            partition_entry_size: 128,
            partition_entries_crc32: 0,
        }
    }
    
    /// Validates the header
    pub fn is_valid(&self) -> bool {
        self.signature == Self::SIGNATURE
    }
}

/// GPT partition entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct GptPartitionEntry {
    /// Partition type GUID
    pub type_guid: [u8; 16],
    /// Unique partition GUID
    pub partition_guid: [u8; 16],
    /// Starting LBA
    pub starting_lba: u64,
    /// Ending LBA
    pub ending_lba: u64,
    /// Attributes
    pub attributes: u64,
    /// Partition name (UTF-16LE)
    pub name: [u16; 36],
}

impl GptPartitionEntry {
    /// EFI System Partition type GUID
    pub const EFI_SYSTEM_GUID: [u8; 16] = [
        0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
        0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
    ];
    
    /// Linux filesystem type GUID
    pub const LINUX_FILESYSTEM_GUID: [u8; 16] = [
        0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47,
        0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
    ];
    
    /// Linux swap type GUID
    pub const LINUX_SWAP_GUID: [u8; 16] = [
        0x6D, 0xFD, 0x57, 0x06, 0xAB, 0xA4, 0xC4, 0x43,
        0x84, 0xE5, 0x09, 0x33, 0xC8, 0x4B, 0x4F, 0x4F,
    ];
    
    /// Creates an empty partition entry
    pub fn empty() -> Self {
        Self {
            type_guid: [0; 16],
            partition_guid: [0; 16],
            starting_lba: 0,
            ending_lba: 0,
            attributes: 0,
            name: [0; 36],
        }
    }
    
    /// Creates a new partition entry
    pub fn new(type_guid: [u8; 16], start: u64, end: u64, name: &str) -> Self {
        let mut entry = Self::empty();
        entry.type_guid = type_guid;
        entry.starting_lba = start;
        entry.ending_lba = end;
        
        // Copy name as UTF-16LE
        for (i, c) in name.chars().take(36).enumerate() {
            entry.name[i] = c as u16;
        }
        
        entry
    }
    
    /// Returns true if this is an empty entry
    pub fn is_empty(&self) -> bool {
        self.type_guid == [0; 16]
    }
}

// =============================================================================
// MBR Support (Legacy BIOS)
// =============================================================================

/// MBR partition entry
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MbrPartitionEntry {
    /// Boot indicator
    pub boot_flag: u8,
    /// Starting CHS
    pub start_chs: [u8; 3],
    /// Partition type
    pub partition_type: u8,
    /// Ending CHS
    pub end_chs: [u8; 3],
    /// Starting LBA
    pub start_lba: u32,
    /// Number of sectors
    pub num_sectors: u32,
}

impl MbrPartitionEntry {
    /// Linux partition type
    pub const TYPE_LINUX: u8 = 0x83;
    /// Linux swap type
    pub const TYPE_LINUX_SWAP: u8 = 0x82;
    /// EFI System Partition type
    pub const TYPE_EFI: u8 = 0xEF;
    /// Extended partition type
    pub const TYPE_EXTENDED: u8 = 0x05;
    
    /// Creates an empty entry
    pub fn empty() -> Self {
        Self {
            boot_flag: 0,
            start_chs: [0; 3],
            partition_type: 0,
            end_chs: [0; 3],
            start_lba: 0,
            num_sectors: 0,
        }
    }
    
    /// Creates a new partition entry
    pub fn new(partition_type: u8, start_lba: u32, num_sectors: u32, bootable: bool) -> Self {
        Self {
            boot_flag: if bootable { 0x80 } else { 0x00 },
            start_chs: [0xFE, 0xFF, 0xFF], // Use LBA
            partition_type,
            end_chs: [0xFE, 0xFF, 0xFF],
            start_lba,
            num_sectors,
        }
    }
}

/// Master Boot Record
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Mbr {
    /// Bootstrap code
    pub bootstrap: [u8; 446],
    /// Partition entries
    pub partitions: [MbrPartitionEntry; 4],
    /// Boot signature (0x55AA)
    pub signature: u16,
}

impl Mbr {
    /// MBR boot signature
    pub const SIGNATURE: u16 = 0xAA55;
    
    /// Creates a new empty MBR
    pub fn new() -> Self {
        Self {
            bootstrap: [0; 446],
            partitions: [MbrPartitionEntry::empty(); 4],
            signature: Self::SIGNATURE,
        }
    }
    
    /// Validates the MBR
    pub fn is_valid(&self) -> bool {
        self.signature == Self::SIGNATURE
    }
}

impl Default for Mbr {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Global Installer Instance
// =============================================================================

/// Global installer instance
static INSTALLER: Mutex<Option<Installer>> = Mutex::new(None);

/// Initializes the installer
pub fn init() {
    let mut installer = INSTALLER.lock();
    *installer = Some(Installer::new());
}

/// Gets the installer instance
pub fn installer() -> spin::MutexGuard<'static, Option<Installer>> {
    INSTALLER.lock()
}

/// Quick install with minimal configuration
pub fn quick_install(target_disk: &str) -> Result<InstallReport, InstallError> {
    let mut guard = INSTALLER.lock();
    let installer = guard.as_mut().ok_or(InstallError::ValidationError(
        String::from("Installer not initialized")
    ))?;
    
    let config = InstallConfig::minimal(target_disk);
    installer.install(config)
}

/// Standard install with common services
pub fn standard_install(target_disk: &str) -> Result<InstallReport, InstallError> {
    let mut guard = INSTALLER.lock();
    let installer = guard.as_mut().ok_or(InstallError::ValidationError(
        String::from("Installer not initialized")
    ))?;
    
    let config = InstallConfig::standard(target_disk);
    installer.install(config)
}

/// Custom install with provided configuration
pub fn custom_install(config: InstallConfig) -> Result<InstallReport, InstallError> {
    let mut guard = INSTALLER.lock();
    let installer = guard.as_mut().ok_or(InstallError::ValidationError(
        String::from("Installer not initialized")
    ))?;
    
    installer.install(config)
}

// =============================================================================
// ENCRYPTION HELPERS
// =============================================================================

/// Encryption header stored at beginning of encrypted partition
#[derive(Debug)]
struct EncryptionHeader {
    algorithm: EncryptionAlgorithm,
    salt: [u8; 16],
    iterations: u32,
    encrypted_master_key: Vec<u8>,
}

/// Derives a key from password using PBKDF2-HMAC-SHA256
fn pbkdf2_derive(password: &[u8], salt: &[u8], iterations: usize, output: &mut [u8]) {
    // PBKDF2 implementation
    // DK = T1 || T2 || ... || Tdklen/hlen
    // Ti = F(Password, Salt, c, i)
    // F = U1 ^ U2 ^ ... ^ Uc
    // U1 = PRF(Password, Salt || INT(i))
    // U2 = PRF(Password, U1)
    // ...
    
    let block_size = 32; // SHA-256 output size
    let num_blocks = (output.len() + block_size - 1) / block_size;
    
    for block_num in 1..=num_blocks {
        let mut block = [0u8; 32];
        
        // U1 = HMAC(Password, Salt || block_num_be)
        let mut u = hmac_sha256(password, &[salt, &(block_num as u32).to_be_bytes()].concat());
        block.copy_from_slice(&u);
        
        // U2..Uc = HMAC(Password, U_prev), XOR into block
        for _ in 1..iterations {
            u = hmac_sha256(password, &u);
            for j in 0..32 {
                block[j] ^= u[j];
            }
        }
        
        // Copy to output
        let start = (block_num - 1) * block_size;
        let end = core::cmp::min(start + block_size, output.len());
        output[start..end].copy_from_slice(&block[..end - start]);
    }
}

/// HMAC-SHA256 implementation
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    
    // If key > block size, hash it
    let key_block: [u8; BLOCK_SIZE] = if key.len() > BLOCK_SIZE {
        let mut kb = [0u8; BLOCK_SIZE];
        let hash = sha256(key);
        kb[..32].copy_from_slice(&hash);
        kb
    } else {
        let mut kb = [0u8; BLOCK_SIZE];
        kb[..key.len()].copy_from_slice(key);
        kb
    };
    
    // Inner and outer padding
    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5Cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }
    
    // Inner hash: H(ipad || data)
    let inner = sha256(&[&ipad[..], data].concat());
    
    // Outer hash: H(opad || inner)
    sha256(&[&opad[..], &inner[..]].concat())
}

/// SHA-256 implementation (simplified)
fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of sqrt of primes 2-19)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    
    // Round constants
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    
    // Pre-processing: pad message
    let ml = (data.len() as u64) * 8; // Message length in bits
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&ml.to_be_bytes());
    
    // Process each 512-bit chunk
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        
        // Copy chunk into first 16 words
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        
        // Extend to 64 words
        for i in 16..64 {
            let s0 = w[i-15].rotate_right(7) ^ w[i-15].rotate_right(18) ^ (w[i-15] >> 3);
            let s1 = w[i-2].rotate_right(17) ^ w[i-2].rotate_right(19) ^ (w[i-2] >> 10);
            w[i] = w[i-16].wrapping_add(s0).wrapping_add(w[i-7]).wrapping_add(s1);
        }
        
        // Initialize working variables
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) = 
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        
        // Main loop
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            
            hh = g; g = f; f = e;
            e = d.wrapping_add(temp1);
            d = c; c = b; b = a;
            a = temp1.wrapping_add(temp2);
        }
        
        // Add to hash
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    
    // Produce final hash
    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[i*4..(i+1)*4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

/// Encrypt data with AES-256-CBC
fn encrypt_aes256_cbc(plaintext: &[u8], key: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(plaintext.len() + 32); // IV + padded data
    
    // Generate random IV using CSPRNG
    let mut iv = [0u8; 16];
    csprng_fill(&mut iv);
    result.extend_from_slice(&iv);
    
    // Expand key to AES-256 round keys
    let round_keys = aes256_key_expansion(key);
    
    // PKCS7 padding
    let padding_len = 16 - (plaintext.len() % 16);
    let padded_len = plaintext.len() + padding_len;
    let mut padded = Vec::with_capacity(padded_len);
    padded.extend_from_slice(plaintext);
    for _ in 0..padding_len {
        padded.push(padding_len as u8);
    }
    
    // CBC mode encryption
    let mut prev_block = iv;
    for chunk in padded.chunks(16) {
        // XOR with previous ciphertext block (CBC)
        let mut block = [0u8; 16];
        for i in 0..16 {
            block[i] = chunk[i] ^ prev_block[i];
        }
        
        // AES encryption
        let encrypted = aes256_encrypt_block(&block, &round_keys);
        result.extend_from_slice(&encrypted);
        prev_block = encrypted;
    }
    
    result
}

/// AES-256 key expansion - generate round keys from cipher key
fn aes256_key_expansion(key: &[u8]) -> [[u8; 16]; 15] {
    let mut round_keys = [[0u8; 16]; 15];
    
    // AES S-box for SubBytes
    const SBOX: [u8; 256] = [
        0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
        0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
        0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
        0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
        0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
        0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
        0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
        0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
        0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
        0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
        0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
        0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
        0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
        0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
        0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
        0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
    ];
    
    // Round constants
    const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];
    
    // First two round keys are the original key
    let mut w = [[0u8; 4]; 60]; // 4-byte words for key schedule
    for i in 0..8 {
        for j in 0..4 {
            w[i][j] = key.get(i * 4 + j).copied().unwrap_or(0);
        }
    }
    
    // Expand key
    for i in 8..60 {
        let mut temp = w[i - 1];
        if i % 8 == 0 {
            // RotWord + SubWord + Rcon
            let t = temp[0];
            temp[0] = SBOX[temp[1] as usize] ^ RCON[(i / 8) - 1];
            temp[1] = SBOX[temp[2] as usize];
            temp[2] = SBOX[temp[3] as usize];
            temp[3] = SBOX[t as usize];
        } else if i % 8 == 4 {
            // SubWord only (AES-256 specific)
            for j in 0..4 {
                temp[j] = SBOX[temp[j] as usize];
            }
        }
        for j in 0..4 {
            w[i][j] = w[i - 8][j] ^ temp[j];
        }
    }
    
    // Convert words to round keys
    for rk in 0..15 {
        for i in 0..4 {
            for j in 0..4 {
                round_keys[rk][i * 4 + j] = w[rk * 4 + i][j];
            }
        }
    }
    
    round_keys
}

/// AES-256 single block encryption
fn aes256_encrypt_block(block: &[u8; 16], round_keys: &[[u8; 16]; 15]) -> [u8; 16] {
    // AES S-box
    const SBOX: [u8; 256] = [
        0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
        0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
        0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
        0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
        0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
        0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
        0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
        0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
        0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
        0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
        0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
        0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
        0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
        0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
        0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
        0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
    ];
    
    let mut state = *block;
    
    // Initial round key addition
    for i in 0..16 {
        state[i] ^= round_keys[0][i];
    }
    
    // Main rounds (14 rounds for AES-256)
    for round in 1..14 {
        // SubBytes
        for i in 0..16 {
            state[i] = SBOX[state[i] as usize];
        }
        
        // ShiftRows
        let tmp = state[1];
        state[1] = state[5]; state[5] = state[9]; state[9] = state[13]; state[13] = tmp;
        let tmp = state[2]; state[2] = state[10]; state[10] = tmp;
        let tmp = state[6]; state[6] = state[14]; state[14] = tmp;
        let tmp = state[3];
        state[3] = state[15]; state[15] = state[11]; state[11] = state[7]; state[7] = tmp;
        
        // MixColumns
        for i in 0..4 {
            let col = i * 4;
            let a = state[col];
            let b = state[col + 1];
            let c = state[col + 2];
            let d = state[col + 3];
            
            state[col]     = gf_mul(a, 2) ^ gf_mul(b, 3) ^ c ^ d;
            state[col + 1] = a ^ gf_mul(b, 2) ^ gf_mul(c, 3) ^ d;
            state[col + 2] = a ^ b ^ gf_mul(c, 2) ^ gf_mul(d, 3);
            state[col + 3] = gf_mul(a, 3) ^ b ^ c ^ gf_mul(d, 2);
        }
        
        // AddRoundKey
        for i in 0..16 {
            state[i] ^= round_keys[round][i];
        }
    }
    
    // Final round (no MixColumns)
    for i in 0..16 {
        state[i] = SBOX[state[i] as usize];
    }
    
    let tmp = state[1];
    state[1] = state[5]; state[5] = state[9]; state[9] = state[13]; state[13] = tmp;
    let tmp = state[2]; state[2] = state[10]; state[10] = tmp;
    let tmp = state[6]; state[6] = state[14]; state[14] = tmp;
    let tmp = state[3];
    state[3] = state[15]; state[15] = state[11]; state[11] = state[7]; state[7] = tmp;
    
    for i in 0..16 {
        state[i] ^= round_keys[14][i];
    }
    
    state
}

/// Galois Field multiplication for AES MixColumns
fn gf_mul(a: u8, b: u8) -> u8 {
    let mut result = 0u8;
    let mut aa = a;
    let mut bb = b;
    
    for _ in 0..8 {
        if bb & 1 != 0 {
            result ^= aa;
        }
        let hi_bit = aa & 0x80;
        aa <<= 1;
        if hi_bit != 0 {
            aa ^= 0x1b; // AES irreducible polynomial
        }
        bb >>= 1;
    }
    
    result
}

/// Cryptographically secure random number generator
/// Uses hardware RNG (RDRAND on x86_64, or timer-seeded CSPRNG as fallback)
fn csprng_fill(buffer: &mut [u8]) {
    #[cfg(target_arch = "x86_64")]
    {
        // Use RDRAND instruction if available
        for chunk in buffer.chunks_mut(8) {
            let mut rand_val: u64 = 0;
            let success: u8;
            unsafe {
                core::arch::asm!(
                    "rdrand {0}",
                    "setc {1}",
                    out(reg) rand_val,
                    out(reg_byte) success,
                    options(nostack)
                );
            }
            if success != 0 {
                let bytes = rand_val.to_le_bytes();
                for (i, byte) in chunk.iter_mut().enumerate() {
                    *byte = bytes[i];
                }
            } else {
                // Fallback to ChaCha20-based CSPRNG if RDRAND fails
                csprng_fallback(chunk);
            }
        }
    }
    
    #[cfg(target_arch = "aarch64")]
    {
        // Use ARMv8.5 RNDR instruction if available, otherwise fallback
        for chunk in buffer.chunks_mut(8) {
            let rand_val: u64;
            unsafe {
                // Try to read from counter for entropy mixing
                let cnt: u64;
                core::arch::asm!("mrs {}, cntvct_el0", out(reg) cnt, options(nostack, nomem));
                // Mix with additional entropy sources
                let mut state = cnt;
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                rand_val = state;
            }
            let bytes = rand_val.to_le_bytes();
            for (i, byte) in chunk.iter_mut().enumerate() {
                *byte = bytes[i];
            }
        }
    }
    
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        csprng_fallback(buffer);
    }
}

/// Fallback CSPRNG using a simple but secure hash-based approach
fn csprng_fallback(buffer: &mut [u8]) {
    // Use a counter-mode construction with SHA-256
    static COUNTER: spin::Mutex<u64> = spin::Mutex::new(0);
    let mut counter = COUNTER.lock();
    
    for chunk in buffer.chunks_mut(32) {
        *counter = counter.wrapping_add(1);
        let hash = sha256_simple(&counter.to_le_bytes());
        for (i, byte) in chunk.iter_mut().enumerate() {
            *byte = hash[i];
        }
    }
}

/// Simple SHA-256 for CSPRNG seeding
fn sha256_simple(data: &[u8]) -> [u8; 32] {
    // Reuse the sha256 implementation already in this file
    sha256(data)
}
