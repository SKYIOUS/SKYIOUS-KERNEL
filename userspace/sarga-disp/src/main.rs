#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::ffi::CString;
use alloc::boxed::Box;
use libsarga::{Color, Backbuffer, Framebuffer, Event, App};
use libsarga::app::{SkyTerm, SkyCalc};
use libsarga::event::MouseState;

const MENU_H: usize = 24;
const DOCK_H: usize = 42;
const TITLE_H: usize = 18;
const DOCK_ICON: usize = 28;
const PAGE_KB: u64 = 4;

// ── Colors (macOS dark mode inspired) ──
const C_MENU_BG: Color = Color::from_u32(0xFF1C1C1E);
const C_MENU_BORDER: Color = Color::from_u32(0xFF38383A);
const C_DESKTOP_TOP: Color = Color::from_u32(0xFF1D1D20);

const C_DOCK_BG: Color = Color::from_u32(0xFF2C2C2E);
const C_WIN_BG: Color = Color::from_u32(0xFF1E1E1E);
const C_WIN_BORDER: Color = Color::from_u32(0xFF48484A);

const C_TITLE_ACTIVE: Color = Color::from_u32(0xFF3A3A3C);
const C_CLOSE: Color = Color::from_u32(0xFFFF5F57);
const C_MINIMIZE: Color = Color::from_u32(0xFFFEBC2E);
const C_MAXIMIZE: Color = Color::from_u32(0xFF28C840);
const C_TEXT: Color = Color::from_u32(0xFFE5E5E5);
const C_SUBTLE: Color = Color::from_u32(0xFF8E8E93);
const C_ACCENT: Color = Color::from_u32(0xFF007AFF);

// ── App definitions for the dock ──
#[derive(Clone, Copy)]
struct DockApp {
    _name: &'static str,
    glyph: &'static str,
    color: Color,
    launcher: fn() -> Box<dyn App>,
    default_w: usize,
    default_h: usize,
}

static DOCK_APPS: &[DockApp] = &[
    DockApp { _name: "Terminal", glyph: "~", color: Color::ICON_TERM, launcher: || Box::new(SkyTerm::new()), default_w: 560, default_h: 380 },
    DockApp { _name: "Calculator", glyph: "+", color: Color::ICON_CALC, launcher: || Box::new(SkyCalc::new()), default_w: 320, default_h: 380 },
    DockApp { _name: "Monitor", glyph: "@", color: Color::ICON_MONITOR, launcher: || Box::new(SkyMon::new()), default_w: 420, default_h: 300 },
    DockApp { _name: "Files", glyph: "#", color: Color::ICON_FILES, launcher: || Box::new(SkyFiles::new()), default_w: 480, default_h: 340 },
    DockApp { _name: "About", glyph: "i", color: Color::ICON_ABOUT, launcher: || Box::new(SkyAbout::new()), default_w: 360, default_h: 240 },
    DockApp { _name: "Notes", glyph: "N", color: Color::ICON_SETTINGS, launcher: || Box::new(SkyNotes::new()), default_w: 420, default_h: 300 },
];

// ── Apps ──
struct SkyMon {
    total_ram: u64, free_ram: u64, uptime: u64, proc_count: u64, load_avg: u64,
}

impl SkyMon {
    fn new() -> Self { SkyMon { total_ram: 0, free_ram: 0, uptime: 0, proc_count: 0, load_avg: 0 } }
    fn refresh(&mut self) {
        if let Some(info) = libskyos::sysinfo() {
            self.total_ram = info.total_ram_pages * PAGE_KB;
            self.free_ram = info.free_ram_pages * PAGE_KB;
            self.uptime = info.uptime_seconds;
            self.proc_count = info.process_count;
            self.load_avg = info.load_avg_1m;
        }
    }
}

