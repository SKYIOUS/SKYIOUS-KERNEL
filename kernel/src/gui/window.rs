use crate::gui::drawing;
use crate::gui::{SCREEN_WIDTH, SCREEN_HEIGHT};

pub struct Window {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub title: &'static str,
    pub content: Option<alloc::boxed::Box<[u32]>>,
    pub phys_addr: Option<u64>, // Physical address for shared memory buffer
    pub widgets: alloc::vec::Vec<crate::gui::widgets::Widget>,
}

impl Window {
    pub fn new(x: usize, y: usize, width: usize, height: usize, title: &'static str) -> Self {
        Window { x, y, width, height, title, content: None, phys_addr: None, widgets: alloc::vec::Vec::new() }
    }

    pub fn render(&self, buffer: &mut [u32]) {
        // Draw Window Frame (Gray)
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.width, self.height, 0xFFC0C0C0);
        
        // Draw Title Bar (Blue)
        let title_bar_height = 20;
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.width, title_bar_height, 0xFF000080);
        
        // Draw Title Text (White)
        drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x + 5, self.y + 4, self.title, 0xFFFFFFFF);
        
        // Draw Window Border (Black)
        drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.width, 0xFF000000);
        drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y + self.height - 1, self.width, 0xFF000000);
        drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x, self.y, self.height, 0xFF000000);
        drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, self.x + self.width - 1, self.y, self.height, 0xFF000000);

        // Draw content if present
        let content_x = self.x + 1;
        let content_y = self.y + 21; // title bar is 20 + 1 line border
        let content_w = self.width.saturating_sub(2);
        let content_h = self.height.saturating_sub(22);

        if let Some(ref content) = self.content {
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
            // Render from physical memory directly (using kernel offset)
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

    pub fn is_within_title_bar(&self, mx: usize, my: usize) -> bool {
        mx >= self.x && mx < self.x + self.width && my >= self.y && my < self.y + 20
    }
    
    pub fn is_within_content(&self, mx: usize, my: usize) -> bool {
        mx >= self.x + 1 && mx < self.x + self.width - 1 && my >= self.y + 21 && my < self.y + self.height - 1
    }

    pub fn handle_mouse(&mut self, mx: usize, my: usize, pressed: bool) {
        let content_mx = mx.saturating_sub(self.x + 1);
        let content_my = my.saturating_sub(self.y + 21);
        
        for widget in &mut self.widgets {
            widget.handle_mouse(content_mx, content_my, pressed);
        }
    }

    pub fn handle_keyboard(&mut self, _key: pc_keyboard::DecodedKey) {
        // To be implemented by specific window types or widgets
    }
}
