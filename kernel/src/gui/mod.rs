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

pub mod surface;
pub mod input;
pub mod windows;
pub mod menu;
pub mod paint;

use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::sync::IrqSafeMutex as Mutex;
use self::surface::DamageTracker;

pub const SCREEN_WIDTH: usize = 800;
pub const SCREEN_HEIGHT: usize = 600;

pub static mut ACCENT_COLOR: u32 = 0xFF0078D4;

pub fn accent_color() -> u32 { unsafe { ACCENT_COLOR } }

lazy_static::lazy_static! {
    pub static ref COMPOSITOR: Mutex<Compositor> = Mutex::new(Compositor::new());
}

pub struct Compositor {
    pub backbuffer: Box<[u32]>,
    pub(crate) background_cache: Box<[u32]>,
    pub windows: Vec<window::Window>,
    pub(crate) drag_index: Option<usize>,
    pub(crate) drag_offset_x: usize,
    pub(crate) drag_offset_y: usize,
    pub(crate) start_menu_open: bool,
    pub(crate) close_pending: Option<usize>,
    pub(crate) prev_click_ticks: u64,
    pub(crate) prev_click_win: Option<usize>,
    pub(crate) resize_edge: window::ResizeEdge,
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
    pub damage: DamageTracker,
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
            damage: DamageTracker::new(SCREEN_WIDTH, SCREEN_HEIGHT),
        }
    }

    pub fn add_window(&mut self, mut window: window::Window) {
        let idx = self.windows.len();
        window.dirty = true;
        self.damage.mark(window.x, window.y, window.width, window.height);
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
        for win in &mut self.windows {
            if win.x + win.width > new_w {
                win.x = new_w.saturating_sub(win.width);
            }
            if win.y + win.height > new_h.saturating_sub(40) {
                win.y = new_h.saturating_sub(win.height + 40);
            }
        }
        if self.wallpaper_path.is_none() {
            shell::draw_background(&mut self.background_cache);
        } else {
            self.wallpaper_dirty = true;
        }
        self.damage.mark_full();
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