impl App for SkyMon {
    fn title(&self) -> &str { "System Monitor" }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any { self }
    fn handle_event(&mut self, _ev: Event, _cx: usize, _cy: usize, _cw: usize, _ch: usize) {}
    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize) {
        let p = 8;
        bb.fill_rect(cx, cy, cw, ch, C_WIN_BG);
        let used_pct = if self.total_ram > 0 { ((self.total_ram - self.free_ram) * 100 / self.total_ram) as usize } else { 0 };
        let (uh, um, us) = (self.uptime / 3600, (self.uptime % 3600) / 60, self.uptime % 60);
        let lines = [
            alloc::format!("Memory: {} KB / {} KB", self.total_ram - self.free_ram, self.total_ram),
            alloc::format!("Uptime: {}h {:02}m {:02}s", uh, um, us),
            alloc::format!("Processes: {}", self.proc_count),
            alloc::format!("Load (1m): {}%", self.load_avg),
        ];
        let mut y = cy + p + 4;
        for line in &lines {
            bb.draw_text(cx + p + 4, y, line, C_TEXT, C_WIN_BG);
            y += 14;
        }
        let bar_y = y + 6;
        bb.draw_text(cx + p + 4, bar_y, "Memory", C_SUBTLE, C_WIN_BG);
        bb.draw_progress_bar(cx + p + 4, bar_y + 12, cw - p * 2 - 8, 12, used_pct, C_ACCENT, Color::BG_INPUT);
        let pct = alloc::format!("{}%", used_pct);
        bb.draw_text(cx + cw - 8 - pct.len() * 8, bar_y + 14, &pct, C_ACCENT, C_WIN_BG);
    }
}

struct SkyFiles { entries: Vec<String> }

impl SkyFiles {
    fn new() -> Self {
        let entries = libskyos::list_dir("/").unwrap_or_else(|| alloc::vec!["(empty)".into()]);
        SkyFiles { entries }
    }
}

impl App for SkyFiles {
    fn title(&self) -> &str { "File Browser" }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any { self }
    fn handle_event(&mut self, _ev: Event, _cx: usize, _cy: usize, _cw: usize, _ch: usize) {}
    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize) {
        let p = 6;
        bb.fill_rect(cx, cy, cw, ch, C_WIN_BG);
        let header = alloc::format!("  / -- {} items", self.entries.len());
        bb.fill_rect(cx + p, cy + p, cw - p * 2, 16, Color::BG_SURFACE);
        bb.draw_text(cx + p + 6, cy + p + 4, &header, C_ACCENT, Color::BG_SURFACE);
        let ly = cy + p + 16 + 4;
        let lh = ch - p * 2 - 16 - 4;
        bb.fill_rect(cx + p, ly, cw - p * 2, lh, Color::BG_SURFACE);
        let mut y = ly + 4;
        for entry in &self.entries {
            if y + 10 > ly + lh - 4 { break; }
            bb.draw_text(cx + p + 8, y, entry, C_TEXT, Color::BG_SURFACE);
            y += 11;
        }
    }
}

struct SkyAbout { _dummy: u8 }

impl SkyAbout {
    fn new() -> Self { SkyAbout { _dummy: 0 } }
}

impl App for SkyAbout {
    fn title(&self) -> &str { "About This Desktop" }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any { self }
    fn handle_event(&mut self, _ev: Event, _cx: usize, _cy: usize, _cw: usize, _ch: usize) {}
    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize) {
        bb.fill_rect(cx, cy, cw, ch, C_WIN_BG);
        let lines = [
            "SkyOS Desktop",
            "Version 1.0",
            "macOS Monterey Edition",
            "",
            "Software-rendered GUI",
            "8x8 bitmap font engine",
        ];
        let mut y = cy + 14;
        for (i, line) in lines.iter().enumerate() {
            let col = if i == 0 { C_ACCENT } else if i == 1 { C_SUBTLE } else { C_TEXT };
            bb.draw_text(cx + (cw - line.len() * 8) / 2, y, line, col, C_WIN_BG);
            y += 14;
        }
    }
}

struct SkyNotes { text: String, cursor: usize }

impl SkyNotes {
    fn new() -> Self { SkyNotes { text: String::new(), cursor: 0 } }
}

