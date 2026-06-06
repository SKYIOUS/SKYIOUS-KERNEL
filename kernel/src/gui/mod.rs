//! GUI Subsystem
//!
//! Provides window management, compositing, and double-buffering.

pub mod drawing;
pub mod mouse;
pub mod window;
pub mod shell;
pub mod terminal;
pub mod widgets;

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
    pub windows: Vec<window::Window>,
    drag_index: Option<usize>,
    drag_offset_x: usize,
    drag_offset_y: usize,
    start_menu_open: bool,
}

impl Compositor {
    pub fn new() -> Self {
        let size = SCREEN_WIDTH * SCREEN_HEIGHT;
        let mut buffer = Vec::with_capacity(size);
        for _ in 0..size { buffer.push(0x001A237E); } // Deep Blue Background
        
        Self {
            backbuffer: buffer.into_boxed_slice(),
            windows: Vec::new(),
            drag_index: None,
            drag_offset_x: 0,
            drag_offset_y: 0,
            start_menu_open: false,
        }
    }

    pub fn add_window(&mut self, window: window::Window) {
        self.windows.push(window);
    }

    pub fn handle_mouse(&mut self, x: usize, y: usize, buttons: u8) {
        let left_pressed = (buttons & 1) != 0;
        static mut PREV_LEFT_PRESSED: bool = false;

        if left_pressed && unsafe { !PREV_LEFT_PRESSED } {
             // New click
             if x >= 5 && x < 65 && y >= SCREEN_HEIGHT - 35 && y < SCREEN_HEIGHT - 5 {
                 self.start_menu_open = !self.start_menu_open;
             }
        }
        unsafe { PREV_LEFT_PRESSED = left_pressed; }

        if left_pressed {
            if let Some(idx) = self.drag_index {
                // Currently dragging
                self.windows[idx].x = x.saturating_sub(self.drag_offset_x);
                self.windows[idx].y = y.saturating_sub(self.drag_offset_y);
            } else {
                // Check if we started dragging or interacting with content
                for (i, win) in self.windows.iter_mut().enumerate().rev() {
                    if win.is_within_title_bar(x, y) {
                        self.drag_index = Some(i);
                        self.drag_offset_x = x - win.x;
                        self.drag_offset_y = y - win.y;
                        
                        // Bring to front logic (needs care with iter_mut)
                        break;
                    } else if win.is_within_content(x, y) {
                        win.handle_mouse(x, y, true);
                        break;
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

    pub fn handle_keyboard(&mut self, key: pc_keyboard::DecodedKey) {
        if let Some(idx) = self.focused_window() {
            self.windows[idx].handle_keyboard(key);
        }
    }

    pub fn render(&mut self) {
        // 1. Draw Desktop Background
        shell::draw_background(&mut self.backbuffer);

        // 2. Draw Taskbar
        shell::draw_taskbar(&mut self.backbuffer);

        // 3. Draw Desktop Icons
        shell::draw_icons(&mut self.backbuffer);

        // 4. Render all windows
        for window in &self.windows {
            window.render(&mut self.backbuffer);
        }

        // 4.1 Draw Start Menu if open
        if self.start_menu_open {
            shell::draw_start_menu(&mut self.backbuffer);
        }

        // 5. Render Mouse Cursor
        let mouse = crate::drivers::mouse::MOUSE.lock();
        mouse::draw_cursor(&mut self.backbuffer, mouse.x, mouse.y);
        drop(mouse);

        // 4. Commit to hardware framebuffer
        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        if !fb_ptr.is_null() {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.backbuffer.as_ptr(),
                    fb_ptr,
                    SCREEN_WIDTH * SCREEN_HEIGHT
                );
            }
        }
    }
}

pub fn init() {
    let mut comp = COMPOSITOR.lock();
    comp.add_window(window::Window::new(50, 50, 400, 300, "Welcome to Vahi"));
    
    let mut monitor = window::Window::new(500, 100, 300, 200, "System Monitor");
    monitor.widgets.push(widgets::Widget::new_label(10, 10, "CPU Usage: 5%"));
    monitor.widgets.push(widgets::Widget::new_label(10, 25, "Memory: 128MB / 4GB"));
    monitor.widgets.push(widgets::Widget::new_button(10, 50, 80, 25, "Refresh"));
    comp.add_window(monitor);
    
    comp.render();
}
