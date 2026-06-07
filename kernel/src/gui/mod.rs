//! GUI Subsystem
//!
//! Provides window management, compositing, and double-buffering.

pub mod drawing;
pub mod mouse;
pub mod window;
pub mod shell;
pub mod terminal;
pub mod widgets;
pub mod filemanager;

use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::sync::atomic::Ordering;
use crate::drivers::graphics::FRAMEBUFFER;

pub const SCREEN_WIDTH: usize = 800;
pub const SCREEN_HEIGHT: usize = 600;

lazy_static::lazy_static! {
    pub static ref COMPOSITOR: Mutex<Compositor> = Mutex::new(Compositor::new());
}

pub struct Compositor {
    backbuffer: Box<[u32]>,
    background_cache: Box<[u32]>,
    pub windows: Vec<window::Window>,
    drag_index: Option<usize>,
    drag_offset_x: usize,
    drag_offset_y: usize,
    start_menu_open: bool,
    close_pending: Option<usize>,
    prev_click_ticks: u64,
    prev_click_win: Option<usize>,
    resize_edge: window::ResizeEdge,
}

impl Compositor {
    pub fn new() -> Self {
        let size = SCREEN_WIDTH * SCREEN_HEIGHT;
        let mut buffer = Vec::with_capacity(size);
        for _ in 0..size { buffer.push(0x001A237E); } // Deep Blue Background
        
        let backbuffer = buffer.into_boxed_slice();
        let mut bg_cache = alloc::vec::Vec::with_capacity(size);
        for _ in 0..size { bg_cache.push(0x001A237E); }
        let bg_cache = bg_cache.into_boxed_slice();
        Self {
            backbuffer,
            background_cache: bg_cache,
            windows: Vec::new(),
            drag_index: None,
            drag_offset_x: 0,
            drag_offset_y: 0,
            start_menu_open: false,
            close_pending: None,
            prev_click_ticks: 0,
            prev_click_win: None,
            resize_edge: window::ResizeEdge::None,
        }
    }

    pub fn add_window(&mut self, window: window::Window) {
        self.windows.push(window);
    }

    fn create_terminal_window(&mut self) {
        let term_w = 600;
        let term_h = 360;
        let mut term_win = window::Window::new(100, 60, term_w + 2, term_h + 22, "Terminal");
        term_win.terminal = Some(crate::gui::terminal::TerminalWidget::new(term_w, term_h));
        if let Some(ref mut t) = term_win.terminal {
            t.print_str("SkyOS Terminal v0.1\n");
            t.print_str("Type commands below...\n\n$ ");
        }
        self.windows.push(term_win);
    }

    fn create_info_window(&mut self, title: &'static str, body: &str) {
        let w = 340;
        let h = 200;
        let mut info_win = window::Window::new(120, 80, w, h, title);
        let mut term = crate::gui::terminal::TerminalWidget::new(w - 4, h - 24);
        term.print_str(body);
        info_win.terminal = Some(term);
        self.windows.push(info_win);
    }

    fn create_monitor_window(&mut self) {
        let w = 360;
        let h = 280;
        let mut mon_win = window::Window::new(150, 90, w, h, "System Monitor");
        let mut term = crate::gui::terminal::TerminalWidget::new(w - 4, h - 24);
        term.is_monitor = true;
        term.refresh_monitor();
        mon_win.terminal = Some(term);
        self.windows.push(mon_win);
    }

    fn create_file_manager_window(&mut self) {
        let w = 400;
        let h = 300;
        let mut fm_win = window::Window::new(130, 70, w, h, "File Manager");
        fm_win.file_manager = Some(crate::gui::filemanager::FileManagerWidget::new(w - 4, h - 24));
        self.windows.push(fm_win);
    }

    fn shutdown_qemu(&mut self) {
        unsafe { x86_64::instructions::port::Port::<u16>::new(0x604).write(0x2000); }
        x86_64::instructions::interrupts::disable();
        loop { x86_64::instructions::hlt(); }
    }

