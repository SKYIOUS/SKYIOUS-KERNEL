//! # Vahi Kernel
//!
//! "Vahi" (वाहि) is derived from Sanskrit, meaning "the carrier" — that which
//! flows and transports. The kernel carries all processes, flows all data, and
//! transports instructions from software to hardware.
//!
//! The name was chosen for its clean pronunciation (VAH-hee), its absence from
//! existing software trademarks, and its subtle Sanskrit heritage that is
//! invisible to those unfamiliar with Vedic literature.
#![no_std]
#![no_main]
#![cfg_attr(not(target_arch = "aarch64"), feature(abi_x86_interrupt))]
#![feature(alloc_error_handler)]
#![deny(warnings)]
// Targeted clippy suppressions — each lint is named and documented.
#![allow(
    dead_code,
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
    clippy::needless_borrow,
    clippy::useless_format,
    clippy::map_entry,
    clippy::manual_range_contains,
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::match_ref_pats,
    clippy::redundant_closure,
    clippy::let_and_return,
    clippy::or_fun_call,
    clippy::single_char_pattern,
    clippy::redundant_field_names,
    clippy::clone_on_copy,
    clippy::iter_cloned_collect,
    clippy::unnecessary_unwrap,
    clippy::comparison_to_empty,
    clippy::write_with_newline,
    clippy::single_match,
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
    clippy::redundant_closure_for_method_calls,
    clippy::format_push_string,
    clippy::manual_let_else,
    clippy::redundant_else,
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
    clippy::empty_line_after_doc_comments,
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
mod acpi_prt;
mod allocator;
#[cfg(not(target_arch = "aarch64"))]
mod apic;
pub mod arch;
#[cfg(feature = "ash")]
pub mod ash;
pub mod boot;
#[cfg(feature = "gpu")]
pub mod compositor;
pub mod coverage;
pub mod crypto;
pub mod debug;
pub mod drivers;
pub mod ebpf;
pub mod elf_dyn;
pub mod emulation;
#[cfg(not(target_arch = "aarch64"))]
mod gdt;
pub mod gui;
pub mod hal;
#[cfg(feature = "hypervisor")]
pub mod hypervisor;
#[cfg(not(target_arch = "aarch64"))]
mod interrupts;
pub mod iommu;
pub mod ipc;
#[cfg(not(target_arch = "aarch64"))]
mod keyboard;
pub mod limine;
mod memory;
#[cfg(feature = "net")]
mod net;
pub mod objects;
#[cfg(not(target_arch = "aarch64"))]
mod pci;
pub mod pty;
mod security;
mod selftest;
mod shell;
#[cfg(feature = "smp")]
mod smp;
mod sync;
mod syscalls;
mod task;
mod tests;
mod tty;
#[cfg(feature = "verification")]
mod verified;
mod vfs;
#[cfg(not(target_arch = "aarch64"))]
mod vga_buffer;

use crate::arch::Arch;
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
    kernel_main()
}

/// KASLR: kernel base slide offset (0 if not randomized)
pub static KERNEL_SLIDE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

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
    loop {
        crate::arch::CurrentArch::halt();
    }
}

pub fn oom_kill() -> ! {
    crate::task::oom::handle_oom()
}

fn init_kaslr() {
    let val = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let val = if val == 0 { 0x1000 } else { val };
    // 30-bit entropy: 2MB-aligned offset up to 1GB. Önceki 16-bit (64KB range)
    // was trivially brutable. 30-bit = 512 possible slide values.
    // ponytail: increase to 40-bit when kernel supports 1GB huge page KASLR.
    KERNEL_SLIDE.store(
        val & 0x0000_0000_3FFF_F000,
        core::sync::atomic::Ordering::Relaxed,
    );
}

pub fn init_serial() {
    #[cfg(not(target_arch = "aarch64"))]
    let _ = crate::drivers::serial::init(0x3F8);
}

pub fn serial_putc(c: u8) {
    #[cfg(not(target_arch = "aarch64"))]
    {
        crate::drivers::serial::putc(c);
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let uart = 0x0900_0000 as *mut u32;
        while (uart.add(0x18 / 4).read_volatile() & (1 << 5)) != 0 {}
        uart.add(0x00).write_volatile(c as u32);
    }
}

pub fn serial_write(msg: &str) {
    for &b in msg.as_bytes() {
        serial_putc(b);
    }
}

/// Kernel-provided sink for `vahi-vfs` serial output: routes `/dev/tty0`
/// (userspace stdin/stdout/stderr) writes to the real serial port.
///
/// Referenced by `crates/vfs/src/lib.rs` as an `extern "Rust"` symbol;
/// a no-op stub is deliberately NOT defined in the crate (a duplicate
/// `#[no_mangle]` symbol fails the link).
#[no_mangle]
pub fn vahi_kernel_serial_putc(c: u8) {
    serial_putc(c);
}

