//! # GIC (Generic Interrupt Controller) Driver
//!
//! Support for GICv2 and GICv3 interrupt controllers on ARM64.
//!
//! ## Architecture
//! - Distributor: Global interrupt routing
//! - CPU Interface: Per-CPU interrupt handling
//! - Redistributor (GICv3): Per-CPU configuration

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};

/// GIC Distributor registers (GICD).
#[repr(C)]
pub struct GicDistributor {
    /// Control register
    pub ctlr: u32,
    /// Type register
    pub typer: u32,
    /// Implementer ID
    pub iidr: u32,
    _reserved0: [u32; 29],
    /// Interrupt group registers
    pub igroupr: [u32; 32],
    /// Interrupt set-enable registers
    pub isenabler: [u32; 32],
    /// Interrupt clear-enable registers
    pub icenabler: [u32; 32],
    /// Interrupt set-pending registers
    pub ispendr: [u32; 32],
    /// Interrupt clear-pending registers
    pub icpendr: [u32; 32],
    /// Interrupt set-active registers
    pub isactiver: [u32; 32],
    /// Interrupt clear-active registers
    pub icactiver: [u32; 32],
    /// Interrupt priority registers
    pub ipriorityr: [u8; 1020],
    _reserved1: [u32; 1],
    /// Interrupt target registers
    pub itargetsr: [u8; 1020],
    _reserved2: [u32; 1],
    /// Interrupt configuration registers
    pub icfgr: [u32; 64],
}

/// GIC CPU Interface registers (GICC).
#[repr(C)]
pub struct GicCpuInterface {
    /// Control register
    pub ctlr: u32,
    /// Priority mask register
    pub pmr: u32,
    /// Binary point register
    pub bpr: u32,
    /// Interrupt acknowledge register
    pub iar: u32,
    /// End of interrupt register
    pub eoir: u32,
    /// Running priority register
    pub rpr: u32,
    /// Highest pending interrupt register
    pub hppir: u32,
    /// Aliased binary point register
    pub abpr: u32,
    /// Aliased interrupt acknowledge register
    pub aiar: u32,
    /// Aliased end of interrupt register
    pub aeoir: u32,
    /// Aliased highest pending interrupt register
    pub ahppir: u32,
}

/// GIC version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GicVersion {
    V2,
    V3,
}

/// GIC interrupt IDs.
pub mod irq {
    /// SGI (Software Generated Interrupt) range: 0-15
    pub const SGI_START: u32 = 0;
    pub const SGI_END: u32 = 15;
    
    /// PPI (Private Peripheral Interrupt) range: 16-31
    pub const PPI_START: u32 = 16;
    pub const PPI_END: u32 = 31;
    
    /// Generic timer (PPI 14 = IRQ 30)
    pub const TIMER: u32 = 30;
    
    /// SPI (Shared Peripheral Interrupt) range: 32+
    pub const SPI_START: u32 = 32;
    
    /// UART (commonly IRQ 33 on QEMU virt)
    pub const UART: u32 = 33;
    
    /// Spurious interrupt ID
    pub const SPURIOUS: u32 = 1023;
}

/// GIC state.
static mut GIC: Option<Gic> = None;

/// GIC driver.
pub struct Gic {
    version: GicVersion,
    distributor: *mut GicDistributor,
    cpu_interface: *mut GicCpuInterface,
    max_irqs: u32,
}

// SAFETY: GIC is only accessed from kernel mode with interrupts disabled during init
unsafe impl Send for Gic {}
unsafe impl Sync for Gic {}

impl Gic {
    /// QEMU virt machine GIC base addresses.
    const GICD_BASE_QEMU: u64 = 0x0800_0000;
    const GICC_BASE_QEMU: u64 = 0x0801_0000;
    
    /// Initialize the GIC.
    ///
    /// # Safety
    ///
    /// Must only be called once during boot.
    pub unsafe fn init() {
        let gicd = Self::GICD_BASE_QEMU as *mut GicDistributor;
        let gicc = Self::GICC_BASE_QEMU as *mut GicCpuInterface;
        
        // Read TYPER to get max IRQs
        let typer = unsafe { read_volatile(&(*gicd).typer) };
        let it_lines = (typer & 0x1F) + 1;
        let max_irqs = it_lines * 32;
        
        let gic = Gic {
            version: GicVersion::V2,
            distributor: gicd,
            cpu_interface: gicc,
            max_irqs,
        };
        
        gic.init_distributor();
        gic.init_cpu_interface();
        
        unsafe { GIC = Some(gic); }
    }
    
