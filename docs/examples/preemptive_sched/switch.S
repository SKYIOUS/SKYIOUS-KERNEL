# context_switch(PCB *old, PCB *new)
#   Save all callee-saved registers from old,
#   restore them from new, then resume new->rip.
#
# Calling convention:  rcx = old, rdx = new  (Windows x64)
#                      rdi = old, rsi = new  (System V AMD64)
# We use the System V ABI here; adjust registers for Windows.

.intel_syntax noprefix
.text
.globl  context_switch

context_switch:
    # ── Save old ──
    mov     [rdi +   0], rsp       # PCB.rsp
    mov     [rdi +  64], r15       # PCB.r15
    mov     [rdi +  56], r14       # PCB.r14
    mov     [rdi +  48], r13       # PCB.r13
    mov     [rdi +  40], r12       # PCB.r12
    mov     [rdi +  32], rbp       # PCB.rbp
    mov     [rdi +  24], rbx       # PCB.rbx

    pushfq
    pop     [rdi +  16]            # PCB.rflags

    mov     rax, cr3
    mov     [rdi +  72], rax       # PCB.cr3

    mov     rax, [rsp]
    mov     [rdi +   8], rax       # PCB.rip

    # ── Restore new ──
    mov     rsp, [rsi +   0]
    mov     r15, [rsi +  64]
    mov     r14, [rsi +  56]
    mov     r13, [rsi +  48]
    mov     r12, [rsi +  40]
    mov     rbp, [rsi +  32]
    mov     rbx, [rsi +  24]

    mov     rax, [rsi +  72]
    mov     cr3, rax

    push    [rsi +  16]
    popfq

    jmp     [rsi +   8]
