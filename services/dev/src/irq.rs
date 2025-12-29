//! # IRQ Handling for S-DEV
//!
//! Interrupt forwarding from kernel to userspace drivers.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::DevError;

/// IRQ trigger mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqTrigger {
    /// Edge triggered
    Edge,
    /// Level triggered
    Level,
}

/// IRQ polarity
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqPolarity {
    /// Active high
    High,
    /// Active low
    Low,
}

/// IRQ flags
#[derive(Debug, Clone, Copy)]
pub struct IrqFlags {
    /// Trigger mode
    pub trigger: IrqTrigger,
    /// Polarity
    pub polarity: IrqPolarity,
    /// Shared IRQ
    pub shared: bool,
}

impl Default for IrqFlags {
    fn default() -> Self {
        Self {
            trigger: IrqTrigger::Level,
            polarity: IrqPolarity::High,
            shared: true,
        }
    }
}

/// IRQ handler entry
#[derive(Debug, Clone)]
pub struct IrqHandler {
    /// Handler name/ID
    pub name: String,
    /// Driver name
    pub driver: String,
    /// Device handle
    pub device: u64,
    /// IPC channel for notification
    pub channel: u64,
    /// Handler flags
    pub flags: IrqFlags,
    /// Enabled
    pub enabled: bool,
    /// Interrupt count
    pub count: u64,
    /// Last interrupt timestamp
    pub last_interrupt: u64,
}

/// IRQ line
#[derive(Debug)]
pub struct IrqLine {
    /// IRQ number
    pub irq: u32,
    /// Handlers registered to this IRQ
    pub handlers: Vec<IrqHandler>,
    /// Masked (disabled)
    pub masked: bool,
    /// Total interrupt count
    pub count: u64,
    /// Spurious count
    pub spurious: u64,
}

impl IrqLine {
    /// Creates a new IRQ line
    pub fn new(irq: u32) -> Self {
        Self {
            irq,
            handlers: Vec::new(),
            masked: false,
            count: 0,
            spurious: 0,
        }
    }

    /// Adds a handler
    pub fn add_handler(&mut self, handler: IrqHandler) -> Result<(), DevError> {
        // Check if shared
        if !self.handlers.is_empty() {
            if !handler.flags.shared {
                return Err(DevError::AlreadyBound);
            }
            // All handlers must be shared
            if self.handlers.iter().any(|h| !h.flags.shared) {
                return Err(DevError::AlreadyBound);
            }
        }
        self.handlers.push(handler);
        Ok(())
    }

    /// Removes a handler by name
    pub fn remove_handler(&mut self, name: &str) -> Option<IrqHandler> {
        if let Some(pos) = self.handlers.iter().position(|h| h.name == name) {
            Some(self.handlers.remove(pos))
        } else {
            None
        }
    }

    /// Handles an interrupt
    pub fn handle(&mut self, timestamp: u64) -> Vec<u64> {
        self.count += 1;
        
        let mut channels = Vec::new();
        for handler in &mut self.handlers {
            if handler.enabled {
                handler.count += 1;
                handler.last_interrupt = timestamp;
                channels.push(handler.channel);
            }
        }

        if channels.is_empty() {
            self.spurious += 1;
        }

        channels
    }

    /// Masks (disables) the IRQ
    pub fn mask(&mut self) {
        self.masked = true;
    }

    /// Unmasks (enables) the IRQ
    pub fn unmask(&mut self) {
        self.masked = false;
    }
}

/// MSI (Message Signaled Interrupt) entry
#[derive(Debug, Clone)]
pub struct MsiEntry {
    /// Device handle
    pub device: u64,
    /// Vector number
    pub vector: u32,
    /// Target CPU
    pub target_cpu: u32,
    /// Address for MSI
    pub address: u64,
    /// Data for MSI
    pub data: u32,
    /// Enabled
    pub enabled: bool,
    /// IPC channel
    pub channel: u64,
}

/// IRQ domain (for hierarchical interrupt handling)
#[derive(Debug)]
pub struct IrqDomain {
    /// Domain name
    pub name: String,
    /// IRQ base in this domain
    pub hwirq_base: u32,
    /// Number of IRQs
    pub count: u32,
    /// Mapping from hardware IRQ to Linux IRQ
    mapping: BTreeMap<u32, u32>,
}

impl IrqDomain {
    /// Creates a new IRQ domain
    pub fn new(name: &str, hwirq_base: u32, count: u32) -> Self {
        Self {
            name: String::from(name),
            hwirq_base,
            count,
            mapping: BTreeMap::new(),
        }
    }

    /// Maps a hardware IRQ to a Linux IRQ
    pub fn map(&mut self, hwirq: u32, virq: u32) -> Result<(), DevError> {
        if hwirq < self.hwirq_base || hwirq >= self.hwirq_base + self.count {
            return Err(DevError::InvalidArgument);
        }
        self.mapping.insert(hwirq, virq);
        Ok(())
    }

    /// Gets the virtual IRQ for a hardware IRQ
    pub fn get_virq(&self, hwirq: u32) -> Option<u32> {
        self.mapping.get(&hwirq).copied()
    }
}

/// IRQ affinity (CPU binding)
#[derive(Debug, Clone)]
pub struct IrqAffinity {
    /// CPU mask (bit per CPU)
    pub cpu_mask: u64,
    /// Preferred CPU (for automatic balancing)
    pub preferred_cpu: Option<u32>,
}

impl Default for IrqAffinity {
    fn default() -> Self {
        Self {
            cpu_mask: !0u64, // All CPUs
            preferred_cpu: None,
        }
    }
}

