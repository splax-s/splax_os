/* Splax OS Boot Assembly for x86_64
 * 
 * GRUB drops us in 32-bit protected mode. We need to:
 * 1. Set up page tables for identity mapping
 * 2. Enable PAE
 * 3. Enable long mode
 * 4. Jump to 64-bit kernel entry
 *
 * Uses Intel syntax for LLVM/Rust inline assembly compatibility
 */

.intel_syntax noprefix

/* Multiboot1 header (simpler and widely supported) */
.set MULTIBOOT_MAGIC, 0x1BADB002
.set MULTIBOOT_FLAGS, 0x00000003  /* Align + Memory info */
.set MULTIBOOT_CHECKSUM, -(MULTIBOOT_MAGIC + MULTIBOOT_FLAGS)

.section ".multiboot_header", "aw"
.align 4

multiboot_header:
    .long MULTIBOOT_MAGIC
    .long MULTIBOOT_FLAGS
    .long MULTIBOOT_CHECKSUM
multiboot_header_end:

/* Put page tables in a known safe location - below 1MB (before kernel)
 * Using 0x1000-0x7000 range which is typically unused after BIOS
 * We need 4 PDs to map 4GB of physical memory for hardware access (PCI, LAPIC, etc) */
.set BOOT_PML4, 0x1000
.set BOOT_PDP,  0x2000  
.set BOOT_PD0,  0x3000   /* PD for 0-1GB */
.set BOOT_PD1,  0x4000   /* PD for 1GB-2GB */
.set BOOT_PD2,  0x5000   /* PD for 2GB-3GB */
.set BOOT_PD3,  0x6000   /* PD for 3GB-4GB */
/* Stack at 0x7C00 region (below kernel, unused after boot) */
.set BOOT_STACK_TOP, 0x7C00

/* Kernel stack in BSS - used after entering 64-bit mode */
.bss
.align 16
stack_bottom:
    .skip 32768  /* 32 KB stack */
stack_top:

.text
.code32

.global _start
_start:
    /* Early debug: write '1' to serial (before anything) */
    mov dx, 0x3F8
    mov al, '1'
    out dx, al

    /* Save multiboot info pointer */
    mov edi, ebx

    /* Set up stack below 1MB in safe area */
    mov esp, BOOT_STACK_TOP

    /* Early debug: write '2' */
    mov dx, 0x3F8
    mov al, '2'
    out dx, al

    /* Clear page tables at safe addresses below 1MB */
    mov edi, BOOT_PML4
    xor eax, eax
    mov ecx, 6144  /* 6 pages * 1024 dwords = 24KB for PML4+PDP+4×PD */
    rep stosd

    /* Set up page tables for identity mapping first 4GB
     * This covers all typical hardware addresses:
     * - 0-1GB: Low memory, kernel, ACPI tables
     * - 1GB-2GB: Extended ACPI, possible RAM
     * - 2GB-3GB: PCI memory hole, device memory
     * - 3GB-4GB: Local APIC (0xFEE00000), IOAPIC, PCI config space */
    
    /* PML4[0] -> PDP table (BOOT_PDP | 0x03) */
    mov eax, BOOT_PDP
    or eax, 0x03
    /* mov DWORD PTR [BOOT_PML4], eax - encoded manually for 32-bit absolute */
    .byte 0xa3
    .long BOOT_PML4

    /* PDP[0] -> PD0 (0-1GB) */
    mov eax, BOOT_PD0
    or eax, 0x03
    /* mov DWORD PTR [BOOT_PDP], eax */
    .byte 0xa3
    .long BOOT_PDP

    /* PDP[1] -> PD1 (1GB-2GB) */
    mov eax, BOOT_PD1
    or eax, 0x03
    mov edi, BOOT_PDP
    add edi, 8
    mov [edi], eax
    mov DWORD PTR [edi+4], 0

    /* PDP[2] -> PD2 (2GB-3GB) */
    mov eax, BOOT_PD2
    or eax, 0x03
    mov edi, BOOT_PDP
    add edi, 16
    mov [edi], eax
    mov DWORD PTR [edi+4], 0

    /* PDP[3] -> PD3 (3GB-4GB) */
    mov eax, BOOT_PD3
    or eax, 0x03
    mov edi, BOOT_PDP
    add edi, 24
    mov [edi], eax
    mov DWORD PTR [edi+4], 0

    /* Fill PD0 with 512 entries for 0-1GB (512 x 2MB = 1GB)
     * Each entry: base_addr | 0x83 (present + writable + huge page) */
    mov edi, BOOT_PD0
    mov eax, 0x83          /* First entry: 0x0 | flags */
    mov ecx, 512
