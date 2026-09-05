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