/// IRQ manager
pub struct IrqManager {
    /// IRQ lines
    lines: BTreeMap<u32, IrqLine>,
    /// MSI entries
    msi_entries: BTreeMap<(u64, u32), MsiEntry>,
    /// IRQ domains
    domains: BTreeMap<String, IrqDomain>,
    /// IRQ affinity settings
    affinity: BTreeMap<u32, IrqAffinity>,
    /// Next virtual IRQ
    next_virq: u32,
    /// Current timestamp (updated externally)
    current_time: u64,
    /// Global interrupt count
    total_interrupts: u64,
}

impl IrqManager {
    /// Creates a new IRQ manager
    pub fn new() -> Self {
        Self {
            lines: BTreeMap::new(),
            msi_entries: BTreeMap::new(),
            domains: BTreeMap::new(),
            affinity: BTreeMap::new(),
            next_virq: 256, // Start virtual IRQs after legacy IRQs
            current_time: 0,
            total_interrupts: 0,
        }
    }

    /// Registers an IRQ handler
    pub fn register_handler(
        &mut self,
        irq: u32,
        name: &str,
        driver: &str,
        device: u64,
        channel: u64,
        flags: IrqFlags,
    ) -> Result<(), DevError> {
        let line = self.lines.entry(irq).or_insert_with(|| IrqLine::new(irq));

        let handler = IrqHandler {
            name: String::from(name),
            driver: String::from(driver),
            device,
            channel,
            flags,
            enabled: true,
            count: 0,
            last_interrupt: 0,
        };

        line.add_handler(handler)
    }

    /// Unregisters an IRQ handler
    pub fn unregister_handler(&mut self, irq: u32, name: &str) -> Result<(), DevError> {
        let line = self.lines.get_mut(&irq).ok_or(DevError::DeviceNotFound)?;
        line.remove_handler(name).ok_or(DevError::DeviceNotFound)?;
        
        // Remove line if no handlers left
        if line.handlers.is_empty() {
            self.lines.remove(&irq);
        }
        
        Ok(())
    }

    /// Handles an interrupt from the kernel
    pub fn handle_interrupt(&mut self, irq: u32) -> Vec<u64> {
        self.total_interrupts += 1;
        
        if let Some(line) = self.lines.get_mut(&irq) {
            if line.masked {
                return Vec::new();
            }
            line.handle(self.current_time)
        } else {
            Vec::new()
        }
    }

    /// Masks an IRQ
    pub fn mask(&mut self, irq: u32) -> Result<(), DevError> {
        let line = self.lines.get_mut(&irq).ok_or(DevError::DeviceNotFound)?;
        line.mask();
        Ok(())
    }

    /// Unmasks an IRQ
    pub fn unmask(&mut self, irq: u32) -> Result<(), DevError> {
        let line = self.lines.get_mut(&irq).ok_or(DevError::DeviceNotFound)?;
        line.unmask();
        Ok(())
    }

    /// Allocates MSI vectors
    pub fn allocate_msi(
        &mut self,
        device: u64,
        count: u32,
        channel: u64,
    ) -> Result<Vec<u32>, DevError> {
        let mut vectors = Vec::new();

        for i in 0..count {
            let vector = self.next_virq;
            self.next_virq += 1;

            let entry = MsiEntry {
                device,
                vector,
                target_cpu: 0,
                address: 0xFEE00000, // Standard MSI address for x86
                data: vector,
                enabled: true,
                channel,
            };

            self.msi_entries.insert((device, i), entry);
            vectors.push(vector);
        }

        Ok(vectors)
    }

    /// Frees MSI vectors for a device
    pub fn free_msi(&mut self, device: u64) {
        self.msi_entries.retain(|(dev, _), _| *dev != device);
    }

    /// Gets MSI entry
    pub fn get_msi(&self, device: u64, index: u32) -> Option<&MsiEntry> {
        self.msi_entries.get(&(device, index))
    }

    /// Sets IRQ affinity
    pub fn set_affinity(&mut self, irq: u32, affinity: IrqAffinity) {
        self.affinity.insert(irq, affinity);
    }

    /// Gets IRQ affinity
    pub fn get_affinity(&self, irq: u32) -> IrqAffinity {
        self.affinity.get(&irq).cloned().unwrap_or_default()
    }

    /// Adds an IRQ domain
    pub fn add_domain(&mut self, name: &str, hwirq_base: u32, count: u32) {
        self.domains.insert(
            String::from(name),
            IrqDomain::new(name, hwirq_base, count),
        );
    }

    /// Gets a domain
    pub fn get_domain(&mut self, name: &str) -> Option<&mut IrqDomain> {
        self.domains.get_mut(name)
    }

    /// Updates current time
    pub fn update_time(&mut self, time: u64) {
        self.current_time = time;
    }

    /// Gets statistics
    pub fn stats(&self) -> IrqStats {
        let mut total_handlers = 0;
        let mut masked_count = 0;
        let mut spurious_count = 0;

        for line in self.lines.values() {
            total_handlers += line.handlers.len();
            if line.masked {
                masked_count += 1;
            }
            spurious_count += line.spurious;
        }

        IrqStats {
            total_lines: self.lines.len(),
            total_handlers,
            masked_count,
            msi_count: self.msi_entries.len(),
            total_interrupts: self.total_interrupts,
            spurious_interrupts: spurious_count,
        }
    }
}

/// IRQ statistics
#[derive(Debug, Clone)]
pub struct IrqStats {
    /// Total IRQ lines
    pub total_lines: usize,
    /// Total handlers
    pub total_handlers: usize,
    /// Masked IRQ count
    pub masked_count: usize,
    /// MSI entry count
    pub msi_count: usize,
    /// Total interrupts handled
    pub total_interrupts: u64,
    /// Spurious interrupts
    pub spurious_interrupts: u64,
}

impl Default for IrqManager {
    fn default() -> Self {
        Self::new()
    }
}
