//! # Intermediate Representation
//!
//! This module defines the IR used by the JIT compiler. The IR is a
//! typed, SSA-form representation that's easier to optimize and lower
//! to machine code than raw WASM bytecode.

use alloc::string::String;
use alloc::vec::Vec;

use super::JitError;

pub mod optimize;

// =============================================================================
// Types
// =============================================================================

/// IR value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrType {
    /// 32-bit integer.
    I32,
    /// 64-bit integer.
    I64,
    /// 32-bit float.
    F32,
    /// 64-bit float.
    F64,
    /// Pointer/reference.
    Ptr,
    /// Void (no value).
    Void,
}

impl IrType {
    /// Size in bytes.
    pub fn size(&self) -> u32 {
        match self {
            IrType::I32 | IrType::F32 => 4,
            IrType::I64 | IrType::F64 | IrType::Ptr => 8,
            IrType::Void => 0,
        }
    }

    /// Check if this is a floating point type.
    pub fn is_float(&self) -> bool {
        matches!(self, IrType::F32 | IrType::F64)
    }
}

/// Virtual register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VReg(pub u32);

impl VReg {
    /// Create a new virtual register.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

// =============================================================================
// Instructions
// =============================================================================

/// IR instruction opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrOp {
    // Constants
    /// Load 32-bit immediate.
    Iconst32,
    /// Load 64-bit immediate.
    Iconst64,
    /// Load 32-bit float immediate.
    Fconst32,
    /// Load 64-bit float immediate.
    Fconst64,

    // Arithmetic (integer)
    /// Integer add.
    Iadd,
    /// Integer subtract.
    Isub,
    /// Integer multiply.
    Imul,
    /// Signed integer divide.
    Idiv,
    /// Unsigned integer divide.
    Udiv,
    /// Signed integer remainder.
    Irem,
    /// Unsigned integer remainder.
    Urem,

    // Bitwise
    /// Bitwise AND.
    And,
    /// Bitwise OR.
    Or,
    /// Bitwise XOR.
    Xor,
    /// Shift left.
    Shl,
    /// Signed shift right.
    Shr,
    /// Unsigned shift right.
    Ushr,
    /// Rotate left.
    Rotl,
    /// Rotate right.
    Rotr,

    // Unary integer
    /// Count leading zeros.
    Clz,
    /// Count trailing zeros.
    Ctz,
    /// Population count.
    Popcnt,

    // Floating point arithmetic
    /// Float add.
    Fadd,
    /// Float subtract.
    Fsub,
    /// Float multiply.
    Fmul,
    /// Float divide.
    Fdiv,
    /// Float minimum.
    Fmin,
    /// Float maximum.
    Fmax,
    /// Float square root.
    Fsqrt,
    /// Float absolute value.
    Fabs,
    /// Float negate.
    Fneg,
    /// Float ceiling.
    Fceil,
    /// Float floor.
    Ffloor,
    /// Float truncate.
    Ftrunc,
    /// Float nearest.
    Fnearest,

    // Comparison
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Signed less than.
    Lt,
    /// Unsigned less than.
    Ult,
    /// Signed less or equal.
    Le,
    /// Unsigned less or equal.
    Ule,
    /// Signed greater than.
    Gt,
    /// Unsigned greater than.
    Ugt,
    /// Signed greater or equal.
    Ge,
    /// Unsigned greater or equal.
    Uge,

    // Conversion
    /// Wrap i64 to i32.
    Wrap,
    /// Extend i32 to i64 (signed).
    Extend,
    /// Extend i32 to i64 (unsigned).
    Uextend,
    /// Truncate f32/f64 to i32 (signed).
    Trunc,
    /// Truncate f32/f64 to i32 (unsigned).
    Utrunc,
    /// Convert i32/i64 to f32/f64 (signed).
    Convert,
    /// Convert i32/i64 to f32/f64 (unsigned).
    Uconvert,
    /// Demote f64 to f32.
    Demote,
    /// Promote f32 to f64.
    Promote,
    /// Reinterpret bits.
    Reinterpret,

