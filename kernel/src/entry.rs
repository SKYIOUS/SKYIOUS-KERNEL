//! Kernel boot entry and low-level runtime hooks.

#[cfg(target_arch = "aarch64")]
use super::{allocator, memory};
use super::{boot, drivers, gui, init_kaslr, task};
#[cfg(feature = "self_test")]
use super::{coverage, selftest, tests};
use crate::arch::Arch;

pub fn oom_kill() -> ! {
    crate::task::oom::handle_oom()
}

/// Emit the early serial boot marker.
///
/// # Safety
/// On x86_64, the caller must be in kernel mode with access to COM1.
pub unsafe fn km_entry_marker() {
    #[cfg(target_arch = "x86_64")]
    // SAFETY: the boot entry runs in kernel mode, where COM1 I/O is permitted.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") 0x3F8u16,
            in("al") b'M',
            options(nostack, nomem, preserves_flags)
        );
    }
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
    // SAFETY: this platform exposes the PL011 MMIO registers at this address.
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

pub fn serial_write_hex(mut val: u64) {
    if val == 0 {
        serial_putc(b'0');
        return;
    }
    let mut buf = [0u8; 16];
    let mut i = 16;
    while val > 0 {
        i -= 1;
        let digit = (val & 0xF) as u8;
        buf[i] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        };
        val >>= 4;
    }
    for &b in &buf[i..] {
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

/// Late initialization helper: RTC and vDSO initialization.
// Must be called after frame allocator is ready and timer is calibrated.
fn init_rtc_and_vdso() {
    // Initialize RTC before vDSO (vDSO needs RTC for realtime initialization)
    serial_write("[BOOT] RTC init...\n");
    if let Err(_e) = drivers::rtc::init() {
        serial_write("[BOOT] WARNING: RTC init failed: ");
        serial_write(
            "[BOOT] RTC initialization failed, realtime will be zero until cached RTC updates\n",
        );
    }
    serial_write("[BOOT] RTC initialized\n");

    // Initialize vDSO page (requires frame allocator ready AND timer calibrated)
    #[cfg(target_arch = "x86_64")]
    {
        serial_write("[BOOT] vDSO init...\n");
        if let Err(()) = crate::vdso::init_vdso(crate::memory::buddy::BuddyFrameAllocator) {
            serial_write("[BOOT] ERROR: vDSO initialization failed\n");
        }
        serial_write("[BOOT] vDSO initialized\n");
    }
}

/// Post-KASLR continuation: runs after the trampoline switches to the randomized address space.
#[no_mangle]
pub extern "C" fn post_kaslr_continue() -> ! {
    // INSTRUMENTATION: Capture actual RIP at function entry to prove randomized execution
    let actual_rip: u64;
    // SAFETY: LEA reads the instruction address without touching memory or flags.
    unsafe {
        core::arch::asm!("lea {0}, [rip]", out(reg) actual_rip, options(nostack, preserves_flags));
    }
    let slide = crate::KASLR_DEBUG_SLIDE.load(core::sync::atomic::Ordering::Relaxed);
    serial_write("[KASLR-VERIFY] post_kaslr_continue actual RIP: 0x");
    serial_write_hex(actual_rip);
    serial_write("\n[KASLR-VERIFY] KERNEL_SLIDE: 0x");
    serial_write_hex(slide);
    serial_write("\n");
    // Compute expected link-time address of this function
    let link_va = post_kaslr_continue as *const () as u64;
    let unslid = actual_rip.wrapping_sub(slide);
    serial_write("[KASLR-VERIFY] link-time VA (post_kaslr_continue): 0x");
    serial_write_hex(link_va);
    serial_write("\n[KASLR-VERIFY] unslid RIP (actual - slide): 0x");
    serial_write_hex(unslid);
    serial_write("\n[KASLR-VERIFY] match: ");
    if unslid == link_va {
        serial_write("YES (actual_rip == link_va + slide)\n");
    } else {
        serial_write("NO\n");
    }

    // The KASLR relocations have been applied and CR3 has been switched.
    // Now continue with the rest of kernel initialization at the new virtual addresses.
    // Memory and graphics were already initialized before KASLR activation.

    crate::vga_buffer::init();
    #[cfg(feature = "ash")]
    crate::hal::exec_mem::init_pool();
    #[cfg(not(target_arch = "aarch64"))]
    boot::init::init_architecture();
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] arch init...\n");
        // SAFETY: the boot CPU already has a stack and active memory mappings.
        unsafe {
            crate::arch::CurrentArch::init_boot();
        }
    }

    #[cfg(feature = "self_test")]
    tests::init::test_memory_allocations();

    #[cfg(not(target_arch = "aarch64"))]
    // SAFETY: memory, GDT, IDT, and HAL are initialized before device probing.
    unsafe {
        boot::init::init_devices();
    }
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] aarch64 platform init...\n");
    }

    // Initialize RTC and vDSO
    init_rtc_and_vdso();

    boot::init::init_vfs_network();

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
    task::scheduler::spawn(boot::tasks::rtc_cache_update_task);

    #[cfg(not(target_arch = "aarch64"))]
    {
        x86_64::instructions::interrupts::enable();
    }
    #[cfg(target_arch = "aarch64")]
    // SAFETY: the boot path has installed the kernel interrupt handlers.
    unsafe {
        core::arch::asm!("msr daifclr, #2");
    } // Clear IRQ mask

    // Terminal marker for the boot-only QEMU gate: every startup step that
    // must complete before a boot counts as finished has now run.
    serial_write("[BOOT] init complete\n");

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

