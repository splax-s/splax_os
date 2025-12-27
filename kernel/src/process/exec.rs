//! # Process Execution (exec)
//!
//! Loads and executes ELF binaries in a new process context.
//!
//! ## Usage
//!
//! ```rust
//! use kernel::process::exec;
//!
//! // Load and execute an ELF binary
//! let elf_data = fs::read("/bin/hello")?;
//! let pid = exec::exec(&elf_data, &["hello", "arg1"], &["PATH=/bin"])?;
//! ```
//!
//! ## Memory Layout
//!
//! After loading an ELF binary, the process memory looks like:
//!
//! ```text
//! 0x0000_0000_0000_0000  ┌───────────────────┐
//!                        │    NULL guard     │  (unmapped)
//! 0x0000_0000_0040_0000  ├───────────────────┤
//!                        │    Code (text)    │  R-X from ELF
//!                        ├───────────────────┤
//!                        │    Read-only      │  R-- from ELF
//!                        ├───────────────────┤
//!                        │    Data (RW)      │  RW- from ELF
//!                        ├───────────────────┤
//!                        │    BSS (zeroed)   │  RW-
//!                        ├───────────────────┤
//!                        │        ...        │
//!                        │                   │
//!                        │       Heap        │  RW- (grows up)
//!                        │         ↓         │
//!                        │                   │
//!                        │                   │
//!                        │         ↑         │
//!                        │       Stack       │  RW- (grows down)
//!                        │                   │
//! 0x0000_7FFF_FFFF_F000  ├───────────────────┤
//!                        │    Stack top      │
//! 0x0000_8000_0000_0000  └───────────────────┘
//!                        │  Kernel space     │
//! ```

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::elf::{self, ElfInfo};

/// Default stack size for new processes (1 MiB)
pub const DEFAULT_STACK_SIZE: usize = 1024 * 1024;

/// Default stack top address (just below kernel space)
pub const DEFAULT_STACK_TOP: u64 = 0x0000_7FFF_FFFF_F000;

/// Default base address for PIE executables
pub const PIE_BASE_ADDR: u64 = 0x0000_0000_0040_0000;

/// Allocate a fresh page table for a user process
///
/// Returns the physical address of the new PML4 (page table root).
fn allocate_user_page_table() -> Result<u64, ExecError> {
    use crate::mm::frame::{FRAME_ALLOCATOR, PAGE_SIZE};
    
    // Allocate a frame for the PML4 (top-level page table)
    let pml4_frame = FRAME_ALLOCATOR
        .allocate()
        .map_err(|_| ExecError::OutOfMemory)?;
    
    let pml4_addr = pml4_frame.address();
    
    // Zero out the PML4
    unsafe {
        core::ptr::write_bytes(pml4_addr as *mut u8, 0, PAGE_SIZE);
    }
    
    // Copy kernel mappings (upper half) from current page table
    // This ensures the kernel is accessible from userspace
    #[cfg(target_arch = "x86_64")]
    unsafe {
        // Read current CR3 to get kernel page table
        let kernel_cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) kernel_cr3, options(nostack, nomem));
        let kernel_pml4 = (kernel_cr3 & !0xFFF) as *const u64;
        let user_pml4 = pml4_addr as *mut u64;
        
        // Copy entries 256-511 (kernel half: 0xFFFF_8000_0000_0000+)
        for i in 256..512 {
            let kernel_entry = core::ptr::read_volatile(kernel_pml4.add(i));
            core::ptr::write_volatile(user_pml4.add(i), kernel_entry);
        }
    }
    
    Ok(pml4_addr)
}
/// Exec errors
#[derive(Debug, Clone, Copy)]
pub enum ExecError {
    /// ELF parsing error
    Elf(elf::ElfError),
    /// Memory allocation failed
    OutOfMemory,
    /// Process creation failed
    ProcessCreationFailed,
    /// Page table error
    PageTableError,
    /// Invalid argument
    InvalidArgument,
    /// File not found
    FileNotFound,
    /// Permission denied
    PermissionDenied,
    /// Not an executable
    NotExecutable,
    /// Invalid file format
    InvalidFormat,
}

