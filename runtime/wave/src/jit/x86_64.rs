//! # x86_64 Code Generation
//!
//! This module generates native x86_64 machine code from allocated IR.

use alloc::vec::Vec;

use super::ir::{IrOp, IrType, Operand};
use super::regalloc::{AllocatedFunction, PhysReg, RegAlloc};
use super::{JitConfig, JitError};

// =============================================================================
// x86_64 Encoding Constants
// =============================================================================

/// REX prefix bits.
mod rex {
    pub const BASE: u8 = 0x40;
    pub const W: u8 = 0x08; // 64-bit operand
    pub const R: u8 = 0x04; // ModRM reg extension
    pub const X: u8 = 0x02; // SIB index extension
    pub const B: u8 = 0x01; // ModRM r/m or SIB base extension
}

/// ModR/M byte modes.
mod modrm {
    pub const INDIRECT: u8 = 0x00;
    pub const DISP8: u8 = 0x40;
    pub const DISP32: u8 = 0x80;
    pub const DIRECT: u8 = 0xC0;
}

// =============================================================================
// Code Buffer
// =============================================================================

/// Code buffer for emitting machine code.
pub struct CodeBuffer {
    code: Vec<u8>,
    /// Pending relocations.
    relocations: Vec<Relocation>,
    /// Label positions.
    labels: Vec<Option<u32>>,
}

/// Relocation entry.
#[derive(Debug, Clone)]
pub struct Relocation {
    /// Offset in code buffer.
    pub offset: u32,
    /// Target label.
    pub label: u32,
    /// Relocation kind.
    pub kind: RelocKind,
}

/// Relocation kinds.
#[derive(Debug, Clone, Copy)]
pub enum RelocKind {
    /// PC-relative 32-bit.
    Rel32,
    /// Absolute 64-bit.
    Abs64,
}

impl CodeBuffer {
    /// Create a new code buffer.
    pub fn new() -> Self {
        Self {
            code: Vec::with_capacity(4096),
            relocations: Vec::new(),
            labels: Vec::new(),
        }
    }

    /// Current position in the buffer.
    pub fn position(&self) -> u32 {
        self.code.len() as u32
    }

    /// Emit a single byte.
    pub fn emit_u8(&mut self, byte: u8) {
        self.code.push(byte);
    }

    /// Emit a 16-bit value.
    pub fn emit_u16(&mut self, value: u16) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 32-bit value.
    pub fn emit_u32(&mut self, value: u32) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 64-bit value.
    pub fn emit_u64(&mut self, value: u64) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit bytes.
    pub fn emit_bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    /// Create a new label.
    pub fn create_label(&mut self) -> u32 {
        let id = self.labels.len() as u32;
        self.labels.push(None);
        id
    }

    /// Bind a label to the current position.
    pub fn bind_label(&mut self, label: u32) {
        self.labels[label as usize] = Some(self.position());
    }

    /// Emit a relocation.
    pub fn emit_relocation(&mut self, label: u32, kind: RelocKind) {
        self.relocations.push(Relocation {
            offset: self.position(),
            label,
            kind,
        });

        // Emit placeholder
        match kind {
            RelocKind::Rel32 => self.emit_u32(0),
            RelocKind::Abs64 => self.emit_u64(0),
        }
    }

    /// Resolve all relocations.
    pub fn resolve_relocations(&mut self) {
        for reloc in &self.relocations {
            if let Some(target) = self.labels[reloc.label as usize] {
                match reloc.kind {
                    RelocKind::Rel32 => {
                        let offset = reloc.offset as usize;
                        let rel = (target as i32) - (offset as i32 + 4);
                        self.code[offset..offset + 4].copy_from_slice(&rel.to_le_bytes());
                    }
                    RelocKind::Abs64 => {
                        let offset = reloc.offset as usize;
                        self.code[offset..offset + 8].copy_from_slice(&target.to_le_bytes());
                    }
                }
            }
        }
    }

