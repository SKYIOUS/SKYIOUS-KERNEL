ENTRY(_start)

SECTIONS
{
    . = 0xFFFFFFFF80000000;

    .text : ALIGN(4K)
    {
        *(.text .text.*)
    }

    . = ALIGN(4K);
    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    . = ALIGN(4K);
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

    /* Stack for initial boot (64KB) */
    .stack : ALIGN(16)
    {
        _stack_bottom = .;
        . += 64K;
        _stack_top = .;
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
