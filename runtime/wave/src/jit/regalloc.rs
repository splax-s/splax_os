//! # Register Allocation
//!
//! This module implements a linear-scan register allocator for the JIT compiler.
//! It maps virtual registers to physical machine registers, spilling to the
//! stack when necessary.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::ir::{IrFunction, IrType, VReg};
use super::JitError;

// =============================================================================
// Physical Registers
// =============================================================================

/// Physical register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysReg(pub u8);

impl PhysReg {
    /// Check if this is a callee-saved register.
    pub fn is_callee_saved(&self) -> bool {
        // x86_64 callee-saved: RBX, RBP, R12-R15
        matches!(self.0, 3 | 5 | 12..=15)
    }

    /// Check if this is a caller-saved register.
    pub fn is_caller_saved(&self) -> bool {
        !self.is_callee_saved()
    }
}

/// x86_64 register names.
#[cfg(target_arch = "x86_64")]
pub mod x86_64_regs {
    use super::PhysReg;

    pub const RAX: PhysReg = PhysReg(0);
    pub const RCX: PhysReg = PhysReg(1);
    pub const RDX: PhysReg = PhysReg(2);
    pub const RBX: PhysReg = PhysReg(3);
    pub const RSP: PhysReg = PhysReg(4);  // Stack pointer (reserved)
    pub const RBP: PhysReg = PhysReg(5);  // Frame pointer (callee-saved)
    pub const RSI: PhysReg = PhysReg(6);
    pub const RDI: PhysReg = PhysReg(7);
    pub const R8: PhysReg = PhysReg(8);
    pub const R9: PhysReg = PhysReg(9);
    pub const R10: PhysReg = PhysReg(10);
    pub const R11: PhysReg = PhysReg(11);
    pub const R12: PhysReg = PhysReg(12);
    pub const R13: PhysReg = PhysReg(13);
    pub const R14: PhysReg = PhysReg(14);
    pub const R15: PhysReg = PhysReg(15);

    // XMM registers for floating point
    pub const XMM0: PhysReg = PhysReg(16);
    pub const XMM1: PhysReg = PhysReg(17);
    pub const XMM2: PhysReg = PhysReg(18);
    pub const XMM3: PhysReg = PhysReg(19);
    pub const XMM4: PhysReg = PhysReg(20);
    pub const XMM5: PhysReg = PhysReg(21);
    pub const XMM6: PhysReg = PhysReg(22);
    pub const XMM7: PhysReg = PhysReg(23);

    /// Allocatable general-purpose registers (excluding RSP, RBP).
    pub const GP_REGS: &[PhysReg] = &[
        RAX, RCX, RDX, RBX, RSI, RDI, R8, R9, R10, R11, R12, R13, R14, R15,
    ];

    /// Allocatable floating-point registers.
    pub const FP_REGS: &[PhysReg] = &[XMM0, XMM1, XMM2, XMM3, XMM4, XMM5, XMM6, XMM7];
}

// =============================================================================
// Live Interval
// =============================================================================

/// Live interval for a virtual register.
#[derive(Debug, Clone)]
pub struct LiveInterval {
    /// Virtual register.
    pub vreg: VReg,
    /// Start position (instruction index).
    pub start: u32,
    /// End position (instruction index).
    pub end: u32,
    /// Assigned physical register (if any).
    pub phys_reg: Option<PhysReg>,
    /// Spill slot (if spilled).
    pub spill_slot: Option<u32>,
    /// Register class.
    pub reg_class: RegClass,
}

impl LiveInterval {
    /// Create a new live interval.
    pub fn new(vreg: VReg, start: u32, end: u32, reg_class: RegClass) -> Self {
        Self {
            vreg,
            start,
            end,
            phys_reg: None,
            spill_slot: None,
            reg_class,
        }
    }

    /// Check if this interval overlaps another.
    pub fn overlaps(&self, other: &LiveInterval) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Check if this interval contains a position.
    pub fn contains(&self, pos: u32) -> bool {
        self.start <= pos && pos < self.end
    }
}

/// Register class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegClass {
    /// General-purpose integer register.
    GP,
    /// Floating-point register.
    FP,
}

impl From<IrType> for RegClass {
    fn from(ty: IrType) -> Self {
        match ty {
            IrType::F32 | IrType::F64 => RegClass::FP,
            _ => RegClass::GP,
        }
    }
}

// =============================================================================
// Allocation Result
// =============================================================================

