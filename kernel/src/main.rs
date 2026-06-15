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
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![deny(warnings)]

extern crate alloc;

mod vga_buffer;
mod interrupts;
mod gdt;
mod keyboard;
mod memory;
mod allocator;
mod shell;
mod task;
mod syscalls;
mod acpi;
mod vfs;
mod apic;
mod pci;
mod security;
mod tty;
pub mod drivers;
pub mod gui;
#[cfg(feature = "net")]
mod net;
pub mod korlang;
#[cfg(feature = "smp")]
mod smp;
pub mod debug;
#[cfg(feature = "ai_rule")]
pub mod vahiai;
pub mod elf_dyn;
pub mod emulation;
pub mod ebpf;
pub mod crypto;
pub mod pty;
pub mod arch;
#[cfg(feature = "self_test")]
mod selftest;
#[cfg(feature = "self_test")]
mod tests;

use core::panic::PanicInfo;
use bootloader_api::{entry_point, BootInfo, BootloaderConfig, config::Mapping};


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
    use x86_64::instructions::port::Port;
    let mut data = Port::<u8>::new(0x3f8);
    let mut lsr = Port::<u8>::new(0x3fd);
    let msg = b"\nPANIC: Stack smashing detected!\n";
    for &b in msg {
        unsafe { while lsr.read() & 0x20 == 0 {} }
        unsafe { data.write(b); }
    }
    loop { x86_64::instructions::hlt(); }
}

pub fn oom_kill() -> ! {
    use x86_64::instructions::port::Port;
    let mut data = Port::<u8>::new(0x3f8);
    let mut lsr = Port::<u8>::new(0x3fd);
    let msg = b"\n[OOM] Out of memory - killing process\n";
    for &b in msg {
        unsafe { while lsr.read() & 0x20 == 0 {} }
        unsafe { data.write(b); }
    }
    // Kill the last spawned userspace process (highest PID, excluding init=1 and kernel=0)
    let table = crate::task::process::PROCESS_TABLE.lock();
    let mut largest_pid: u64 = 0;
    for (pid, _) in table.iter() {
        if *pid > 1 && *pid > largest_pid {
            largest_pid = *pid;
        }
    }
    drop(table);
    if largest_pid > 1 {
        let msg2 = alloc::format!("[OOM] Killing pid {}\n", largest_pid);
        for &b in msg2.as_bytes() {
            unsafe { while lsr.read() & 0x20 == 0 {} }
            unsafe { data.write(b); }
        }
        // Send SIGKILL directly
        let table2 = crate::task::process::PROCESS_TABLE.lock();
        if let Some(proc) = table2.get(&largest_pid) {
            proc.signals.lock().raise(crate::syscalls::signal::Signal::_SIGKILL);
        }
    }
    loop { x86_64::instructions::hlt(); }
}

fn init_kaslr() {
    // Use RDTSC as cheap entropy (available on all x86_64; rdrand #UDs on QEMU's default CPU)
    let lo: u32;
    let hi: u32;
    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags)); }
    let val = ((hi as u64) << 32) | (lo as u64);
    let val = if val == 0 { 0x1000 } else { val };
    KERNEL_SLIDE.store(val & 0xFFFF_FFFF_FFFF_0000, core::sync::atomic::Ordering::Relaxed);
    // Seed the stack canary. The kernel is NOT compiled with -Z stack-protector
    // (no such flag in .cargo/config.toml), so no function has canary instrumentation.
    // This provides the symbol for external code that may reference it.
    unsafe { __stack_chk_guard = ((val << 1) | val.wrapping_mul(0x9E3779B97F4A7C15).rotate_left(17)) as usize; }
}

pub fn serial_putc(c: u8) {
    use x86_64::instructions::port::Port;
    unsafe {
        let mut data = Port::<u8>::new(0x3f8);
        let mut lsr = Port::<u8>::new(0x3fd);
        while lsr.read() & 0x20 == 0 {}
        data.write(c);
    }
}

