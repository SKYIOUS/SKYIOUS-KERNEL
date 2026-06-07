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
        self.print_str(&format!("Free pages: ~{}\n", free_pages));

        self.print_str("\n-- Processes --\n");
        let table = crate::task::process::PROCESS_TABLE.lock();
        for (pid, proc) in table.iter() {
            let cwd = proc.cwd.lock();
            self.print_str(&format!("  PID {}: cwd={}\n", pid, cwd));
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
                            let mut line = String::new();
                            for child in children {
                                if child.is_dir() {
                                    line.push_str(&child.name());
                                    line.push('/');
                                } else {
                                    line.push_str(&child.name());
                                }
                                line.push(' ');
                            }
                            self.print_str(&format!("{}\n", line.trim()));
                        }
                    } else {
                        self.print_str(&format!("{}\n", node.name()));
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
            "neofetch" => {
                let ticks = crate::interrupts::get_ticks();
                self.print_str("   .---.    User: root@skyos\n");
                self.print_str("  /     \\   Host: QEMU x86_64\n");
                self.print_str("  |  |  |   Kernel: Vahi v0.3.0\n");
                self.print_str(&format!("  \\     /   Uptime: {}s\n", ticks / 100));
                self.print_str("   '---'    Shell: SkyOS Terminal\n");
            }
            "ps" => {
                self.print_str("PID   CWD\n");
                let table = crate::task::process::PROCESS_TABLE.lock();
                for (pid, proc) in table.iter() {
                    let cwd = proc.cwd.lock();
                    self.print_str(&format!("{:3}   {}\n", pid, cwd));
                }
            }
            "mem" => {
                let free_pages = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let free_kb = free_pages * 4;
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