/// Register allocation for a vreg.
#[derive(Debug, Clone, Copy)]
pub enum RegAlloc {
    /// Allocated to a physical register.
    Reg(PhysReg),
    /// Spilled to stack.
    Spill(u32),
}

/// Allocated function.
#[derive(Debug, Clone)]
pub struct AllocatedFunction {
    /// Original IR function.
    pub ir: IrFunction,
    /// Register allocations.
    pub allocations: BTreeMap<u32, RegAlloc>,
    /// Stack frame size.
    pub frame_size: u32,
    /// Callee-saved registers used.
    pub callee_saved: Vec<PhysReg>,
}

// =============================================================================
// Register Allocator
// =============================================================================

/// Linear-scan register allocator.
pub struct RegisterAllocator {
    /// Live intervals.
    intervals: Vec<LiveInterval>,
    /// Active intervals (sorted by end point).
    active: Vec<usize>,
    /// Free general-purpose registers.
    free_gp_regs: Vec<PhysReg>,
    /// Free floating-point registers.
    free_fp_regs: Vec<PhysReg>,
    /// Next spill slot.
    next_spill_slot: u32,
    /// Used callee-saved registers.
    used_callee_saved: Vec<PhysReg>,
}

impl RegisterAllocator {
    /// Create a new register allocator.
    pub fn new() -> Self {
        Self {
            intervals: Vec::new(),
            active: Vec::new(),
            free_gp_regs: Vec::new(),
            free_fp_regs: Vec::new(),
            next_spill_slot: 0,
            used_callee_saved: Vec::new(),
        }
    }

    /// Allocate registers for a function.
    pub fn allocate(&mut self, func: &IrFunction) -> Result<AllocatedFunction, JitError> {
        // Reset state
        self.reset();

        // Compute live intervals
        self.compute_live_intervals(func);

        // Sort intervals by start position
        self.intervals.sort_by_key(|i| i.start);

        // Linear scan allocation
        self.linear_scan()?;

        // Build allocation map
        let mut allocations = BTreeMap::new();
        for interval in &self.intervals {
            let alloc = if let Some(reg) = interval.phys_reg {
                RegAlloc::Reg(reg)
            } else if let Some(slot) = interval.spill_slot {
                RegAlloc::Spill(slot)
            } else {
                return Err(JitError::RegisterAllocationFailed);
            };
            allocations.insert(interval.vreg.0, alloc);
        }

        // Calculate frame size
        let frame_size = self.calculate_frame_size(func);

        Ok(AllocatedFunction {
            ir: func.clone(),
            allocations,
            frame_size,
            callee_saved: self.used_callee_saved.clone(),
        })
    }

    /// Reset allocator state.
    fn reset(&mut self) {
        self.intervals.clear();
        self.active.clear();
        self.next_spill_slot = 0;
        self.used_callee_saved.clear();

        // Initialize free register lists
        #[cfg(target_arch = "x86_64")]
        {
            self.free_gp_regs = x86_64_regs::GP_REGS.to_vec();
            self.free_fp_regs = x86_64_regs::FP_REGS.to_vec();
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            // Placeholder for other architectures
            self.free_gp_regs = (0..14).map(PhysReg).collect();
            self.free_fp_regs = (16..24).map(PhysReg).collect();
        }
    }

    /// Compute live intervals for all virtual registers.
    fn compute_live_intervals(&mut self, func: &IrFunction) {
        use alloc::collections::BTreeMap as Map;

        let mut live_ranges: Map<u32, (u32, u32, RegClass)> = Map::new();
        let mut pos = 0u32;

        for block in &func.blocks {
            for inst in &block.instructions {
                // Definition
                if let Some(dest) = inst.dest {
                    let class = RegClass::from(inst.result_type);
                    live_ranges
                        .entry(dest.0)
                        .or_insert((pos, pos + 1, class))
                        .0 = pos;
                }

                // Uses
                for op in &inst.operands {
                    if let super::ir::Operand::Reg(vreg) = op {
                        if let Some(range) = live_ranges.get_mut(&vreg.0) {
                            range.1 = pos + 1;
                        }
                    }
                }

                pos += 1;
            }
        }

        // Convert to intervals
        for (vreg_id, (start, end, class)) in live_ranges {
            self.intervals.push(LiveInterval::new(
                VReg(vreg_id),
                start,
                end,
                class,
            ));
        }
    }