impl App for SkyNotes {
    fn title(&self) -> &str { "Notes" }
    fn as_any_mut(&mut self) -> &mut dyn core::any::Any { self }
    fn handle_event(&mut self, event: Event, _cx: usize, _cy: usize, _cw: usize, _ch: usize) {
        if let Event::KeyDown(code) = event {
            match code {
                14 => { self.text.pop(); self.cursor = self.text.len(); }
                28 => { self.text.push('\n'); self.cursor = self.text.len(); }
                57 => { self.text.push(' '); self.cursor = self.text.len(); }
                16..=25 => { let c = (b'a' + (code - 16) as u8) as char; self.text.push(c); self.cursor = self.text.len(); }
                2..=11 => {
                    let n = if code == 2 { '1' } else if code == 3 { '2' } else if code == 4 { '3' }
                        else if code == 5 { '4' } else if code == 6 { '5' } else if code == 7 { '6' }
                        else if code == 8 { '7' } else if code == 9 { '8' } else if code == 10 { '9' } else { '0' };
                    self.text.push(n); self.cursor = self.text.len();
                }
                _ => {}
            }
        }
    }
    fn render(&self, bb: &mut Backbuffer, cx: usize, cy: usize, cw: usize, ch: usize) {
        bb.fill_rect(cx, cy, cw, ch, Color::from_u32(0xFFF5F5F0));
        bb.fill_rect(cx, cy + 2, cw, 1, Color::from_u32(0xFFE0E0D8));
        let mut y = cy + 10;
        for &b in self.text.as_bytes() {
            if y + 10 > cy + ch - 4 { break; }
            if b == b'\n' { y += 14; continue; }
            let ch_byte = b;
            bb.draw_char(cx + 10, y, ch_byte, Color::from_u32(0xFF333333), Color::from_u32(0xFFF5F5F0));
            let cw_used = cx + 10 + 8;
            if cw_used > cx + cw - 10 { y += 14; }
        }
    }
}

// ── Window ──
#[derive(Clone, Copy, PartialEq)]
enum WinState { Normal, Minimized, Maximized }

struct Win {
    x: i32, y: i32, w: usize, h: usize,
    app: Box<dyn App>,
    dragging: bool, drag_off_x: i32, drag_off_y: i32,
    state: WinState,
    rx: i32, ry: i32, rw: usize, rh: usize,
    app_idx: usize,
}

impl Win {
    fn traffic_x(&self) -> i32 { self.x + 8 }
    fn traffic_y(&self) -> i32 { self.y + 5 }
    fn close_hit(&self, mx: i32, my: i32) -> bool {
        let (tx, ty) = (self.traffic_x(), self.traffic_y());
        mx >= tx && mx < tx + 10 && my >= ty && my < ty + 10
    }
    fn min_hit(&self, mx: i32, my: i32) -> bool {
        let (tx, ty) = (self.traffic_x() + 14, self.traffic_y());
        mx >= tx && mx < tx + 10 && my >= ty && my < ty + 10
    }
    fn max_hit(&self, mx: i32, my: i32) -> bool {
        let (tx, ty) = (self.traffic_x() + 28, self.traffic_y());
        mx >= tx && mx < tx + 10 && my >= ty && my < ty + 10
    }
}

// ── Drawing ──
fn draw_desktop(bb: &mut Backbuffer, sw: usize, sh: usize) {
    let dh = sh.saturating_sub(MENU_H + DOCK_H);
    for y in 0..dh {
        let t = y as f32 / dh as f32;
        let r = ((27.0 + t * 10.0) as u8).max(0);
        let g = ((29.0 + t * 12.0) as u8).max(0);
        let b = ((32.0 + t * 16.0) as u8).max(0);
        bb.fill_rect(0, MENU_H + y, sw, 1, Color::rgb(r, g, b));
    }
    bb.draw_text(sw / 2 - 16, MENU_H + dh / 2 - 20, "SkyOS", C_ACCENT, C_DESKTOP_TOP);
}

fn draw_menu_bar(bb: &mut Backbuffer, app_name: &str, uptime_secs: u64, sw: usize) {
    bb.fill_rect(0, 0, sw, MENU_H, C_MENU_BG);
    bb.fill_rect(0, MENU_H - 1, sw, 1, C_MENU_BORDER);

    bb.fill_rounded_rect(6, 4, 44, 16, 4, Color::from_u32(0xFF3A3A3C));
    bb.draw_text(10, 7, "SkyOS", C_ACCENT, Color::from_u32(0xFF3A3A3C));

    bb.draw_text(56, 7, app_name, C_TEXT, C_MENU_BG);

    bb.draw_text(56 + app_name.len() * 8 + 12, 7, "File", C_SUBTLE, C_MENU_BG);
    bb.draw_text(56 + (app_name.len() + 4) * 8 + 12, 7, "Edit", C_SUBTLE, C_MENU_BG);
    bb.draw_text(56 + (app_name.len() + 8) * 8 + 12, 7, "View", C_SUBTLE, C_MENU_BG);

    let h = uptime_secs / 3600;
    let m = (uptime_secs % 3600) / 60;
    let s = uptime_secs % 60;
    let clock = alloc::format!("{:02}:{:02}:{:02}", h, m, s);
    let cw = clock.len() * 8 + 16;
    let cx = sw - cw;
    bb.fill_rect(cx, 4, cw, MENU_H - 8, Color::from_u32(0xFF3A3A3C));
    bb.draw_text(cx + 8, 7, &clock, C_SUBTLE, Color::from_u32(0xFF3A3A3C));
}

