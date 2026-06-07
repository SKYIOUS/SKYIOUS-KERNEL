use crate::gui::drawing;
use crate::gui::{SCREEN_WIDTH, SCREEN_HEIGHT};

#[derive(Clone, Copy, PartialEq)]
pub enum ResizeEdge {
    None,
    Right,
    Bottom,
    Corner,
}

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title: &'static str,
    pub content: Option<alloc::boxed::Box<[u32]>>,
    pub phys_addr: Option<u64>, // Physical address for shared memory buffer
    pub widgets: alloc::vec::Vec<crate::gui::widgets::Widget>,
    pub minimized: bool,
    pub saved_rect: Option<(usize, usize, usize, usize)>,
    pub terminal: Option<crate::gui::terminal::TerminalWidget>,
    pub file_manager: Option<crate::gui::filemanager::FileManagerWidget>,
}

impl Window {
    pub fn new(x: usize, y: usize, width: usize, height: usize, title: &'static str) -> Self {
        Window { x, y, width, height, title, content: None, phys_addr: None, widgets: alloc::vec::Vec::new(), minimized: false, saved_rect: None, terminal: None, file_manager: None }
    }

    pub fn render(&self, buffer: &mut [u32]) {
        // Draw Window Frame (Gray)
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.width, self.height, 0xFFC0C0C0);
        
