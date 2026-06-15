ENTRY(_start)

SECTIONS
{
    . = 0x40080000;

    .text : ALIGN(4K)
    {
        *(.text .text.*)
    }

    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : ALIGN(4K)
    {
        __bss_start = .;
        *(.bss .bss.*)
        __bss_end = .;
    }

    .got : ALIGN(4K)
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