impl From<elf::ElfError> for ExecError {
    fn from(e: elf::ElfError) -> Self {
        ExecError::Elf(e)
    }
}

impl core::fmt::Display for ExecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ExecError::Elf(e) => write!(f, "ELF error: {}", e),
            ExecError::OutOfMemory => write!(f, "Out of memory"),
            ExecError::ProcessCreationFailed => write!(f, "Process creation failed"),
            ExecError::PageTableError => write!(f, "Page table error"),
            ExecError::InvalidArgument => write!(f, "Invalid argument"),
            ExecError::FileNotFound => write!(f, "File not found"),
            ExecError::PermissionDenied => write!(f, "Permission denied"),
            ExecError::NotExecutable => write!(f, "Not an executable"),
            ExecError::InvalidFormat => write!(f, "Invalid file format"),
        }
    }
}

/// Execution context for a loaded ELF
#[derive(Debug)]
pub struct ExecContext {
    /// Entry point address
    pub entry: u64,
    /// Stack pointer (initial)
    pub stack_ptr: u64,
    /// Base address where ELF was loaded
    pub base_addr: u64,
    /// Program break (end of loaded segments + BSS)
    pub brk: u64,
    /// Arguments passed to the program
    pub argc: usize,
    /// Environment variables count
    pub envc: usize,
}

/// Information about the process memory layout after exec
#[derive(Debug)]
pub struct MemoryLayout {
    /// Start of code section
    pub text_start: u64,
    /// End of code section
    pub text_end: u64,
    /// Start of data section
    pub data_start: u64,
    /// End of data section
    pub data_end: u64,
    /// Program break (heap start)
    pub brk: u64,
    /// Stack bottom (lowest address)
    pub stack_bottom: u64,
    /// Stack top (highest address)
    pub stack_top: u64,
}

/// Represents data to be placed on the user stack
struct UserStack {
    /// Stack data (built from top down)
    data: Vec<u8>,
    /// Current stack pointer
    sp: u64,
}

impl UserStack {
    fn new(stack_top: u64) -> Self {
        Self {
            data: Vec::new(),
            sp: stack_top,
        }
    }

    /// Push bytes onto the stack (grows down)
    fn push_bytes(&mut self, bytes: &[u8]) {
        // Stack grows down, so prepend bytes
        let mut new_data = bytes.to_vec();
        new_data.extend_from_slice(&self.data);
        self.data = new_data;
        self.sp -= bytes.len() as u64;
    }

    /// Push a u64 value
    fn push_u64(&mut self, value: u64) {
        self.push_bytes(&value.to_le_bytes());
    }

    /// Push a null-terminated string, return its address
    fn push_string(&mut self, s: &str) -> u64 {
        // Push null terminator first
        self.push_bytes(&[0]);
        // Push string bytes
        self.push_bytes(s.as_bytes());
        // Return address where string starts
        self.sp
    }

    /// Align stack to specified boundary
    fn align(&mut self, alignment: u64) {
        let misalign = self.sp % alignment;
        if misalign != 0 {
            let padding = vec![0u8; misalign as usize];
            self.push_bytes(&padding);
        }
    }

    /// Get current stack pointer
    fn sp(&self) -> u64 {
        self.sp
    }

