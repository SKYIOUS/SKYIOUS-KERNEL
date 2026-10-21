ENTRY(_start)

VIRT_BASE = 0xFFFFFFFF80000000;
PHYS_BASE = 0x100000;

SECTIONS
{
    . = VIRT_BASE;

    .text : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.text .text.*)
        KEEP(*(.text.kaslr_trampoline))
    }

    . = ALIGN(4K);
    .rodata : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.rodata .rodata.*)
        KEEP(*(.limine_requests.start))
        KEEP(*(.limine_requests))
        KEEP(*(.limine_requests.end))
    }

    . = ALIGN(4K);
    .data : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.data .data.*)
    }

    .bss : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.bss .bss.*)
        *(COMMON)
    }

    . = ALIGN(4K);
    .got : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.got .got.*)
    }

    /* Dynamic metadata lld synthesizes for a PIC/PIE link: .dynamic (DT_* tags)
       and .dynsym/.dynstr (a single null symbol each). Without explicit
       placement lld's orphan-section logic put .dynsym at VIRT_BASE,
       overlapping .text and failing the link. Placed before .rela so .rela
       stays the last section and __kernel_end = image end remains true. */
    . = ALIGN(8);
    .dynamic : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.dynamic)
    }

    /* 4 KiB alignment, not 8: the bootloader's ELF loader rejects PT_LOADs
       with different permissions sharing one memory page. .dynamic is
       writable and .dynsym is not, and an 8-byte boundary put both in the
       same page -- the loader panicked before reaching the kernel. */
    . = ALIGN(4K);
    .dynsym : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.dynsym)
    }

    .dynstr : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.dynstr)
    }

    /* Relocation tables for KASLR. lld emits .rela.text/.rela.rodata/.rela.data
       as NON-alloc sections by default (sh_addr = 0), so they live only in the
       file and are never mapped -- the kernel cannot read them at runtime, and
       apply_kaslr_relocations has nothing to walk (issue A-6). Making them alloc
       and placing them inside a PT_LOAD makes them reachable once the bootloader
       has loaded the image.

       The .rela.debug_* tables are not collected: they are non-alloc debug
       metadata, so leaving them out keeps ~36 MB out of the image. Note they
       must NOT be added to /DISCARD/: lld resolves /DISCARD/ first, and doing
       so took the runtime tables with them. */
    . = ALIGN(4K);
    .rela : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        KEEP(*(.rela.text))
        KEEP(*(.rela.rodata))
        KEEP(*(.rela.data))
        KEEP(*(.rela.got))
        KEEP(*(.rela.dyn))
    }

    /DISCARD/ :
    {
        *(.hash)
        *(.gnu.hash)
        *(.note*)
        *(.eh_frame)
        *(.comment)
    }
}

/* The kernel's own extent, in link-time addresses. KASLR needs this to know
   how many 2 MiB pages the image occupies. It cannot be read from the ELF
   header at runtime: the first PT_LOAD starts at file offset 0x1000, so the
   header at file offset 0 is never loaded into memory and LINK_BASE holds
   .text bytes instead (issue A-6).

   Assigned here, outside SECTIONS: inside SECTIONS an assignment is
   section-relative, and ADDR/SIZEOF only yield an absolute address here.
   `.rela` is the LAST section in the script, so its end is the image end.
   `.got` was not (it left .got 2184 bytes outside the span) and `.bss` is not
   either. */
__kernel_start = VIRT_BASE;
__rela_start = ADDR(.rela);
__rela_end = __rela_start + SIZEOF(.rela);
__kernel_end = __rela_end;

/* Writability range for relocation targets. The relocation pass validates
   every R_X86_64_RELATIVE target against it before its first write: a word in
   .text or .rodata is mapped read-only, so writing there faults or corrupts
   code. The range is exactly [.data, .dynsym) -- .data/.bss/.got/.dynamic are
   the sections the segment layout maps writable; .dynsym onward is read-only. */
__writable_start = ADDR(.data);
__writable_end = ADDR(.dynsym);
