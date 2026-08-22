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
pub mod hal;
#[cfg(feature = "gpu")]
pub mod compositor;
mod selftest;
#[cfg(feature = "hypervisor")]
pub mod hypervisor;
pub mod boot;

use core::panic::PanicInfo;
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};
use crate::arch::Arch;


pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::FixedAddress(0xFFFF_8000_0000_0000));
    config.kernel_stack_size = 128 * 1024; // 128 KiB
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

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

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
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
    #[cfg(not(target_arch = "aarch64"))]
    let phys_mem_offset = x86_64::VirtAddr::new(boot_info.physical_memory_offset.into_option().expect("physical_memory_offset required"));
    #[cfg(not(target_arch = "aarch64"))]
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    #[cfg(target_arch = "aarch64")]
    let phys_mem_offset_val = boot_info.physical_memory_offset.into_option().expect("physical_memory_offset required");
    #[cfg(target_arch = "aarch64")]
    let mut mapper = unsafe { memory::init_aarch64(phys_mem_offset_val) };
    serial_write("[BOOT] memory::init done\n");

    let fb = boot_info.framebuffer.as_mut();
    if fb.is_some() { serial_write("[BOOT] fb=present\n"); }
    else { serial_write("[BOOT] fb=NONE\n"); }
    drivers::graphics::init(fb);
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
    unsafe { memory::init_frame_allocator(&boot_info.memory_regions) };
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
    let mut max_phys = 0;
    for region in boot_info.memory_regions.iter() {
        if region.end > max_phys {
            max_phys = region.end;
        }
    }
    memory::frame_info::init(max_phys);
    serial_write("[BOOT] -> VAHI Frame Tracker: OK\n");

    test_memory_allocations();

    #[cfg(not(target_arch = "aarch64"))]
    {
        serial_write("[BOOT] ACPI init...\n");
        acpi::init(boot_info.rsdp_addr.into_option());
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
    if let Some(ramdisk_addr) = boot_info.ramdisk_addr.into_option() {
        if boot_info.ramdisk_len > 0 {
            let ramdisk_slice = unsafe {
                core::slice::from_raw_parts(ramdisk_addr as *const u8, boot_info.ramdisk_len as usize)
            };
            *crate::vfs::RAMDISK.lock() = Some(ramdisk_slice);
            serial_write("[BOOT] initrd from bootloader\n");
        }
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
        selftest::run_all();
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
    let _ = executor.spawn(Task::new(gui_refresh_task()));
    executor.run();
}

pub async fn gui_refresh_task() {
    use pc_keyboard::{Keyboard, layouts, ScancodeSet1, HandleControl};
    use crate::task::keyboard::try_pop_scancode;

    // 100Hz tick / 30 FPS = 3.33, floor to 3
    const TICKS_PER_FRAME: u64 = 3;
    let mut last_frame_tick: u64 = 0;
    let mut kbd = Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore);

    loop {
        // Drain any pending scancodes
        while let Some(scancode) = try_pop_scancode() {
            // Track modifier keys via raw scancodes (make codes)
            {
                let mut comp = crate::gui::COMPOSITOR.lock();
                match scancode {
                    0x38 => { comp.alt_held = true; }      // Left Alt make
                    0xB8 => { comp.alt_held = false; }      // Left Alt break
                    0xE0 => { /* Extended prefix — next byte is the real scancode */ }
                    0x5B => { comp.super_held = true; }     // Left Win make (after 0xE0)
                    0xDB => { comp.super_held = false; }    // Left Win break (after 0xE0)
                    // Alt+Tab: confirm selection when Alt is released
                    _ if !comp.alt_held && comp.alt_tab_active => {
                        if comp.alt_tab_index < comp.windows.len() {
                            let idx = comp.alt_tab_index;
                            comp.windows[idx].minimized = false;
                            let w = comp.windows.remove(idx);
                            comp.windows.push(w);
                        }
                        comp.alt_tab_active = false;
                    }
                    _ => {}
                }
            }
            if let Ok(Some(key_event)) = kbd.add_byte(scancode) {
                if let Some(key) = kbd.process_keyevent(key_event) {
                    let mut comp = crate::gui::COMPOSITOR.lock();
                    comp.handle_keyboard(key);
                }
            }
        }

        // Use the 100Hz tick counter directly (not hal::timer::get_ticks which may be in microseconds)
        let now = crate::interrupts::get_ticks();

        // Diagnostic: print once at ~1s after first frame, then every 500 ticks
        if now > 100 {
            static DIAG_DONE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
            if !DIAG_DONE.swap(true, core::sync::atomic::Ordering::Relaxed) {
                let irq = crate::drivers::mouse::MOUSE_IRQ_COUNT.load(core::sync::atomic::Ordering::Relaxed);
                let bytes = crate::drivers::mouse::MOUSE_IRQ_BYTES.load(core::sync::atomic::Ordering::Relaxed);
                let cx = crate::drivers::mouse::CURSOR_X.load(core::sync::atomic::Ordering::Relaxed);
                let cy = crate::drivers::mouse::CURSOR_Y.load(core::sync::atomic::Ordering::Relaxed);
                crate::serial_write(&alloc::format!("[MOUSE-DIAG] irq={} bytes={} pos=({},{})\n", irq, bytes, cx, cy));
            }
        }

        if now.wrapping_sub(last_frame_tick) >= TICKS_PER_FRAME {
            last_frame_tick = now;
            let (x, y, buttons, scroll, mouse_x, mouse_y) = {
                use core::sync::atomic::Ordering;
                let x = crate::drivers::mouse::CURSOR_X.load(Ordering::Relaxed) as usize;
                let y = crate::drivers::mouse::CURSOR_Y.load(Ordering::Relaxed) as usize;
                let buttons = crate::drivers::mouse::CURSOR_BUTTONS.load(Ordering::Relaxed);
                let scroll = crate::drivers::mouse::CURSOR_SCROLL.swap(0, Ordering::Relaxed);
                (x, y, buttons, scroll, x, y)
            };
            let mut comp = crate::gui::COMPOSITOR.lock();
            comp.handle_mouse(x, y, buttons);
            if scroll != 0 {
                comp.handle_scroll(scroll);
            }
            comp.render(mouse_x, mouse_y);
        }
        // Yield to scheduler
        crate::task::YieldNow::new().await;
    }
}