    /// Finish and return the generated code.
    pub fn finish(mut self) -> Vec<u8> {
        self.resolve_relocations();
        self.code
    }
}

impl Default for CodeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// x86_64 Instruction Encoding
// =============================================================================

/// x86_64 code emitter.
pub struct X86_64Emitter {
    buf: CodeBuffer,
}

impl X86_64Emitter {
    /// Create a new emitter.
    pub fn new() -> Self {
        Self {
            buf: CodeBuffer::new(),
        }
    }

    /// Get register encoding (low 3 bits).
    fn reg_enc(reg: PhysReg) -> u8 {
        reg.0 & 0x07
    }

    /// Check if register needs REX.B prefix.
    fn needs_rex_b(reg: PhysReg) -> bool {
        reg.0 >= 8
    }

    /// Check if register needs REX.R prefix.
    fn needs_rex_r(reg: PhysReg) -> bool {
        reg.0 >= 8
    }

    /// Emit REX prefix if needed.
    fn emit_rex(&mut self, w: bool, r: PhysReg, rm: PhysReg) {
        let mut rex_byte = 0u8;

        if w {
            rex_byte |= rex::W;
        }
        if Self::needs_rex_r(r) {
            rex_byte |= rex::R;
        }
        if Self::needs_rex_b(rm) {
            rex_byte |= rex::B;
        }

        if rex_byte != 0 || w {
            self.buf.emit_u8(rex::BASE | rex_byte);
        }
    }

    /// Emit ModR/M byte for register-to-register.
    fn emit_modrm_rr(&mut self, reg: PhysReg, rm: PhysReg) {
        self.buf.emit_u8(modrm::DIRECT | (Self::reg_enc(reg) << 3) | Self::reg_enc(rm));
    }

    /// Emit ModR/M byte for register and memory with displacement.
    fn emit_modrm_rm_disp(&mut self, reg: PhysReg, base: PhysReg, disp: i32) {
        if disp == 0 && Self::reg_enc(base) != 5 {
            // [base]
            self.buf.emit_u8(modrm::INDIRECT | (Self::reg_enc(reg) << 3) | Self::reg_enc(base));
        } else if disp >= -128 && disp <= 127 {
            // [base + disp8]
            self.buf.emit_u8(modrm::DISP8 | (Self::reg_enc(reg) << 3) | Self::reg_enc(base));
            self.buf.emit_u8(disp as u8);
        } else {
            // [base + disp32]
            self.buf.emit_u8(modrm::DISP32 | (Self::reg_enc(reg) << 3) | Self::reg_enc(base));
            self.buf.emit_u32(disp as u32);
        }
    }

    // =========================================================================
    // Instruction Emitters
    // =========================================================================

    /// PUSH reg
    pub fn push(&mut self, reg: PhysReg) {
        if Self::needs_rex_b(reg) {
            self.buf.emit_u8(rex::BASE | rex::B);
        }
        self.buf.emit_u8(0x50 + Self::reg_enc(reg));
    }

    /// POP reg
    pub fn pop(&mut self, reg: PhysReg) {
        if Self::needs_rex_b(reg) {
            self.buf.emit_u8(rex::BASE | rex::B);
        }
        self.buf.emit_u8(0x58 + Self::reg_enc(reg));
    }

