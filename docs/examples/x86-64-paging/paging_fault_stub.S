# ── Page-fault interrupt stub ──
# Saves volatile regs, calls page_fault_handler(), restores, returns.
#
# Install this in your IDT entry for vector 14.
#
# IDT gate setup:
#   offset[0:15]  = _page_fault_stub & 0xFFFF
#   offset[16:31] = (_page_fault_stub >> 16) & 0xFFFF
#   offset[32:63] = (_page_fault_stub >> 32) & 0xFFFFFFFF
#   selector      = 0x08  (kernel code segment)
#   ist           = 1     (use a dedicated stack if you have one)
#   type_attr     = 0x8E  (interrupt gate, ring 0)

.intel_syntax noprefix
.text
.globl  _page_fault_stub

_page_fault_stub:
    # CPU already pushed:  error_code, RIP, CS, RFLAGS, RSP, SS
    push    rax
    push    rcx
    push    rdx
    push    rdi
    push    rsi
    push    r8
    push    r9
    push    r10
    push    r11

    mov     rdi, cr2                        # fault address as first arg
    mov     rsi, [rsp + 9*8]               # error_code from stack
    call    page_fault_handler_c            # C handler

    pop     r11
    pop     r10
    pop     r9
    pop     r8
    pop     rsi
    pop     rdi
    pop     rdx
    pop     rcx
    pop     rax
    add     rsp, 8                          # skip error code
    iretq

# ── C-callable wrapper — reads CR2, calls page_fault_handler() ──
.globl  page_fault_handler_c
page_fault_handler_c:
    .extern page_fault_handler
    jmp     page_fault_handler              # tail-call the C function
