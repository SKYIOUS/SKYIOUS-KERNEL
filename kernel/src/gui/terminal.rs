use crate::gui::drawing;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;

pub struct TerminalWidget {
    pub width_chars: usize,
    pub height_chars: usize,
    pub buffer: Vec<String>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub current_line: String,
    pub scroll_offset: usize,
    pub is_monitor: bool,
    pub monitor_lines: Vec<String>,
    prompt_len: usize,
}

impl TerminalWidget {
    pub fn new(width_pixels: usize, height_pixels: usize) -> Self {
        let width_chars = width_pixels / 8;
        let height_chars = height_pixels / 8;
        Self {
            width_chars,
            height_chars,
            buffer: Vec::new(),
            cursor_x: 0,
            cursor_y: 0,
            current_line: String::with_capacity(width_chars),
            scroll_offset: 0,
            is_monitor: false,
            monitor_lines: Vec::new(),
            prompt_len: 0,
        }
    }

    pub fn handle_char(&mut self, c: char) {
        if c == '\n' {
            let cmd = if self.cursor_x > self.prompt_len {
                alloc::string::String::from(self.current_line[self.prompt_len..].trim())
            } else {
                String::new()
            };
            self.buffer.push(self.current_line.clone());
            self.cursor_y += 1;
            self.current_line.clear();
            self.cursor_x = 0;
            self.prompt_len = 0;
            self.scroll_offset = 0;
            self.flush_scroll();
            self.execute_command(&cmd);
            self.write_prompt();
        } else if c == '\u{0008}' {
            if self.cursor_x > self.prompt_len {
                self.current_line.pop();
                self.cursor_x -= 1;
            }
        } else {
            if self.cursor_x < self.width_chars {
                self.current_line.push(c);
                self.cursor_x += 1;
            }
        }
        self.flush_scroll();
    }

    fn flush_scroll(&mut self) {
        if self.cursor_y >= self.height_chars {
            if !self.buffer.is_empty() {
                self.buffer.remove(0);
            }
            self.cursor_y = self.height_chars.saturating_sub(1);
        }
    }

    fn write_prompt(&mut self) {
        self.current_line = alloc::string::String::from("$ ");
        self.cursor_x = 2;
        self.prompt_len = 2;
    }

    pub fn print_str(&mut self, s: &str) {
        self.prompt_len = 0;
        for c in s.chars() {
            if c == '\n' {
                self.buffer.push(self.current_line.clone());
                self.current_line.clear();
                self.cursor_x = 0;
                self.cursor_y += 1;
            } else {
                if self.cursor_x < self.width_chars {
                    self.current_line.push(c);
                    self.cursor_x += 1;
                }
            }
            self.flush_scroll();
        }
    }

    pub fn render(&self, pixel_buffer: &mut [u32], pw: usize, ph: usize, start_x: usize, start_y: usize, _content_w: usize, _content_h: usize) {
        let term_w = self.width_chars * 8;
        let term_h = self.height_chars * 8;
        drawing::draw_rect(pixel_buffer, pw, ph, start_x, start_y, term_w, term_h, 0xFF000000);

        if !self.is_monitor {
            drawing::draw_line_h(pixel_buffer, pw, ph, start_x, start_y + term_h - 1, term_w, 0xFF00AA00);
        }

        let total_lines = self.buffer.len();
        let scroll_start = if self.scroll_offset > 0 {
            let offset = self.scroll_offset.min(total_lines);
            total_lines.saturating_sub(offset).saturating_sub(self.height_chars)
        } else {
            0
        };

        for i in 0..self.height_chars {
            let line_idx = scroll_start + i;
            let line = if line_idx < total_lines {
                &self.buffer[line_idx]
            } else if self.scroll_offset == 0 && line_idx == total_lines {
                &self.current_line
            } else {
                continue;
            };
            drawing::draw_string(pixel_buffer, pw, ph, start_x, start_y + i * 8, line, 0xFFFFFFFF);
        }

        if self.scroll_offset == 0 && !self.is_monitor {
            drawing::draw_rect(pixel_buffer, pw, ph, start_x + self.cursor_x * 8, start_y + self.cursor_y * 8, 8, 8, 0xFFAAAAAA);
        }
    }