    /// MOV reg, reg (64-bit)
    pub fn mov_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, src, dst);
        self.buf.emit_u8(0x89);
        self.emit_modrm_rr(src, dst);
    }

    /// MOV reg, imm32 (sign-extended to 64-bit)
    pub fn mov_ri32(&mut self, dst: PhysReg, imm: i32) {
        self.emit_rex(true, PhysReg(0), dst);
        self.buf.emit_u8(0xC7);
        self.emit_modrm_rr(PhysReg(0), dst);
        self.buf.emit_u32(imm as u32);
    }

    /// MOV reg, imm64
    pub fn mov_ri64(&mut self, dst: PhysReg, imm: i64) {
        self.emit_rex(true, PhysReg(0), dst);
        self.buf.emit_u8(0xB8 + Self::reg_enc(dst));
        self.buf.emit_u64(imm as u64);
    }

    /// MOV reg, [base + disp]
    pub fn mov_rm(&mut self, dst: PhysReg, base: PhysReg, disp: i32) {
        self.emit_rex(true, dst, base);
        self.buf.emit_u8(0x8B);
        self.emit_modrm_rm_disp(dst, base, disp);
    }

    /// MOV [base + disp], reg
    pub fn mov_mr(&mut self, base: PhysReg, disp: i32, src: PhysReg) {
        self.emit_rex(true, src, base);
        self.buf.emit_u8(0x89);
        self.emit_modrm_rm_disp(src, base, disp);
    }

    /// ADD reg, reg
    pub fn add_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, src, dst);
        self.buf.emit_u8(0x01);
        self.emit_modrm_rr(src, dst);
    }

    /// ADD reg, imm32
    pub fn add_ri(&mut self, dst: PhysReg, imm: i32) {
        self.emit_rex(true, PhysReg(0), dst);
        if imm >= -128 && imm <= 127 {
            self.buf.emit_u8(0x83);
            self.emit_modrm_rr(PhysReg(0), dst);
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81);
            self.emit_modrm_rr(PhysReg(0), dst);
            self.buf.emit_u32(imm as u32);
        }
    }

    /// SUB reg, reg
    pub fn sub_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, src, dst);
        self.buf.emit_u8(0x29);
        self.emit_modrm_rr(src, dst);
    }

    /// SUB reg, imm32
    pub fn sub_ri(&mut self, dst: PhysReg, imm: i32) {
        self.emit_rex(true, PhysReg(5), dst);
        if imm >= -128 && imm <= 127 {
            self.buf.emit_u8(0x83);
            self.emit_modrm_rr(PhysReg(5), dst);
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81);
            self.emit_modrm_rr(PhysReg(5), dst);
            self.buf.emit_u32(imm as u32);
        }
    }

    /// IMUL reg, reg
    pub fn imul_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, dst, src);
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0xAF);
        self.emit_modrm_rr(dst, src);
    }

    /// AND reg, reg
    pub fn and_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, src, dst);
        self.buf.emit_u8(0x21);
        self.emit_modrm_rr(src, dst);
    }

    /// OR reg, reg
    pub fn or_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, src, dst);
        self.buf.emit_u8(0x09);
        self.emit_modrm_rr(src, dst);
    }

    /// XOR reg, reg
    pub fn xor_rr(&mut self, dst: PhysReg, src: PhysReg) {
        self.emit_rex(true, src, dst);
        self.buf.emit_u8(0x31);
        self.emit_modrm_rr(src, dst);
    }

    /// SHL reg, imm8
    pub fn shl_ri(&mut self, dst: PhysReg, imm: u8) {
        self.emit_rex(true, PhysReg(4), dst);
        self.buf.emit_u8(0xC1);
        self.emit_modrm_rr(PhysReg(4), dst);
        self.buf.emit_u8(imm);
    }

    /// SHR reg, imm8
    pub fn shr_ri(&mut self, dst: PhysReg, imm: u8) {
        self.emit_rex(true, PhysReg(5), dst);
        self.buf.emit_u8(0xC1);
        self.emit_modrm_rr(PhysReg(5), dst);
        self.buf.emit_u8(imm);
    }

    /// SAR reg, imm8
    pub fn sar_ri(&mut self, dst: PhysReg, imm: u8) {
        self.emit_rex(true, PhysReg(7), dst);
        self.buf.emit_u8(0xC1);
        self.emit_modrm_rr(PhysReg(7), dst);
        self.buf.emit_u8(imm);
    }

    /// CMP reg, reg
    pub fn cmp_rr(&mut self, a: PhysReg, b: PhysReg) {
        self.emit_rex(true, b, a);
        self.buf.emit_u8(0x39);
        self.emit_modrm_rr(b, a);
    }

    /// CMP reg, imm32
    pub fn cmp_ri(&mut self, dst: PhysReg, imm: i32) {
        self.emit_rex(true, PhysReg(7), dst);
        if imm >= -128 && imm <= 127 {
            self.buf.emit_u8(0x83);
            self.emit_modrm_rr(PhysReg(7), dst);
            self.buf.emit_u8(imm as u8);
        } else {
            self.buf.emit_u8(0x81);
            self.emit_modrm_rr(PhysReg(7), dst);
            self.buf.emit_u32(imm as u32);
        }
    }

    /// JMP rel32
    pub fn jmp(&mut self, label: u32) {
        self.buf.emit_u8(0xE9);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// JE rel32
    pub fn je(&mut self, label: u32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x84);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// JNE rel32
    pub fn jne(&mut self, label: u32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x85);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// JL rel32 (signed less than)
    pub fn jl(&mut self, label: u32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x8C);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// JLE rel32 (signed less or equal)
    pub fn jle(&mut self, label: u32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x8E);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// JG rel32 (signed greater than)
    pub fn jg(&mut self, label: u32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x8F);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// JGE rel32 (signed greater or equal)
    pub fn jge(&mut self, label: u32) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x8D);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// CALL rel32
    pub fn call(&mut self, label: u32) {
        self.buf.emit_u8(0xE8);
        self.buf.emit_relocation(label, RelocKind::Rel32);
    }

    /// CALL reg (indirect)
    pub fn call_indirect(&mut self, reg: PhysReg) {
        if Self::needs_rex_b(reg) {
            self.buf.emit_u8(rex::BASE | rex::B);
        }
        self.buf.emit_u8(0xFF);
        self.emit_modrm_rr(PhysReg(2), reg);
    }

    /// RET
    pub fn ret(&mut self) {
        self.buf.emit_u8(0xC3);
    }

    /// NOP
    pub fn nop(&mut self) {
        self.buf.emit_u8(0x90);
    }

    /// INT3 (breakpoint)
    pub fn int3(&mut self) {
        self.buf.emit_u8(0xCC);
    }

    /// UD2 (undefined instruction - for unreachable)
    pub fn ud2(&mut self) {
        self.buf.emit_u8(0x0F);
        self.buf.emit_u8(0x0B);
    }

    /// Create a label.
    pub fn create_label(&mut self) -> u32 {
        self.buf.create_label()
    }

    /// Bind a label.
    pub fn bind_label(&mut self, label: u32) {
        self.buf.bind_label(label);
    }

    /// Finish and return generated code.
    pub fn finish(self) -> Vec<u8> {
        self.buf.finish()
    }
}