    // Memory
    /// Load from memory.
    Load,
    /// Store to memory.
    Store,

    // Control flow
    /// Unconditional jump.
    Jump,
    /// Conditional branch.
    BrIf,
    /// Switch/table branch.
    BrTable,
    /// Function call.
    Call,
    /// Indirect function call.
    CallIndirect,
    /// Return from function.
    Return,

    // Stack operations
    /// Copy value.
    Copy,
    /// Select (ternary).
    Select,

    // Special
    /// No operation.
    Nop,
    /// Unreachable code marker.
    Unreachable,
    /// Memory size query.
    MemorySize,
    /// Memory grow.
    MemoryGrow,
}

/// IR instruction.
#[derive(Debug, Clone)]
pub struct IrInst {
    /// Opcode.
    pub op: IrOp,
    /// Result type.
    pub result_type: IrType,
    /// Destination register (if any).
    pub dest: Option<VReg>,
    /// Source operands.
    pub operands: Vec<Operand>,
}

/// Instruction operand.
#[derive(Debug, Clone)]
pub enum Operand {
    /// Virtual register.
    Reg(VReg),
    /// 32-bit immediate.
    Imm32(i32),
    /// 64-bit immediate.
    Imm64(i64),
    /// 32-bit float immediate.
    F32(f32),
    /// 64-bit float immediate.
    F64(f64),
    /// Basic block label.
    Label(u32),
    /// Function index.
    FuncIdx(u32),
    /// Memory offset.
    Offset(u32),
}

impl IrInst {
    /// Create a new instruction.
    pub fn new(op: IrOp, result_type: IrType) -> Self {
        Self {
            op,
            result_type,
            dest: None,
            operands: Vec::new(),
        }
    }

    /// Set destination register.
    pub fn with_dest(mut self, dest: VReg) -> Self {
        self.dest = Some(dest);
        self
    }

    /// Add an operand.
    pub fn with_operand(mut self, op: Operand) -> Self {
        self.operands.push(op);
        self
    }

    /// Check if this instruction has side effects.
    pub fn has_side_effects(&self) -> bool {
        matches!(
            self.op,
            IrOp::Store
                | IrOp::Call
                | IrOp::CallIndirect
                | IrOp::MemoryGrow
                | IrOp::Unreachable
        )
    }

    /// Check if this is a terminator instruction.
    pub fn is_terminator(&self) -> bool {
        matches!(
            self.op,
            IrOp::Jump | IrOp::BrIf | IrOp::BrTable | IrOp::Return | IrOp::Unreachable
        )
    }
}

// =============================================================================
// Basic Blocks
// =============================================================================

/// Basic block.
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// Block ID.
    pub id: u32,
    /// Instructions in the block.
    pub instructions: Vec<IrInst>,
    /// Predecessor block IDs.
    pub predecessors: Vec<u32>,
    /// Successor block IDs.
    pub successors: Vec<u32>,
}

impl BasicBlock {
    /// Create a new basic block.
    pub fn new(id: u32) -> Self {
        Self {
            id,
            instructions: Vec::new(),
            predecessors: Vec::new(),
            successors: Vec::new(),
        }
    }

    /// Add an instruction.
    pub fn push(&mut self, inst: IrInst) {
        self.instructions.push(inst);
    }

    /// Get the terminator instruction.
    pub fn terminator(&self) -> Option<&IrInst> {
        self.instructions.last().filter(|i| i.is_terminator())
    }
}

// =============================================================================
// Function
// =============================================================================

/// IR function signature.
#[derive(Debug, Clone)]
pub struct FunctionSig {
    /// Parameter types.
    pub params: Vec<IrType>,
    /// Return types.
    pub returns: Vec<IrType>,
}

