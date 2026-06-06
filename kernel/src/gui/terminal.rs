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

    pub fn render(&self, pixel_buffer: &mut [u32], pw: usize, ph: usize, start_x: usize, start_y: usize) {
        // Draw background
        drawing::draw_rect(pixel_buffer, pw, ph, start_x, start_y, self.width_chars * 8, self.height_chars * 8, 0xFF000000); // Black background

        for (i, line) in self.buffer.iter().enumerate() {
            drawing::draw_string(pixel_buffer, pw, ph, start_x, start_y + i * 8, line, 0xFFFFFFFF);
        }
        
        // Draw current line
        drawing::draw_string(pixel_buffer, pw, ph, start_x, start_y + self.cursor_y * 8, &self.current_line, 0xFFFFFFFF);
        
        // Draw cursor block
        drawing::draw_rect(pixel_buffer, pw, ph, start_x + self.cursor_x * 8, start_y + self.cursor_y * 8, 8, 8, 0xFFAAAAAA);
    }
}
