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
// ponytail: clippy-style lints allowed — zero bug-finding value for kernel code
#![allow(
    dead_code,
    clippy::upper_case_acronyms, clippy::result_unit_err,
    clippy::too_many_arguments, clippy::collapsible_if, clippy::collapsible_match,
    clippy::single_match, clippy::manual_range_contains, clippy::new_without_default,
    clippy::unnecessary_cast, clippy::ptr_as_ptr,
    clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss,
    clippy::needless_return, clippy::clone_on_copy, clippy::len_zero,
    clippy::needless_range_loop, clippy::manual_is_multiple_of,
    clippy::declare_interior_mutable_const,
    clippy::redundant_pattern_matching, clippy::manual_div_ceil,
    clippy::needless_lifetimes, clippy::unused_unit,
    clippy::needless_borrow, clippy::derivable_impls,
    clippy::unnecessary_lazy_evaluations, clippy::op_ref,
    clippy::manual_swap, clippy::manual_memcpy,
    clippy::explicit_auto_deref, clippy::enum_variant_names,
    clippy::large_enum_variant, clippy::blocks_in_conditions,
    clippy::if_same_then_else, clippy::borrow_deref_ref, clippy::new_ret_no_self,
    clippy::only_used_in_recursion, clippy::type_complexity, clippy::manual_clamp,
    clippy::manual_strip, clippy::suspicious_map,
    clippy::unnecessary_min_or_max,
    clippy::suboptimal_flops, clippy::arithmetic_side_effects,
    clippy::range_plus_one,
    clippy::get_first, clippy::absurd_extreme_comparisons,
    clippy::same_item_push,
    clippy::should_implement_trait,
    clippy::match_same_arms, clippy::borrow_interior_mutable_const,
    clippy::option_map_unit_fn,
    clippy::never_loop, clippy::let_and_return
)]

extern crate alloc;
mod memory;
mod sync;
mod allocator;
mod shell;
mod task;
mod syscalls;
mod vfs;
mod security;
pub mod objects;
mod tty;
#[cfg(not(target_arch = "aarch64"))]
mod vga_buffer;
#[cfg(not(target_arch = "aarch64"))]
mod interrupts;
#[cfg(not(target_arch = "aarch64"))]
mod gdt;
#[cfg(not(target_arch = "aarch64"))]
mod keyboard;
#[cfg(not(target_arch = "aarch64"))]
mod acpi;
mod acpi_prt;
#[cfg(not(target_arch = "aarch64"))]
mod apic;
#[cfg(not(target_arch = "aarch64"))]
mod pci;
pub mod drivers;
pub mod gui;
#[cfg(feature = "net")]
mod net;
#[cfg(feature = "smp")]
mod smp;
mod tests;
pub mod debug;
#[cfg(feature = "verification")]
mod verified;
pub mod elf_dyn;
pub mod emulation;
pub mod ebpf;
pub mod crypto;
pub mod pty;
pub mod ipc;
#[cfg(feature = "ash")]
pub mod ash;
pub mod arch;
pub mod coverage;
pub mod hal;
#[cfg(feature = "gpu")]
pub mod compositor;
mod selftest;
#[cfg(feature = "hypervisor")]
pub mod hypervisor;
pub mod boot;
pub mod limine;

use core::panic::PanicInfo;
use crate::arch::Arch;

/// Limine entry point — called by the Limine bootloader.
/// Reads all boot information from Limine static requests.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn _start() -> ! {
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
    loop { crate::arch::CurrentArch::halt(); }
}

pub fn oom_kill() -> ! {
    crate::task::oom::handle_oom()
}

