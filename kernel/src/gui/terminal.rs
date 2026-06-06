use crate::gui::drawing;
use alloc::vec::Vec;
use alloc::string::String;

pub struct TerminalWidget {
    pub width_chars: usize,
    pub height_chars: usize,
    pub buffer: Vec<String>,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub current_line: String,
    pub scroll_offset: usize,
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
        }
    }

    pub fn handle_char(&mut self, c: char) {
        if c == '\n' {
            self.buffer.push(self.current_line.clone());
            self.current_line.clear();
            self.cursor_x = 0;
            self.cursor_y += 1;
        } else if c == '\u{0008}' { // Backspace
            if !self.current_line.is_empty() {
                self.current_line.pop();
                if self.cursor_x > 0 { self.cursor_x -= 1; }
            }
        } else {
            if self.cursor_x < self.width_chars {
                self.current_line.push(c);
                self.cursor_x += 1;
            }
        }

        if self.cursor_y >= self.height_chars {
            if !self.buffer.is_empty() {
                self.buffer.remove(0);
            }
            self.cursor_y = self.height_chars.saturating_sub(1);
        }
    }

    pub fn print_str(&mut self, s: &str) {
        for c in s.chars() {
            self.handle_char(c);
        }
    }

    pub fn render(&self, pixel_buffer: &mut [u32], pw: usize, ph: usize, start_x: usize, start_y: usize, _content_w: usize, _content_h: usize) {
        let term_w = self.width_chars * 8;
        let term_h = self.height_chars * 8;
        drawing::draw_rect(pixel_buffer, pw, ph, start_x, start_y, term_w, term_h, 0xFF000000);

        // Draw a green bottom border to indicate active terminal
        drawing::draw_line_h(pixel_buffer, pw, ph, start_x, start_y + term_h - 1, term_w, 0xFF00AA00);

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

        // Draw cursor block only when not scrolled back
        if self.scroll_offset == 0 {
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
}