        // Draw Title Bar (Blue)
        let title_bar_height = 20;
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.width, title_bar_height, 0xFF000080);
        
        // Draw Title Text (White)
        drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x + 5, self.y + 4, self.title, 0xFFFFFFFF);

        // Draw Minimize Button (Yellow square with underscore)
        let mbx = self.x + self.width - 2 * (Self::BTN_SIZE + 2);
        let mby = self.y + 3;
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, mbx, mby, Self::BTN_SIZE, Self::BTN_SIZE, 0xFFE0B040);
        drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, mbx + 3, mby + Self::BTN_SIZE - 4, Self::BTN_SIZE - 6, 0xFFFFFFFF);

        // Draw Close Button (Red square with white X)
        let bx = self.x + self.width - Self::BTN_SIZE - 2;
        let by = self.y + 3;
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, by, Self::BTN_SIZE, Self::BTN_SIZE, 0xFFE04040);
        for i in 0..4 {
            drawing::draw_pixel(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx + 4 + i, by + 4 + i, 0xFFFFFFFF);
            drawing::draw_pixel(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx + 4 + i, by + 9 - i, 0xFFFFFFFF);
        }

        // Draw Window Border (Black)
        drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.width, 0xFF000000);
        drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y + self.height - 1, self.width, 0xFF000000);
        drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.height, 0xFF000000);
        drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x + self.width - 1, self.y, self.height, 0xFF000000);

        // Draw content area
        let content_x = self.x + 1;
        let content_y = self.y + 21;
        let content_w = self.width.saturating_sub(2);
        let content_h = self.height.saturating_sub(22);

        if let Some(ref term) = self.terminal {
            term.render(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, content_x, content_y, content_w, content_h);
        } else if let Some(ref fm) = self.file_manager {
            fm.render(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, content_x, content_y, content_w, content_h);
        } else if let Some(ref content) = self.content {
            for row in 0..content_h {
                for col in 0..content_w {
                    let target_y = content_y + row;
                    let target_x = content_x + col;
                    if target_x < SCREEN_WIDTH && target_y < SCREEN_HEIGHT {
                         buffer[target_y * SCREEN_WIDTH + target_x] = content[row * content_w + col];
                    }
                }
            }
        } else if let Some(phys) = self.phys_addr {
            let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().expect("phys offset not init");
            let k_ptr = (offset + phys) as *const u32;
            for row in 0..content_h {
                for col in 0..content_w {
                    let target_y = content_y + row;
                    let target_x = content_x + col;
                    if target_x < SCREEN_WIDTH && target_y < SCREEN_HEIGHT {
                         unsafe {
                             buffer[target_y * SCREEN_WIDTH + target_x] = *k_ptr.add(row * content_w + col);
                         }
                    }
                }
            }
        }

        // Render widgets
        for widget in &self.widgets {
            widget.render(buffer, self.x + 1, self.y + 21);
        }
    }

    pub const BTN_SIZE: usize = 14;
    pub const EDGE: usize = 4;

    pub fn get_resize_edge(&self, mx: usize, my: usize) -> ResizeEdge {
        let on_right = mx + Self::EDGE >= self.x + self.width && mx < self.x + self.width;
        let on_bottom = my + Self::EDGE >= self.y + self.height && my < self.y + self.height;
        if on_right && on_bottom { ResizeEdge::Corner }
        else if on_right && my >= self.y + 20 { ResizeEdge::Right }
        else if on_bottom { ResizeEdge::Bottom }
        else { ResizeEdge::None }
    }

    pub fn is_within_title_bar(&self, mx: usize, my: usize) -> bool {
        mx >= self.x && mx < self.x + self.width - 2 * (Self::BTN_SIZE + 2) && my >= self.y && my < self.y + 20
    }

    pub fn is_close_button(&self, mx: usize, my: usize) -> bool {
        let bx = self.x + self.width - Self::BTN_SIZE - 2;
        mx >= bx && mx < bx + Self::BTN_SIZE && my >= self.y + 3 && my < self.y + 3 + Self::BTN_SIZE
    }

    pub fn is_minimize_button(&self, mx: usize, my: usize) -> bool {
        let bx = self.x + self.width - 2 * (Self::BTN_SIZE + 2);
        mx >= bx && mx < bx + Self::BTN_SIZE && my >= self.y + 3 && my < self.y + 3 + Self::BTN_SIZE
    }
    
    pub fn is_within_content(&self, mx: usize, my: usize) -> bool {
        mx >= self.x + 1 && mx < self.x + self.width - 1 && my >= self.y + 21 && my < self.y + self.height - 1
    }

    pub fn handle_mouse(&mut self, mx: usize, my: usize, pressed: bool) {
        let content_mx = mx.saturating_sub(self.x + 1);
        let content_my = my.saturating_sub(self.y + 21);
        
        if pressed {
            if let Some(ref mut fm) = self.file_manager {
                if fm.handle_click(content_mx, content_my) {
                    return;
                }
            }
        }

        for widget in &mut self.widgets {
            widget.handle_mouse(content_mx, content_my, pressed);
        }
    }

    pub fn toggle_maximize(&mut self) {
        if let Some(saved) = self.saved_rect {
            self.x = saved.0;
            self.y = saved.1;
            self.width = saved.2;
            self.height = saved.3;
            self.saved_rect = None;
        } else {
            self.saved_rect = Some((self.x, self.y, self.width, self.height));
            self.x = 0;
            self.y = 0;
            self.width = crate::gui::SCREEN_WIDTH;
            self.height = crate::gui::SCREEN_HEIGHT;
        }
    }

    pub fn handle_scroll(&mut self, delta: i8) {
        if let Some(ref mut term) = self.terminal {
            term.handle_scroll(delta);
        } else if let Some(ref mut fm) = self.file_manager {
            fm.handle_scroll(delta);
        }
    }

    pub fn handle_keyboard(&mut self, key: pc_keyboard::DecodedKey) {
        if let Some(ref mut term) = self.terminal {
            match key {
                pc_keyboard::DecodedKey::Unicode(c) => {
                    if c == '\r' || c == '\n' {
                        term.handle_char('\n');
                    } else if c == '\u{0008}' || c == '\u{007f}' {
                        term.handle_char('\u{0008}');
                    } else {
                        term.handle_char(c);
                    }
                }
                pc_keyboard::DecodedKey::RawKey(_k) => {}
            }
        }
    }
}
