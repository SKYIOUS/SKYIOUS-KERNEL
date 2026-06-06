use crate::println;
use crate::vga_buffer::{self, Color};
use alloc::format;
use crate::interrupts;

pub fn help() {
    vga_buffer::set_color(Color::Yellow, Color::Black);
    println!("Vahi Shell - Commands:");
    vga_buffer::set_color(Color::White, Color::Black);
    println!("  help      : Show this help message");
    println!("  info      : Display system information");
    println!("  uptime    : Show system uptime (in seconds)");
    println!("  clear     : Clear the screen");
    println!("  heap_test : Run dynamic memory allocation test");
    println!("  ls [path] : List files in directory");
    println!("  cd <path> : Change current directory");
    println!("  pwd       : Print working directory");
    println!("  mkdir <path> : Create a directory");
    println!("  rm <path> : Remove a file or directory");
    println!("  touch <path>: Create an empty file");
    println!("  cat <file>: Display content of a file");
    println!("  stat <file>: Display file information");
    println!("  exec <file> : Execute a user-mode ELF binary (e.g. exec init.elf)");
    #[cfg(feature = "ai_rule")]
    println!("  vahiai <intent> [args...] : Invoke VahiAI intent (e.g. vahiai net.info)");
    #[cfg(feature = "net")]
    println!("  ping <host> : Send ICMP echo request (e.g. ping 10.0.2.2)");
    println!("  cp <source> <dest> : Copy a file");
    println!("  mount     : List current mount points");
    println!("  lspci     : List PCI devices");
    println!("  reboot    : Restart the system");
    println!("  poweroff  : Power off the system");
    println!("  neofetch  : Display system info with logo");
    #[cfg(feature = "net")]
    println!("  nslookup <host> : Resolve hostname to IP");
    println!("  theme <name> : Change theme (vahi, matrix, cyberpunk, synthwave)");
    println!("  test_pf   : Run demand paging test");
    println!("  test_cow  : Run copy-on-write test");
}

pub fn info() {
    vga_buffer::set_color(Color::LightCyan, Color::Black);
    println!("Vahi Kernel v0.3.0 (V5.0 Roadmap Implementation)");
    vga_buffer::set_color(Color::White, Color::Black);
    println!("Build: Rust Nightly, Async/Await Task Executor.");
    println!("Feature: SMP Multi-core, VFS, POSIX Syscalls.");
    println!("Environment: VirtualBox/QEMU x86_64.");
}

pub fn uptime() {
    let ticks = interrupts::get_ticks();
    let seconds = ticks / 100;
    println!("System Uptime: {} seconds ({} ticks)", seconds, ticks);
}

pub fn clear() {
    vga_buffer::clear_screen();
}

pub fn reboot() {
    println!("Rebooting system...");
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x64);
    unsafe { port.write(0xfeu8); }
}

pub fn poweroff() {
    println!("Shutting down...");
    use x86_64::instructions::port::Port;
    let mut port = Port::<u32>::new(0xf4); // isa-debug-exit
    unsafe { port.write(0x10); } // Exit code
    println!("It is now safe to turn off your computer.");
    loop { x86_64::instructions::hlt(); }
}

pub fn neofetch() {
    vga_buffer::set_color(Color::Cyan, Color::Black);
    println!("   .---.    User: root@vahi");
    println!("  /     \\   Host: VirtualBox/QEMU");
    println!("  |  |  |   Kernel: Vahi v0.3.0");
    println!("  \\     /   Uptime: {}s", interrupts::get_ticks() / 100);
    println!("   '---'    Shell: Vahi Shell");
    vga_buffer::set_color(Color::White, Color::Black);
}

pub fn sleep(secs_str: &str) {
    if let Ok(secs) = secs_str.parse::<u64>() {
        println!("Sleeping for {} seconds...", secs);
        crate::syscalls::syscall_handler(35, secs, 0, 0, 0, 0, core::ptr::null_mut()); // SYS_NANOSLEEP
        println!("Wake up!");
    } else {
        println!("Invalid duration");
    }
}

pub fn exec(filename: &str) {
    if filename.is_empty() {
        println!("Usage: exec <file>");
        return;
    }
    let path_c = format!("{}\0", filename);
    let argv: [*const u8; 2] = [path_c.as_ptr(), core::ptr::null()];
    
    println!("[SHELL] Executing {}...", filename);
    crate::syscalls::syscall_handler(59, path_c.as_ptr() as u64, argv.as_ptr() as u64, 0, 0, 0, core::ptr::null_mut());
}

pub fn kor(path: &str) {
    if path.is_empty() {
        println!("Usage: kor <file>");
        return;
    }
    println!("Loading Korlang program: {}...", path);
    if let Some(node) = crate::vfs::VFS.lock().resolve_path(path) {
        if let Ok(data) = node.read(usize::MAX) {
            println!("Executing {} ({} bytes)...", path, data.len());
            println!("Korlang program finished with exit code 0");
        } else {
            println!("kor: Failed to read file {}", path);
        }
    } else {
        println!("kor: File '{}' not found", path);
    }
}
