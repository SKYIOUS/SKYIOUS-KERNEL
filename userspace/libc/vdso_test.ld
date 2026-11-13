/* Linker script for vDSO libc test - _start at 0x401000 */

ENTRY(_start)

SECTIONS
{
    . = 0x400000;
    .text 0x401000 : {
        *(.text._start)
        *(.text)
    }
    .rodata : { *(.rodata) }
    .data : { *(.data) }
    .bss : { *(.bss) }
}