pub fn serial_write(msg: &str) {
    for &b in msg.as_bytes() {
        serial_putc(b);
    }
}

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use x86_64::VirtAddr;
    use core::sync::atomic::Ordering;

    init_kaslr();

    unsafe {
        use x86_64::registers::control::Cr4;
        use x86_64::registers::control::Cr4Flags;
        // Query CPUID leaf 7 for feature bits
        let ebx7: u32;
        let ecx7: u32;
        core::arch::asm!(
            "push rbx",
            "mov eax, 7",
            "xor ecx, ecx",
            "cpuid",
            "mov {0:e}, ebx",
            "mov {1:e}, ecx",
            "pop rbx",
            out(reg) ebx7, out(reg) ecx7,
            out("eax") _, out("edx") _,
            options(nostack, preserves_flags));
        Cr4::update(|flags| {
            flags.insert(Cr4Flags::OSFXSR);
            flags.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
            if ebx7 & 1 != 0 {
                flags.insert(Cr4Flags::FSGSBASE);
                crate::task::thread::HAS_FSGSBASE.store(true, Ordering::SeqCst);
            }
            // SMEP (bit 20): CPUID.(EAX=7,ECX=0):EBX[7]
            // Cr4Flags doesn't export SMEP in x86_64 0.14.13, so set via raw bits
            if ebx7 & (1 << 7) != 0 {
                flags.insert(Cr4Flags::from_bits_truncate(0x100000));
            }
            // UMIP (bit 11): CPUID.(EAX=7,ECX=0):ECX[2]
            if ecx7 & (1 << 2) != 0 {
                flags.insert(Cr4Flags::from_bits_truncate(0x800));
            }
        });
    }

    serial_write("[BOOT] memory::init...\n");
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset.into_option().expect("physical_memory_offset required"));
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
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
    serial_write("[SPLASH] 🚀 SARGA OS loading...\n");

    serial_write("[BOOT] frame allocator...\n");
    unsafe { memory::init_frame_allocator(&boot_info.memory_regions) };
    let mut frame_allocator = memory::buddy::BuddyFrameAllocator;
    serial_write("[BOOT] heap init...\n");
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");
    serial_write("[BOOT] gdt init...\n");
    gdt::init();
    serial_write("[BOOT] idt+pic init...\n");
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    serial_write("[BOOT] syscalls init...\n");
    syscalls::init();
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

    serial_write("[BOOT] ACPI init...\n");
    acpi::init(boot_info.rsdp_addr.into_option());
    serial_write("[BOOT] APIC init...\n");
    apic::init();
    #[cfg(feature = "smp")]
    { serial_write("[BOOT] SMP init...\n"); smp::init(); }
    serial_write("[BOOT] PS/2 init...\n");
    drivers::ps2::init();
    serial_write("[BOOT] PCI enumerate...\n");
    pci::enumerate_pci();
    serial_write("[BOOT] VFS init...\n");
    vfs::init();
    #[cfg(feature = "net")]
    { serial_write("[BOOT] net init...\n"); net::init(); }

    // Now that the network stack is ready, enable E1000 interrupts
    #[cfg(feature = "net")]
    {
        if let Some(crate::drivers::net::NicDevice::E1000(ref dev)) = *crate::drivers::net::NIC.lock() {
            dev.lock().inner.enable_interrupts();
        }
    }
    serial_write("[BOOT] LSM init...\n");
    security::init();
    serial_write("[BOOT] korlang init...\n");
    korlang::init();
    #[cfg(feature = "ai_rule")]
    { serial_write("[BOOT] vahiai init...\n"); vahiai::init(); }
    serial_write("[BOOT] -> SARGA OS: Graphical Console Mode Active!\n");

    serial_write("[BOOT] RTC init...\n");
    drivers::rtc::init();
    serial_write("[BOOT] RTC initialized\n");

    #[cfg(feature = "self_test")]
    {
        serial_write("[SELF-TEST] registering tests...\n");
        tests::register_all();
        serial_write("[SELF-TEST] running...\n");
        selftest::run_all();
    }

    serial_write("[BOOT] scheduler init...\n");
    task::scheduler::init();
    serial_write("[BOOT] GUI init...\n");
    gui::init();

    serial_write("[BOOT] spawning run_async_tasks...\n");
    task::scheduler::spawn(run_async_tasks);
    serial_write("[BOOT] spawning init_os_task...\n");
    task::scheduler::spawn(init_os_task);

    serial_write("[BOOT] enabling interrupts...\n");
    x86_64::instructions::interrupts::enable();
    serial_write("[BOOT] interrupts enabled\n");

    serial_write("[BOOT] entering scheduler\n");
    task::scheduler::schedule();
}

