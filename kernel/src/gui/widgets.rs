//! Standard UI Widget Library

use crate::gui::drawing;
use crate::gui::{SCREEN_WIDTH, SCREEN_HEIGHT};
use alloc::string::String;

pub enum WidgetType {
    Button { text: String, pressed: bool },
    Label { text: String },
    Input { text: String, focused: bool },
}

pub struct Widget {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub data: WidgetType,
}

impl Widget {
    pub fn new_button(x: usize, y: usize, width: usize, height: usize, text: &str) -> Self {
        Widget {
            x, y, width, height,
            data: WidgetType::Button { text: String::from(text), pressed: false },
        }
    }

    pub fn new_label(x: usize, y: usize, text: &str) -> Self {
        Widget {
            x, y, width: text.len() * 8, height: 8,
            data: WidgetType::Label { text: String::from(text) },
        }
    }

    pub fn render(&self, buffer: &mut [u32], ox: usize, oy: usize) {
        let x = ox + self.x;
        let y = oy + self.y;

        match &self.data {
            WidgetType::Button { text, pressed } => {
                let color = if *pressed { 0xFF094771 } else { 0xFF0E639C };
                drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, y, self.width, self.height, color);
                let tx = x + (self.width.saturating_sub(text.len() * 8)) / 2;
                let ty = y + (self.height.saturating_sub(8)) / 2;
                drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, tx, ty, text, 0xFFFFFFFF);
            }
            WidgetType::Label { text } => {
                drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, y, text, 0xFFCCCCCC);
            }
            WidgetType::Input { text, focused } => {
                drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, y, self.width, self.height, 0xFF3C3C3C);
                let border = if *focused { 0xFF007ACC } else { 0xFF555555 };
                drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, y, self.width, border);
                drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, y + self.height - 1, self.width, border);
                drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, y, self.height, border);
                drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + self.width - 1, y, self.height, border);
                
                drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + 4, y + 4, text, 0xFFCCCCCC);
            }
        }
    }

    pub fn handle_mouse(&mut self, mx: usize, my: usize, pressed: bool) -> bool {
        if mx >= self.x && mx < self.x + self.width && my >= self.y && my < self.y + self.height {
            if let WidgetType::Button { pressed: ref mut p, .. } = self.data {
                *p = pressed;
            }
            return true;
        } else {
            if let WidgetType::Button { pressed: ref mut p, .. } = self.data {
                *p = false;
            }
        }
        false
    }
}