/// IR function.
#[derive(Debug, Clone)]
pub struct IrFunction {
    /// Function index.
    pub index: u32,
    /// Function name (if any).
    pub name: Option<String>,
    /// Signature.
    pub signature: FunctionSig,
    /// Local variable types.
    pub locals: Vec<IrType>,
    /// Basic blocks.
    pub blocks: Vec<BasicBlock>,
    /// Next virtual register ID.
    pub next_vreg: u32,
}

impl IrFunction {
    /// Create a new function.
    pub fn new(index: u32, signature: FunctionSig) -> Self {
        Self {
            index,
            name: None,
            signature,
            locals: Vec::new(),
            blocks: Vec::new(),
            next_vreg: 0,
        }
    }

    /// Allocate a new virtual register.
    pub fn alloc_vreg(&mut self) -> VReg {
        let vreg = VReg(self.next_vreg);
        self.next_vreg += 1;
        vreg
    }

    /// Add a basic block.
    pub fn add_block(&mut self) -> u32 {
        let id = self.blocks.len() as u32;
        self.blocks.push(BasicBlock::new(id));
        id
    }

    /// Get a block by ID.
    pub fn block(&self, id: u32) -> Option<&BasicBlock> {
        self.blocks.get(id as usize)
    }

    /// Get a mutable block by ID.
    pub fn block_mut(&mut self, id: u32) -> Option<&mut BasicBlock> {
        self.blocks.get_mut(id as usize)
    }

    /// Entry block.
    pub fn entry_block(&self) -> Option<&BasicBlock> {
        self.blocks.first()
    }
}

// =============================================================================
// Module
// =============================================================================

/// IR module.
#[derive(Debug, Clone)]
pub struct IrModule {
    /// Functions.
    pub functions: Vec<IrFunction>,
    /// Function signatures (type section).
    pub signatures: Vec<FunctionSig>,
    /// Imported functions count.
    pub num_imports: u32,
    /// Memory size (pages).
    pub memory_pages: u32,
    /// Global variables.
    pub globals: Vec<IrGlobal>,
}

/// IR global variable.
#[derive(Debug, Clone)]
pub struct IrGlobal {
    /// Type.
    pub ty: IrType,
    /// Mutable flag.
    pub mutable: bool,
    /// Initial value.
    pub init: i64,
}

impl IrModule {
    /// Create a new module.
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
            signatures: Vec::new(),
            num_imports: 0,
            memory_pages: 0,
            globals: Vec::new(),
        }
    }
}

impl Default for IrModule {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// IR Generation
// =============================================================================

/// Generate IR from WASM bytecode.
pub fn generate_ir(wasm: &[u8]) -> Result<IrModule, JitError> {
    let mut generator = IrGenerator::new(wasm);
    generator.generate()
}

/// IR generator state.
struct IrGenerator<'a> {
    wasm: &'a [u8],
    pos: usize,
    module: IrModule,
}

impl<'a> IrGenerator<'a> {
    fn new(wasm: &'a [u8]) -> Self {
        Self {
            wasm,
            pos: 0,
            module: IrModule::new(),
        }
    }

    fn generate(&mut self) -> Result<IrModule, JitError> {
        // Validate magic and version
        self.validate_header()?;

        // Parse sections
        while self.pos < self.wasm.len() {
            self.parse_section()?;
        }

        Ok(self.module.clone())
    }

    fn validate_header(&mut self) -> Result<(), JitError> {
        if self.wasm.len() < 8 {
            return Err(JitError::InvalidBytecode("Too short".into()));
        }

        // Magic: \0asm
        if &self.wasm[0..4] != b"\0asm" {
            return Err(JitError::InvalidBytecode("Invalid magic".into()));
        }

        // Version: 1
        if &self.wasm[4..8] != &[1, 0, 0, 0] {
            return Err(JitError::InvalidBytecode("Unsupported version".into()));
        }

        self.pos = 8;
        Ok(())
    }