/// Late initialization entry point: runs after KASLR trampoline has switched CR3.
/// This is called from kernel_main() when KASLR_APPLIED is already set.
fn goto_late_init() -> ! {
    crate::serial_write("[DEBUG] goto_late_init entry\n");

    // Skip KASLR re-initialization; trampoline already ran.
    crate::serial_write("[DEBUG] calling init_serial\n");
    init_serial();

    // Early debug output to verify serial works
    let slide = crate::KASLR_DEBUG_SLIDE.load(core::sync::atomic::Ordering::Relaxed);
    serial_write("[KASLR] init: slide = 0x");
    serial_write_hex(slide);
    serial_write("\n[KASLR] init done\n");

    // T-00 test-mode announce
    #[cfg(feature = "self_test")]
    serial_write("[SELFTEST] T-00 test mode active\n");

    // SAFETY: this is the privileged boot path on the bootstrap CPU.
    unsafe {
        crate::arch::CurrentArch::init_cpu();
    }

    #[cfg(not(target_arch = "aarch64"))]
    let hhdm = crate::limine::hhdm_offset();
    #[cfg(not(target_arch = "aarch64"))]
    // SAFETY: Limine supplied the direct-map offset for the active page tables.
    let (mut _mapper, mut _frame_allocator) = unsafe {
        let phys_mem_offset = x86_64::VirtAddr::new(hhdm);
        boot::init::init_memory(phys_mem_offset)
    };
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] memory::init...\n");
        let hhdm = crate::limine::hhdm_offset();
        // SAFETY: Limine supplied the offset for the active direct map.
        let mut mapper = unsafe { memory::init_aarch64(hhdm) };
        serial_write("[BOOT] memory::init done\n");
        serial_write("[BOOT] frame allocator...\n");
        // SAFETY: the Limine memory map is still valid during boot.
        unsafe { memory::init_frame_allocator_limine() };
        let mut frame_allocator = memory::buddy::BuddyFrameAllocator;
        serial_write("[BOOT] heap init...\n");
        if allocator::init_heap(&mut mapper, &mut frame_allocator).is_err() {
            serial_write("[BOOT] heap initialization failed\n");
            crate::arch::CurrentArch::halt_loop();
        }
        serial_write("[BOOT] HHDM mapping done\n");
    }

    #[cfg(not(target_arch = "aarch64"))]
    // SAFETY: the mapper and frame allocator were just initialized for this boot.
    unsafe {
        boot::init::init_graphics(&mut _mapper, &mut _frame_allocator)
    };

    // Initialize RTC and vDSO
    init_rtc_and_vdso();

    // KASLR was already applied by the trampoline on first kernel_main entry.
    // This is the late init path - no need to activate KASLR again.
    // Debug: print KASLR slide value
    #[cfg(not(target_arch = "aarch64"))]
    {
        let slide = crate::KASLR_DEBUG_SLIDE.load(core::sync::atomic::Ordering::Relaxed);
        serial_write("[KASLR] slide = 0x");
        serial_write_hex(slide);
        serial_write("\n");
    }

    crate::serial_write("[DEBUG] before scheduler init\n");
    // Continue with normal late init
    serial_write("[BOOT] scheduler init...\n");
    crate::serial_write("[DEBUG] calling task::scheduler::init\n");
    task::scheduler::init();

    #[cfg(target_arch = "aarch64")]
    // SAFETY: the boot path has installed the kernel interrupt handlers.
    unsafe {
        core::arch::asm!("msr daifclr, #2");
    }

    // Terminal marker for the boot-only QEMU gate: every startup step that
    // must complete before a boot counts as finished has now run.
    serial_write("[BOOT] init complete\n");

    #[cfg(feature = "verification")]
    {
        let _vreport = crate::verified::runner::VERIFICATION_RUNNER.lock().report();
        serial_write("[VERIFY] boot-phase invariant checks complete\n");
    }

    task::scheduler::schedule();
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}

