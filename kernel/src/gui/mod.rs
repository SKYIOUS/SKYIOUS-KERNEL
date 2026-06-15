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
pub mod splash;
pub mod wallpaper;

use alloc::vec::Vec;
use alloc::boxed::Box;
use spin::Mutex;
use core::sync::atomic::Ordering;
use crate::drivers::graphics::FRAMEBUFFER;

pub const SCREEN_WIDTH: usize = 800;
pub const SCREEN_HEIGHT: usize = 600;

pub static mut ACCENT_COLOR: u32 = 0xFF0078D4;

pub fn accent_color() -> u32 { unsafe { ACCENT_COLOR } }

lazy_static::lazy_static! {
    pub static ref COMPOSITOR: Mutex<Compositor> = Mutex::new(Compositor::new());
}

pub struct Compositor {
    pub backbuffer: Box<[u32]>,
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
    pub clipboard: Vec<u8>,
    pub notifications: Vec<Notification>,
    pub alt_held: bool,
    pub super_held: bool,
    pub alt_tab_active: bool,
    pub alt_tab_index: usize,
    pub context_menu: ContextMenu,
    pub wallpaper_path: Option<alloc::string::String>,
    pub wallpaper_dirty: bool,
    pub(crate) animations: alloc::vec::Vec<WindowAnimation>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum NotifKind {
    Info,
    Warning,
    Error,
}

#[derive(Clone)]
pub struct Notification {
    pub text: alloc::string::String,
    pub kind: NotifKind,
    pub ticks_remaining: u64,
    pub x: usize,
    pub y: usize,
}

impl Notification {
    pub fn notif_color(&self) -> u32 {
        match self.kind {
            NotifKind::Info => 0xFF2196F3,    // Blue
            NotifKind::Warning => 0xFFFF9800,  // Orange
            NotifKind::Error => 0xFFF44336,    // Red
        }
    }
}

pub struct ContextMenu {
    pub open: bool,
    pub x: usize,
    pub y: usize,
    pub items: alloc::vec::Vec<(&'static str, ContextAction)>,
    pub selected: Option<usize>,
}

pub enum ContextAction {
    OpenTerminal,
    OpenFileManager,
    OpenMonitor,
    CloseWindow,
    MinimizeWindow,
    MaximizeWindow,
    Shutdown,
}

pub(crate) struct WindowAnimation {
    pub(crate) window_idx: usize,
    pub(crate) frame: u32,
    pub(crate) total: u32,
    pub(crate) fade_out: bool,
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
            clipboard: Vec::new(),
            notifications: Vec::new(),
            alt_held: false,
            super_held: false,
            alt_tab_active: false,
            alt_tab_index: 0,
            context_menu: ContextMenu {
                open: false,
                x: 0, y: 0,
                items: alloc::vec::Vec::new(),
                selected: None,
            },
            wallpaper_path: None,
            wallpaper_dirty: false,
            animations: alloc::vec::Vec::new(),
        }
    }

    pub fn add_window(&mut self, window: window::Window) {
        let idx = self.windows.len();
        self.windows.push(window);
        self.animations.push(WindowAnimation {
            window_idx: idx,
            frame: 0,
            total: 10,
            fade_out: false,
        });
    }

    pub fn set_resolution(&mut self, new_w: usize, new_h: usize) {
        let size = new_w * new_h;
        self.backbuffer = alloc::vec![0x001A237E; size].into_boxed_slice();
        self.background_cache = alloc::vec![0x001A237E; size].into_boxed_slice();
        // Re-center windows that are off-screen
        for win in &mut self.windows {
            if win.x + win.width > new_w {
                win.x = new_w.saturating_sub(win.width);
            }
            if win.y + win.height > new_h.saturating_sub(40) {
                win.y = new_h.saturating_sub(win.height + 40);
            }
        }
        // Regenerate gradient if no wallpaper
        if self.wallpaper_path.is_none() {
            shell::draw_background(&mut self.background_cache);
        } else {
            self.wallpaper_dirty = true;
        }
    }

    pub fn set_wallpaper(&mut self, path: alloc::string::String) {
        self.wallpaper_path = Some(path);
        self.wallpaper_dirty = true;
    }

    pub fn clear_wallpaper(&mut self) {
        self.wallpaper_path = None;
        self.wallpaper_dirty = false;
        shell::draw_background(&mut self.background_cache);
    }