.fill_pd0:
    mov [edi], eax
    mov DWORD PTR [edi+4], 0
    add edi, 8
    add eax, 0x200000      /* Next 2MB physical address */
    dec ecx
    jnz .fill_pd0

    /* Fill PD1 for 1GB-2GB */
    mov edi, BOOT_PD1
    mov eax, 0x40000083    /* Start at 1GB (0x40000000) | flags */
    mov ecx, 512
.fill_pd1:
    mov [edi], eax
    mov DWORD PTR [edi+4], 0
    add edi, 8
    add eax, 0x200000
    dec ecx
    jnz .fill_pd1

    /* Fill PD2 for 2GB-3GB */
    mov edi, BOOT_PD2
    mov eax, 0x80000083    /* Start at 2GB (0x80000000) | flags */
    mov ecx, 512
.fill_pd2:
    mov [edi], eax
    mov DWORD PTR [edi+4], 0
    add edi, 8
    add eax, 0x200000
    dec ecx
    jnz .fill_pd2

    /* Fill PD3 for 3GB-4GB (includes LAPIC at 0xFEE00000) */
    mov edi, BOOT_PD3
    mov eax, 0xC0000083    /* Start at 3GB (0xC0000000) | flags */
    mov ecx, 512
.fill_pd3:
    mov [edi], eax
    mov DWORD PTR [edi+4], 0
    add edi, 8
    add eax, 0x200000
    dec ecx
    jnz .fill_pd3

    /* Early debug: write '3' */
    mov dx, 0x3F8
    mov al, '3'
    out dx, al

    /* Load page table address into CR3 */
    mov eax, BOOT_PML4
    mov cr3, eax

    /* Enable PAE in CR4 */
    mov eax, cr4
    or eax, 0x20         /* Set PAE bit */
    mov cr4, eax

    /* Enable long mode in EFER MSR */
    mov ecx, 0xC0000080  /* EFER MSR */
    rdmsr
    or eax, 0x100        /* Set LME bit */
    wrmsr

    /* Early debug: write '4' */
    mov dx, 0x3F8
    mov al, '4'
    out dx, al

    /* Enable paging in CR0 */
    mov eax, cr0
    or eax, 0x80000001   /* Set PG and PE bits */
    mov cr0, eax

    /* Load 64-bit GDT - manually encode lgdt with absolute address */
    /* lgdt [gdt64_ptr] */
    .byte 0x0f, 0x01, 0x15  /* lgdt m16&64 with disp32 */
    .long gdt64_ptr

    /* Far jump to 64-bit code using absolute jump */
    .byte 0xea          /* far jmp opcode */
    .long long_mode_start  /* 32-bit offset */
    .word 0x08          /* code segment selector */

.code64
long_mode_start:
    /* Set up segment registers */
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* Set up 64-bit stack - use lea to get address */
    lea rsp, [rip + stack_top]

    /* Early debug: write 'S' to serial port */
    mov dx, 0x3F8
    mov al, 'S'
    out dx, al

    /* Call Rust kernel entry */
    /* RDI already contains multiboot info pointer (zero-extended from EDI) */
    call kernel_entry

    /* Should never return */
1:  hlt
    jmp 1b

/* 64-bit GDT - placed in text section to avoid section directive issues */
.align 16
gdt64:
    .quad 0                 /* null descriptor */
gdt64_code:
    .quad 0x00AF9A000000FFFF  /* 64-bit code segment */
gdt64_data:
    .quad 0x00CF92000000FFFF  /* 64-bit data segment */
gdt64_end:

gdt64_ptr:
    .word gdt64_end - gdt64 - 1
    .quad gdt64
