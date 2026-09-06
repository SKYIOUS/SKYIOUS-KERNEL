//! SMP CPU identification (x86_64 only).

#[cfg(target_arch = "x86_64")]
/// Get the current CPU's APIC ID.
///
/// Uses CPUID leaf 1 (EBX[31:24]) which returns the initial APIC ID.
/// This is unique per CPU on x86_64 systems.
pub fn get_cpu_id() -> usize {
    // SAFETY: CPUID leaf 1 is safe and returns the initial APIC ID in EBX[31:24].
    let r = core::arch::x86_64::__cpuid(1);
    ((r.ebx >> 24) & 0xFF) as usize
}

#[cfg(not(target_arch = "x86_64"))]
/// Get the current CPU ID — returns 0 for unsupported architectures.
pub fn get_cpu_id() -> usize {
    0
}