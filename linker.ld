ENTRY(_start)

SECTIONS
{
    . = 0xFFFFFFFF80000000;

    .text :
    {
        *(.text .text.*)
    }

    . = ALIGN(4K);
    .rodata :
    {
        *(.rodata .rodata.*)
    }

    . = ALIGN(4K);
    .data :
    {
        *(.data .data.*)
    }

    .bss :
    {
        *(.bss .bss.*)
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