impl Default for X86_64Emitter {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Code Generation
// =============================================================================

/// Generate x86_64 code from an allocated function.
pub fn generate(func: &AllocatedFunction, _config: &JitConfig) -> Result<Vec<u8>, JitError> {
    let mut emitter = X86_64Emitter::new();

    // Emit prologue
    emit_prologue(&mut emitter, func);

    // Create labels for each block
    let block_labels: Vec<u32> = (0..func.ir.blocks.len())
        .map(|_| emitter.create_label())
        .collect();

    // Emit each block
    for (block_idx, block) in func.ir.blocks.iter().enumerate() {
        emitter.bind_label(block_labels[block_idx]);

        for inst in &block.instructions {
            emit_instruction(&mut emitter, inst, func, &block_labels)?;
        }
    }

    // Emit epilogue (if not already emitted by return)
    emit_epilogue(&mut emitter, func);

    Ok(emitter.finish())
}

/// Emit function prologue.
fn emit_prologue(emitter: &mut X86_64Emitter, func: &AllocatedFunction) {
    use super::regalloc::x86_64_regs::*;

    // Push callee-saved registers
    for &reg in &func.callee_saved {
        emitter.push(reg);
    }

    // Set up frame pointer
    emitter.push(RBP);
    emitter.mov_rr(RBP, RSP);

    // Allocate stack space
    if func.frame_size > 0 {
        emitter.sub_ri(RSP, func.frame_size as i32);
    }
}

/// Emit function epilogue.
fn emit_epilogue(emitter: &mut X86_64Emitter, func: &AllocatedFunction) {
    use super::regalloc::x86_64_regs::*;

    // Deallocate stack space
    if func.frame_size > 0 {
        emitter.add_ri(RSP, func.frame_size as i32);
    }

    // Restore frame pointer
    emitter.pop(RBP);

    // Pop callee-saved registers (reverse order)
    for &reg in func.callee_saved.iter().rev() {
        emitter.pop(reg);
    }

    emitter.ret();
}

/// Emit a single IR instruction.
fn emit_instruction(
    emitter: &mut X86_64Emitter,
    inst: &super::ir::IrInst,
    func: &AllocatedFunction,
    block_labels: &[u32],
) -> Result<(), JitError> {
    use super::regalloc::x86_64_regs::*;

    // Get destination register
    let dst = inst.dest.and_then(|vreg| {
        func.allocations.get(&vreg.0).and_then(|alloc| match alloc {
            RegAlloc::Reg(r) => Some(*r),
            RegAlloc::Spill(_) => None,
        })
    });

    match inst.op {
        IrOp::Nop => {
            emitter.nop();
        }

        IrOp::Unreachable => {
            emitter.ud2();
        }

        IrOp::Iconst32 => {
            if let (Some(dst), Some(Operand::Imm32(imm))) = (dst, inst.operands.first()) {
                emitter.mov_ri32(dst, *imm);
            }
        }

        IrOp::Iconst64 => {
            if let (Some(dst), Some(Operand::Imm64(imm))) = (dst, inst.operands.first()) {
                emitter.mov_ri64(dst, *imm);
            }
        }

        IrOp::Iadd => {
            if let Some(dst) = dst {
                emit_binary_op(emitter, func, &inst.operands, dst, |e, d, s| {
                    e.add_rr(d, s);
                })?;
            }
        }

        IrOp::Isub => {
            if let Some(dst) = dst {
                emit_binary_op(emitter, func, &inst.operands, dst, |e, d, s| {
                    e.sub_rr(d, s);
                })?;
            }
        }

        IrOp::Imul => {
            if let Some(dst) = dst {
                emit_binary_op(emitter, func, &inst.operands, dst, |e, d, s| {
                    e.imul_rr(d, s);
                })?;
            }
        }

        IrOp::And => {
            if let Some(dst) = dst {
                emit_binary_op(emitter, func, &inst.operands, dst, |e, d, s| {
                    e.and_rr(d, s);
                })?;
            }
        }

        IrOp::Or => {
            if let Some(dst) = dst {
                emit_binary_op(emitter, func, &inst.operands, dst, |e, d, s| {
                    e.or_rr(d, s);
                })?;
            }
        }

        IrOp::Xor => {
            if let Some(dst) = dst {
                emit_binary_op(emitter, func, &inst.operands, dst, |e, d, s| {
                    e.xor_rr(d, s);
                })?;
            }
        }

        IrOp::Shl => {
            if let (Some(dst), Some(Operand::Imm32(shift))) =
                (dst, inst.operands.get(1))
            {
                // Move first operand to dst
                if let Some(Operand::Reg(src)) = inst.operands.first() {
                    if let Some(RegAlloc::Reg(src_reg)) = func.allocations.get(&src.0) {
                        emitter.mov_rr(dst, *src_reg);
                    }
                }
                emitter.shl_ri(dst, *shift as u8);
            }
        }

        IrOp::Shr => {
            if let (Some(dst), Some(Operand::Imm32(shift))) =
                (dst, inst.operands.get(1))
            {
                if let Some(Operand::Reg(src)) = inst.operands.first() {
                    if let Some(RegAlloc::Reg(src_reg)) = func.allocations.get(&src.0) {
                        emitter.mov_rr(dst, *src_reg);
                    }
                }
                emitter.sar_ri(dst, *shift as u8);
            }
        }

        IrOp::Ushr => {
            if let (Some(dst), Some(Operand::Imm32(shift))) =
                (dst, inst.operands.get(1))
            {
                if let Some(Operand::Reg(src)) = inst.operands.first() {
                    if let Some(RegAlloc::Reg(src_reg)) = func.allocations.get(&src.0) {
                        emitter.mov_rr(dst, *src_reg);
                    }
                }
                emitter.shr_ri(dst, *shift as u8);
            }
        }

        IrOp::Jump => {
            if let Some(Operand::Label(target)) = inst.operands.first() {
                if let Some(&label) = block_labels.get(*target as usize) {
                    emitter.jmp(label);
                }
            }
        }

        IrOp::Return => {
            // Return value should be in RAX
            emit_epilogue(emitter, func);
        }

        IrOp::Copy => {
            if let (Some(dst), Some(Operand::Reg(src))) = (dst, inst.operands.first()) {
                if let Some(RegAlloc::Reg(src_reg)) = func.allocations.get(&src.0) {
                    if dst != *src_reg {
                        emitter.mov_rr(dst, *src_reg);
                    }
                }
            }
        }

        IrOp::Load => {
            if let (Some(dst), Some(Operand::Reg(base)), Some(Operand::Offset(offset))) =
                (dst, inst.operands.get(0), inst.operands.get(1))
            {
                if let Some(RegAlloc::Reg(base_reg)) = func.allocations.get(&base.0) {
                    emitter.mov_rm(dst, *base_reg, *offset as i32);
                }
            }
        }

        IrOp::Store => {
            if let (Some(Operand::Reg(base)), Some(Operand::Offset(offset)), Some(Operand::Reg(val))) =
                (inst.operands.get(0), inst.operands.get(1), inst.operands.get(2))
            {
                if let (Some(RegAlloc::Reg(base_reg)), Some(RegAlloc::Reg(val_reg))) =
                    (func.allocations.get(&base.0), func.allocations.get(&val.0))
                {
                    emitter.mov_mr(*base_reg, *offset as i32, *val_reg);
                }
            }
        }

        _ => {
            // Unsupported instruction - emit NOP as placeholder
            emitter.nop();
        }
    }

    Ok(())
}

/// Helper for binary operations.
fn emit_binary_op<F>(
    emitter: &mut X86_64Emitter,
    func: &AllocatedFunction,
    operands: &[Operand],
    dst: PhysReg,
    emit_fn: F,
) -> Result<(), JitError>
where
    F: FnOnce(&mut X86_64Emitter, PhysReg, PhysReg),
{
    let lhs = match operands.get(0) {
        Some(Operand::Reg(vreg)) => {
            func.allocations.get(&vreg.0).and_then(|a| match a {
                RegAlloc::Reg(r) => Some(*r),
                _ => None,
            })
        }
        _ => None,
    };

    let rhs = match operands.get(1) {
        Some(Operand::Reg(vreg)) => {
            func.allocations.get(&vreg.0).and_then(|a| match a {
                RegAlloc::Reg(r) => Some(*r),
                _ => None,
            })
        }
        _ => None,
    };

    if let (Some(lhs), Some(rhs)) = (lhs, rhs) {
        // Move lhs to dst if needed
        if dst != lhs {
            emitter.mov_rr(dst, lhs);
        }
        emit_fn(emitter, dst, rhs);
    }

    Ok(())
}