    /// Perform linear scan register allocation.
    fn linear_scan(&mut self) -> Result<(), JitError> {
        for i in 0..self.intervals.len() {
            let interval_start = self.intervals[i].start;

            // Expire old intervals
            self.expire_old_intervals(interval_start);

            // Get register class
            let reg_class = self.intervals[i].reg_class;

            // Try to allocate a register
            let free_regs = match reg_class {
                RegClass::GP => &mut self.free_gp_regs,
                RegClass::FP => &mut self.free_fp_regs,
            };

            if let Some(reg) = free_regs.pop() {
                // Allocate register
                self.intervals[i].phys_reg = Some(reg);

                // Track callee-saved
                if reg.is_callee_saved() && !self.used_callee_saved.contains(&reg) {
                    self.used_callee_saved.push(reg);
                }

                // Add to active set
                self.active.push(i);
                self.active.sort_by_key(|&idx| self.intervals[idx].end);
            } else {
                // Spill
                self.spill_at_interval(i)?;
            }
        }

        Ok(())
    }

    /// Expire old intervals at position.
    fn expire_old_intervals(&mut self, pos: u32) {
        let mut expired = Vec::new();

        for (idx, &interval_idx) in self.active.iter().enumerate() {
            if self.intervals[interval_idx].end <= pos {
                expired.push(idx);

                // Return register to free pool
                if let Some(reg) = self.intervals[interval_idx].phys_reg {
                    let free_regs = match self.intervals[interval_idx].reg_class {
                        RegClass::GP => &mut self.free_gp_regs,
                        RegClass::FP => &mut self.free_fp_regs,
                    };
                    free_regs.push(reg);
                }
            }
        }

        // Remove expired intervals from active set
        for idx in expired.into_iter().rev() {
            self.active.remove(idx);
        }
    }

    /// Spill an interval.
    fn spill_at_interval(&mut self, i: usize) -> Result<(), JitError> {
        // Find the interval with the furthest end point
        if let Some(&spill_idx) = self.active.last() {
            if self.intervals[spill_idx].end > self.intervals[i].end {
                // Spill the current interval's would-be register holder
                let reg = self.intervals[spill_idx].phys_reg.take();
                self.intervals[i].phys_reg = reg;

                // Spill the old interval
                self.intervals[spill_idx].spill_slot = Some(self.allocate_spill_slot());

                // Update active set
                self.active.pop();
                self.active.push(i);
                self.active.sort_by_key(|&idx| self.intervals[idx].end);

                return Ok(());
            }
        }

        // Spill current interval
        self.intervals[i].spill_slot = Some(self.allocate_spill_slot());
        Ok(())
    }

    /// Allocate a spill slot.
    fn allocate_spill_slot(&mut self) -> u32 {
        let slot = self.next_spill_slot;
        self.next_spill_slot += 1;
        slot
    }

    /// Calculate stack frame size.
    fn calculate_frame_size(&self, func: &IrFunction) -> u32 {
        let mut size = 0u32;

        // Space for spilled registers
        size += self.next_spill_slot * 8;

        // Space for local variables
        for local_ty in &func.locals {
            size += local_ty.size();
        }

        // Space for callee-saved registers
        size += (self.used_callee_saved.len() as u32) * 8;

        // Align to 16 bytes
        (size + 15) & !15
    }
}

impl Default for RegisterAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Move Resolution
// =============================================================================

/// Represents a move between locations.
#[derive(Debug, Clone)]
pub struct Move {
    pub from: MoveLocation,
    pub to: MoveLocation,
}

/// Location for a move.
#[derive(Debug, Clone, Copy)]
pub enum MoveLocation {
    /// Physical register.
    Reg(PhysReg),
    /// Stack slot.
    Stack(u32),
}

/// Resolve moves to break cycles.
pub fn resolve_moves(moves: Vec<Move>) -> Vec<Move> {
    // Simple implementation: just return as-is
    // A full implementation would detect cycles and insert temporary moves
    moves
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_live_interval_overlap() {
        let a = LiveInterval::new(VReg(0), 0, 10, RegClass::GP);
        let b = LiveInterval::new(VReg(1), 5, 15, RegClass::GP);
        let c = LiveInterval::new(VReg(2), 15, 20, RegClass::GP);

        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
        assert!(!a.overlaps(&c));
        assert!(!c.overlaps(&a));
    }

    #[test]
    fn test_reg_class() {
        assert_eq!(RegClass::from(IrType::I32), RegClass::GP);
        assert_eq!(RegClass::from(IrType::I64), RegClass::GP);
        assert_eq!(RegClass::from(IrType::F32), RegClass::FP);
        assert_eq!(RegClass::from(IrType::F64), RegClass::FP);
    }
}