pub(super) fn kernel_main() -> ! {
    // SAFETY: kernel_main runs in kernel mode and can emit the COM1 marker.
    unsafe {
        core::arch::asm!(
            "out dx, al",
            in("dx") 0x3F8u16,
            in("al") b'N',
            options(nostack, nomem, preserves_flags)
        );
    }
    // Check if KASLR is already applied by comparing __kernel_start to LINK_BASE.
    // If KASLR was applied, __kernel_start will be at LINK_BASE + slide.
    extern "C" {
        static __kernel_start: u8;
    }
    const LINK_BASE: usize = 0xFFFFFFFF80000000;
    // SAFETY: the linker defines this symbol in the loaded kernel image.
    let kernel_start_addr = unsafe { &__kernel_start as *const u8 as usize };
    if kernel_start_addr != LINK_BASE {
        // KASLR already applied - jump to late init
        goto_late_init();
    }

    // Seed stack canary BEFORE any function with stack protection runs.
    let entropy = crate::crypto::GLOBAL_ENTROPY.get_u64();
    let base = if entropy == 0 {
        0x9E3779B97F4A7C15
    } else {
        entropy
    };
    // SAFETY: the bootstrap CPU writes the canary before other cores start.
    unsafe {
        crate::__stack_chk_guard =
            ((base << 1) | base.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17)) as usize;
    }

    init_kaslr();
    init_serial();

    // Early debug output to verify serial works - use direct writes to avoid alloc
    let slide = crate::KASLR_DEBUG_SLIDE.load(core::sync::atomic::Ordering::Relaxed);
    serial_write("[KASLR] init: slide = 0x");
    serial_write_hex(slide);
    serial_write("\n[KASLR] init done\n");

    // T-00 test-mode announce: emitted before any other output so the host
    // runner can distinguish an automated self_test run from a normal boot
    // even if a later boot stage fails. Never printed without the feature.
    #[cfg(feature = "self_test")]
    serial_write("[SELFTEST] T-00 test mode active\n");

    // SAFETY: this is the privileged boot path on the bootstrap CPU.
    unsafe {
        crate::arch::CurrentArch::init_cpu();
    }

    #[cfg(not(target_arch = "aarch64"))]
    let hhdm = crate::limine::hhdm_offset();
    #[cfg(not(target_arch = "aarch64"))]
    // SAFETY: Limine supplied the direct-map offset for the active page tables.
    let (mut _mapper, mut _frame_allocator) = unsafe {
        let phys_mem_offset = x86_64::VirtAddr::new(hhdm);
        boot::init::init_memory(phys_mem_offset)
    };
    #[cfg(target_arch = "aarch64")]
    {
        serial_write("[BOOT] memory::init...\n");
        let hhdm = crate::limine::hhdm_offset();
        // SAFETY: Limine supplied the offset for the active direct map.
        let mut mapper = unsafe { memory::init_aarch64(hhdm) };
        serial_write("[BOOT] memory::init done\n");
        serial_write("[BOOT] frame allocator...\n");
        // SAFETY: the Limine memory map is still valid during boot.
        unsafe { memory::init_frame_allocator_limine() };
        let mut frame_allocator = memory::buddy::BuddyFrameAllocator;
        serial_write("[BOOT] heap init...\n");
        if allocator::init_heap(&mut mapper, &mut frame_allocator).is_err() {
            serial_write("[BOOT] heap initialization failed\n");
            crate::arch::CurrentArch::halt_loop();
        }
        serial_write("[BOOT] HHDM mapping done\n");
    }

    #[cfg(not(target_arch = "aarch64"))]
    // SAFETY: the mapper and frame allocator were just initialized for this boot.
    unsafe {
        boot::init::init_graphics(&mut _mapper, &mut _frame_allocator)
    };

    // Debug: print KASLR slide value
    #[cfg(not(target_arch = "aarch64"))]
    {
        let slide = crate::KASLR_DEBUG_SLIDE.load(core::sync::atomic::Ordering::Relaxed);
        serial_write("[KASLR] slide = 0x");
        serial_write_hex(slide);
        serial_write("\n");

        // Activate KASLR: build new page tables, apply relocations, and jump via trampoline.
        // This must happen after memory is initialized (so we can allocate page tables)
        // but before we touch any relocated statics.
        // SAFETY: the active mappings and boot frame allocator are ready.
        unsafe {
            crate::kaslr_reloc::activate_kaslr_mapping();
        }
        // Reached only when the relocation pass refused: refusal makes zero
        // writes, so the link-time world is still coherent (no GOT slot was
        // slid) and the late boot can run from the link mapping, without a
        // slide, instead of jumping into the slid address space on
        // unrelocated pointers.
        post_kaslr_continue();
    }
}

/// Kernel-provided line output for `vahi-vfs`. See `vahi_kernel_serial_putc`.
#[no_mangle]
pub fn vahi_kernel_serial_write(msg: &str) {
    serial_write(msg);
}
