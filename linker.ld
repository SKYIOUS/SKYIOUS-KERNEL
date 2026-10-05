ENTRY(_start)

VIRT_BASE = 0xFFFFFFFF80000000;
PHYS_BASE = 0x100000;

SECTIONS
{
    . = VIRT_BASE;

    .text : AT(PHYS_BASE + (. - VIRT_BASE))
    {
        *(.text .text.*)
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
   `.got` is the LAST section in the script, so its end is the image end --
   `.bss` is not, and using it left `.got` 2184 bytes outside the span. */
__kernel_start = VIRT_BASE;
__kernel_end = ADDR(.got) + SIZEOF(.got);
