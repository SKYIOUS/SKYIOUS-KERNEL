use alloc::format;

pub fn help(out: &mut dyn FnMut(&str)) {
    out("Commands:\n");
    out("  help       : Show this help message\n");
    out("  info       : Display system information\n");
    out("  uptime     : Show system uptime\n");
    out("  echo <t>   : Echo text\n");
    out("  date       : Show current date/time\n");
    out("  whoami     : Show current user\n");
    out("  ps         : List processes\n");
    out("  mem        : Memory info\n");
    out("  clear      : Clear the screen\n");
    out("  ls [path]  : List files\n");
    out("  cd <path>  : Change current directory\n");
    out("  pwd        : Print working directory\n");
    out("  mkdir <p>  : Create directory\n");
    out("  rm <path>  : Remove file or directory\n");
    out("  touch <p>  : Create empty file\n");
    out("  cat <file> : Display file content\n");
    out("  stat <file>: Display file information\n");
    out("  cp <s> <d> : Copy file\n");
    out("  mount      : List mount points\n");
    out("  exec <file>: Execute a user-mode ELF binary\n");
    out("  sleep <n>  : Sleep N seconds\n");
    out("  neofetch   : Display system info with logo\n");
    out("  heap_test  : Run heap allocation test\n");
    out("  lspci      : List PCI devices\n");
    out("  test_pf    : Run demand paging test [vga]\n");
    out("  test_cow   : Run copy-on-write test [vga]\n");
    out("  theme <n>  : Change console theme [vga]\n");
    out("  panic      : Trigger a kernel panic\n");
    out("  reboot     : Restart the system\n");
    out("  poweroff   : Power off the system\n");
    #[cfg(feature = "net")]
    out("  ping <ip>  : Send ICMP echo request\n");
    #[cfg(feature = "net")]
    out("  nslookup <host> : Resolve hostname\n");
    #[cfg(feature = "net")]
    out("  fetch <url>: Fetch a URL\n");
}

pub fn info(out: &mut dyn FnMut(&str)) {
    out("Vahi Kernel v0.3.0 (SARGA OS)\n");
    out("Build: Rust Nightly, Async/Await Task Executor.\n");
    out("Feature: SMP Multi-core, VFS, POSIX Syscalls.\n");
    out("Environment: QEMU x86_64.\n");
}

pub fn uptime(out: &mut dyn FnMut(&str)) {
    let ticks = crate::interrupts::get_ticks();
    let seconds = ticks / 100;
    out(&format!("Uptime: {} seconds ({} ticks)\n", seconds, ticks));
}

pub fn reboot(out: &mut dyn FnMut(&str)) {
    out("Rebooting system...\n");
    use x86_64::instructions::port::Port;
    let mut port = Port::new(0x64);
    unsafe { port.write(0xfeu8); }
}

pub fn poweroff(out: &mut dyn FnMut(&str)) {
    out("Shutting down...\n");
    use x86_64::instructions::port::Port;
    let mut port = Port::<u32>::new(0xf4); // isa-debug-exit
    unsafe { port.write(0x10); }
    out("It is now safe to turn off your computer.\n");
    loop { x86_64::instructions::hlt(); }
}

pub fn neofetch(out: &mut dyn FnMut(&str)) {
    out("   .---.    User: root@vahi\n");
    out("  /     \\   Host: QEMU\n");
    out("  |  |  |   Kernel: Vahi v0.3.0\n");
    out(&format!("  \\     /   Uptime: {}s\n", crate::interrupts::get_ticks() / 100));
    out("   '---'    Shell: SkyOS Terminal\n");
}

pub fn sleep(secs_str: &str, out: &mut dyn FnMut(&str)) {
    if let Ok(secs) = secs_str.parse::<u64>() {
        out(&format!("Sleeping for {} seconds...\n", secs));
        crate::syscalls::syscall_handler(35, secs, 0, 0, 0, 0, core::ptr::null_mut()); // SYS_NANOSLEEP
        out("Wake up!\n");
    } else {
        out("Invalid duration\n");
    }
}

pub fn exec(filename: &str, out: &mut dyn FnMut(&str)) {
    if filename.is_empty() {
        out("Usage: exec <file>\n");
        return;
    }
    let path_c = format!("{}\0", filename);
    let argv: [*const u8; 2] = [path_c.as_ptr(), core::ptr::null()];

    out(&format!("[SHELL] Executing {}...\n", filename));
    crate::syscalls::syscall_handler(59, path_c.as_ptr() as u64, argv.as_ptr() as u64, 0, 0, 0, core::ptr::null_mut());
}

pub fn echo(args: &[&str], out: &mut dyn FnMut(&str)) {
    let line = args.join(" ");
    out(&format!("{}\n", line));
}

pub fn date(out: &mut dyn FnMut(&str)) {
    let (secs, _) = crate::drivers::rtc::read_realtime();
    if secs <= 0 {
        out("RTC not available\n");
        return;
    }
    let total = secs as u64;
    let days = total / 86400;
    let s = total % 86400;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    let mut y = 1970u64;
    let mut d = days;
    loop {
        let leap = (y % 400 == 0) || (y % 4 == 0 && y % 100 != 0);
        let diy = if leap { 366 } else { 365 };
        if d < diy { break; }
        d -= diy;
        y += 1;
    }
    let leap = (y % 400 == 0) || (y % 4 == 0 && y % 100 != 0);
    let mdays: [u64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u64;
    for &md in mdays.iter() {
        if d < md { break; }
        d -= md;
        mo += 1;
    }
    let day = d + 1;
    out(&format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}\n", y, mo, day, h, m, sec));
}

pub fn whoami(out: &mut dyn FnMut(&str)) {
    out("root\n");
}

pub fn ps(out: &mut dyn FnMut(&str)) {
    out("PID  UID  CWD\n");
    let table = crate::task::process::PROCESS_TABLE.lock();
    for (pid, proc) in table.iter() {
        let cwd = proc.files.lock().cwd.clone();
        let uid = proc.creds.lock().uid;
        out(&format!("{:3}  {:3}  {}\n", pid, uid, cwd));
    }
}

pub fn mem(out: &mut dyn FnMut(&str)) {
    let free_pages = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
    let free_kb = (free_pages * 4) as u64;
    out(&format!("Free memory: ~{} KB ({} pages)\n", free_kb, free_pages));
}