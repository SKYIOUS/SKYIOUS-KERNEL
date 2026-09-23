ENTRY(_start)

DYNAMIC_BASE = 0x1000000;
KERNEL_BASE = 0xFFFFFFFF80000000;
PHYS_BASE = 0x100000;

PHDRS
{
    dyn PT_LOAD FLAGS(5) FILEHDR PHDRS;
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
    dyn_dyn PT_DYNAMIC;
}

SECTIONS
{
    /* Dynamic sections at low virtual address - FIRST in SECTIONS */
    . = 0x1000000;

    .dynsym : AT(0x100000 + (. - 0x1000000))
    {
        *(.dynsym)
    } :dyn

    . = ALIGN(8);
    .dynstr : AT(0x100000 + (. - 0x1000000))
    {
        *(.dynstr)
    } :dyn

    . = ALIGN(8);
    .dynamic : AT(0x100000 + (. - 0x1000000))
    {
        *(.dynamic)
    } :dyn :dyn_dyn

    . = ALIGN(8);
    .rela.dyn : AT(0x100000 + (. - 0x1000000))
    {
        *(.rela.dyn)
    } :dyn

    . = ALIGN(8);
    .rela.plt : AT(0x100000 + (. - 0x1000000))
    {
        *(.rela.plt)
    } :dyn

    . = ALIGN(4K);
    .got : AT(0x100000 + (. - 0x1000000))
    {
        *(.got .got.*)
    } :dyn

    . = ALIGN(4K);
    .got.plt : AT(0x100000 + (. - 0x1000000))
    {
        *(.got.plt)
    } :dyn

    . = ALIGN(4K);
    .rela.plt : AT(0x100000 + (. - 0x1000000))
    {
        *(.rela.plt)
    } :dyn

    /* Kernel at higher-half */
    . = 0xFFFFFFFF80000000;

    .text : AT(0x100000 + (. - 0xFFFFFFFF80000000))
    {
        *(.text .text.*)
    } :text

    . = ALIGN(4K);
    .rodata : AT(0x100000 + (. - 0xFFFFFFFF80000000))
    {
        *(.rodata .rodata.*)
    } :text

    . = ALIGN(4K);
    .data : AT(0x100000 + (. - 0xFFFFFFFF80000000))
    {
        *(.data .data.*)
    } :data

    .bss : AT(0x100000 + (. - 0xFFFFFFFF80000000))
    {
        *(.bss .bss.*)
        *(COMMON)
    } :data

    . = ALIGN(4K);
    .got : AT(0x100000 + (. - 0xFFFFFFFF80000000))
    {
        *(.got .got.*)
    } :data

    /DISCARD/ :
    {
        *(.hash)
        *(.gnu.hash)
        *(.note*)
        *(.eh_frame)
        *(.comment)
    }
}