//! Input handling — mouse and keyboard dispatch for the compositor.

use super::*;
use super::window;

impl Compositor {
    pub fn handle_mouse(&mut self, x: usize, y: usize, buttons: u8) {
        let left_pressed = (buttons & 1) != 0;
        let right_pressed = (buttons & 2) != 0;
        static mut PREV_LEFT_PRESSED: bool = false;
        static mut PREV_RIGHT_PRESSED: bool = false;

        let left_click = left_pressed && unsafe { !PREV_LEFT_PRESSED };
        let right_click_new = right_pressed && unsafe { !PREV_RIGHT_PRESSED };
        unsafe { PREV_LEFT_PRESSED = left_pressed; }
        unsafe { PREV_RIGHT_PRESSED = right_pressed; }

        // Context menu is open — handle dismissal or item selection
        if self.context_menu.open {
            if left_click {
                let item_h = 24;
                let menu_w = 160;
                let menu_h = self.context_menu.items.len() * item_h + 8;
                if x >= self.context_menu.x && x < self.context_menu.x + menu_w
                    && y >= self.context_menu.y && y < self.context_menu.y + menu_h
                {
                    let idx = (y - self.context_menu.y).saturating_sub(4) / item_h;
                    if idx < self.context_menu.items.len() {
                        self.execute_context_action(idx);
                    }
                }
                self.context_menu.open = false;
            } else if right_click_new {
                self.context_menu.open = false;
                return;
            }
        }

        // Right-click new press — open context menu
        if right_click_new {
            // Don't open context menu on taskbar
            if y >= SCREEN_HEIGHT - 40 && y < SCREEN_HEIGHT {
                return;
            }
            let mut hit = false;
            for (i, win) in self.windows.iter().enumerate().rev() {
                if win.is_within_title_bar(x, y) {
                    self.show_window_context_menu(x, y, i);
                    hit = true;
                    break;
                }
                if win.is_within_content(x, y) {
                    hit = true;
                    break;
                }
            }
            if !hit && y < SCREEN_HEIGHT - 40 {
                self.show_desktop_context_menu(x, y);
            }
            return;
        }

        // Original left-click handling
        if left_click {
             // Check start menu button
             if x >= 5 && x < 65 && y >= SCREEN_HEIGHT - 35 && y < SCREEN_HEIGHT - 5 {
                 self.start_menu_open = !self.start_menu_open;
                 return;
             }
              // Check start menu items
               if self.start_menu_open {
                   let (menu_x, menu_y, menu_w) = shell::start_menu_rects();
                   let header_h = 24;
                   if x >= menu_x && x < menu_x + menu_w && y >= menu_y && y < menu_y + header_h + shell::MENU_ITEM_COUNT * 36 + 10 {
                       let clicked_idx = (y.saturating_sub(menu_y + header_h + 5)) / 36;
                       if clicked_idx < shell::MENU_ITEM_COUNT {
                           self.start_menu_open = false;
                           match clicked_idx {
                               0 => self.create_file_manager_window(),
                               1 => self.create_terminal_window(),
                               2 => self.create_monitor_window(),
                                3 => self.create_info_window("About SARGA OS",
                                    "SARGA OS v0.3.0\n\nKernel: Vahi\nArch: x86_64\n\nA modern kernel\nwritten in Rust."),
                               4 => self.create_info_window("Settings", "Settings\n\nNot yet implemented.\nCheck back in a future release."),
                               5 => self.shutdown_qemu(),
                               _ => {}
                           }
                       }
                       return;
                   }
                   self.start_menu_open = false;
               }

                // Desktop icon clicks (SYSTEM, FILES)
               if y >= 20 && y < 70 && x >= 20 && x < 68 {
                    self.create_info_window("SYSTEM",
                        "SARGA OS System\n\nKernel: Vahi v0.3.0\nCPU: x86_64\nMemory: Managed");
                   return;
               }
               if y >= 100 && y < 150 && x >= 20 && x < 68 {
                   self.create_file_manager_window();
                   return;
               }
              // Check notification click-to-dismiss
              let mut notif_y = 50usize;
              for (n_idx, notif) in self.notifications.clone().iter().enumerate() {
                  let text_w = notif.text.len() * 8 + 36;
                  let nx = SCREEN_WIDTH - text_w - 10;
                  if x >= nx && x < nx + text_w && y >= notif_y && y < notif_y + 30 {
                      self.notifications.remove(n_idx);
                      return;
                  }
                  notif_y += 36;
              }
              // Check minimize/close buttons on all windows (reverse order = top first)
              for (i, win) in self.windows.iter().enumerate().rev() {
                   if win.is_minimize_button(x, y) {
                       self.damage.mark(self.windows[i].x, self.windows[i].y, self.windows[i].width, self.windows[i].height);
                       self.windows[i].minimized = !self.windows[i].minimized;
                       // Bring to front
                       let w = self.windows.remove(i);
                       self.windows.push(w);
                       return;
                   }
                  if win.is_close_button(x, y) {
                      self.close_pending = Some(i);
                      return;
                  }
              }
             // Check taskbar window buttons
             let taskbar_y_start = SCREEN_HEIGHT - 40;
             if y >= taskbar_y_start && y < SCREEN_HEIGHT - 5 {
                 let btn_x = 70usize;
                 for (i, _win) in self.windows.iter().enumerate() {
                     let bx = btn_x + i * 120;
                     if x >= bx && x < bx + 115 {
                         if self.windows[i].minimized {
                             self.windows[i].minimized = false;
                         }
                         // Bring to front
                         let w = self.windows.remove(i);
                         self.windows.push(w);
                         return;
                     }
                 }
             }
        }

        if left_pressed {
            if let Some(idx) = self.drag_index {
                if self.resize_edge != window::ResizeEdge::None {
                    let win = &mut self.windows[idx];
                    self.damage.mark(win.x, win.y, win.width, win.height);
                    match self.resize_edge {
                        window::ResizeEdge::Right => {
                            win.width = x.saturating_sub(win.x).max(150);
                        }
                        window::ResizeEdge::Bottom => {
                            win.height = y.saturating_sub(win.y).max(100);
                        }
                        window::ResizeEdge::Corner => {
                            win.width = x.saturating_sub(win.x).max(150);
                            win.height = y.saturating_sub(win.y).max(100);
                        }
                        _ => {}
                    }
                    self.damage.mark(win.x, win.y, win.width, win.height);
                } else {
                    let (old_x, old_y, old_w, old_h) = {
                        let w = &self.windows[idx];
                        (w.x, w.y, w.width, w.height)
                    };
                    self.damage.mark(old_x, old_y, old_w, old_h);
                    self.windows[idx].x = x.saturating_sub(self.drag_offset_x);
                    self.windows[idx].y = y.saturating_sub(self.drag_offset_y);
                    let new_win = &self.windows[idx];
                    self.damage.mark(new_win.x, new_win.y, new_win.width, new_win.height);
                }
            } else {
                // Check if we started dragging or interacting with content
                for (i, win) in self.windows.iter_mut().enumerate().rev() {
                    let edge = win.get_resize_edge(x, y);
                    if edge != window::ResizeEdge::None {
                        self.drag_index = Some(i);
                        self.resize_edge = edge;
                        self.drag_offset_x = x - win.x;
                        self.drag_offset_y = y - win.y;
                        break;
                    } else if win.is_within_title_bar(x, y) {
                        // Double-click check
                        let now = crate::interrupts::get_ticks();
                        if self.prev_click_win == Some(i) && now.saturating_sub(self.prev_click_ticks) < 50 {
                            win.toggle_maximize();
                            self.prev_click_win = None;
                        } else {
                            self.prev_click_ticks = now;
                            self.prev_click_win = Some(i);
                        }
                        self.drag_index = Some(i);
                        self.drag_offset_x = x - win.x;
                        self.drag_offset_y = y - win.y;
                        break;
                    } else if win.is_within_content(x, y) {
                        self.prev_click_win = None;
                        win.handle_mouse(x, y, true);
                        break;
                    } else {
                        self.prev_click_win = None;
                    }
                }
            }
        } else {
            if let Some(idx) = self.drag_index {
                // Snap zones: check mouse position on release
                let win = &mut self.windows[idx];
                if x < 5 {
                    win.x = 0;
                    win.y = 0;
                    win.width = SCREEN_WIDTH / 2;
                    win.height = SCREEN_HEIGHT - 40;
                } else if x > SCREEN_WIDTH - 5 {
                    win.x = SCREEN_WIDTH / 2;
                    win.y = 0;
                    win.width = SCREEN_WIDTH / 2;
                    win.height = SCREEN_HEIGHT - 40;
                } else if y < 5 {
                    win.toggle_maximize();
                }
                let w = self.windows.remove(idx);
                self.windows.push(w);
            }
            self.drag_index = None;
            self.resize_edge = window::ResizeEdge::None;

            if let Some(idx) = self.close_pending {
                if idx < self.windows.len() {
                    let win = &self.windows[idx];
                    self.damage.mark(win.x, win.y, win.width, win.height);
                    self.windows.remove(idx);
                }
                self.close_pending = None;
            }

            for win in &mut self.windows {
                win.handle_mouse(x, y, false);
            }
        }
    }

