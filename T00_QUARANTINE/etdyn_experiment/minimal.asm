// Minimal test for ET_DYN with split address space
// Tests: can we have dynamic sections at low address and kernel at higher-half?

section .text
global _start
_start:
    mov rax, 0x12345678
    ret

section .rodata
global rodata_val
rodata_val: dq 0xdeadbeef

section .data
global data_val
data_val: dq 0xcafebabe

section .bss
global bss_val
bss_val: resq 1