extern "C" fn init_os_task() -> ! {
    crate::serial_write("[INIT] searching for /bin/init...\n");
    
    // Give VFS/Disk/PCI a moment to settle and discover devices
    for _ in 0..1_000_000 { core::hint::spin_loop(); }

    crate::serial_write("[INIT] spin done, locking VFS...\n");
    let init_data = {
        let search_paths = [
            "/bin/init",
            "/init",
            "/sbin/init",
        ];
        let mut data = None;
        let vfs_mgr = crate::vfs::VFS.lock();
        crate::serial_write("[INIT] VFS locked, resolving...\n");
        for path in search_paths {
            crate::serial_write("[INIT] checking: ");
            crate::serial_write(path);
            crate::serial_write("\n");
            if let Some(node) = vfs_mgr.resolve_path(path) {
                crate::serial_write("[INIT] FOUND!\n");
                data = node.read(usize::MAX).ok();
                break;
            }
        }
        drop(vfs_mgr);
        crate::serial_write("[INIT] VFS unlocked\n");
        data
    };

    if let Some(elf_data) = init_data {
        crate::serial_write("[INIT] Loading ELF...\n");
        use alloc::sync::Arc;
        let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
        let address_space = crate::memory::paging::AddressSpace::new(&mut frame_allocator)
            .expect("AS creation failed");
        crate::serial_write("[INIT] AS created\n");

        match crate::task::process::Process::load_elf(&elf_data, address_space) {
            Ok(process) => {
                crate::serial_write("[INIT] ELF loaded\n");
                let entry = process.entry_point;
                let process_arc = Arc::new(process);
                crate::serial_write("[INIT] register process...\n");
                crate::task::process::Process::register(process_arc.clone());
                crate::serial_write("[INIT] set current process...\n");
                {
                    let mut cur = crate::task::process::CURRENT_PROCESS.lock();
                    *cur = Some(process_arc.clone());
                }
                // NOTE: thread.process is set AFTER tty setup, to prevent timer ISR
                // from activating the user address space via prepare_switch before we
                // finish kernel-side initialization.
                // Open stdin/stdout/stderr as /dev/tty0
                {
                    let tty_node = crate::vfs::VFS.lock().resolve_path("/dev/tty0");
                    if let Some(tty) = tty_node {
                        use crate::task::process::FileDescriptor;
                        let mut fd_table = process_arc.fd_table.lock();
                        fd_table.resize(3, None);
                        fd_table[0] = Some(FileDescriptor::File { node: tty.clone(), offset: 0 });
                        fd_table[1] = Some(FileDescriptor::File { node: tty.clone(), offset: 0 });
                        fd_table[2] = Some(FileDescriptor::File { node: tty, offset: 0 });
                        drop(fd_table);
                        crate::serial_write("[INIT] opened /dev/tty0 as stdin/stdout/stderr\n");
                    } else {
                        crate::serial_write("[INIT] WARNING: /dev/tty0 not found!\n");
                    }
                }
                crate::serial_write("[INIT] set thread process...\n");
                {
                    if let Some(mut thread) = crate::task::scheduler::current_thread() {
                        thread.process = Some(process_arc.clone());
                        crate::task::scheduler::set_current_thread(thread);
                    }
                }
                crate::serial_write("[INIT] activate address space...\n");
                unsafe { process_arc.address_space.activate(); }
                crate::serial_write("[INIT] setup_user_stack...\n");
                let argv = alloc::vec!["/bin/init".into()];
                let user_rsp = process_arc.setup_user_stack(&argv);
                crate::serial_write("[INIT] entry=0x"); 
                let mut eb = [0u8; 16]; let mut ei = 16u8; let mut en = entry;
                loop { ei -= 1; let d = (en & 0xf) as u8; eb[ei as usize] = if d < 10 { b'0'+d } else { b'a'+d-10 }; en >>= 4; if en == 0 { break; } }
                crate::serial_write(core::str::from_utf8(&eb[ei as usize..]).unwrap_or("?"));
                crate::serial_write(" rsp=0x");
                let mut eb2 = [0u8; 16]; let mut ei2 = 16u8; let mut en2 = user_rsp;
                loop { ei2 -= 1; let d = (en2 & 0xf) as u8; eb2[ei2 as usize] = if d < 10 { b'0'+d } else { b'a'+d-10 }; en2 >>= 4; if en2 == 0 { break; } }
                crate::serial_write(core::str::from_utf8(&eb2[ei2 as usize..]).unwrap_or("?"));
                crate::serial_write("\n");
                crate::serial_write("[INIT] Jumping to userspace...\n");
                unsafe {
                    crate::task::thread::jump_to_usermode(entry, user_rsp);
                }
            }
            Err(e) => {
                crate::serial_write("[INIT] ELF load FAILED: ");
                crate::serial_write(e);
                crate::serial_write("\n");
            }
        }
    } else {
        crate::serial_write("[INIT] /bin/init not found.\n");
    }

    loop {
        core::hint::spin_loop();
    }
}

