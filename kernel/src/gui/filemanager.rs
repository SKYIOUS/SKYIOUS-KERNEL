use crate::gui::drawing;


pub struct FileManagerWidget {
    pub current_path: alloc::string::String,
    entries: alloc::vec::Vec<(alloc::string::String, bool)>,
    height_chars: usize,
    scroll_offset: usize,
}

impl FileManagerWidget {
    pub fn new(_width_pixels: usize, height_pixels: usize) -> Self {
        let hc = height_pixels / 8;
        let mut fm = FileManagerWidget {
            current_path: alloc::string::String::from("/"),
            entries: alloc::vec::Vec::new(),
            height_chars: hc,
            scroll_offset: 0,
        };
        fm.refresh();
        fm
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        let path = if self.current_path.is_empty() { "/" } else { &self.current_path };
        if let Some(node) = crate::vfs::VFS.lock().resolve_path(path) {
            if node.is_dir() {
                if let Ok(children) = node.children() {
                    // Add parent directory entry if not at root
                    if path != "/" {
                        self.entries.push((alloc::string::String::from(".."), true));
                    }
                    for child in children {
                        self.entries.push((child.name(), child.is_dir()));
                    }
                }
            }
        }
        if self.entries.is_empty() {
            self.entries.push((alloc::string::String::from("(empty)"), false));
        }
    }

    pub fn navigate_to(&mut self, path: &str) {
        self.current_path = alloc::string::String::from(path);
        self.scroll_offset = 0;
        self.refresh();
    }

    pub fn navigate_up(&mut self) {
        if self.current_path == "/" { return; }
        let parent = if self.current_path.ends_with('/') {
            let trimmed = &self.current_path[..self.current_path.len() - 1];
            match trimmed.rfind('/') {
                Some(pos) => alloc::string::String::from(&trimmed[..pos + 1]),
                None => alloc::string::String::from("/"),
            }
        } else {
            match self.current_path.rfind('/') {
                Some(0) => alloc::string::String::from("/"),
                Some(pos) => alloc::string::String::from(&self.current_path[..pos + 1]),
                None => alloc::string::String::from("/"),
            }
        };
        self.navigate_to(&parent);
    }

    pub fn navigate_into(&mut self, name: &str) {
        if name == ".." { self.navigate_up(); return; }
        let new_path = if self.current_path == "/" {
            alloc::format!("/{}", name)
        } else {
            alloc::format!("{}/{}", self.current_path.trim_end_matches('/'), name)
        };
        self.navigate_to(&new_path);
    }

    pub fn handle_click(&mut self, _mx: usize, my: usize) -> bool {
        let path_bar_height = 16usize;
        if my < path_bar_height { return false; }
        let list_y = my.saturating_sub(path_bar_height);
        let line_h = 12usize;
        let clicked_idx = list_y / line_h + self.scroll_offset;
        if clicked_idx < self.entries.len() {
            let is_dir = self.entries[clicked_idx].1;
            if is_dir {
                let name = self.entries[clicked_idx].0.clone();
                self.navigate_into(&name);
                return true;
            }
        }
        false
    }

    pub fn render(&self, pixel_buffer: &mut [u32], pw: usize, ph: usize, start_x: usize, start_y: usize, content_w: usize, _content_h: usize) {
        let path_bar_height = 16usize;
        // Draw path bar
        drawing::draw_rect(pixel_buffer, pw, ph, start_x, start_y, content_w, path_bar_height, 0xFF252526);
        let path_display = if self.current_path.len() > 40 {
            alloc::format!("...{}", &self.current_path[self.current_path.len().saturating_sub(37)..])
        } else {
            self.current_path.clone()
        };
        drawing::draw_string(pixel_buffer, pw, ph, start_x + 2, start_y + 4, &path_display, 0xFF007ACC);

        // Separator
        drawing::draw_line_h(pixel_buffer, pw, ph, start_x, start_y + path_bar_height, content_w, 0xFF333333);

        // List entries
        let list_start_y = start_y + path_bar_height + 1;
        let line_h = 12usize;
        for i in 0..self.height_chars.saturating_sub(1) {
            let idx = self.scroll_offset + i;
            if idx >= self.entries.len() { break; }
            let (ref name, is_dir) = &self.entries[idx];
            let ly = list_start_y + i * line_h;
            let color = if *is_dir { 0xFF007ACC } else { 0xFFCCCCCC };
            let display = if *is_dir {
                alloc::format!("d {}/", name)
            } else {
                alloc::format!("- {}", name)
            };
            drawing::draw_string(pixel_buffer, pw, ph, start_x + 4, ly, &display, color);
        }

        // Scroll indicators
        if self.scroll_offset > 0 {
            drawing::draw_string(pixel_buffer, pw, ph, start_x + content_w - 12, start_y + path_bar_height + 2, "^", 0xFF888888);
        }
        let max_offset = self.entries.len().saturating_sub(self.height_chars.saturating_sub(2));
        if self.scroll_offset < max_offset {
            let bottom_y = start_y + _content_h - 12;
            drawing::draw_string(pixel_buffer, pw, ph, start_x + content_w - 12, bottom_y, "^", 0xFF888888);
        }
    }

    pub fn handle_scroll(&mut self, delta: i8) {
        let max_offset = self.entries.len().saturating_sub(self.height_chars.saturating_sub(2));
        if delta > 0 {
            self.scroll_offset = self.scroll_offset.saturating_add(delta as usize).min(max_offset);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as usize);
        }
    }
}