    pub fn load_wallpaper(&mut self) {
        if !self.wallpaper_dirty { return; }
        self.wallpaper_dirty = false;
        let path = match &self.wallpaper_path {
            Some(p) => p.clone(),
            None => { shell::draw_background(&mut self.background_cache); return; }
        };
        let vfs = crate::vfs::VFS.lock();
        let node = match vfs.resolve_path(&path) {
            Some(n) => n,
            None => { drop(vfs); shell::draw_background(&mut self.background_cache); return; }
        };
        let data = match node.read(usize::MAX) {
            Ok(d) => d,
            Err(_) => { drop(vfs); shell::draw_background(&mut self.background_cache); return; }
        };
        drop(vfs);
        let img = match wallpaper::decode_bmp(&data) {
            Some(i) => i,
            None => { shell::draw_background(&mut self.background_cache); return; }
        };
        let scaled = wallpaper::scale_to_screen(&img, SCREEN_WIDTH, SCREEN_HEIGHT);
        self.background_cache = scaled.into_boxed_slice();
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
        term.print_str("Loading system data...\n");
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
                    self.windows.remove(idx);
                }
                self.close_pending = None;
            }

            for win in &mut self.windows {
                win.handle_mouse(x, y, false);
            }
        }
    }

    fn show_desktop_context_menu(&mut self, x: usize, y: usize) {
        self.context_menu = ContextMenu {
            open: true,
            x, y,
            items: alloc::vec![
                ("Terminal", ContextAction::OpenTerminal),
                ("File Manager", ContextAction::OpenFileManager),
                ("System Monitor", ContextAction::OpenMonitor),
                ("---", ContextAction::CloseWindow),
                ("Shutdown", ContextAction::Shutdown),
            ],
            selected: None,
        };
    }

    fn show_window_context_menu(&mut self, x: usize, y: usize, _win_idx: usize) {
        self.context_menu = ContextMenu {
            open: true,
            x, y,
            items: alloc::vec![
                ("Minimize", ContextAction::MinimizeWindow),
                ("Maximize", ContextAction::MaximizeWindow),
                ("Close", ContextAction::CloseWindow),
            ],
            selected: None,
        };
    }

    fn execute_context_action(&mut self, idx: usize) {
        if idx >= self.context_menu.items.len() { return; }
        let action = &self.context_menu.items[idx].1;
        match action {
            ContextAction::OpenTerminal => self.create_terminal_window(),
            ContextAction::OpenFileManager => self.create_file_manager_window(),
            ContextAction::OpenMonitor => self.create_monitor_window(),
            ContextAction::Shutdown => self.shutdown_qemu(),
            ContextAction::CloseWindow => {
                if let Some(idx) = self.focused_window() {
                    self.close_pending = Some(idx);
                }
            }
            ContextAction::MinimizeWindow => {
                if let Some(idx) = self.focused_window() {
                    self.windows[idx].minimized = true;
                }
            }
            ContextAction::MaximizeWindow => {
                if let Some(idx) = self.focused_window() {
                    self.windows[idx].toggle_maximize();
                }
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
        match key {
            pc_keyboard::DecodedKey::RawKey(raw) => {
                // Track modifier keys
                match raw {
                    pc_keyboard::KeyCode::AltLeft | pc_keyboard::KeyCode::AltRight => {
                        // Alt press/release tracked via make/break codes
                        // We detect release by checking if the key is now up
                        // For simplicity, toggle on each Alt press
                    }
                    _ => {}
                }
                // Forward RawKey to focused window for non-character keys
                if let Some(idx) = self.focused_window() {
                    self.windows[idx].handle_keyboard(key);
                }
            }
            pc_keyboard::DecodedKey::Unicode(c) => {
                // Alt+F4: close focused window
                if self.alt_held && c == '\u{0004}' { // Ctrl+D = EOT, but Alt+F4 is special
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

    pub fn render(&mut self, mouse_x: usize, mouse_y: usize) {
        // Load wallpaper if dirty
        self.load_wallpaper();

        // Decay notifications
        self.notifications.retain(|n| n.ticks_remaining > 0);
        for notif in &mut self.notifications {
            notif.ticks_remaining = notif.ticks_remaining.saturating_sub(1);
        }

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
            let btn_color = if win.minimized { 0xFF3A3A3A } else if is_active { 0xFF2D2D2D } else { 0xFF252526 };
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, taskbar_y + 5, 115, 30, btn_color);
            if is_active {
                drawing::draw_line_h(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, taskbar_y + 5, 115, crate::gui::accent_color());
            }
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
            if !window.minimized { window.render(&mut self.backbuffer, mouse_x, mouse_y); }
        }

        // Apply window animations (fade-in overlays)
        self.animations.retain(|anim| anim.frame < anim.total);
        for anim in &mut self.animations {
            anim.frame += 1;
            if anim.window_idx >= self.windows.len() { continue; }
            let win = &self.windows[anim.window_idx];
            if win.minimized { continue; }
            if anim.fade_out {
                // Fade out: alpha increases from low to full
                let a = (255 * anim.frame / anim.total) as u32;
                let overlay = (a.min(255) << 24) | 0x000000;
                drawing::draw_rect_alpha(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                    win.x, win.y, win.width, win.height, overlay);
            } else {
                // Fade in: alpha decreases from full to 0
                let a = 255 - (255 * anim.frame / anim.total) as u32;
                let overlay = (a.min(255) << 24) | 0x000000;
                drawing::draw_rect_alpha(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                    win.x, win.y, win.width, win.height, overlay);
            }
        }

        if self.start_menu_open {
            shell::draw_start_menu(&mut self.backbuffer, mouse_x, mouse_y);
        }

        // Context menu
        if self.context_menu.open {
            let item_h = 24;
            let menu_w = 160;
            let menu_h = self.context_menu.items.len() * item_h + 8;
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                self.context_menu.x, self.context_menu.y, menu_w, menu_h, 0xE02D2D2D);
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                self.context_menu.x, self.context_menu.y, menu_w, menu_h, accent_color());
            for (i, (name, _action)) in self.context_menu.items.iter().enumerate() {
                if *name == "---" {
                    let sep_y = self.context_menu.y + 4 + i * item_h + item_h / 2;
                    drawing::draw_line_h(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                        self.context_menu.x + 8, sep_y, menu_w - 16, 0xFF555555);
                    continue;
                }
                let iy = self.context_menu.y + 4 + i * item_h;
                let hover = mouse_x >= self.context_menu.x + 2
                    && mouse_x < self.context_menu.x + menu_w - 2
                    && mouse_y >= iy && mouse_y < iy + item_h;
                if hover {
                    drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                        self.context_menu.x + 2, iy, menu_w - 4, item_h, 0xFF3A3A3A);
                }
                drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                    self.context_menu.x + 12, iy + 5, name, 0xFFCCCCCC);
            }
        }

        // Snap zone hint (when dragging near screen edges)
        if let Some(_idx) = self.drag_index {
            if mouse_x < 5 {
                // Left half snap hint
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, 0, SCREEN_WIDTH / 2, SCREEN_HEIGHT - 40, 0x300078D4);
            } else if mouse_x > SCREEN_WIDTH - 5 {
                // Right half snap hint
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH / 2, 0, SCREEN_WIDTH / 2, SCREEN_HEIGHT - 40, 0x300078D4);
            } else if mouse_y < 5 {
                // Maximize snap hint
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, 0, SCREEN_WIDTH, SCREEN_HEIGHT - 40, 0x300078D4);
            }
        }

        // Alt+Tab overlay
        if self.alt_tab_active && self.windows.len() > 1 {
            let overlay_w = (self.windows.len() as u32) * 130 + 20;
            let overlay_h: u32 = 60;
            let overlay_x = (SCREEN_WIDTH as u32 - overlay_w) / 2;
            let overlay_y = (SCREEN_HEIGHT as u32 - overlay_h) / 2;
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, overlay_x as usize, overlay_y as usize, overlay_w as usize, overlay_h as usize, 0xE01E1E1E);

            for (i, win) in self.windows.iter().enumerate() {
                let bx = overlay_x as usize + 10 + i * 130;
                let by = overlay_y as usize + 10;
                let bg = if i == self.alt_tab_index { 0xFF0078D4 } else { 0xFF3A3A3A };
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, by, 120, 40, bg);
                let display = if win.title.len() > 14 { &win.title[..14] } else { win.title };
                drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx + 4, by + 14, display, 0xFFFFFFFF);
            }
        }

        // Notifications (toast popups) with type-colored left border and icon
        let mut notif_y = 50usize;
        for notif in &self.notifications {
            let text_w = notif.text.len() * 8 + 36;
            let x = SCREEN_WIDTH - text_w - 10;
            let color = notif.notif_color();
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, notif_y, text_w, 30, 0xE0252526);
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, notif_y, 4, 30, color);
            let icon = match notif.kind {
                crate::gui::NotifKind::Info => "i",
                crate::gui::NotifKind::Warning => "!",
                crate::gui::NotifKind::Error => "x",
            };
            drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + 8, notif_y + 8, icon, color);
            drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + 20, notif_y + 8, &notif.text, 0xFFCCCCCC);
            notif_y += 36;
        }

        mouse::draw_cursor(&mut self.backbuffer, mouse_x, mouse_y);

        // Commit to hardware framebuffer
        let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
        if !fb_ptr.is_null() {
            unsafe {
                core::ptr::copy_nonoverlapping(self.backbuffer.as_ptr(), fb_ptr, SCREEN_WIDTH * SCREEN_HEIGHT);
            }
        }

        // If VirtIO GPU is active, flip (transfer + flush) to display
        crate::drivers::gpu::virtio_gpu::flip();
    }
}

pub fn init() {
    // Clear boot splash — the desktop will take over
    splash::clear();

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