extern "C" fn run_async_tasks() -> ! {
    crate::serial_write("[ASYNC] Async Executor Started.\n");
    use task::{Task, executor::Executor};
    let mut executor = Executor::new();

    executor.spawn(Task::new(shell::kernel_shell()));
    executor.spawn(Task::new(network_poll_task()));
    executor.spawn(Task::new(gui_refresh_task()));
    executor.run();
}

pub async fn gui_refresh_task() {
    use pc_keyboard::{Keyboard, layouts, ScancodeSet1, HandleControl};
    use crate::task::keyboard::try_pop_scancode;

    const FPS: u64 = 30;
    const TICKS_PER_FRAME: u64 = 100 / FPS; // Assumes 100Hz timer
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

        let now = crate::interrupts::get_ticks();
        if now.wrapping_sub(last_frame_tick) >= TICKS_PER_FRAME {
            last_frame_tick = now;
            let (x, y, buttons, scroll, mouse_x, mouse_y) = {
                let m = crate::drivers::mouse::MOUSE.lock();
                (m.x, m.y, m.buttons, m.scroll, m.x, m.y)
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
    for _ in 0..1000000 {
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
                if let Ok(mut process) = crate::task::process::Process::load_elf(&elf_data, address_space) {
                    process.uid = spin::Mutex::new(1000);
                    process.gid = spin::Mutex::new(1000);
                    process.euid = spin::Mutex::new(1000);
                    process.egid = spin::Mutex::new(1000);
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
                            fd_table[0] = Some(FileDescriptor::File { node: tty.clone(), offset: 0 });
                            fd_table[1] = Some(FileDescriptor::File { node: tty.clone(), offset: 0 });
                            fd_table[2] = Some(FileDescriptor::File { node: tty, offset: 0 });
                            drop(fd_table);
                        }
                    }
                    if let Some(mut thread) = crate::task::scheduler::current_thread() {
                        thread.process = Some(process_arc.clone());
                        crate::task::scheduler::set_current_thread(thread);
                    }
                    unsafe { process_arc.address_space.activate(); }
                    let user_rsp = process_arc.setup_user_stack(&alloc::vec![path.clone()]);
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
    static ref APP_PATH_TO_LAUNCH: spin::Mutex<alloc::string::String> = spin::Mutex::new(alloc::string::String::new());
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    crate::debug::print_stack_trace();
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
    loop {}
}
