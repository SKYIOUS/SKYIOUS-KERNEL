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
        drawing::draw_rect(pixel_buffer, pw, ph, start_x, start_y, term_w, term_h, 0xFF0C0C0C);

        if !self.is_monitor {
            drawing::draw_line_h(pixel_buffer, pw, ph, start_x, start_y + term_h - 1, term_w, 0xFF333333);
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
            drawing::draw_string(pixel_buffer, pw, ph, start_x, start_y + i * 8, line, 0xFFD4D4D4);
        }

        if self.scroll_offset == 0 && !self.is_monitor {
            drawing::draw_rect(pixel_buffer, pw, ph, start_x + self.cursor_x * 8, start_y + self.cursor_y * 8, 8, 8, 0xFF007ACC);
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

        use core::sync::atomic::{AtomicU32, Ordering};
        static FRAME_CNT: AtomicU32 = AtomicU32::new(0);
        FRAME_CNT.fetch_add(1, Ordering::Relaxed);
        if FRAME_CNT.load(Ordering::Relaxed) % 30 != 0 { return; }

        self.buffer.clear();
        self.current_line.clear();
        self.cursor_x = 0;
        self.cursor_y = 0;

        self.print_str("=== System Monitor ===\n\n");

        if let Some(node) = crate::vfs::VFS.lock().resolve_path("/ctl/sys/cpu/0/load") {
            if let Ok(data) = node.read(256) {
                if let Ok(s) = core::str::from_utf8(&data) {
                    self.print_str("CPU:  ");
                    self.print_str(s.trim());
                    self.print_str("\n");
                }
            }
        }

        if let Some(node) = crate::vfs::VFS.lock().resolve_path("/ctl/sys/mem/free") {
            if let Ok(data) = node.read(256) {
                if let Ok(s) = core::str::from_utf8(&data) {
                    self.print_str("Free: ");
                    self.print_str(s.trim());
                    self.print_str("\n");
                }
            }
        }

        if let Some(node) = crate::vfs::VFS.lock().resolve_path("/ctl/sys/mem/total") {
            if let Ok(data) = node.read(256) {
                if let Ok(s) = core::str::from_utf8(&data) {
                    self.print_str("Total:");
                    self.print_str(s.trim());
                    self.print_str("\n");
                }
            }
        }

        if let Some(node) = crate::vfs::VFS.lock().resolve_path("/ctl/kernel/uptime") {
            if let Ok(data) = node.read(256) {
                if let Ok(s) = core::str::from_utf8(&data) {
                    self.print_str("\nUptime: ");
                    self.print_str(s.trim());
                    self.print_str("\n");
                }
            }
        }

        self.print_str("\nProcesses:\n");
        if let Some(node) = crate::vfs::VFS.lock().resolve_path("/ctl/proc/list") {
            if let Ok(data) = node.read(4096) {
                if let Ok(s) = core::str::from_utf8(&data) {
                    for line in s.lines().take(15) {
                        self.print_str("  ");
                        self.print_str(line);
                        self.print_str("\n");
                    }
                }
            }
        }
    }

    fn execute_command(&mut self, command: &str) {
        if command.is_empty() { return; }

        let mut parts = command.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();

        // GUI clear resets terminal state (the shared table's clear is VGA-only)
        if cmd == "clear" {
            self.buffer.clear();
            self.current_line.clear();
            self.cursor_x = 0;
            self.cursor_y = 0;
            self.scroll_offset = 0;
            self.prompt_len = 0;
            return;
        }

        if !crate::shell::commands::dispatch(cmd, &args, &mut |s| self.print_str(s), true) {
            self.print_str(&format!("Unknown command: {}\n", cmd));
        }
    }
}