    pub fn handle_scroll(&mut self, delta: i8) {
        let total_visible = self.buffer.len().saturating_add(1);
        let max_offset = total_visible.saturating_sub(self.height_chars);
        if delta > 0 {
            self.scroll_offset = self.scroll_offset.saturating_add(delta as usize).min(max_offset);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as usize);
        }
    }

    pub fn refresh_monitor(&mut self) {
        if !self.is_monitor { return; }
        self.buffer.clear();
        self.current_line.clear();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.prompt_len = 0;

        let ticks = crate::interrupts::get_ticks();
        let secs = ticks / 100;
        let mins = secs / 60;
        let hrs = mins / 60;

        self.print_str(&format!("===== System Monitor =====\n"));
        self.print_str(&format!("Uptime: {}h {}m {}s\n", hrs, mins % 60, secs % 60));
        self.print_str(&format!("Ticks: {}\n", ticks));

        let proc_count = crate::task::process::PROCESS_TABLE.lock().len();
        self.print_str(&format!("Processes: {}\n", proc_count));

        let free_pages = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
        let free_kb = (free_pages * 4) as u64;
        let total_kb = 131072u64; // ~512 MB total
        let used_kb = total_kb.saturating_sub(free_kb);
        let pct = if total_kb > 0 { used_kb * 100 / total_kb } else { 0 };
        self.print_str(&format!("Memory: {}% used ({} KB / {} KB)\n", pct, used_kb, total_kb));

        self.print_str("\nPID  UID  CWD\n");
        let table = crate::task::process::PROCESS_TABLE.lock();
        for (pid, proc) in table.iter() {
            let cwd = proc.cwd.lock();
            let uid = *proc.uid.lock();
            self.print_str(&format!("{:3}  {:3}  {}\n", pid, uid, cwd));
        }
    }

    fn execute_command(&mut self, command: &str) {
        if command.is_empty() { return; }

        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();

        match cmd {
            "help" => {
                self.print_str("Available commands:\n");
                self.print_str("  help      - Show this help\n");
                self.print_str("  clear     - Clear terminal\n");
                self.print_str("  info      - System info\n");
                self.print_str("  uptime    - Show uptime\n");
                self.print_str("  echo      - Echo text\n");
                self.print_str("  pwd       - Print working directory\n");
                self.print_str("  ls [path] - List files\n");
                self.print_str("  cat <file>- Show file content\n");
                self.print_str("  mkdir <p> - Create directory\n");
                self.print_str("  rm <path> - Remove file/dir\n");
                self.print_str("  touch <p> - Create empty file\n");
                self.print_str("  cp <s> <d>- Copy file\n");
                self.print_str("  stat <p>  - File info\n");
                self.print_str("  date      - Show date/time\n");
                self.print_str("  whoami    - Show user name\n");
                self.print_str("  sleep <n> - Sleep N seconds\n");
                self.print_str("  neofetch  - Display system info\n");
                self.print_str("  ps        - List processes\n");
                self.print_str("  mem       - Memory info\n");
                self.print_str("  reboot    - Restart system\n");
                self.print_str("  poweroff  - Shutdown\n");
            }
            "clear" => {
                self.buffer.clear();
                self.current_line.clear();
                self.cursor_x = 0;
                self.cursor_y = 0;
                self.scroll_offset = 0;
                self.prompt_len = 0;
            }
            "info" => {
                self.print_str("Vahi Kernel v0.3.0\n");
                self.print_str("Build: Rust Nightly, Async/Await\n");
                self.print_str("Feature: SMP, VFS, POSIX Syscalls\n");
                self.print_str("Environment: QEMU x86_64\n");
            }
            "uptime" => {
                let ticks = crate::interrupts::get_ticks();
                let secs = ticks / 100;
                self.print_str(&format!("Uptime: {} seconds ({} ticks)\n", secs, ticks));
            }
            "echo" => {
                let line = args.join(" ");
                self.print_str(&format!("{}\n", line));
            }
            "pwd" => {
                let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
                if let Some(ref p) = *proc_lock {
                    let cwd = p.cwd.lock();
                    self.print_str(&format!("{}\n", cwd));
                } else {
                    self.print_str("/\n");
                }
            }
            "ls" => {
                let path = args.get(0).copied().unwrap_or(".");
                let p = if path.is_empty() { "." } else { path };
                if let Some(node) = crate::vfs::VFS.lock().resolve_path(p) {
                    if node.is_dir() {
                        if let Ok(children) = node.children() {
                            for child in children {
                                if child.is_dir() {
                                    self.print_str(&format!("d {}/\n", child.name()));
                                } else {
                                    self.print_str(&format!("- {}\n", child.name()));
                                }
                            }
                        }
                    } else {
                        self.print_str(&format!("- {}\n", node.name()));
                    }
                } else {
                    self.print_str(&format!("ls: {}: No such file or directory\n", p));
                }
            }
            "cat" => {
                let filename = args.get(0).copied().unwrap_or("");
                if filename.is_empty() {
                    self.print_str("Usage: cat <file>\n");
                    return;
                }
                match crate::syscalls::sys_open_path(filename) {
                    Ok(fd) => {
                        let mut buf = [0u8; 4096];
                        let mut total = 0usize;
                        loop {
                            let r = crate::syscalls::syscall_handler(0, fd, buf.as_mut_ptr() as u64, 4000, 0, 0, core::ptr::null_mut());
                            if r <= 0 { break; }
                            if let Ok(s) = core::str::from_utf8(&buf[..r as usize]) {
                                self.print_str(s);
                            }
                            total += r as usize;
                        }
                        crate::syscalls::syscall_handler(3, fd, 0, 0, 0, 0, core::ptr::null_mut());
                        if total == 0 {
                            self.print_str("(empty)\n");
                        }
                    }
                    Err(_) => {
                        self.print_str(&format!("cat: {}: No such file\n", filename));
                    }
                }
            }
            "mkdir" => {
                let path = args.get(0).copied().unwrap_or("");
                if path.is_empty() {
                    self.print_str("Usage: mkdir <path>\n");
                    return;
                }
                let p = alloc::format!("{}\0", path);
                if crate::syscalls::syscall_handler(83, p.as_ptr() as u64, 0o755, 0, 0, 0, core::ptr::null_mut()) != 0 {
                    self.print_str(&format!("mkdir: failed to create {}\n", path));
                }
            }
            "rm" => {
                let path = args.get(0).copied().unwrap_or("");
                if path.is_empty() {
                    self.print_str("Usage: rm <path>\n");
                    return;
                }
                let p = alloc::format!("{}\0", path);
                if crate::syscalls::syscall_handler(87, p.as_ptr() as u64, 0, 0, 0, 0, core::ptr::null_mut()) != 0 {
                    self.print_str(&format!("rm: failed to remove {}\n", path));
                }
            }
            "touch" => {
                let path = args.get(0).copied().unwrap_or("");
                if path.is_empty() {
                    self.print_str("Usage: touch <path>\n");
                    return;
                }
                let p = alloc::format!("{}\0", path);
                let fd = crate::syscalls::syscall_handler(2, p.as_ptr() as u64, 0x40, 0, 0, 0, core::ptr::null_mut());
                if fd < 1000 {
                    crate::syscalls::syscall_handler(3, fd, 0, 0, 0, 0, core::ptr::null_mut());
                } else {
                    self.print_str(&format!("touch: failed to create {}\n", path));
                }
            }
            "stat" => {
                let path = args.get(0).copied().unwrap_or("");
                if path.is_empty() {
                    self.print_str("Usage: stat <path>\n");
                    return;
                }
                let p = alloc::format!("{}\0", path);
                let mut sb = crate::vfs::Stat {
                    st_dev: 0, st_ino: 0, st_mode: 0, st_nlink: 0,
                    st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
                    st_atime: 0, st_mtime: 0, st_ctime: 0,
                };
                let res = crate::syscalls::syscall_handler(4, p.as_ptr() as u64, &mut sb as *mut _ as u64, 0, 0, 0, core::ptr::null_mut());
                if res == 0 {
                    let kind = if sb.st_mode & crate::vfs::S_IFDIR != 0 { "directory" } else { "file" };
                    self.print_str(&format!("  File: {}\n", path));
                    self.print_str(&format!("  Type: {}\n", kind));
                    self.print_str(&format!("  Size: {} bytes\n", sb.st_size));
                    self.print_str(&format!("  Mode: {:o}\n", sb.st_mode));
                    self.print_str(&format!("  Inode: {}\n", sb.st_ino));
                } else {
                    self.print_str(&format!("stat: {}: No such file\n", path));
                }
            }
            "date" => {
                let (secs, _) = crate::drivers::rtc::read_realtime();
                if secs <= 0 {
                    self.print_str("RTC not available\n");
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
                self.print_str(&format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}\n", y, mo, day, h, m, sec));
            }
            "whoami" => {
                self.print_str("root\n");
            }
            "sleep" => {
                let secs_str = args.get(0).copied().unwrap_or("");
                if secs_str.is_empty() {
                    self.print_str("Usage: sleep <seconds>\n");
                    return;
                }
                if let Ok(secs) = secs_str.parse::<u64>() {
                    crate::syscalls::syscall_handler(35, secs, 0, 0, 0, 0, core::ptr::null_mut());
                    self.print_str("Wake up!\n");
                } else {
                    self.print_str("Invalid duration\n");
                }
            }
            "cp" => {
                let src = args.get(0).copied().unwrap_or("");
                let dst = args.get(1).copied().unwrap_or("");
                if src.is_empty() || dst.is_empty() {
                    self.print_str("Usage: cp <source> <dest>\n");
                    return;
                }
                let src_c = alloc::format!("{}\0", src);
                let dst_c = alloc::format!("{}\0", dst);
                let fd_src = crate::syscalls::syscall_handler(2, src_c.as_ptr() as u64, 0, 0, 0, 0, core::ptr::null_mut());
                if fd_src >= 0xFFFF_FFFF_FFFF_FF00 {
                    self.print_str(&format!("cp: failed to open source {}\n", src));
                } else {
                    let fd_dst = crate::syscalls::syscall_handler(2, dst_c.as_ptr() as u64, 0x41, 0, 0, 0, core::ptr::null_mut());
                    if fd_dst >= 0xFFFF_FFFF_FFFF_FF00 {
                        self.print_str(&format!("cp: failed to create destination {}\n", dst));
                        crate::syscalls::syscall_handler(3, fd_src, 0, 0, 0, 0, core::ptr::null_mut());
                    } else {
                        let mut buf = [0u8; 4096];
                        loop {
                            let n = crate::syscalls::syscall_handler(0, fd_src, buf.as_mut_ptr() as u64, 4096u64, 0, 0, core::ptr::null_mut());
                            if n == 0 || n >= 0xFFFF_FFFF_FFFF_FF00 { break; }
                            crate::syscalls::syscall_handler(1, fd_dst, buf.as_ptr() as u64, n as u64, 0, 0, core::ptr::null_mut());
                        }
                        crate::syscalls::syscall_handler(3, fd_src, 0, 0, 0, 0, core::ptr::null_mut());
                        crate::syscalls::syscall_handler(3, fd_dst, 0, 0, 0, 0, core::ptr::null_mut());
                        self.print_str(&format!("cp: copied {} to {}\n", src, dst));
                    }
                }
            }
            "neofetch" => {
                let ticks = crate::interrupts::get_ticks();
                self.print_str("   .---.    User: root@skyos\n");
                self.print_str("  /     \\   Host: QEMU x86_64\n");
                self.print_str("  |  |  |   Kernel: Vahi v0.3.0\n");
                self.print_str(&format!("  \\     /   Uptime: {}s\n", ticks / 100));
                self.print_str("   '---'    Shell: SkyOS Terminal\n");
            }
            "ps" => {
                self.print_str("PID  UID  CWD\n");
                let table = crate::task::process::PROCESS_TABLE.lock();
                for (pid, proc) in table.iter() {
                    let cwd = proc.cwd.lock();
                    let uid = *proc.uid.lock();
                    self.print_str(&format!("{:3}  {:3}  {}\n", pid, uid, cwd));
                }
            }
            "mem" => {
                let free_pages = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let free_kb = (free_pages * 4) as u64;
                self.print_str(&format!("Free memory: ~{} KB ({} pages)\n", free_kb, free_pages));
            }
            "reboot" => {
                self.print_str("Rebooting...\n");
                use x86_64::instructions::port::Port;
                let mut port = Port::new(0x64);
                unsafe { port.write(0xfeu8); }
            }
            "poweroff" => {
                self.print_str("Shutting down...\n");
                unsafe { x86_64::instructions::port::Port::<u16>::new(0x604).write(0x2000); }
                x86_64::instructions::interrupts::disable();
                loop { x86_64::instructions::hlt(); }
            }
            _ => {
                self.print_str(&format!("Unknown command: {}\n", cmd));
            }
        }
    }
}