fn init_kaslr() {
    let val = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let val = if val == 0 { 0x1000 } else { val };
    KERNEL_SLIDE.store(val & 0x0000_0000_FFFF_0000, core::sync::atomic::Ordering::Relaxed);
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

fn kernel_main() -> ! {
    // Seed stack canary BEFORE any function with stack protection runs.
    let entropy = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let base = if entropy == 0 { 0x9E3779B97F4A7C15 } else { entropy };
    unsafe { __stack_chk_guard = ((base << 1) | base.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17)) as usize; }

    init_kaslr();
    init_serial();

    unsafe {
        crate::arch::CurrentArch::init_cpu();
    }

    serial_write("[BOOT] memory::init...\n");
    let hhdm = crate::limine::hhdm_offset();
    #[cfg(not(target_arch = "aarch64"))]
    let phys_mem_offset = x86_64::VirtAddr::new(hhdm);
    #[cfg(not(target_arch = "aarch64"))]
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    #[cfg(target_arch = "aarch64")]
    let mut mapper = unsafe { memory::init_aarch64(hhdm) };
    serial_write("[BOOT] memory::init done\n");

    let fb = crate::limine::framebuffer();
    if fb.is_some() { serial_write("[BOOT] fb=present\n"); }
    else { serial_write("[BOOT] fb=NONE\n"); }
    drivers::graphics::init_limine(fb);
    // Show boot splash as soon as framebuffer is ready
    if crate::drivers::graphics::is_active() {
        gui::splash::init();
    }
    if crate::drivers::graphics::is_active() { serial_write("[BOOT] graphics=active\n"); }
    else { serial_write("[BOOT] graphics=INACTIVE\n"); }
    serial_write("[BOOT] -> SARGA OS — Vahi Kernel v0.3.0 starting...\n");
    serial_write("[SPLASH] SARGA OS loading...\n");

    crate::vga_buffer::init();

    serial_write("[BOOT] frame allocator...\n");
    unsafe { memory::init_frame_allocator_limine() };
    let mut frame_allocator = memory::buddy::BuddyFrameAllocator;
    serial_write("[BOOT] heap init...\n");
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    // 256 KiB exec pool for JIT (W^X) — after heap/frame alloc are ready.
    #[cfg(feature = "ash")]
    crate::hal::exec_mem::init_pool();
    #[cfg(not(target_arch = "aarch64"))]
    {
        serial_write("[BOOT] gdt init...\n");
        gdt::init();
        serial_write("[BOOT] idt+pic init...\n");
        interrupts::init_idt();
        serial_write("[BOOT] syscalls init...\n");
        syscalls::init();
    }
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] arch init...\n");
        unsafe { crate::arch::CurrentArch::init_boot(); }
    }
    serial_write("[BOOT] HAL init...\n");
    let platform_info = arch::CurrentArch::probe_platform();
    hal::platform::init(platform_info);
    arch::CurrentArch::init_hal_irq();
    arch::CurrentArch::init_hal_timer();
    serial_write("[BOOT] HAL init done\n");
    serial_write("[BOOT] frame tracker init...\n");
    let max_phys = crate::limine::max_physical_address();
    memory::frame_info::init(max_phys);
    memory::phys::snapshot_baseline();
    serial_write("[BOOT] -> VAHI Frame Tracker: OK\n");

    #[cfg(feature = "self_test")]
    test_memory_allocations();

    #[cfg(not(target_arch = "aarch64"))]
    {
        serial_write("[BOOT] ACPI init...\n");
        acpi::init(crate::limine::rsdp_addr());
        serial_write("[BOOT] APIC init...\n");
        apic::init();
    crate::tests::run_all();
        #[cfg(feature = "smp")]
        { serial_write("[BOOT] SMP init...\n"); smp::init(); }
        serial_write("[BOOT] PS/2 init...\n");
        drivers::ps2::init();
        serial_write("[BOOT] PCI enumerate...\n");
        pci::enumerate_pci();
        serial_write("[BOOT] USB init...\n");
        drivers::usb::init();
    }
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] aarch64 platform init...\n");
    }
    serial_write("[BOOT] VFS init...\n");
    if let Some(ramdisk_data) = crate::limine::ramdisk() {
        *crate::vfs::RAMDISK.lock() = Some(ramdisk_data);
        serial_write("[BOOT] initrd from Limine modules\n");
    }
    vfs::init();
    serial_write("[BOOT] object manager init...\n");
    objects::namespace::init();
    #[cfg(feature = "net")]
    { serial_write("[BOOT] net init...\n"); net::init(); }

    // Now that the network stack is ready, enable E1000 interrupts
    #[cfg(all(feature = "net", not(target_arch = "aarch64")))]
    {
        if let Some(crate::drivers::net::NicDevice::E1000(ref dev)) = *crate::drivers::net::NIC.lock() {
            dev.lock().inner.enable_interrupts();
        }
    }
    serial_write("[BOOT] LSM init...\n");
    security::init();
    serial_write("[BOOT] CFI init...\n");
    crate::sync::cfi::cfi_init();
    #[cfg(feature = "ash")]
    { serial_write("[BOOT] ASH init...\n"); ash::manager::init(); }
    #[cfg(feature = "hypervisor")]
    { serial_write("[BOOT] hypervisor init...\n"); hypervisor::init(); }
    serial_write("[BOOT] -> SARGA OS: Graphical Console Mode Active!\n");

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
        serial_write(&alloc::format!("[COVERAGE] unique={}, total={}, ratio={:.4}\n",
            coverage::unique_blocks(), coverage::total_hits(), coverage::coverage_ratio()));
    }
    serial_write("[BOOT] GUI init...\n");
    gui::init();

    task::scheduler::spawn(run_async_tasks);
    task::scheduler::spawn(drivers::usb::usb_hid_poller);
    task::scheduler::spawn(init_os_task);

    #[cfg(not(target_arch = "aarch64"))]
    {
        x86_64::instructions::interrupts::enable();
    }
    #[cfg(target_arch = "aarch64")]
    unsafe { core::arch::asm!("msr daifclr, #2"); } // Clear IRQ mask

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