    pub fn handle_keyboard(&mut self, key: pc_keyboard::DecodedKey) {
        match key {
            pc_keyboard::DecodedKey::RawKey(raw) => {
                // Track modifier keys (main.rs also tracks via raw scancodes)
                match raw {
                    pc_keyboard::KeyCode::F4 if self.alt_held => {
                        if let Some(idx) = self.focused_window() {
                            self.close_pending = Some(idx);
                        }
                        self.alt_held = false;
                        return;
                    }
                    _ => {}
                }
                // Forward RawKey to focused window for non-character keys
                if let Some(idx) = self.focused_window() {
                    self.windows[idx].handle_keyboard(key);
                }
            }
            pc_keyboard::DecodedKey::Unicode(c) => {
                // Alt+Ctrl shortcut detection
                if self.alt_held && c == '\u{0004}' {
                    if let Some(idx) = self.focused_window() {
                        self.close_pending = Some(idx);
                    }
                    self.alt_held = false;
                    return;
                }
                // Super+Arrow: snap focused window
                if self.super_held {
                    if let Some(idx) = self.focused_window() {
                        match c {
                            '\u{0010}' => { // Left arrow
                                self.windows[idx].x = 0;
                                self.windows[idx].y = 0;
                                self.windows[idx].width = SCREEN_WIDTH / 2;
                                self.windows[idx].height = SCREEN_HEIGHT - 40;
                            }
                            '\u{0012}' => { // Right arrow
                                self.windows[idx].x = SCREEN_WIDTH / 2;
                                self.windows[idx].y = 0;
                                self.windows[idx].width = SCREEN_WIDTH / 2;
                                self.windows[idx].height = SCREEN_HEIGHT - 40;
                            }
                            '\u{0011}' => { // Up arrow = maximize
                                self.windows[idx].toggle_maximize();
                            }
                            '\u{000E}' => { // Down arrow = restore/minimize
                                self.windows[idx].minimized = !self.windows[idx].minimized;
                            }
                            _ => {}
                        }
                        self.super_held = false;
                        return;
                    }
                }
                // Alt+Tab: cycle window focus
                if self.alt_held && c == '\t' {
                    if !self.alt_tab_active {
                        self.alt_tab_active = true;
                        self.alt_tab_index = if self.windows.len() > 1 { self.windows.len() - 1 } else { 0 };
                    } else {
                        self.alt_tab_index = if self.alt_tab_index > 0 {
                            self.alt_tab_index - 1
                        } else {
                            self.windows.len().saturating_sub(1)
                        };
                    }
                    return;
                }
                // Alt released (non-tab char while alt held) = confirm Alt+Tab selection
                if self.alt_tab_active && self.alt_held && c != '\t' {
                    if self.alt_tab_index < self.windows.len() {
                        let idx = self.alt_tab_index;
                        self.windows[idx].minimized = false;
                        let w = self.windows.remove(idx);
                        self.windows.push(w);
                    }
                    self.alt_tab_active = false;
                    self.alt_held = false;
                    return;
                }
                // Escape: cancel Alt+Tab
                if self.alt_tab_active && c == '\u{001B}' {
                    self.alt_tab_active = false;
                    self.alt_held = false;
                    return;
                }

                if let Some(idx) = self.focused_window() {
                    self.windows[idx].handle_keyboard(key);
                }
            }
        }
    }
}
