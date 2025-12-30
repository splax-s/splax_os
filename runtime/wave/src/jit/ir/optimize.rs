//! # IR Optimization Passes
//!
//! This module implements optimization passes for the IR.

use super::{BasicBlock, IrFunction, IrInst, IrOp, Operand, VReg};
use alloc::collections::BTreeSet;

/// Constant folding optimization.
///
/// Evaluates constant expressions at compile time.
pub fn constant_fold(func: &mut IrFunction) {
    for block in &mut func.blocks {
        let mut i = 0;
        while i < block.instructions.len() {
            if let Some(folded) = try_fold_constant(&block.instructions[i]) {
                block.instructions[i] = folded;
            }
            i += 1;
        }
    }
}

/// Try to fold a constant expression.
fn try_fold_constant(inst: &IrInst) -> Option<IrInst> {
    // Check if all operands are immediate values
    let is_const_binary = inst.operands.len() == 2
        && matches!(inst.operands[0], Operand::Imm32(_) | Operand::Imm64(_))
        && matches!(inst.operands[1], Operand::Imm32(_) | Operand::Imm64(_));

    if !is_const_binary {
        return None;
    }

    // Extract 32-bit values
    let (a, b) = match (&inst.operands[0], &inst.operands[1]) {
        (Operand::Imm32(a), Operand::Imm32(b)) => (*a, *b),
        _ => return None,
    };

    let result = match inst.op {
        IrOp::Iadd => a.wrapping_add(b),
        IrOp::Isub => a.wrapping_sub(b),
        IrOp::Imul => a.wrapping_mul(b),
        IrOp::And => a & b,
        IrOp::Or => a | b,
        IrOp::Xor => a ^ b,
        IrOp::Shl => a.wrapping_shl(b as u32),
        IrOp::Shr => a.wrapping_shr(b as u32),
        _ => return None,
    };

    Some(
        IrInst::new(IrOp::Iconst32, inst.result_type)
            .with_dest(inst.dest?)
            .with_operand(Operand::Imm32(result)),
    )
}

/// Dead code elimination.
///
/// Removes instructions whose results are never used.
pub fn eliminate_dead_code(func: &mut IrFunction) {
    // Collect used registers
    let mut used_regs: BTreeSet<u32> = BTreeSet::new();

    // First pass: find all used registers
    for block in &func.blocks {
        for inst in &block.instructions {
            // Instructions with side effects are always live
            if inst.has_side_effects() || inst.is_terminator() {
                for op in &inst.operands {
                    if let Operand::Reg(vreg) = op {
                        used_regs.insert(vreg.0);
                    }
                }
            }

            // Collect operand uses
            for op in &inst.operands {
                if let Operand::Reg(vreg) = op {
                    used_regs.insert(vreg.0);
                }
            }
        }
    }

    // Second pass: remove dead instructions
    for block in &mut func.blocks {
        block.instructions.retain(|inst| {
            // Keep if has side effects or is terminator
            if inst.has_side_effects() || inst.is_terminator() {
                return true;
            }

            // Keep if result is used
            if let Some(dest) = inst.dest {
                return used_regs.contains(&dest.0);
            }

            // No destination and no side effects - dead
            false
        });
    }
}

/// Copy propagation.
///
/// Replaces uses of copies with the original value.
pub fn propagate_copies(func: &mut IrFunction) {
    use alloc::collections::BTreeMap;
    
    // Build copy map: dest -> source
    let mut copy_map: BTreeMap<u32, VReg> = BTreeMap::new();

    for block in &func.blocks {
        for inst in &block.instructions {
            if inst.op == IrOp::Copy {
                if let (Some(dest), Some(Operand::Reg(src))) =
                    (inst.dest, inst.operands.first())
                {
                    copy_map.insert(dest.0, *src);
                }
            }
        }
    }

    // Replace uses
    for block in &mut func.blocks {
        for inst in &mut block.instructions {
            for op in &mut inst.operands {
                if let Operand::Reg(vreg) = op {
                    // Follow copy chain
                    let mut current = vreg.0;
                    while let Some(&source) = copy_map.get(&current) {
                        current = source.0;
                    }
                    *vreg = VReg(current);
                }
            }
        }
    }
}