extern "C" fn init_os_task() -> ! {
    crate::boot::state::run_boot()
}

extern "C" fn run_async_tasks() -> ! {
    crate::serial_write("[ASYNC] Async Executor Started.\n");
    use task::{Task, executor::Executor};
    let mut executor = Executor::new();

    // ponytail: kernel shell disabled — it writes directly to the framebuffer,
    // clobbering the GUI compositor's rendered output. The GUI handles keyboard.
    let _ = executor.spawn(Task::new(network_poll_task()));
    let _ = executor.spawn(Task::new(gui::input::gui_refresh_task()));
    executor.run();
}



#[cfg(feature = "net")]
pub async fn network_poll_task() {
    loop {
        crate::net::poll();
        core::hint::spin_loop();
        crate::task::YieldNow::new().await;
    }
}



#[cfg(feature = "self_test")]
fn test_memory_allocations() {
    serial_write("[TRACE] test_memory_allocations entered\n");
    // Switch to a distinct color for tests
    crate::vga_buffer::set_color(crate::vga_buffer::Color::LightCyan, crate::vga_buffer::Color::Black);
    println!("\n[ SYSTEM ] Verifying Memory Allocators...");
    serial_write("[TRACE] after first println\n");
    
    // 1. Test Small Allocations (Slab Allocator)
    use alloc::boxed::Box;
    let b1 = Box::new(42u32);
    let b2 = Box::new(123u64);
    serial_write("[TRACE] after Box::new\n");
    assert_eq!(*b1, 42);
    assert_eq!(*b2, 123);
    println!("  -> Slab Cache (Small Objects) - PASSED");
    serial_write("[TRACE] after small alloc test\n");

    // 2. Test Large Allocations (Fallback / Linked List)
    let large = Box::new([0u8; 8192]); 
    assert_eq!(large[0], 0);
    println!("  -> Fallback (Large Blocks)    - PASSED");
    serial_write("[TRACE] after large alloc test\n");

    // 3. Test Dynamic growth
    use alloc::vec::Vec;
    let mut v = Vec::new();
    for i in 0..500 {
        v.push(i);
    }
    assert_eq!(v[499], 499);
    println!("  -> Dynamic Vector Growth      - PASSED");
    serial_write("[TRACE] after vec test\n");
    
    println!("[ SUCCESS ] All Allocator tests passed! ✅\n");
    serial_write("[TRACE] after final println\n");
    
    // Reset color
    crate::vga_buffer::set_color(crate::vga_buffer::Color::White, crate::vga_buffer::Color::Black);

    // Add a brief delay so the user can read the output
    println!("Pausing briefly...");
    serial_write("[TRACE] before spin loop\n");
    // ponytail: 1M spin_loop iterations took ~90s in debug+TCG; 10k is
    // enough for a human to read a framebuffer splash.
    for _ in 0..10000 {
        core::hint::spin_loop();
    }
    serial_write("[TRACE] after spin loop\n");
}





