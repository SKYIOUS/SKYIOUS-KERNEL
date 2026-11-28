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
    clippy::missing_safety_doc,
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
    // Mechanical style: safe to suppress, fix incrementally
    clippy::unnecessary_cast,
    clippy::map_entry,
    clippy::manual_range_contains,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::match_ref_pats,
    clippy::or_fun_call,
    clippy::single_char_pattern,
    clippy::clone_on_copy,
    clippy::iter_cloned_collect,
    clippy::unnecessary_unwrap,
    clippy::comparison_to_empty,
    clippy::write_with_newline,
    clippy::needless_return,
    clippy::manual_map,
    clippy::match_like_matches_macro,
    clippy::useless_attribute,
    clippy::manual_is_ascii_check,
    clippy::into_iter_on_ref,
    clippy::items_after_statements,
    clippy::needless_pass_by_value,
    clippy::redundant_pattern_matching,
    clippy::match_single_binding,
    clippy::from_over_into,
    clippy::enum_variant_names,
    clippy::module_name_repetitions,
    clippy::manual_div_ceil,
    clippy::identity_op,
    clippy::cast_ptr_alignment,
    clippy::arithmetic_side_effects,
    clippy::question_mark,
    clippy::if_same_then_else,
    clippy::stable_sort_primitive,
    clippy::repeat_once,
    clippy::unnecessary_lazy_evaluations,
    clippy::needless_range_loop,
    clippy::manual_clamp,
    clippy::needless_borrows_for_generic_args,
    clippy::string_add,
    clippy::uninlined_format_args,
    clippy::ref_option_ref,
    clippy::option_map_unit_fn,
    clippy::flat_map_option,
    clippy::manual_retain,
    clippy::manual_string_new,
    clippy::needless_late_init,
    clippy::init_numbered_fields,
    clippy::derivable_impls,
    clippy::manual_contains,
    clippy::single_char_add_str,
    clippy::needless_raw_string_hashes,
    clippy::explicit_auto_deref,
    clippy::unused_enumerate_index,
    clippy::bool_assert_comparison,
    clippy::empty_enums,
    clippy::collapsible_str_replace,
    clippy::unreadable_literal,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::absolute_paths,
    clippy::unconditional_recursion,
    clippy::fallible_impl_from,
    clippy::ignored_unit_patterns,
    clippy::missing_trait_methods,
    clippy::format_push_string,
    clippy::manual_let_else,
    clippy::unnecessary_to_owned,
    clippy::use_self,
    clippy::map_unwrap_or,
    clippy::get_first,
    clippy::suboptimal_flops,
    clippy::unnecessary_map_or,
    clippy::map_or_identity,
    clippy::unnecessary_sort_by,
    clippy::for_kv_map,
    clippy::manual_strip,
    clippy::same_item_push,
    clippy::vec_init_then_push,
    clippy::drop_non_drop,
    clippy::fn_to_numeric_cast,
    clippy::byte_char_slices,
    clippy::chunks_exact_to_as_chunks,
    clippy::declare_interior_mutable_const,
    clippy::doc_overindented_list_items,
    clippy::implicit_saturating_add,
    clippy::implicit_saturating_sub,
    clippy::int_plus_one,
    clippy::manual_abs_diff,
    clippy::manual_checked_ops,
    clippy::manual_memcpy,
    clippy::manual_repeat_n,
    clippy::manual_unwrap_or_default,
    clippy::needless_bool,
    clippy::needless_question_mark,
    clippy::new_ret_no_self,
    clippy::only_used_in_recursion,
    clippy::possible_missing_else,
    clippy::replace_box,
    clippy::should_implement_trait,
    clippy::unnecessary_mut_passed,
    clippy::unwrap_or_default,
    clippy::manual_unwrap_or,
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
    vdso_page, vdso_read_tsc_time, vdso_set_kernel_flag, vdso_update_pid_cpu,
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

/// Stack canary value for `-Z stack-protector=strong`
#[used]
#[no_mangle]
pub static mut __stack_chk_guard: usize = 0;

#[no_mangle]
pub extern "C" fn __stack_chk_fail() -> ! {
    let msg = b"\nPANIC: Stack smashing detected!\n";
    for &b in msg {
        serial_putc(b);
    }
    use crate::arch::Arch;
    loop {
        crate::arch::CurrentArch::halt();
    }
}

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