fn draw_dock(bb: &mut Backbuffer, sw: usize, sh: usize, active_app_idx: Option<usize>, minimized: &[bool]) {
    let dy = sh - DOCK_H;
    let item_total = DOCK_APPS.len();
    let dock_w = item_total * (DOCK_ICON + 8) + 16;
    let dx = (sw - dock_w) / 2;

    bb.fill_rounded_rect(dx, dy + 4, dock_w, DOCK_H - 4, 8, C_DOCK_BG);
    bb.draw_rounded_rect(dx, dy + 4, dock_w, DOCK_H - 4, 8, Color::from_u32(0xFF3A3A3C));

    for (i, da) in DOCK_APPS.iter().enumerate() {
        let ix = dx + 8 + i * (DOCK_ICON + 8);
        let iy = dy + (DOCK_H - DOCK_ICON) / 2;
        bb.fill_rounded_rect(ix, iy, DOCK_ICON, DOCK_ICON, 6, da.color);
        bb.draw_text(ix + (DOCK_ICON - 8) / 2, iy + (DOCK_ICON - 8) / 2, da.glyph, C_TEXT, da.color);

        let has_minimized = minimized.get(i).copied().unwrap_or(false);
        if has_minimized || active_app_idx.map_or(false, |a| a == i && a < minimized.len()) {
            let dot_y = dy + DOCK_H - 6;
            bb.fill_rounded_rect(ix + 8, dot_y, 8, 3, 2, C_ACCENT);
        }
    }

    let trash_x = sw - 44;
    bb.fill_rounded_rect(trash_x, dy + 10, 32, DOCK_H - 20, 6, Color::from_u32(0xFF3A3A3C));
    bb.draw_text(trash_x + 12, dy + 15, "T", Color::from_u32(0xFF8E8E93), Color::from_u32(0xFF3A3A3C));
}

fn draw_window_frame(bb: &mut Backbuffer, win: &Win, active: bool, sw: usize, sh: usize) {
    let x = win.x as usize;
    let y = win.y as usize;
    let w = core::cmp::min(win.w, sw);
    let h = core::cmp::min(win.h, sh);

    // Shadow
    let sd = 2;
    if x + w + sd < sw && y + sd < sh {
        bb.fill_rect(x + sd, y + sd, sd, h, Color::SHADOW);
        bb.fill_rect(x + sd, y + h, w + sd, sd, Color::SHADOW);
    }

    let tb = if active { C_TITLE_ACTIVE } else { Color::BG_SURFACE };
    bb.fill_rect(x, y, w, TITLE_H, tb);
    bb.fill_rect(x, y + TITLE_H, w, h - TITLE_H, C_WIN_BG);
    bb.draw_rect(x, y, w, h, C_WIN_BORDER);

    if active {
        bb.fill_rect(x, y, w, 1, C_ACCENT);
    }

    // Traffic lights
    let (tx, ty) = (win.traffic_x() as usize, win.traffic_y() as usize);
    let tl_r = 4;
    bb.fill_rounded_rect(tx, ty, 10, 10, tl_r, C_CLOSE);
    bb.fill_rounded_rect(tx + 14, ty, 10, 10, tl_r, C_MINIMIZE);
    bb.fill_rounded_rect(tx + 28, ty, 10, 10, tl_r, C_MAXIMIZE);

    // Title centered
    let title = win.app.title();
    let tw = title.len() * 8;
    let tx_center = x + (w - tw) / 2;
    bb.draw_text(tx_center, y + (TITLE_H - 8) / 2, title, if active { C_TEXT } else { C_SUBTLE }, tb);

    // Content
    let cx = x + 2;
    let cy = y + TITLE_H + 2;
    let cw = w - 4;
    let ch = h - TITLE_H - 4;
    win.app.render(bb, cx, cy, cw, ch);
}