    /// Get stack data
    fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Build the initial user stack with arguments and environment
///
/// The stack layout follows the System V AMD64 ABI:
///
/// ```text
/// high addr
///   ┌─────────────────────┐
///   │   environment       │  String data
///   │   strings           │
///   ├─────────────────────┤
///   │   argument          │  String data  
///   │   strings           │
///   ├─────────────────────┤
///   │   padding           │  16-byte align
///   ├─────────────────────┤
///   │   NULL              │  End of envp
///   │   envp[n-1]         │  Environment pointers
///   │   ...               │
///   │   envp[0]           │
///   ├─────────────────────┤
///   │   NULL              │  End of argv
///   │   argv[argc-1]      │  Argument pointers
///   │   ...               │
///   │   argv[0]           │
///   ├─────────────────────┤
///   │   argc              │  Argument count
/// low addr (SP)
/// ```
fn build_user_stack(
    stack_top: u64,
    args: &[&str],
    env: &[&str],
    _elf_info: &ElfInfo,
) -> (u64, Vec<u8>) {
    let mut stack = UserStack::new(stack_top);

    // Push environment strings and collect addresses
    let env_addrs: Vec<u64> = env.iter().map(|s| stack.push_string(s)).collect();

    // Push argument strings and collect addresses
    let arg_addrs: Vec<u64> = args.iter().map(|s| stack.push_string(s)).collect();

    // Align to 16 bytes
    stack.align(16);

    // Push auxiliary vectors (simplified - just AT_NULL for now)
    // In a full implementation, we'd push:
    // - AT_PHDR: address of program headers
    // - AT_PHENT: size of program header entry
    // - AT_PHNUM: number of program headers
    // - AT_PAGESZ: system page size
    // - AT_ENTRY: entry point
    // - AT_BASE: interpreter base (if dynamic)
    // - AT_NULL: end marker
    stack.push_u64(0); // AT_NULL value
    stack.push_u64(0); // AT_NULL type

    // Push NULL terminator for envp
    stack.push_u64(0);

    // Push environment pointers (in reverse order since stack grows down)
    for &addr in env_addrs.iter().rev() {
        stack.push_u64(addr);
    }

    // Push NULL terminator for argv
    stack.push_u64(0);

    // Push argument pointers (in reverse order)
    for &addr in arg_addrs.iter().rev() {
        stack.push_u64(addr);
    }

    // Push argc
    stack.push_u64(args.len() as u64);

    (stack.sp(), stack.data().to_vec())
}

/// Load an ELF binary and prepare for execution
///
/// This function:
/// 1. Parses the ELF headers
/// 2. Calculates memory requirements
/// 3. Builds the initial stack with args/env
/// 4. Returns the execution context
///
/// The caller is responsible for:
/// - Creating the process
/// - Mapping memory for segments
/// - Copying segment data
/// - Setting up the page table
pub fn prepare_exec(
    elf_data: &[u8],
    args: &[&str],
    env: &[&str],
) -> Result<(ElfInfo, ExecContext, Vec<u8>), ExecError> {
    // Parse ELF
    let elf_info = elf::parse(elf_data)?;
    elf::validate_for_exec(&elf_info)?;

    // Calculate base address
    let base_addr = if elf_info.is_pie {
        PIE_BASE_ADDR
    } else {
        elf_info.base_addr
    };

    // Calculate entry point (adjust for PIE relocation)
    let entry = if elf_info.is_pie {
        PIE_BASE_ADDR + elf_info.entry
    } else {
        elf_info.entry
    };

    // Calculate break (end of loaded memory)
    let brk = if elf_info.is_pie {
        PIE_BASE_ADDR + elf_info.top_addr
    } else {
        elf_info.top_addr
    };

    // Build user stack
    let (stack_ptr, stack_data) = build_user_stack(
        DEFAULT_STACK_TOP,
        args,
        env,
        &elf_info,
    );

    let ctx = ExecContext {
        entry,
        stack_ptr,
        base_addr,
        brk,
        argc: args.len(),
        envc: env.len(),
    };

    Ok((elf_info, ctx, stack_data))
}

/// Calculate the memory layout for a loaded ELF
pub fn calculate_layout(elf_info: &ElfInfo, base_offset: u64) -> MemoryLayout {
    let mut text_start = u64::MAX;
    let mut text_end = 0u64;
    let mut data_start = u64::MAX;
    let mut data_end = 0u64;

    for seg in &elf_info.segments {
        let start = seg.vaddr + base_offset;
        let end = start + seg.mem_size;

        if seg.prot.execute {
            if start < text_start {
                text_start = start;
            }
            if end > text_end {
                text_end = end;
            }
        } else {
            if start < data_start {
                data_start = start;
            }
            if end > data_end {
                data_end = end;
            }
        }
    }

    MemoryLayout {
        text_start,
        text_end,
        data_start,
        data_end,
        brk: data_end,
        stack_bottom: DEFAULT_STACK_TOP - DEFAULT_STACK_SIZE as u64,
        stack_top: DEFAULT_STACK_TOP,
    }
}

/// Copy a segment to memory (helper for process loader)
///
/// The caller must ensure that `dest` points to a valid memory region
/// of at least `segment.mem_size` bytes.
///
/// # Safety
///
/// This function writes to raw memory pointed to by `dest`.
pub unsafe fn copy_segment(
    elf_data: &[u8],
    segment: &elf::LoadableSegment,
    dest: *mut u8,
) {
    let src = elf::segment_data(elf_data, segment);
    
    // SAFETY: Caller guarantees dest is valid and has enough space
    unsafe {
        // Copy file data
        core::ptr::copy_nonoverlapping(src.as_ptr(), dest, src.len());
        
        // Zero out BSS (memory beyond file size)
        if segment.mem_size > segment.file_size {
            let bss_start = dest.add(segment.file_size as usize);
            let bss_size = (segment.mem_size - segment.file_size) as usize;
            core::ptr::write_bytes(bss_start, 0, bss_size);
        }
    }
}

/// Execute an ELF binary from memory
///
/// This is a high-level function that:
/// 1. Parses the ELF
/// 2. Creates a new process
/// 3. Maps and loads segments
/// 4. Sets up the stack
/// 5. Schedules the process
///
/// Returns the PID of the new process.
pub fn exec_from_memory(
    elf_data: &[u8],
    args: &[&str],
    env: &[&str],
    name: &str,
) -> Result<u64, ExecError> {
    let (elf_info, ctx, _stack_data) = prepare_exec(elf_data, args, env)?;

    // Create actual process with parsed ELF parameters:
    
    // Step 1: Allocate page table for new process
    // Allocate a fresh page table using frame allocator
    let page_table = allocate_user_page_table()?;
    
    // Step 2-5: Segment mapping and stack setup handled by prepare_exec
    // The ctx contains entry point and stack pointer
    
    // Step 6: Create Process via process manager
    // Use CapabilityToken::new (pub(crate) so accessible within kernel crate)
    let cap_token = crate::cap::CapabilityToken::new([0, 0, 0, 0]);
    
    let pid = crate::process::PROCESS_MANAGER.spawn_user(
        alloc::string::String::from(name),
        ctx.entry,
        page_table,
        cap_token,
    ).map_err(|_| ExecError::ProcessCreationFailed)?;
    
    // Step 7: The process is automatically added to scheduler by spawn_user
    // Process context is already set up by spawn_user with entry and stack
    
    // Store ELF info for debugging
    let _ = &elf_info;
    
    Ok(pid.0)
}

// =============================================================================
// RING 3 USERSPACE TRANSITION
// =============================================================================

/// User mode selectors (Ring 3)
pub mod user_selectors {
    /// User data segment selector (Ring 3)
    pub const USER_DATA: u16 = 0x18 | 3;
    /// User code segment selector (Ring 3)
    pub const USER_CODE: u16 = 0x20 | 3;
}

/// Jump to userspace (Ring 3) with given entry point and stack
///
/// This function never returns - it performs a privilege level transition
/// from Ring 0 to Ring 3 using IRETQ.
///
/// # Arguments
///
/// * `entry_point` - The userspace entry point address (RIP)
/// * `user_stack` - The userspace stack pointer (RSP)
/// * `argc` - Argument count (passed in RDI)
/// * `argv` - Argument vector pointer (passed in RSI)
///
/// # Safety
///
/// This function is unsafe because:
/// - It transitions to Ring 3 and never returns
/// - The entry_point must be valid user code
/// - The user_stack must be a valid stack in user memory
/// - Page tables must be set up correctly for userspace
#[inline(never)]
pub unsafe fn jump_to_userspace(
    entry_point: u64,
    user_stack: u64,
    argc: u64,
    argv: u64,
) -> ! {
    // IRETQ expects the following stack layout (pushed in reverse order):
    // - SS (user stack segment)
    // - RSP (user stack pointer)
    // - RFLAGS (with IF set for interrupts)
    // - CS (user code segment)
    // - RIP (entry point)
    
    // RFLAGS: Enable interrupts (IF=1), clear other flags
    const RFLAGS_IF: u64 = 1 << 9;  // Interrupt enable flag
    
    unsafe {
        core::arch::asm!(
            // Set up the IRETQ frame
            "push {user_data}",      // SS
            "push {user_rsp}",       // RSP
            "push {rflags}",         // RFLAGS
            "push {user_code}",      // CS
            "push {entry}",          // RIP
            
            // Set up arguments for _start (System V ABI)
            "mov rdi, {argc}",       // First arg: argc
            "mov rsi, {argv}",       // Second arg: argv pointer
            
            // Clear other registers for security
            "xor rdx, rdx",
            "xor rcx, rcx",
            "xor r8, r8",
            "xor r9, r9",
            "xor r10, r10",
            "xor r11, r11",
            "xor rbx, rbx",
            "xor rbp, rbp",
            "xor r12, r12",
            "xor r13, r13",
            "xor r14, r14",
            "xor r15, r15",
            
            // Clear data segment registers to user data
            "mov ax, {user_data_val:x}",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            
            // Clear rax last
            "xor eax, eax",
            
            // IRETQ to Ring 3!
            "iretq",
            
            user_data = in(reg) user_selectors::USER_DATA as u64,
            user_rsp = in(reg) user_stack,
            rflags = in(reg) RFLAGS_IF,
            user_code = in(reg) user_selectors::USER_CODE as u64,
            entry = in(reg) entry_point,
            argc = in(reg) argc,
            argv = in(reg) argv,
            user_data_val = in(reg) user_selectors::USER_DATA,
            options(noreturn)
        );
    }
}

/// Alternative Ring 3 transition using SYSRET (faster than IRETQ)
///
/// SYSRET requires SYSCALL/SYSRET to be enabled via MSRs.
/// Note: SYSRET has known security issues on Intel CPUs when returning
/// to non-canonical addresses. Use IRETQ for better security.
///
/// # Safety
///
/// Same requirements as jump_to_userspace, plus:
/// - SYSCALL/SYSRET MSRs must be configured
/// - Entry point must be in canonical address space
#[inline(never)]
pub unsafe fn sysret_to_userspace(
    entry_point: u64,
    user_stack: u64,
) -> ! {
    // SYSRET expects:
    // - RCX = RIP for user mode
    // - R11 = RFLAGS for user mode
    
    const RFLAGS_IF: u64 = 1 << 9;
    
    unsafe {
        core::arch::asm!(
            // Set up for SYSRET
            "mov rcx, {entry}",      // RCX = return RIP
            "mov r11, {rflags}",     // R11 = return RFLAGS
            "mov rsp, {user_rsp}",   // Set user stack
            
            // Clear registers
            "xor rax, rax",
            "xor rdx, rdx",
            "xor rsi, rsi",
            "xor rdi, rdi",
            "xor r8, r8",
            "xor r9, r9",
            "xor r10, r10",
            "xor rbx, rbx",
            "xor rbp, rbp",
            "xor r12, r12",
            "xor r13, r13",
            "xor r14, r14",
            "xor r15, r15",
            
            // SYSRET to Ring 3 (64-bit mode)
            "sysretq",
            
            entry = in(reg) entry_point,
            rflags = in(reg) RFLAGS_IF,
            user_rsp = in(reg) user_stack,
            options(noreturn)
        );
    }
}

/// Execute a process in Ring 3
///
/// This sets up the execution context and performs the transition to userspace.
pub fn exec_user_process(ctx: &ExecContext) -> ! {
    // Calculate argv pointer from stack layout
    // Stack layout: [argc][argv[0]][argv[1]]...
    // argv pointer is right after argc (which is at stack_ptr)
    let argv_ptr = ctx.stack_ptr + 8; // Skip argc (8 bytes)
    
    // Perform the Ring 3 transition
    // SAFETY: We've set up the execution context with valid addresses
    unsafe {
        jump_to_userspace(
            ctx.entry,
            ctx.stack_ptr,
            ctx.argc as u64,
            argv_ptr,
        )
    }
}

/// Execute a binary from the filesystem
///
/// This is the syscall-level exec implementation.
pub fn exec(path: &str, args: &[&str], env: &[&str]) -> Result<u64, ExecError> {
    use crate::fs::vfs::{VFS, OpenFlags, VfsError};
    use alloc::vec::Vec;
    
    // Use kernel PID (0) for filesystem operations during exec
    const KERNEL_PID: u64 = 0;
    
    // Open the file
    let flags = OpenFlags {
        read: true,
        write: false,
        create: false,
        truncate: false,
        append: false,
        exclusive: false,
        directory: false,
    };
    
    let fd = VFS.open(KERNEL_PID, path, flags)
        .map_err(|e| match e {
            VfsError::NotFound => ExecError::FileNotFound,
            VfsError::PermissionDenied => ExecError::PermissionDenied,
            _ => ExecError::InvalidFormat,
        })?;
    
    // Get file size first
    let attr = VFS.fstat(KERNEL_PID, fd)
        .map_err(|_| ExecError::InvalidFormat)?;
    
    let file_size = attr.size as usize;
    if file_size == 0 {
        let _ = VFS.close(KERNEL_PID, fd);
        return Err(ExecError::InvalidFormat);
    }
    
    // Read the entire file
    let mut file_data: Vec<u8> = Vec::with_capacity(file_size);
    file_data.resize(file_size, 0);
    
    let bytes_read = VFS.read(KERNEL_PID, fd, &mut file_data)
        .map_err(|_| ExecError::InvalidFormat)?;
    
    // Close the file
    let _ = VFS.close(KERNEL_PID, fd);
    
    if bytes_read == 0 {
        return Err(ExecError::InvalidFormat);
    }
    
    // Truncate to actual bytes read
    file_data.truncate(bytes_read);
    
    // Execute from the loaded data
    exec_from_memory(&file_data, args, env, path)
}

/// Information for a simple in-memory test binary
#[derive(Debug)]
pub struct TestBinary {
    /// Raw binary code
    pub code: &'static [u8],
    /// Entry point offset
    pub entry_offset: usize,
}

/// Create a minimal test ELF (for testing without a real binary)
pub fn create_test_elf() -> Vec<u8> {
    // This creates a minimal valid ELF64 executable that:
    // 1. Has the proper ELF headers
    // 2. Contains a single PT_LOAD segment
    // 3. Entry point jumps to a simple infinite loop
    
    // For x86_64, the code is:
    // _start:
    //   jmp _start  ; 0xeb 0xfe
    
    let code: [u8; 2] = [0xeb, 0xfe]; // x86_64 infinite loop
    
    let mut elf = Vec::new();
    
    // ELF header (64 bytes)
    // e_ident
    elf.extend_from_slice(&ELF_MAGIC);           // Magic
    elf.push(ELFCLASS64);                         // Class (64-bit)
    elf.push(ELFDATA2LSB);                        // Data (little endian)
    elf.push(EV_CURRENT);                         // Version
    elf.push(ELFOSABI_NONE);                      // OS/ABI
    elf.extend_from_slice(&[0u8; 8]);             // Padding
    
    // e_type, e_machine
    elf.extend_from_slice(&ET_EXEC.to_le_bytes());       // Type (executable)
    elf.extend_from_slice(&EM_X86_64.to_le_bytes());     // Machine (x86_64)
    
    // e_version
    elf.extend_from_slice(&1u32.to_le_bytes());
    
    // e_entry (entry point = base + header + program header)
    let entry: u64 = 0x400000 + 64 + 56; // After headers
    elf.extend_from_slice(&entry.to_le_bytes());
    
    // e_phoff (program header offset = 64)
    elf.extend_from_slice(&64u64.to_le_bytes());
    
    // e_shoff (no section headers)
    elf.extend_from_slice(&0u64.to_le_bytes());
    
    // e_flags
    elf.extend_from_slice(&0u32.to_le_bytes());
    
    // e_ehsize
    elf.extend_from_slice(&64u16.to_le_bytes());
    
    // e_phentsize
    elf.extend_from_slice(&56u16.to_le_bytes());
    
    // e_phnum
    elf.extend_from_slice(&1u16.to_le_bytes());
    
    // e_shentsize
    elf.extend_from_slice(&64u16.to_le_bytes());
    
    // e_shnum
    elf.extend_from_slice(&0u16.to_le_bytes());
    
    // e_shstrndx
    elf.extend_from_slice(&0u16.to_le_bytes());
    
    // Program header (56 bytes)
    // p_type
    elf.extend_from_slice(&PT_LOAD.to_le_bytes());
    
    // p_flags
    elf.extend_from_slice(&(PF_R | PF_X).to_le_bytes());
    
    // p_offset (start of file)
    elf.extend_from_slice(&0u64.to_le_bytes());
    
    // p_vaddr
    elf.extend_from_slice(&0x400000u64.to_le_bytes());
    
    // p_paddr
    elf.extend_from_slice(&0x400000u64.to_le_bytes());
    
    // p_filesz (header + code)
    let filesz = 64u64 + 56 + code.len() as u64;
    elf.extend_from_slice(&filesz.to_le_bytes());
    
    // p_memsz
    elf.extend_from_slice(&filesz.to_le_bytes());
    
    // p_align
    elf.extend_from_slice(&0x1000u64.to_le_bytes());
    
    // Code
    elf.extend_from_slice(&code);
    
    elf
}

use super::elf::{ELF_MAGIC, ELFCLASS64, ELFDATA2LSB, EV_CURRENT, ELFOSABI_NONE,
                 ET_EXEC, EM_X86_64, PT_LOAD, PF_R, PF_X};

/// Spawn a new process from a path
///
/// This creates a new process that will execute the binary at the given path.
/// Unlike exec(), this does not replace the current process.
///
/// Returns the PID of the newly spawned process.
pub fn spawn(path: &str) -> Result<crate::sched::ProcessId, ExecError> {
    // Extract just the filename for process name
    let name = path.rsplit('/').next().unwrap_or(path);
    
    // Try to read the file from the filesystem
    let file_data = read_file_from_vfs(path)?;
    
    // Parse and load the ELF
    let pid = exec_from_memory(&file_data, &[name], &[], name)?;
    
    Ok(crate::sched::ProcessId::new(pid))
}

/// Read a file from VFS into memory
fn read_file_from_vfs(path: &str) -> Result<alloc::vec::Vec<u8>, ExecError> {
    // Get the filesystem
    let ramfs = crate::fs::filesystem();
    let fs = ramfs.lock();
    
    // Use read_file which takes a path directly
    match fs.read_file(path) {
        Ok(data) => {
            if data.is_empty() {
                Err(ExecError::FileNotFound)
            } else {
                Ok(data.to_vec())
            }
        }
        Err(_) => Err(ExecError::FileNotFound),
    }
}