/// Kernel-provided sink for `vahi-vfs` line output (ext4/tarfs/fuse
/// debug). See `vahi_kernel_serial_putc` for the symbol-ownership note.
#[no_mangle]
pub fn vahi_kernel_serial_write(msg: &str) {
    serial_write(msg);
}

fn kernel_main() -> ! {
    // Seed stack canary BEFORE any function with stack protection runs.
    let entropy = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let base = if entropy == 0 {
        0x9E3779B97F4A7C15
    } else {
        entropy
    };
    unsafe {
        __stack_chk_guard =
            ((base << 1) | base.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17)) as usize;
    }

    init_kaslr();
    init_serial();

    unsafe {
        crate::arch::CurrentArch::init_cpu();
    }

    #[cfg(not(target_arch = "aarch64"))]
    let hhdm = crate::limine::hhdm_offset();
    #[cfg(not(target_arch = "aarch64"))]
    let (mut mapper, mut frame_allocator) = unsafe {
        let phys_mem_offset = x86_64::VirtAddr::new(hhdm);
        boot::init::init_memory(phys_mem_offset)
    };
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] memory::init...\n");
        let hhdm = crate::limine::hhdm_offset();
        let mut mapper = unsafe { memory::init_aarch64(hhdm) };
        serial_write("[BOOT] memory::init done\n");
        serial_write("[BOOT] frame allocator...\n");
        unsafe { memory::init_frame_allocator_limine() };
        let mut frame_allocator = memory::buddy::BuddyFrameAllocator;
        serial_write("[BOOT] heap init...\n");
        allocator::init_heap(&mut mapper, &mut frame_allocator)
            .expect("heap initialization failed");
        serial_write("[BOOT] HHDM mapping done\n");
    }
    #[cfg(not(target_arch = "aarch64"))]
    unsafe {
        boot::init::init_graphics(&mut mapper, &mut frame_allocator)
    };

    crate::vga_buffer::init();
    #[cfg(feature = "ash")]
    crate::hal::exec_mem::init_pool();
    #[cfg(not(target_arch = "aarch64"))]
    boot::init::init_architecture();
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] arch init...\n");
        unsafe {
            crate::arch::CurrentArch::init_boot();
        }
    }

    #[cfg(feature = "self_test")]
    tests::init::test_memory_allocations();

    #[cfg(not(target_arch = "aarch64"))]
    boot::init::init_devices();
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] aarch64 platform init...\n");
    }
    boot::init::init_vfs_network();

    serial_write("[BOOT] RTC init...\n");
    let _ = drivers::rtc::init();
    serial_write("[BOOT] RTC initialized\n");

    #[cfg(feature = "verification")]
    {
        serial_write("[VERIFY] initializing verification runner...\n");
        use crate::verified::runner::VERIFICATION_RUNNER;
        VERIFICATION_RUNNER.lock().set_enabled(true);
        serial_write("[VERIFY] runtime invariant checking enabled\n");
    }

    serial_write("[BOOT] scheduler init...\n");
    task::scheduler::init();

    #[cfg(feature = "self_test")]
    {
        serial_write("[SELF-TEST] registering tests...\n");
        tests::register_all();
        serial_write("[SELF-TEST] running...\n");
        coverage::init();
        selftest::run_all();
        serial_write(&alloc::format!(
            "[COVERAGE] unique={}, total={}, ratio={:.4}\n",
            coverage::unique_blocks(),
            coverage::total_hits(),
            coverage::coverage_ratio()
        ));
    }
    serial_write("[BOOT] GUI init...\n");
    gui::init();

    task::scheduler::spawn(boot::tasks::run_async_tasks);
    task::scheduler::spawn(drivers::usb::usb_hid_poller);
    task::scheduler::spawn(boot::tasks::init_os_task);

    #[cfg(not(target_arch = "aarch64"))]
    {
        x86_64::instructions::interrupts::enable();
    }
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("msr daifclr, #2");
    } // Clear IRQ mask

    #[cfg(feature = "verification")]
    {
        let _vreport = crate::verified::runner::VERIFICATION_RUNNER.lock().report();
        serial_write("[VERIFY] boot-phase invariant checks complete\n");
    }

    task::scheduler::schedule();
    // schedule() returns only when the current thread is the sole runnable
    // work; the boot stack is parked at its first switch and never reaches
    // here, but kernel_main is `-> !`, so idle-wait instead of falling off.
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    panic_handler::handle_panic(info)
}