fn draw_mouse(bb: &mut Backbuffer, mx: i32, my: i32, sw: usize, sh: usize) {
    let x = mx as usize;
    let y = my as usize;
    const CURSOR: [[u8; 12]; 12] = [
        [1,0,0,0,0,0,0,0,0,0,0,0],
        [1,1,0,0,0,0,0,0,0,0,0,0],
        [1,1,1,0,0,0,0,0,0,0,0,0],
        [1,1,1,1,0,0,0,0,0,0,0,0],
        [1,1,1,1,1,0,0,0,0,0,0,0],
        [1,1,1,1,1,1,0,0,0,0,0,0],
        [1,1,1,1,1,1,1,0,0,0,0,0],
        [1,1,1,1,1,1,1,1,0,0,0,0],
        [1,1,1,1,1,1,1,1,1,0,0,0],
        [1,1,1,1,1,1,0,0,0,0,0,0],
        [1,1,0,0,1,1,0,0,0,0,0,0],
        [1,0,0,0,0,1,1,0,0,0,0,0],
    ];
    for row in 0..12 {
        for col in 0..12 {
            if CURSOR[row][col] != 0 {
                let (px, py) = (x.wrapping_add(col), y.wrapping_add(row));
                if px < sw && py < sh { bb.set_px(px, py, Color::WHITE); }
            }
        }
    }
}

// ── Event I/O ──
fn read_events(fd: u64, buf: &mut [u8]) -> usize {
    let n = skyos_libc::syscall::read(fd, buf);
    if (n as i64) > 0 { n as usize } else { 0 }
}

fn parse_events(buf: &[u8], len: usize, mouse: &mut MouseState, sw: usize, sh: usize) -> alloc::vec::Vec<Event> {
    let mut evs = alloc::vec::Vec::new();
    let mut off = 0;
    while off + 8 <= len {
        let kind = u16::from_le_bytes([buf[off], buf[off+1]]);
        let code = u16::from_le_bytes([buf[off+2], buf[off+3]]);
        let val = i32::from_le_bytes([buf[off+4], buf[off+5], buf[off+6], buf[off+7]]);
        off += 8;
        match kind {
            1 => { if val == 1 { evs.push(Event::KeyDown(code)); } else if val == 0 { evs.push(Event::KeyUp(code)); } }
            2 => match code {
                0 => { mouse.x = core::cmp::max(0, core::cmp::min((sw-1) as i32, mouse.x + val)); evs.push(Event::MouseMove(mouse.x, mouse.y)); }
                1 => { mouse.y = core::cmp::max(0, core::cmp::min((sh-1) as i32, mouse.y + val)); evs.push(Event::MouseMove(mouse.x, mouse.y)); }
                _ => {}
            }
            _ => {}
        }
    }
    evs
}

fn zorder(wins: &[Win], mx: i32, my: i32) -> Option<usize> {
    for i in (0..wins.len()).rev() {
        let w = &wins[i];
        if w.state == WinState::Minimized { continue; }
        if mx >= w.x && mx < w.x + w.w as i32 && my >= w.y && my < w.y + w.h as i32 {
            return Some(i);
        }
    }
    None
}

fn hit_dock_icon(mx: i32, my: i32, sw: usize, sh: usize) -> Option<usize> {
    if my < sh as i32 - DOCK_H as i32 { return None; }
    let item_total = DOCK_APPS.len();
    let dock_w = item_total * (DOCK_ICON + 8) + 16;
    let dx = ((sw as i32 - dock_w as i32) / 2) as i32;
    let rel_x = mx - dx;
    if rel_x < 8 || rel_x >= (dock_w - 8) as i32 { return None; }
    let idx = ((rel_x - 8) / (DOCK_ICON as i32 + 8)) as usize;
    if idx < DOCK_APPS.len() { Some(idx) } else { None }
}

