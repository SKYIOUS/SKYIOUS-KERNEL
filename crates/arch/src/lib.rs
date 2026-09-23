//! # vahi-arch — Architecture-Specific Code
//!
//! x86_64 and aarch64 specific implementations. Provides CPU detection,
//! register manipulation, segment management, and MSRs.
//!
//! ## Architecture Dispatch
//!
//! ```text
//! #[cfg(target_arch = "x86_64")]  → mod x86_64_impl;
//! #[cfg(target_arch = "aarch64")] → mod aarch64_impl;
//! ```
//!
//! All architecture-specific code MUST be behind `#[cfg]` guards.
//!
//! ## Dependency Breaking
//!
//! ```text
//! Original:     arch → interrupts (IDT) + task (FORK_CHILD_CS)
//! With traits:  arch → vahi-gdt (Selectors) + vahi-types::ArchOps
//! ```
//!
//! ## Invariants
//!
//! - Segment registers must be valid before any user-mode code
//! - GDT must be loaded before IDT setup
//! - MSR access must use proper serializing instructions

#![no_std]

pub mod cpu;
pub mod iommu;
pub mod smp;

/// CPU feature detection.
#[derive(Debug, Clone, Copy)]
pub struct CpuFeatures {
    pub has_x2apic: bool,
    pub has_xsave: bool,
    pub has_fsgsbase: bool,
    pub has_smep: bool,
    pub has_smap: bool,
    pub has_pku: bool,
    pub has_tsc_deadline: bool,
    pub has_rdrand: bool,
    pub max_extended_leaf: u32,
}

/// CPU feature detection (x86_64 only).
#[cfg(target_arch = "x86_64")]
impl CpuFeatures {
    /// Detect CPU features via CPUID leaves 0x01, 0x07, and 0x80000001.
    pub fn detect() -> Self {
        fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
            let (a, b, c, d);
            unsafe {
                core::arch::asm!(
                    "push rbx",
                    "cpuid",
                    "mov {b:e}, ebx",
                    "pop rbx",
                    inlateout("eax") leaf => a,
                    out("ecx") c,
                    out("edx") d,
                    b = out(reg) b,
                    options(nostack, preserves_flags)
                );
            }
            (a, b, c, d)
        }

        let (_eax1, _, ecx1, edx1) = cpuid(0x01);
        let (eax7, _, _ecx7, _) = cpuid(0x07);
        let (_, _, ecx_ext, _) = cpuid(0x8000_0001);

        // Leaf 1 ECX bits
        let has_xsave = (ecx1 & (1 << 26)) != 0;
        let has_rdrand = (ecx1 & (1 << 30)) != 0;

        // Leaf 7 EBX bits
        let has_fsgsbase = (eax7 & (1 << 0)) != 0;
        let has_smep = (eax7 & (1 << 7)) != 0;
        let has_smap = (eax7 & (1 << 20)) != 0;
        let has_pku = (eax7 & (1 << 3)) != 0;

        // Leaf 1 EDX bits
        let has_tsc_deadline = (edx1 & (1 << 24)) != 0;

        // Extended leaf 1 ECX bits (x2APIC)
        let has_x2apic = (ecx_ext & (1 << 21)) != 0;

        Self {
            has_x2apic,
            has_xsave,
            has_fsgsbase,
            has_smep,
            has_smap,
            has_pku,
            has_tsc_deadline,
            has_rdrand,
            max_extended_leaf: 0x8000_0001,
        }
    }
}

/// Stub implementation for non-x86_64 targets.
#[cfg(not(target_arch = "x86_64"))]
impl CpuFeatures {
    /// Detect CPU features — returns defaults for unsupported architectures.
    pub fn detect() -> Self {
        Self {
            has_x2apic: false,
            has_xsave: false,
            has_fsgsbase: false,
            has_smep: false,
            has_smap: false,
            has_pku: false,
            has_tsc_deadline: false,
            has_rdrand: false,
            max_extended_leaf: 0,
        }
    }
}