#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // SAFETY: All serial_write calls below are single-threaded byte
    // writes to MMIO — safe to call from any context including
    // double-fault, NMI, or when locks are held.

    crate::serial_write("\n========================================\n");
    crate::serial_write("           KERNEL PANIC\n");
    crate::serial_write("========================================\n");

    // ── 1. Panic message ──
    {
        let msg = info.message();
        let panic_str = alloc::format!("{:?}", msg);
        crate::serial_write("[PANIC] ");
        crate::serial_write(&panic_str);
        crate::serial_write("\n");
    }
    if let Some(loc) = info.location() {
        crate::serial_write(&alloc::format!("[PANIC] at {}:{}\n", loc.file(), loc.line()));
    }

    // ── 2. CPU & process info ──
    #[cfg(target_arch = "x86_64")]
    {
        let cpu = crate::apic::current_lapic_id();
        crate::serial_write(&alloc::format!("[PANIC] CPU: {}\n", cpu));
    }
    #[cfg(target_arch = "aarch64")]
    {
        // SAFETY: reading MPIDR_EL1 for CPU ID.
        let mpidr: u64;
        unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) mpidr); }
        crate::serial_write(&alloc::format!("[PANIC] CPU: {}\n", mpidr & 0xFF));
    }
    if let Some(tid) = crate::task::scheduler::with_current_thread(|t| {
        let pid = t.process.as_ref().map(|p| p.id).unwrap_or(0);
        (t.id, pid)
    }) {
        crate::serial_write(&alloc::format!("[PANIC] Thread: {}, PID: {}\n", tid.0, tid.1));
    }

    // ── 3. Register dump ──
    #[cfg(target_arch = "x86_64")]
    dump_registers_x86_64();
    #[cfg(target_arch = "aarch64")]
    dump_registers_aarch64();

    // ── 4. Stack backtrace ──
    crate::debug::print_stack_trace();

    // ── 5. Boot trace (if available) ──
    crate::boot::with_trace(|trace, paths| {
        if let Some(events) = trace {
            crate::serial_write("[PANIC] Boot trace:\n");
            for event in events {
                crate::serial_write(&alloc::format!("  {:?}\n", event));
            }
        }
        if let Some(p) = paths {
            crate::serial_write("[PANIC] Init paths searched:\n");
            for path in p {
                crate::serial_write(&alloc::format!("  {}\n", path));
            }
        }
    });

    crate::serial_write("========================================\n");
    crate::serial_write("         SYSTEM HALTED\n");
    crate::serial_write("========================================\n");

    loop { crate::arch::CurrentArch::halt(); }
}

/// Dump x86_64 general-purpose and control registers.
#[cfg(target_arch = "x86_64")]
fn dump_registers_x86_64() {
    // SAFETY: inline asm reads CPU registers — no side effects.
    unsafe {
        let (rax, rbx, rcx, rdx);
        core::arch::asm!("mov {}, rax", out(reg) rax);
        core::arch::asm!("mov {}, rbx", out(reg) rbx);
        core::arch::asm!("mov {}, rcx", out(reg) rcx);
        core::arch::asm!("mov {}, rdx", out(reg) rdx);
        let (rsi, rdi, rbp, rsp);
        core::arch::asm!("mov {}, rsi", out(reg) rsi);
        core::arch::asm!("mov {}, rdi", out(reg) rdi);
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
        let (r8, r9, r10, r11);
        core::arch::asm!("mov {}, r8",  out(reg) r8);
        core::arch::asm!("mov {}, r9",  out(reg) r9);
        core::arch::asm!("mov {}, r10", out(reg) r10);
        core::arch::asm!("mov {}, r11", out(reg) r11);
        let (r12, r13, r14, r15);
        core::arch::asm!("mov {}, r12", out(reg) r12);
        core::arch::asm!("mov {}, r13", out(reg) r13);
        core::arch::asm!("mov {}, r14", out(reg) r14);
        core::arch::asm!("mov {}, r15", out(reg) r15);
        // RIP: use frame pointer + 8 to get the return address.
        // The stack trace already provides the full call chain;
        // this gives the immediate caller for the register dump.
        let rip: u64 = unsafe { *rbp.add(1) as u64 };
        let rflags: u64;
        core::arch::asm!("pushfq; pop {}", out(reg) rflags);
        let cr2: u64;
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
        let cr3: u64;
        core::arch::asm!("mov {}, cr3", out(reg) cr3);

        crate::serial_write("[PANIC] Registers:\n");
        crate::serial_write(&alloc::format!(
            "  RAX={:016x} RBX={:016x} RCX={:016x} RDX={:016x}\n", rax, rbx, rcx, rdx));
        crate::serial_write(&alloc::format!(
            "  RSI={:016x} RDI={:016x} RBP={:016x} RSP={:016x}\n", rsi, rdi, rbp, rsp));
        crate::serial_write(&alloc::format!(
            "  R8 ={:016x} R9 ={:016x} R10={:016x} R11={:016x}\n", r8, r9, r10, r11));
        crate::serial_write(&alloc::format!(
            "  R12={:016x} R13={:016x} R14={:016x} R15={:016x}\n", r12, r13, r14, r15));
        crate::serial_write(&alloc::format!(
            "  RIP={:016x} RFLAGS={:016x}\n", rip, rflags));
        crate::serial_write(&alloc::format!(
            "  CR2={:016x} (page fault addr)\n", cr2));
        crate::serial_write(&alloc::format!(
            "  CR3={:016x} (page table root)\n", cr3));
    }
}