#[cfg(feature = "net")]
pub async fn network_poll_task() {
    loop {
        crate::net::poll();
        core::hint::spin_loop();
        crate::task::YieldNow::new().await;
    }
}



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



/// Launch a userspace ELF binary at the given VFS path.
/// Spawns a new kernel thread that will load the binary and jump to usermode.
pub fn spawn_userspace_app(path: &'static str) {
    extern "C" fn app_starter() -> ! {
        let path = crate::APP_PATH_TO_LAUNCH.lock().clone();
        crate::serial_write(&alloc::format!("[LAUNCH] loading {}\n", path));
        let data = crate::vfs::VFS.lock().resolve_path(&path).and_then(|n| n.read(usize::MAX).ok());
        if let Some(elf_data) = data {
            use alloc::sync::Arc;
            let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
            if let Some(address_space) = crate::memory::paging::AddressSpace::new(&mut frame_allocator) {
                if let Ok(process) = crate::task::process::Process::load_elf(&elf_data, address_space) {
                    {
                        let mut c = process.creds.lock();
                        c.uid = 1000;
                        c.gid = 1000;
                        c.euid = 1000;
                        c.egid = 1000;
                    }
                    let entry = process.entry_point;
                    let process_arc = Arc::new(process);
                    crate::task::process::Process::register(process_arc.clone());
                    {
                        let mut cur = crate::task::process::CURRENT_PROCESS.lock();
                        *cur = Some(process_arc.clone());
                    }
                    {
                        let tty_node = crate::vfs::VFS.lock().resolve_path("/dev/tty0");
                        if let Some(tty) = tty_node {
                            use crate::task::process::FileDescriptor;
                            let mut fd_table = process_arc.fd_table.lock();
                            fd_table.resize(3, None);
                            fd_table[0] = Some(FileDescriptor::File { node: tty.clone(), offset: crate::sync::IrqSafeMutex::new(0) });
                            fd_table[1] = Some(FileDescriptor::File { node: tty.clone(), offset: crate::sync::IrqSafeMutex::new(0) });
                            fd_table[2] = Some(FileDescriptor::File { node: tty, offset: crate::sync::IrqSafeMutex::new(0) });
                            drop(fd_table);
                        }
                    }
                    crate::task::scheduler::with_current_thread(|thread| {
                        thread.process = Some(process_arc.clone());
                    });
                    unsafe { process_arc.address_space.activate(); }
                    let user_rsp = match process_arc.setup_user_stack(&alloc::vec![path.clone()]) {
                        Ok(rsp) => rsp,
                        Err(()) => {
                            crate::serial_write("[LAUNCH] OOM: failed to allocate user stack, halting\n");
                            loop { crate::arch::CurrentArch::halt(); }
                        }
                    };
                    unsafe { crate::task::thread::jump_to_usermode(entry, user_rsp); }
                }
            }
        }
        loop { core::hint::spin_loop(); }
    }
    let mut app_path = crate::APP_PATH_TO_LAUNCH.lock();
    *app_path = alloc::string::String::from(path);
    drop(app_path);
    let thread = crate::task::thread::Thread::new(app_starter);
    crate::task::scheduler::spawn_thread(thread);
}

lazy_static::lazy_static! {
    static ref APP_PATH_TO_LAUNCH: crate::sync::IrqSafeMutex<alloc::string::String> = crate::sync::IrqSafeMutex::new(alloc::string::String::new());
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    crate::serial_write("\n=== KERNEL PANIC ===\n");
    crate::serial_write("[PANIC] ");
    let msg = info.message();
    let panic_str = alloc::format!("{:?}", msg);
    crate::serial_write(&panic_str);
    crate::serial_write("\n");
    if let Some(loc) = info.location() {
        crate::serial_write("[PANIC] at ");
        crate::serial_write(loc.file());
        crate::serial_write(":");
        let line_str = alloc::format!("{}", loc.line());
        crate::serial_write(&line_str);
        crate::serial_write("\n");
    }
    // Dump boot trace if available
    crate::boot::with_trace(|trace, paths| {
        if let Some(events) = trace {
            crate::serial_write("[PANIC] Boot trace:\n");
            for event in events {
                crate::serial_write(&alloc::format!("  {:?}\n", event));
            }
        }
        if let Some(paths) = paths {
            crate::serial_write("[PANIC] Init paths searched:\n");
            for p in paths {
                crate::serial_write(&alloc::format!("  {}\n", p));
            }
        }
    });
    crate::debug::print_stack_trace();

    // Dump key registers
    #[cfg(target_arch = "x86_64")]
    // SAFETY: Reading CR2 in panic context to log page-fault address; no side effects.
    unsafe {
        let cr2: u64;
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
        crate::serial_write(&alloc::format!("[PANIC] CR2 (page fault addr): 0x{:x}\n", cr2));
    }

    loop { crate::arch::CurrentArch::halt(); }
}