    fn parse_section(&mut self) -> Result<(), JitError> {
        if self.pos >= self.wasm.len() {
            return Ok(());
        }

        let section_id = self.read_byte()?;
        let section_size = self.read_leb128_u32()? as usize;
        let section_end = self.pos + section_size;

        match section_id {
            1 => self.parse_type_section(section_end)?,
            2 => self.parse_import_section(section_end)?,
            3 => self.parse_function_section(section_end)?,
            5 => self.parse_memory_section(section_end)?,
            6 => self.parse_global_section(section_end)?,
            10 => self.parse_code_section(section_end)?,
            _ => {
                // Skip unknown sections
                self.pos = section_end;
            }
        }

        Ok(())
    }

    fn parse_type_section(&mut self, end: usize) -> Result<(), JitError> {
        let count = self.read_leb128_u32()?;

        for _ in 0..count {
            let form = self.read_byte()?;
            if form != 0x60 {
                return Err(JitError::InvalidBytecode("Expected func type".into()));
            }

            // Parameters
            let param_count = self.read_leb128_u32()?;
            let mut params = Vec::new();
            for _ in 0..param_count {
                params.push(self.read_valtype()?);
            }

            // Returns
            let return_count = self.read_leb128_u32()?;
            let mut returns = Vec::new();
            for _ in 0..return_count {
                returns.push(self.read_valtype()?);
            }

            self.module.signatures.push(FunctionSig { params, returns });
        }

        self.pos = end;
        Ok(())
    }

    fn parse_import_section(&mut self, end: usize) -> Result<(), JitError> {
        let count = self.read_leb128_u32()?;
        self.module.num_imports = count;

        // Skip import details for now
        self.pos = end;
        Ok(())
    }

    fn parse_function_section(&mut self, end: usize) -> Result<(), JitError> {
        let count = self.read_leb128_u32()?;

        for i in 0..count {
            let type_idx = self.read_leb128_u32()?;
            let sig = self.module.signatures.get(type_idx as usize)
                .cloned()
                .ok_or_else(|| JitError::InvalidBytecode("Invalid type index".into()))?;

            self.module.functions.push(IrFunction::new(
                self.module.num_imports + i,
                sig,
            ));
        }

        self.pos = end;
        Ok(())
    }

    fn parse_memory_section(&mut self, end: usize) -> Result<(), JitError> {
        let count = self.read_leb128_u32()?;

        if count > 0 {
            let flags = self.read_byte()?;
            let initial = self.read_leb128_u32()?;
            self.module.memory_pages = initial;

            if flags & 1 != 0 {
                let _max = self.read_leb128_u32()?;
            }
        }

        self.pos = end;
        Ok(())
    }

    fn parse_global_section(&mut self, end: usize) -> Result<(), JitError> {
        let count = self.read_leb128_u32()?;

        for _ in 0..count {
            let ty = self.read_valtype()?;
            let mutable = self.read_byte()? != 0;

            // Parse init expression (simplified)
            let init = self.parse_init_expr()?;

            self.module.globals.push(IrGlobal { ty, mutable, init });
        }

        self.pos = end;
        Ok(())
    }

    fn parse_init_expr(&mut self) -> Result<i64, JitError> {
        let opcode = self.read_byte()?;
        let value = match opcode {
            0x41 => self.read_leb128_i32()? as i64, // i32.const
            0x42 => self.read_leb128_i64()?,        // i64.const
            0x43 => {
                let bits = self.read_u32()?;
                f32::from_bits(bits) as i64
            }
            0x44 => {
                let bits = self.read_u64()?;
                f64::from_bits(bits) as i64
            }
            _ => 0,
        };

        // Read end opcode
        let end = self.read_byte()?;
        if end != 0x0B {
            return Err(JitError::InvalidBytecode("Expected end".into()));
        }

        Ok(value)
    }

    fn parse_code_section(&mut self, end: usize) -> Result<(), JitError> {
        let count = self.read_leb128_u32()?;

        for i in 0..count {
            let _func_size = self.read_leb128_u32()?;
            self.parse_function_body(i)?;
        }

        self.pos = end;
        Ok(())
    }