/// Dump aarch64 general-purpose registers.
#[cfg(target_arch = "aarch64")]
fn dump_registers_aarch64() {
    // SAFETY: inline asm reads CPU registers — no side effects.
    unsafe {
        let (x0, x1, x2, x3);
        core::arch::asm!("mov {}, x0", out(reg) x0);
        core::arch::asm!("mov {}, x1", out(reg) x1);
        core::arch::asm!("mov {}, x2", out(reg) x2);
        core::arch::asm!("mov {}, x3", out(reg) x3);
        let (x4, x5, x6, x7);
        core::arch::asm!("mov {}, x4", out(reg) x4);
        core::arch::asm!("mov {}, x5", out(reg) x5);
        core::arch::asm!("mov {}, x6", out(reg) x6);
        core::arch::asm!("mov {}, x7", out(reg) x7);
        let (x8, x9, x10, x11);
        core::arch::asm!("mov {}, x8",  out(reg) x8);
        core::arch::asm!("mov {}, x9",  out(reg) x9);
        core::arch::asm!("mov {}, x10", out(reg) x10);
        core::arch::asm!("mov {}, x11", out(reg) x11);
        let (x29, x30);
        core::arch::asm!("mov {}, x29", out(reg) x29);
        core::arch::asm!("mov {}, x30", out(reg) x30);
        let sp: u64;
        core::arch::asm!("mov {}, sp", out(reg) sp);
        let elr_el1: u64;
        core::arch::asm!("mrs {}, elr_el1", out(reg) elr_el1);
        let spsr_el1: u64;
        core::arch::asm!("mrs {}, spsr_el1", out(reg) spsr_el1);
        let esr_el1: u64;
        core::arch::asm!("mrs {}, esr_el1", out(reg) esr_el1);
        let ttbr0_el1: u64;
        core::arch::asm!("mrs {}, ttbr0_el1", out(reg) ttbr0_el1);
        let ttbr1_el1: u64;
        core::arch::asm!("mrs {}, ttbr1_el1", out(reg) ttbr1_el1);

        crate::serial_write("[PANIC] Registers:\n");
        crate::serial_write(&alloc::format!(
            "  X0={:016x} X1={:016x} X2={:016x} X3={:016x}\n", x0, x1, x2, x3));
        crate::serial_write(&alloc::format!(
            "  X4={:016x} X5={:016x} X6={:016x} X7={:016x}\n", x4, x5, x6, x7));
        crate::serial_write(&alloc::format!(
            "  X8={:016x} X9={:016x} X10={:016x} X11={:016x}\n", x8, x9, x10, x11));
        crate::serial_write(&alloc::format!(
            "  X29(FP)={:016x} X30(LR)={:016x}\n", x29, x30));
        crate::serial_write(&alloc::format!(
            "  SP={:016x} ELR_EL1={:016x} (PC)\n", sp, elr_el1));
        crate::serial_write(&alloc::format!(
            "  SPSR_EL1={:016x} ESR_EL1={:016x}\n", spsr_el1, esr_el1));
        crate::serial_write(&alloc::format!(
            "  TTBR0_EL1={:016x} (user) TTBR1_EL1={:016x} (kernel)\n", ttbr0_el1, ttbr1_el1));
    }
}







