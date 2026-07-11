# ── x86-64 paging startup: protected-mode → long-mode transition ──
#
# Assemble with: gcc -m64 -ffreestanding -c startup.S
# Link at a physical address >= 1 MiB (e.g. 0x100000).
#
# This stub assumes the bootloader left you in 32-bit protected mode.
# If UEFI/BIOS already set up long mode (most do), skip to _start64.

.intel_syntax noprefix
.code32
.globl  _start

_start:
    # 1. Set up stack
    mov     esp, stack_top

    # 2. Load GDT that contains 32-bit and 64-bit code segments
    lgdt    [gdt32_ptr]

    # 3. Enable PAE (must be on before long mode)
    mov     eax, cr4
    or      eax, (1 << 5)                   # PAE
    mov     cr4, eax

    # 4. PML4 must already be set up by paging_init() — read from the C global
    #    For a standalone build, embed the address directly or call paging_init first.
    .extern pml4
    mov     eax, pml4                       # phys address of pml4 (identity mapped in flat binary)
    mov     cr3, eax

    # 5. Enable long mode in EFER
    mov     ecx, 0xC0000080                 # EFER MSR
    rdmsr
    or      eax, (1 << 8)                   # LME
    wrmsr

    # 6. Enable paging
    mov     eax, cr0
    or      eax, (1 << 31)                  # PG
    mov     cr0, eax
    # After this instruction we are in compatibility mode.

    # 7. Far jump to 64-bit code segment
    push    0x08                            # 64-bit CS selector
    push    offset _start64
    retf

.code64
_start64:
    # Now running in long mode at CPL 0, paging enabled.
    lgdt    [gdt64_ptr]                     # reload GDT for 64-bit

    # Call paging_init() to set up identity map + phys-map window
    .extern paging_init
    call    paging_init

    # Enable global pages for performance
    mov     rax, cr4
    or      rax, (1 << 7)                   # PGE
    mov     cr4, rax

    # Your kernel main goes here
    .extern kmain
    call    kmain

    cli
.hlt:   hlt
        jmp     .hlt

# ── Minimal GDT (32-bit + 64-bit) ──
.align 8
gdt32:
    .quad 0                                 # null
    .quad 0x00CF9A000000FFFF                 # 32-bit code (ring 0)
    .quad 0x00CF92000000FFFF                 # 32-bit data (ring 0)
    .quad 0x00AF9A000000FFFF                 # 64-bit code (ring 0)
gdt32_end:

gdt32_ptr:
    .word gdt32_end - gdt32 - 1
    .long gdt32

gdt64:
    .quad 0                                 # null
    .quad 0x00AF9A000000FFFF                 # 64-bit code (ring 0)
    .quad 0x00CF92000000FFFF                 # 64-bit data (ring 0)
gdt64_end:

gdt64_ptr:
    .word gdt64_end - gdt64 - 1
    .quad gdt64

# ── Stack ──
.bss
.align 16
    .skip 16384
stack_top:
