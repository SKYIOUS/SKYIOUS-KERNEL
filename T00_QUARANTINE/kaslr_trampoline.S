// KASLR CR3 Transition Trampoline
// This assembly module handles the CR3 switch to activate KASLR virtual mapping

.section .text.kaslr_trampoline, "ax", @progbits
.global kaslr_activate_mapping
.global kaslr_trampoline_entry

.set LINK_BASE, 0xFFFFFFFF80000000

// kaslr_activate_mapping:
// Input:
//   rdi = new_pml4_phys (physical address of new PML4)
//   rsi = slide (KERNEL_SLIDE value)
//   rdx = old_rsp (original stack pointer)
// Returns: never returns (jumps to relocated kernel)
kaslr_activate_mapping:
    // Save original RSP and compute new RIP
    mov [rsp - 8], rdx          // Save old RSP
    lea rax, [rel kaslr_trampoline_continue]
    add rax, rsi                // rax = trampoline_continue + slide
    mov [rsp - 16], rax         // Save new RIP

    // Disable interrupts
    cli

    // Switch CR3 to new PML4
    mov cr3, rdi

    // Flush TLB
    mov rax, cr3
    mov cr3, rax

    // Restore RSP (stack is in HHDM, so it's identity-mapped and unaffected)
    mov rsp, [rsp - 8]

    // Jump to new RIP (relocated trampoline_continue)
    jmp qword ptr [rsp - 16]

// kaslr_trampoline_continue:
// This continues execution at the relocated address
kaslr_trampoline_continue:
    // Re-enable interrupts
    sti

    // Jump to the actual kernel entry point (relocated)
    // The slide is in RSI, entry point is LINK_BASE + _start offset
    lea rax, [rel kaslr_kernel_entry]
    add rax, rsi                // rax = _start + slide
    jmp rax

kaslr_kernel_entry:
    // This symbol will be resolved to _start + slide at runtime
    // We'll patch this at runtime via the relocation mechanism
    .quad 0xFFFFFFFF80000000    // Placeholder for LINK_BASE

.size kaslr_activate_mapping, . - kaslr_activate_mapping