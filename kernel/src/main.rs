//! # Vahi Kernel
//!
//! "Vahi" (वाहि) is derived from Sanskrit, meaning "the carrier" — that which
//! flows and transports. The kernel carries all processes, flows all data, and
//! transports instructions from software to hardware.
//!
//! The name was chosen for its clean pronunciation (VAH-hee), its absence from
//! existing software trademarks, and its subtle Sanskrit heritage that is
//! invisible to those unfamiliar with Vedic literature.
//!
//! Architecture decisions are documented in `docs/decisions/` (ADRs):
//! - ADR-001: Monolithic kernel architecture (this crate)
//! - ADR-002: hashbrown migration for hash maps
//! - ADR-003: Rust nightly + no_std + bootloader ecosystem
//! - ADR-004: Formal boot state machine
//! - ADR-005: Stride scheduling with proportional fairness
//! - ADR-006: POSIX Capabilities + LSM for Security Model
//! - ADR-007: SMP support via SIPI + per-CPU schedulers
//! - ADR-009: Keep and wire up deletion policy
//! - ADR-010: Syscalls decomposition
//! - ADR-011: Single kernel clock
//! - ADR-012: Shared driver primitives
//! - ADR-013: Slab Allocator
//! - ADR-014: Page Cache
//! - ADR-015: epoll Implementation
//! - ADR-016: Scheduler architecture
//! - ADR-017: KASLR
//! - ADR-018: SkyFS journaling
//! - ADR-019: Kernel Object Model Redesign
//! - ADR-023: Vahi IPC Model
//! - ADR-026: Single process/scheduler lineage
//! - ADR-027: T-00 Kernel Self-Test Protocol and QEMU Exit Mechanism
//! - ADR-028: Syscall-Entry Init Through the `Arch::init_syscalls()` Seam
//! - ADR-029: Syscall-module lint hygiene
//! - ADR-030: vDSO
#![no_std]
#![no_main]
#![cfg_attr(not(target_arch = "aarch64"), feature(abi_x86_interrupt))]
#![feature(alloc_error_handler)]
#![deny(warnings)]
// Targeted clippy suppressions — each lint is named and documented.
#![allow(
    // Design-level: require refactoring to fix properly
    clippy::result_unit_err,
    clippy::too_many_arguments,
    clippy::similar_names,
    clippy::new_without_default,
    clippy::len_without_is_empty,
    clippy::needless_lifetimes,
    clippy::type_complexity,
    clippy::fn_params_excessive_bools,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
)]

mod panic_handler;
extern crate alloc;
#[cfg(not(target_arch = "aarch64"))]
mod acpi;
// ponytail: F2.2 — acpi_prt tables partially unwired; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod acpi_prt;
mod allocator;
// ponytail: F2.2 — apic/msi feature surface unwired; see ADR-029-syscall-module-lint-hygiene.md
#[cfg(not(target_arch = "aarch64"))]
#[allow(dead_code)]
mod apic;
pub mod arch;
#[cfg(feature = "ash")]
pub mod ash;
pub mod boot;
#[cfg(feature = "gpu")]
pub mod compositor;
// Dummy coverage module for selftest framework
#[cfg(feature = "self_test")]
#[allow(dead_code)]
mod coverage {
    pub fn init() {}
    pub fn unique_blocks() -> usize {
        0
    }
    pub fn total_hits() -> usize {
        0
    }
    pub fn coverage_ratio() -> f64 {
        0.0
    }
}
pub mod crypto;
pub mod debug;
pub mod ebpf;
pub mod elf_dyn;
pub mod emulation;
pub mod drivers {
    pub use vahi_drivers::*;
}
#[cfg(not(target_arch = "aarch64"))]
mod gdt;
pub mod gui;
pub mod hal;
#[cfg(feature = "hypervisor")]
// ponytail: F2.2 — hypervisor VMCB/SVM syscall surface unwired; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
pub mod hypervisor;
#[cfg(not(target_arch = "aarch64"))]
mod interrupts;
// ponytail: F2.2 — iommu feature surface unwired; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
pub mod iommu;
pub mod ipc;
// ponytail: F2.2 — kaslr_reloc helpers partially unwired; see ADR-029-syscall-module-lint-hygiene.md
mod kaslr;
#[allow(dead_code)]
mod kaslr_reloc;
pub(crate) use kaslr::{init_kaslr, KASLR_APPLIED, KASLR_DEBUG_SLIDE, KERNEL_SLIDE};
#[cfg(target_arch = "x86_64")]
mod vdso;
#[cfg(target_arch = "x86_64")]
pub(crate) use vdso::{
    init_vdso, vdso_page, vdso_read_tsc_time, vdso_set_kernel_flag, vdso_update_pid_cpu,
    vdso_update_process_info, vdso_update_thread_cputime,
};
#[cfg(not(target_arch = "aarch64"))]
// ponytail: F2.2 — keyboard PS/2 surface; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod keyboard;
pub mod limine {
    pub use vahi_limine::*;
}
pub mod memory {
    pub use vahi_memory::*;
}
pub mod net {
    pub use vahi_net::*;
}
// ponytail: F2.2 — object/types surface; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
pub mod objects;
#[cfg(not(target_arch = "aarch64"))]
mod pci;
pub mod pty;
// ponytail: F2.2 — security/namespace/capability surface unwired; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod security;
// ponytail: F2.2 — selftest harness infra; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod selftest;
// ponytail: F2.2 — shell (kernel_shell) not spawned in current boot path; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod shell;
#[cfg(target_arch = "x86_64")]
mod smp;
// ponytail: F2.2 — sync RCU/CFI surface unwired; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod sync;
// ponytail: F2.2 — syscall impls pending dispatch wiring (ptrace/vm/ash-numbers/tmrfd); see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod syscalls;
// ponytail: F2.2 — task cgroup/OOM/scheduler surface unwired; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod task;
#[cfg(feature = "self_test")]
mod tests;
mod tty;

#[cfg(feature = "verification")]
pub use vahi_verified as verified;
// ponytail: F2.2 — vfs pseudo-fs (procfs/epoll/inotify) surface; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
pub mod vfs {
    pub use vahi_vfs::*;
}
#[cfg(not(target_arch = "aarch64"))]
// ponytail: F2.2 — legacy VGA text-mode Color variants unused under Limine framebuffer; see ADR-029-syscall-module-lint-hygiene.md
#[allow(dead_code)]
mod vga_buffer;

mod entry;
use entry::*;

use core::panic::PanicInfo;

/// Limine entry point — called by the Limine bootloader.
/// Reads all boot information from Limine static requests.
///
/// # Safety
/// Called by the Limine bootloader at physical entry. Must only be invoked
/// once per core with a valid Limine-compatible boot context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
    crate::limine::prevent_stripping();
    // Ultra-early COM1 probe: if Limine loaded us, this byte appears on serial.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") b'K',
            options(nostack, nomem, preserves_flags));
    }
    km_entry_marker();
    // Debug marker: O for first pass, Q for second pass
    extern "C" {
        static __kernel_start: u8;
    }
    const LINK_BASE: usize = 0xFFFFFFFF80000000;
    let kernel_start_addr = unsafe { &__kernel_start as *const u8 as usize };
    let marker = if kernel_start_addr != LINK_BASE {
        b'Q'
    } else {
        b'O'
    };
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!(
            "mov dx, 0x3f8",
            "mov al, {}",
            "out dx, al",
            in(reg_byte) marker,
            options(nostack, preserves_flags)
        );
    }
    kernel_main()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    panic_handler::handle_panic(info)
}