    pub fn handle_mouse(&mut self, x: usize, y: usize, buttons: u8) {
        let left_pressed = (buttons & 1) != 0;
        static mut PREV_LEFT_PRESSED: bool = false;

        if left_pressed && unsafe { !PREV_LEFT_PRESSED } {
             // New click - check start menu button
             if x >= 5 && x < 65 && y >= SCREEN_HEIGHT - 35 && y < SCREEN_HEIGHT - 5 {
                 self.start_menu_open = !self.start_menu_open;
                 unsafe { PREV_LEFT_PRESSED = left_pressed; }
                 return;
             }
             // Check start menu items
              if self.start_menu_open && x >= 5 && x < 185 && y >= SCREEN_HEIGHT - 250 && y < SCREEN_HEIGHT - 250 + shell::MENU_ITEM_COUNT * 36 + 10 {
                  let menu_y = SCREEN_HEIGHT - 250;
                  let clicked_idx = (y.saturating_sub(menu_y + 5)) / 36;
                  if clicked_idx < shell::MENU_ITEM_COUNT {
                      self.start_menu_open = false;
                      match clicked_idx {
                          0 => self.create_file_manager_window(),
                          1 => self.create_terminal_window(),
                          2 => self.create_monitor_window(),
                          3 => self.create_info_window("About SkyOS",
                               "SkyOS v0.3.0\n\nKernel: Vahi\nArch: x86_64\n\nA modern kernel\nwritten in Rust."),
                          4 => self.create_info_window("Settings", "Settings\n\nNot yet implemented.\nCheck back in a future release."),
                          5 => self.shutdown_qemu(),
                          _ => {}
                      }
                  }
                  unsafe { PREV_LEFT_PRESSED = left_pressed; }
                  return;
              } else if self.start_menu_open {
                  // Click outside start menu closes it
                  self.start_menu_open = false;
              }

              // Desktop icon clicks (SYSTEM, FILES)
              if y >= 20 && y < 68 && x >= 20 && x < 68 {
                  // SYSTEM desktop icon
                  self.create_info_window("SYSTEM",
                      "SkyOS System\n\nKernel: Vahi v0.3.0\nCPU: x86_64\nMemory: Managed");
                  unsafe { PREV_LEFT_PRESSED = left_pressed; }
                  return;
              }
              if y >= 110 && y < 158 && x >= 20 && x < 68 {
                  // FILES desktop icon
                  self.create_file_manager_window();
                  unsafe { PREV_LEFT_PRESSED = left_pressed; }
                  return;
              }
             // Check minimize/close buttons on all windows (reverse order = top first)
             for (i, win) in self.windows.iter().enumerate().rev() {
                 if win.is_minimize_button(x, y) {
                     self.windows[i].minimized = !self.windows[i].minimized;
                     // Bring to front
                     let w = self.windows.remove(i);
                     self.windows.push(w);
                     unsafe { PREV_LEFT_PRESSED = left_pressed; }
                     return;
                 }
                 if win.is_close_button(x, y) {
                     self.close_pending = Some(i);
                     unsafe { PREV_LEFT_PRESSED = left_pressed; }
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
                         unsafe { PREV_LEFT_PRESSED = left_pressed; }
                         return;
                     }
                 }
             }
        }
        unsafe { PREV_LEFT_PRESSED = left_pressed; }

        if left_pressed {
            if let Some(idx) = self.drag_index {
                if self.resize_edge != window::ResizeEdge::None {
                    let win = &mut self.windows[idx];
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
                } else {
                    self.windows[idx].x = x.saturating_sub(self.drag_offset_x);
                    self.windows[idx].y = y.saturating_sub(self.drag_offset_y);
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
                // Stopped dragging - bring to front here safely
                let w = self.windows.remove(idx);
                self.windows.push(w);
            }
            self.drag_index = None;
            self.resize_edge = window::ResizeEdge::None;

            // Process pending close request
            if let Some(idx) = self.close_pending {
                if idx < self.windows.len() {
                    self.windows.remove(idx);
                }
                self.close_pending = None;
            }

            // Release mouse on all windows
            for win in &mut self.windows {
                win.handle_mouse(x, y, false);
            }
        }
    }

    pub fn focused_window(&self) -> Option<usize> {
        if self.windows.is_empty() {
            None
        } else {
            Some(self.windows.len() - 1)
        }
    }

    pub fn handle_scroll(&mut self, delta: i8) {
        if let Some(idx) = self.focused_window() {
            self.windows[idx].handle_scroll(delta);
        }
    }

    pub fn handle_keyboard(&mut self, key: pc_keyboard::DecodedKey) {
        if let Some(idx) = self.focused_window() {
            self.windows[idx].handle_keyboard(key);
        }
    }

    pub fn render(&mut self, mouse_x: usize, mouse_y: usize) {
        // Copy cached gradient background via raw pointers to avoid borrow conflict
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.background_cache.as_ptr(),
                self.backbuffer.as_mut_ptr(),
                SCREEN_WIDTH * SCREEN_HEIGHT,
            );
        }
        shell::draw_taskbar(&mut self.backbuffer);

        // Window entries on taskbar
        let taskbar_y = SCREEN_HEIGHT - 40;
        for (i, win) in self.windows.iter().enumerate() {
            let bx = 70 + i * 120;
            let is_active = i == self.windows.len() - 1;
            let btn_color = if win.minimized { 0xFF555555 } else if is_active { 0xFF000080 } else { 0xFF404040 };
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, taskbar_y + 5, 115, 30, btn_color);
            let display = if win.title.len() > 13 { &win.title[..13] } else { win.title };
            drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx + 5, taskbar_y + 10, display, 0xFFFFFFFF);
        }

        shell::draw_icons(&mut self.backbuffer);

        // Refresh monitor windows before rendering
        for win in &mut self.windows {
            if let Some(ref mut term) = win.terminal {
                if term.is_monitor {
                    term.refresh_monitor();
                }
            }
        }

        for window in &self.windows {
            if !window.minimized { window.render(&mut self.backbuffer); }
        }

        if self.start_menu_open {
            shell::draw_start_menu(&mut self.backbuffer, mouse_x, mouse_y);
        }

        mouse::draw_cursor(&mut self.backbuffer, mouse_x, mouse_y);

        // Commit to hardware framebuffer
        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        if !fb_ptr.is_null() {
            unsafe {
                core::ptr::copy_nonoverlapping(self.backbuffer.as_ptr(), fb_ptr, SCREEN_WIDTH * SCREEN_HEIGHT);
            }
        }
    }
}

pub fn init() {
    let mut comp = COMPOSITOR.lock();
    
    // Render background gradient to cache (static gradient only)
    shell::draw_background(&mut comp.background_cache);
    
    // Draw initial frame
    unsafe {
        core::ptr::copy_nonoverlapping(
            comp.background_cache.as_ptr(),
            comp.backbuffer.as_mut_ptr(),
            SCREEN_WIDTH * SCREEN_HEIGHT,
        );
    }
    shell::draw_taskbar(&mut comp.backbuffer);
    
    comp.render(SCREEN_WIDTH / 2, SCREEN_HEIGHT / 2);
}






