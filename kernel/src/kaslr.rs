//! KASLR: kernel base slide state and early entropy init.
//!
//! Extracted from `kernel/src/main.rs` (F2.3 main.rs decomposition, stage 1).
// ponytail: kaslr helpers partially unwired (kaslr_reloc path still feature-gated
// under verification-only code); see ADR-029-syscall-module-lint-hygiene.md

/// KASLR: kernel base slide offset (0 if not randomized)
pub static KERNEL_SLIDE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// KASLR: has relocation been applied (guard against double-apply)
pub static KASLR_APPLIED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Debug: KASLR slide value for verification
pub static KASLR_DEBUG_SLIDE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub(crate) fn init_kaslr() {
    // Use TSC as entropy source since crypto/limine may not be ready yet
    // SAFETY: _rdtsc is callable at CPL0 on x86_64; this fn is only linked into
    // the x86_64 boot path (no #[cfg] guard mirrors the original main.rs body).
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    // 30-bit address space (1GB range), 2MB alignment => 9 bits entropy (512 slots).
    // Mask: 0x3FFFFFE00000 = bits 0-20 zeroed (2MB alignment), bits 21-39 random (30-bit range).
    // But we only want 1 GiB (bits 21-30), so mask with 0x3FE00000 (bits 21-30, 10 bits = 1024 slots * 2MiB = 1 GiB).
    // 0x3FE00000 = 0b001111111110000000000000000000 = bits 21-30
    let slide = (tsc & 0x0000_0000_3FE0_0000) | 0x200000;
    KERNEL_SLIDE.store(slide, core::sync::atomic::Ordering::Relaxed);
    KASLR_DEBUG_SLIDE.store(slide, core::sync::atomic::Ordering::Relaxed);
}