    fn parse_function_body(&mut self, func_idx: u32) -> Result<(), JitError> {
        // Parse locals first
        let local_count = self.read_leb128_u32()?;
        let mut local_types = Vec::new();
        for _ in 0..local_count {
            let count = self.read_leb128_u32()?;
            let ty = self.read_valtype()?;
            for _ in 0..count {
                local_types.push(ty);
            }
        }

        // Now get the function and add locals
        let func = self.module.functions.get_mut(func_idx as usize)
            .ok_or_else(|| JitError::InvalidBytecode("Invalid function index".into()))?;

        func.locals = local_types;

        // Create entry block
        let entry = func.add_block();
        drop(func); // Release the borrow

        // Parse instructions
        self.parse_instructions(func_idx, entry)?;

        Ok(())
    }

    fn parse_instructions(&mut self, func_idx: u32, block_id: u32) -> Result<(), JitError> {
        loop {
            let opcode = self.read_byte()?;

            match opcode {
                0x00 => {
                    // unreachable
                    self.emit(func_idx, block_id, IrInst::new(IrOp::Unreachable, IrType::Void));
                }
                0x01 => {
                    // nop
                    self.emit(func_idx, block_id, IrInst::new(IrOp::Nop, IrType::Void));
                }
                0x0B => {
                    // end
                    break;
                }
                0x0F => {
                    // return
                    self.emit(func_idx, block_id, IrInst::new(IrOp::Return, IrType::Void));
                }
                0x41 => {
                    // i32.const
                    let value = self.read_leb128_i32()?;
                    let func = self.module.functions.get_mut(func_idx as usize).unwrap();
                    let dest = func.alloc_vreg();
                    self.emit(
                        func_idx,
                        block_id,
                        IrInst::new(IrOp::Iconst32, IrType::I32)
                            .with_dest(dest)
                            .with_operand(Operand::Imm32(value)),
                    );
                }
                0x42 => {
                    // i64.const
                    let value = self.read_leb128_i64()?;
                    let func = self.module.functions.get_mut(func_idx as usize).unwrap();
                    let dest = func.alloc_vreg();
                    self.emit(
                        func_idx,
                        block_id,
                        IrInst::new(IrOp::Iconst64, IrType::I64)
                            .with_dest(dest)
                            .with_operand(Operand::Imm64(value)),
                    );
                }
                0x6A => {
                    // i32.add
                    self.emit_binary(func_idx, block_id, IrOp::Iadd, IrType::I32)?;
                }
                0x6B => {
                    // i32.sub
                    self.emit_binary(func_idx, block_id, IrOp::Isub, IrType::I32)?;
                }
                0x6C => {
                    // i32.mul
                    self.emit_binary(func_idx, block_id, IrOp::Imul, IrType::I32)?;
                }
                0x6D => {
                    // i32.div_s
                    self.emit_binary(func_idx, block_id, IrOp::Idiv, IrType::I32)?;
                }
                0x6E => {
                    // i32.div_u
                    self.emit_binary(func_idx, block_id, IrOp::Udiv, IrType::I32)?;
                }
                0x71 => {
                    // i32.and
                    self.emit_binary(func_idx, block_id, IrOp::And, IrType::I32)?;
                }
                0x72 => {
                    // i32.or
                    self.emit_binary(func_idx, block_id, IrOp::Or, IrType::I32)?;
                }
                0x73 => {
                    // i32.xor
                    self.emit_binary(func_idx, block_id, IrOp::Xor, IrType::I32)?;
                }
                _ => {
                    // Skip unsupported instructions for now
                }
            }
        }

        Ok(())
    }

    fn emit(&mut self, func_idx: u32, block_id: u32, inst: IrInst) {
        if let Some(func) = self.module.functions.get_mut(func_idx as usize) {
            if let Some(block) = func.blocks.get_mut(block_id as usize) {
                block.push(inst);
            }
        }
    }

