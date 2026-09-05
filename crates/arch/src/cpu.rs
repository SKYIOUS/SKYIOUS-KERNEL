//! CPU detection and feature queries.

/// Read the Time Stamp Counter.
///
/// Returns a monotonically increasing 64-bit counter that ticks at the
/// processor's base frequency. Safe to call from any privilege level.
pub fn rdtsc() -> u64 {
    // SAFETY: RDTSC is a non-privileged instruction that reads the TSC
    // into EDX:EAX. It does not modify memory or control state.
    unsafe {
        let lo: u32;
        let hi: u32;
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
        ((hi as u64) << 32) | (lo as u64)
    }
}

/// Read the Model-Specific Register.
///
/// # Safety
///
/// MSR reads can return sensitive data and must only be called
/// for well-known MSR indices.
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") lo,
        out("edx") hi,
        options(nostack, preserves_flags)
    );
    ((hi as u64) << 32) | (lo as u64)
}

/// Write to a Model-Specific Register.
///
/// # Safety
///
/// Writing to wrong MSR indices can cause triple faults, security
/// vulnerabilities, or data corruption.
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let lo = value as u32;
    let hi = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") lo,
        in("edx") hi,
        options(nostack, preserves_flags)
    );
}

/// Invalidate the TLB entry for a single virtual address.
///
/// Must be called after a page table modification to ensure the CPU
/// does not use a stale TLB entry for the given address.
pub fn invlpg(addr: u64) {
    // SAFETY: INVLPG invalidates the TLB entry for the page containing
    // `addr`. It is a hint — the CPU may use a global TLB entry instead.
    // No memory side effects beyond the TLB.
    unsafe {
        core::arch::asm!(
            "invlpg [{addr}]",
            addr = in(reg) addr,
            options(nostack)
        );
    }
}

/// Flush the entire TLB by reloading CR3.
pub fn flush_tlb() {
    // SAFETY: Reading CR3 and writing it back forces the CPU to reload
    // the page table hierarchy, invalidating all non-global TLB entries.
    // This is the standard x86_64 TLB flush mechanism.
    unsafe {
        let cr3: u64;
        core::arch::asm!("mov {cr3}, cr3", cr3 = out(reg) cr3);
        core::arch::asm!("mov cr3, {cr3}", cr3 = in(reg) cr3);
    }
}

/// Halt the CPU until the next interrupt.
///
/// The CPU enters a low-power state and resumes when an unmasked
/// interrupt fires. Must be called with interrupts enabled,
/// otherwise the CPU halts forever.
pub fn hlt() {
    // SAFETY: HLT puts the CPU to sleep until the next interrupt.
    // Safe to call from kernel context when idle.
    unsafe { core::arch::asm!("hlt") };
}

/// Disable interrupts and halt (for idle without preemption).
pub fn cli_hlt() {
    // SAFETY: CLI disables maskable interrupts, then HLT puts the CPU
    // to sleep. Used during shutdown or when interrupts are already
    // managed by the caller.
    unsafe { core::arch::asm!("cli; hlt") };
}