/// Strength reduction.
///
/// Replaces expensive operations with cheaper equivalents.
pub fn reduce_strength(func: &mut IrFunction) {
    for block in &mut func.blocks {
        for inst in &mut block.instructions {
            // Multiply by power of 2 -> shift
            if inst.op == IrOp::Imul {
                if let Some(Operand::Imm32(val)) = inst.operands.get(1) {
                    if is_power_of_two(*val) {
                        let shift = trailing_zeros(*val as u32) as i32;
                        inst.op = IrOp::Shl;
                        inst.operands[1] = Operand::Imm32(shift);
                    }
                }
            }

            // Divide by power of 2 -> shift (unsigned only)
            if inst.op == IrOp::Udiv {
                if let Some(Operand::Imm32(val)) = inst.operands.get(1) {
                    if is_power_of_two(*val) {
                        let shift = trailing_zeros(*val as u32) as i32;
                        inst.op = IrOp::Ushr;
                        inst.operands[1] = Operand::Imm32(shift);
                    }
                }
            }

            // x * 0 = 0
            if inst.op == IrOp::Imul {
                if matches!(inst.operands.get(1), Some(Operand::Imm32(0))) {
                    inst.op = IrOp::Iconst32;
                    inst.operands.clear();
                    inst.operands.push(Operand::Imm32(0));
                }
            }

            // x + 0 = x, x - 0 = x
            if matches!(inst.op, IrOp::Iadd | IrOp::Isub) {
                if matches!(inst.operands.get(1), Some(Operand::Imm32(0))) {
                    inst.op = IrOp::Copy;
                    inst.operands.pop();
                }
            }
        }
    }
}

/// Check if a number is a power of two.
fn is_power_of_two(n: i32) -> bool {
    n > 0 && (n & (n - 1)) == 0
}

/// Count trailing zeros.
fn trailing_zeros(n: u32) -> u32 {
    if n == 0 {
        return 32;
    }
    let mut count = 0;
    let mut val = n;
    while (val & 1) == 0 {
        count += 1;
        val >>= 1;
    }
    count
}

/// Algebraic simplification.
pub fn simplify_algebraic(func: &mut IrFunction) {
    for block in &mut func.blocks {
        for inst in &mut block.instructions {
            match inst.op {
                // x & 0 = 0
                IrOp::And if matches!(inst.operands.get(1), Some(Operand::Imm32(0))) => {
                    inst.op = IrOp::Iconst32;
                    inst.operands.clear();
                    inst.operands.push(Operand::Imm32(0));
                }
                // x | 0 = x
                IrOp::Or if matches!(inst.operands.get(1), Some(Operand::Imm32(0))) => {
                    inst.op = IrOp::Copy;
                    inst.operands.pop();
                }
                // x ^ 0 = x
                IrOp::Xor if matches!(inst.operands.get(1), Some(Operand::Imm32(0))) => {
                    inst.op = IrOp::Copy;
                    inst.operands.pop();
                }
                // x & -1 = x
                IrOp::And if matches!(inst.operands.get(1), Some(Operand::Imm32(-1))) => {
                    inst.op = IrOp::Copy;
                    inst.operands.pop();
                }
                // x | -1 = -1
                IrOp::Or if matches!(inst.operands.get(1), Some(Operand::Imm32(-1))) => {
                    inst.op = IrOp::Iconst32;
                    inst.operands.clear();
                    inst.operands.push(Operand::Imm32(-1));
                }
                _ => {}
            }
        }
    }
}

/// Run all optimization passes.
pub fn optimize_all(func: &mut IrFunction) {
    // Run passes in order
    constant_fold(func);
    propagate_copies(func);
    reduce_strength(func);
    simplify_algebraic(func);
    eliminate_dead_code(func);

    // Run again for additional opportunities
    constant_fold(func);
    eliminate_dead_code(func);
}