    fn emit_binary(
        &mut self,
        func_idx: u32,
        block_id: u32,
        op: IrOp,
        ty: IrType,
    ) -> Result<(), JitError> {
        let func = self.module.functions.get_mut(func_idx as usize).unwrap();
        let dest = func.alloc_vreg();
        
        // In a real implementation, we'd track the operand stack
        // For now, use placeholder operands
        let inst = IrInst::new(op, ty)
            .with_dest(dest)
            .with_operand(Operand::Reg(VReg(0)))
            .with_operand(Operand::Reg(VReg(1)));

        self.emit(func_idx, block_id, inst);
        Ok(())
    }

    // Helper methods for reading WASM bytecode

    fn read_byte(&mut self) -> Result<u8, JitError> {
        if self.pos >= self.wasm.len() {
            return Err(JitError::InvalidBytecode("Unexpected end".into()));
        }
        let byte = self.wasm[self.pos];
        self.pos += 1;
        Ok(byte)
    }

    fn read_u32(&mut self) -> Result<u32, JitError> {
        if self.pos + 4 > self.wasm.len() {
            return Err(JitError::InvalidBytecode("Unexpected end".into()));
        }
        let value = u32::from_le_bytes([
            self.wasm[self.pos],
            self.wasm[self.pos + 1],
            self.wasm[self.pos + 2],
            self.wasm[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(value)
    }

    fn read_u64(&mut self) -> Result<u64, JitError> {
        if self.pos + 8 > self.wasm.len() {
            return Err(JitError::InvalidBytecode("Unexpected end".into()));
        }
        let value = u64::from_le_bytes([
            self.wasm[self.pos],
            self.wasm[self.pos + 1],
            self.wasm[self.pos + 2],
            self.wasm[self.pos + 3],
            self.wasm[self.pos + 4],
            self.wasm[self.pos + 5],
            self.wasm[self.pos + 6],
            self.wasm[self.pos + 7],
        ]);
        self.pos += 8;
        Ok(value)
    }

    fn read_leb128_u32(&mut self) -> Result<u32, JitError> {
        let mut result = 0u32;
        let mut shift = 0;

        loop {
            let byte = self.read_byte()?;
            result |= ((byte & 0x7f) as u32) << shift;

            if byte & 0x80 == 0 {
                break;
            }

            shift += 7;
            if shift >= 35 {
                return Err(JitError::InvalidBytecode("LEB128 overflow".into()));
            }
        }

        Ok(result)
    }

    fn read_leb128_i32(&mut self) -> Result<i32, JitError> {
        let mut result = 0i32;
        let mut shift = 0;
        let mut byte;

        loop {
            byte = self.read_byte()?;
            result |= ((byte & 0x7f) as i32) << shift;
            shift += 7;

            if byte & 0x80 == 0 {
                break;
            }

            if shift >= 35 {
                return Err(JitError::InvalidBytecode("LEB128 overflow".into()));
            }
        }

        // Sign extend
        if shift < 32 && (byte & 0x40) != 0 {
            result |= !0 << shift;
        }

        Ok(result)
    }

    fn read_leb128_i64(&mut self) -> Result<i64, JitError> {
        let mut result = 0i64;
        let mut shift = 0;
        let mut byte;

        loop {
            byte = self.read_byte()?;
            result |= ((byte & 0x7f) as i64) << shift;
            shift += 7;

            if byte & 0x80 == 0 {
                break;
            }

            if shift >= 70 {
                return Err(JitError::InvalidBytecode("LEB128 overflow".into()));
            }
        }

        // Sign extend
        if shift < 64 && (byte & 0x40) != 0 {
            result |= !0 << shift;
        }

        Ok(result)
    }

    fn read_valtype(&mut self) -> Result<IrType, JitError> {
        let byte = self.read_byte()?;
        match byte {
            0x7F => Ok(IrType::I32),
            0x7E => Ok(IrType::I64),
            0x7D => Ok(IrType::F32),
            0x7C => Ok(IrType::F64),
            _ => Err(JitError::InvalidBytecode("Invalid valtype".into())),
        }
    }
}