    /// Initialize the distributor.
    fn init_distributor(&self) {
        unsafe {
            let gicd = self.distributor;
            
            // Disable distributor
            write_volatile(&mut (*gicd).ctlr, 0);
            
            // Set all interrupts to Group 0
            for i in 0..(self.max_irqs / 32) as usize {
                write_volatile(&mut (*gicd).igroupr[i], 0);
            }
            
            // Disable all interrupts
            for i in 0..(self.max_irqs / 32) as usize {
                write_volatile(&mut (*gicd).icenabler[i], 0xFFFF_FFFF);
            }
            
            // Clear all pending interrupts
            for i in 0..(self.max_irqs / 32) as usize {
                write_volatile(&mut (*gicd).icpendr[i], 0xFFFF_FFFF);
            }
            
            // Set default priority (lower = higher priority)
            for i in 0..self.max_irqs as usize {
                write_volatile(&mut (*gicd).ipriorityr[i], 0xA0);
            }
            
            // Target all SPIs to CPU 0
            for i in irq::SPI_START as usize..self.max_irqs as usize {
                write_volatile(&mut (*gicd).itargetsr[i], 0x01);
            }
            
            // Configure all SPIs as level-triggered
            for i in 2..(self.max_irqs / 16) as usize {
                write_volatile(&mut (*gicd).icfgr[i], 0);
            }
            
            // Enable distributor
            write_volatile(&mut (*gicd).ctlr, 1);
        }
    }
    
    /// Initialize the CPU interface.
    fn init_cpu_interface(&self) {
        unsafe {
            let gicc = self.cpu_interface;
            
            // Set priority mask to allow all interrupts
            write_volatile(&mut (*gicc).pmr, 0xFF);
            
            // Set binary point to 0 (all bits for priority)
            write_volatile(&mut (*gicc).bpr, 0);
            
            // Enable CPU interface
            write_volatile(&mut (*gicc).ctlr, 1);
        }
    }
    
    /// Enable an interrupt.
    pub fn enable_irq(&self, irq: u32) {
        if irq >= self.max_irqs {
            return;
        }
        
        let reg = (irq / 32) as usize;
        let bit = 1u32 << (irq % 32);
        
        unsafe {
            write_volatile(&mut (*self.distributor).isenabler[reg], bit);
        }
    }
    
    /// Disable an interrupt.
    pub fn disable_irq(&self, irq: u32) {
        if irq >= self.max_irqs {
            return;
        }
        
        let reg = (irq / 32) as usize;
        let bit = 1u32 << (irq % 32);
        
        unsafe {
            write_volatile(&mut (*self.distributor).icenabler[reg], bit);
        }
    }
    
    /// Set interrupt priority.
    pub fn set_priority(&self, irq: u32, priority: u8) {
        if irq >= self.max_irqs {
            return;
        }
        
        unsafe {
            write_volatile(&mut (*self.distributor).ipriorityr[irq as usize], priority);
        }
    }
    
    /// Acknowledge an interrupt (read IAR).
    pub fn acknowledge(&self) -> u32 {
        unsafe {
            read_volatile(&(*self.cpu_interface).iar)
        }
    }
    
    /// Signal end of interrupt.
    pub fn end_of_interrupt(&self, irq: u32) {
        unsafe {
            write_volatile(&mut (*self.cpu_interface).eoir, irq);
        }
    }
    
    /// Send a software-generated interrupt (SGI) to a CPU.
    pub fn send_sgi(&self, target_cpu: u8, sgi_id: u8) {
        if sgi_id > 15 {
            return;
        }
        
        // GICD_SGIR format: target list in bits 16-23, SGI ID in bits 0-3
        let sgir = ((1u32 << target_cpu) << 16) | (sgi_id as u32);
        
        unsafe {
            // SGIR is at offset 0xF00 from GICD base
            let sgir_addr = (self.distributor as *mut u8).add(0xF00) as *mut u32;
            write_volatile(sgir_addr, sgir);
        }
    }
}

/// Get the GIC instance.
pub fn gic() -> &'static Gic {
    unsafe { GIC.as_ref().expect("GIC not initialized") }
}

/// Handle an IRQ from the exception handler.
pub fn handle_irq() {
    let gic = gic();
    
    let irq = gic.acknowledge();
    
    if irq == irq::SPURIOUS {
        return;
    }
    
    // Dispatch to appropriate handler
    match irq {
        irq::TIMER => {
            super::timer::handle_timer_irq();
        }
        irq::UART => {
            super::uart::handle_uart_irq();
        }
        _ => {
            // Unknown interrupt, just acknowledge
        }
    }
    
    gic.end_of_interrupt(irq);
}

/// Initialize the GIC.
///
/// # Safety
///
/// Must only be called once during boot.
pub unsafe fn init() {
    unsafe { Gic::init(); }
    
    // Enable timer and UART interrupts
    let gic = gic();
    gic.enable_irq(irq::TIMER);
    gic.enable_irq(irq::UART);
}