// ── Main ──
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let fb = match Framebuffer::open() { Some(f) => f, None => return 1 };
    let (sw, sh) = (fb.width, fb.height);
    let mut bb = Backbuffer::new(sw, sh);
    let mut mouse = MouseState::new();

    let mut wins: Vec<Win> = Vec::new();
    let mut blink = 0u32;
    let mut mon_refresh = 0u32;
    let mut drag_win: Option<usize> = None;
    let mut _start_menu_open = false;
    let mut sys_menu_open = false;

    // Open first terminal
    let idx = 0usize;
    wins.push(Win {
        x: 60, y: MENU_H as i32 + 30, w: 560, h: 380,
        app: (DOCK_APPS[idx].launcher)(), dragging: false,
        drag_off_x: 0, drag_off_y: 0, state: WinState::Normal,
        rx: 0, ry: 0, rw: 0, rh: 0, app_idx: idx,
    });

    let kbd = CString::new("/dev/input/event0").ok().map(|p| skyos_libc::syscall::open(p.as_ptr() as *const u8, 0)).unwrap_or(u64::MAX);
    let mfd = CString::new("/dev/input/event1").ok().map(|p| skyos_libc::syscall::open(p.as_ptr() as *const u8, 0)).unwrap_or(u64::MAX);

    loop {
        bb.clear(Color::BLACK);
        draw_desktop(&mut bb, sw, sh);

        // Figure out active app name
        let active_name = if wins.is_empty() { "Finder" } else { wins[wins.len() - 1].app.title() };

        // Collect uptime for clock
        let mut uptime = 0u64;
        if let Some(info) = libskyos::sysinfo() { uptime = info.uptime_seconds; }

        draw_menu_bar(&mut bb, active_name, uptime, sw);

        // Draw non-active windows first
        for i in (0..wins.len()).rev() {
            if wins[i].state == WinState::Minimized { continue; }
            if i != wins.len() - 1 {
                draw_window_frame(&mut bb, &wins[i], false, sw, sh);
            }
        }
        // Draw active window last (on top)
        if let Some(w) = wins.last() {
            if w.state != WinState::Minimized {
                draw_window_frame(&mut bb, w, true, sw, sh);
            }
        }

        // Minimized indicators on dock
        let mut minimized = [false; 6];
        for w in &wins {
            if w.state == WinState::Minimized && w.app_idx < 6 {
                minimized[w.app_idx] = true;
            }
        }
        let active_idx = if wins.is_empty() { None } else { Some(wins.len() - 1) };
        draw_dock(&mut bb, sw, sh, active_idx, &minimized);
        draw_mouse(&mut bb, mouse.x, mouse.y, sw, sh);
        fb.blit(bb.as_bytes());

        // ── Event handling ──
        let mut evs = alloc::vec::Vec::new();
        let mut buf = [0u8; 512];
        if (kbd as i64) >= 0 { let n = read_events(kbd, &mut buf); evs.extend(parse_events(&buf, n, &mut mouse, sw, sh)); }
        if (mfd as i64) >= 0 { let n = read_events(mfd, &mut buf); evs.extend(parse_events(&buf, n, &mut mouse, sw, sh)); }

        if evs.is_empty() {
            blink += 1;
            if blink > 10 {
                if let Some(w) = wins.last_mut() {
                    if let Some(term) = w.app.as_any_mut().downcast_mut::<SkyTerm>() {
                        term.cursor_blink = !term.cursor_blink;
                    }
                }
                blink = 0;
            }
            mon_refresh += 1;
            if mon_refresh > 30 {
                for w in wins.iter_mut() {
                    if let Some(mon) = w.app.as_any_mut().downcast_mut::<SkyMon>() { mon.refresh(); }
                }
                mon_refresh = 0;
            }
        }

        // Keyboard events
        for &ev in &evs {
            match ev {
                Event::KeyDown(code) => match code {
                    1 => return 0,
                    15 => { if wins.len() > 1 { let w = wins.remove(0); wins.push(w); } }
                    59 => { wins.push(Win {
                        x: 80 + (wins.len() as i32 * 20) % 300, y: MENU_H as i32 + 40 + (wins.len() as i32 * 20) % 200,
                        w: 520, h: 380, app: (DOCK_APPS[0].launcher)(), dragging: false,
                        drag_off_x: 0, drag_off_y: 0, state: WinState::Normal,
                        rx: 0, ry: 0, rw: 0, rh: 0, app_idx: 0,
                    }); }
                    _ => {
                        if let Some(w) = wins.last_mut() {
                            let cx = w.x as usize + 2; let cy = w.y as usize + TITLE_H + 2;
                            w.app.handle_event(ev, cx, cy, w.w - 4, w.h - TITLE_H - 4);
                        }
                    }
                },
                Event::MouseMove(mx, my) => {
                    mouse.x = mx; mouse.y = my;
                    if let Some(di) = drag_win {
                        if let Some(w) = wins.get_mut(di) {
                            w.x = core::cmp::max(0, core::cmp::min(sw as i32 - w.w as i32, mx - w.drag_off_x));
                            w.y = core::cmp::max(MENU_H as i32, core::cmp::min(sh as i32 - DOCK_H as i32 - w.h as i32, my - w.drag_off_y));
                        }
                    }
                }
                _ => {}
            }
        }

        // Mouse button handling
        if (mfd as i64) >= 0 {
            let mut buf2 = [0u8; 256];
            let n = read_events(mfd, &mut buf2);
            let mut off = 0;
            while off + 8 <= n {
                let kind = u16::from_le_bytes([buf2[off], buf2[off+1]]);
                let code = u16::from_le_bytes([buf2[off+2], buf2[off+3]]);
                let val = i32::from_le_bytes([buf2[off+4], buf2[off+5], buf2[off+6], buf2[off+7]]);
                off += 8;
                if kind == 1 && code == 272 {
                    let (mx, my) = (mouse.x, mouse.y);
                    if val == 1 {
                        mouse.left = true;
                        // Sys menu
                        if my < MENU_H as i32 && mx >= 6 && mx < 50 { sys_menu_open = !sys_menu_open; _start_menu_open = false; }
                        else { sys_menu_open = false; _start_menu_open = false; }

                        // Dock click
                        if let Some(di) = hit_dock_icon(mx, my, sw, sh) {
                            // If a minimized window of this type exists, restore it
                            let mut restored = false;
                            for w in wins.iter_mut() {
                                if w.app_idx == di && w.state == WinState::Minimized {
                                    w.state = WinState::Normal;
                                    // Bring to front
                                    restored = true;
                                    break;
                                }
                            }
                            if !restored {
                                let nid = wins.len();
                                wins.push(Win {
                                    x: 100 + (nid as i32 * 25) % 300,
                                    y: MENU_H as i32 + 40 + (nid as i32 * 25) % 200,
                                    w: DOCK_APPS[di].default_w,
                                    h: DOCK_APPS[di].default_h,
                                    app: (DOCK_APPS[di].launcher)(),
                                    dragging: false, drag_off_x: 0, drag_off_y: 0,
                                    state: WinState::Normal,
                                    rx: 0, ry: 0, rw: 0, rh: 0, app_idx: di,
                                });
                            }
                            continue;
                        }

                        // Check window traffic lights & title bar
                        if let Some(idx) = zorder(&wins, mx, my) {
                            // Bring to front
                            if idx != wins.len() - 1 {
                                let w = wins.remove(idx);
                                wins.push(w);
                            }
                            let last = wins.len() - 1;
                            if wins[last].close_hit(mx, my) {
                                wins.pop();
                            } else if wins[last].min_hit(mx, my) {
                                wins[last].state = WinState::Minimized;
                            } else if wins[last].max_hit(mx, my) {
                                let w = &mut wins[last];
                                if w.state == WinState::Maximized {
                                    w.state = WinState::Normal;
                                    w.x = w.rx; w.y = w.ry; w.w = w.rw; w.h = w.rh;
                                } else {
                                    w.rx = w.x; w.ry = w.y; w.rw = w.w; w.rh = w.h;
                                    w.x = 0; w.y = MENU_H as i32;
                                    w.w = sw; w.h = sh - MENU_H - DOCK_H;
                                    w.state = WinState::Maximized;
                                }
                            } else if my >= wins[last].y && my < wins[last].y + TITLE_H as i32 {
                                drag_win = Some(last);
                                let w = &mut wins[last];
                                w.drag_off_x = mx - w.x;
                                w.drag_off_y = my - w.y;
                                w.dragging = true;
                            }
                            // Forward to app
                            if let Some(w) = wins.last_mut() {
                                w.app.handle_event(Event::MouseDown(mx, my, 272),
                                    w.x as usize + 2, w.y as usize + TITLE_H + 2, w.w - 4, w.h - TITLE_H - 4);
                            }
                        }
                    } else if val == 0 {
                        mouse.left = false;
                        if let Some(di) = drag_win {
                            if let Some(w) = wins.get_mut(di) { w.dragging = false; }
                            drag_win = None;
                        }
                    }
                }
            }
        }

        let _ = skyos_libc::syscall::syscall1(skyos_libc::SYS_NANOSLEEP, 33_000_000);
    }
}

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! { loop {} }
