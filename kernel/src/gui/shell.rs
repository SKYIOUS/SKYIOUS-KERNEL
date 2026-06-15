//! Vahi Kernel Shell
//!
//! Handles the rendering of the desktop, taskbar, and start menu.

use crate::gui::{drawing, SCREEN_WIDTH, SCREEN_HEIGHT};

fn format_time() -> [u8; 6] {
    let (secs, _) = crate::drivers::rtc::read_realtime();
    if secs <= 0 {
        return [b'0', b'0', b':', b'0', b'0', 0];
    }
    let total_secs = secs as u64;
    let hours = (total_secs / 3600) % 24;
    let minutes = (total_secs / 60) % 60;
    let mut buf = [0u8; 6];
    buf[0] = b'0' + (hours / 10) as u8;
    buf[1] = b'0' + (hours % 10) as u8;
    buf[2] = b':';
    buf[3] = b'0' + (minutes / 10) as u8;
    buf[4] = b'0' + (minutes % 10) as u8;
    buf[5] = 0;
    buf
}

pub fn draw_background(buffer: &mut [u32]) {
    // Vertical gradient using integer math (no soft-float)
    for y in 0..SCREEN_HEIGHT {
        let r = (20 + (40 - 20) * y / SCREEN_HEIGHT) as u32;
        let g = (30 + (50 - 30) * y / SCREEN_HEIGHT) as u32;
        let b = (80 + (160 - 80) * y / SCREEN_HEIGHT) as u32;
        let color = 0xFF000000 | (r << 16) | (g << 8) | b;
        let row_start = y * SCREEN_WIDTH;
        let row = &mut buffer[row_start..row_start + SCREEN_WIDTH];
        for px in row.iter_mut() {
            *px = color;
        }
    }
}

pub fn draw_taskbar(buffer: &mut [u32]) {
    let taskbar_height = 40;
    let taskbar_color = 0xFF1E1E1E;
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, SCREEN_HEIGHT - taskbar_height, SCREEN_WIDTH, taskbar_height, taskbar_color);
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, SCREEN_HEIGHT - taskbar_height, SCREEN_WIDTH, 0xFF333333);

    // Start Button
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 5, SCREEN_HEIGHT - 35, 60, 30, crate::gui::accent_color());
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 12, SCREEN_HEIGHT - 28, "START", 0xFFFFFFFF);

    // Separator and clock on the right
    drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH - 65, SCREEN_HEIGHT - 35, 30, 0xFF333333);
    let time_str = format_time();
    let time_slice = core::str::from_utf8(&time_str[..5]).unwrap_or("00:00");
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH - 55, SCREEN_HEIGHT - 28, time_slice, 0xFFCCCCCC);
}

pub fn draw_icons(buffer: &mut [u32]) {
    // Monitor icon for SYSTEM
    let (ix, iy) = (20, 20);
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, ix, iy, 48, 36, crate::gui::accent_color());
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, ix + 6, iy + 6, 36, 24, 0xFF1A1A2E);
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, ix + 18, iy + 36, 12, 6, 0xFF555555);
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, ix + 12, iy + 42, 24, 4, 0xFF555555);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, ix + 4, iy + 56, "SYSTEM", 0xFFFFFFFF);

    // Folder icon for FILES
    let (fx, fy) = (20, 100);
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, fx, fy + 10, 48, 36, 0xFFD4A017);
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, fx, fy, 18, 10, 0xFFE8B830);
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, fx + 6, fy + 24, 36, 0xFFFFCC00);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, fx + 8, fy + 56, "FILES", 0xFFFFFFFF);
}

pub const MENU_ITEM_COUNT: usize = 6;
pub const MENU_ITEMS: [&str; MENU_ITEM_COUNT] = ["File Manager", "Terminal", "System Monitor", "About", "Settings", "Shutdown"];
pub const MENU_ICONS: [u32; MENU_ITEM_COUNT] = [0xFFFFD700, 0xFF00C853, 0xFFFF6F00, 0xFF7B1FA2, 0xFF2979FF, 0xFFD50000];

pub fn start_menu_rects() -> (usize, usize, usize) {
    let header_h = 24;
    let menu_h: usize = header_h + MENU_ITEM_COUNT * 36 + 10;
    (5, SCREEN_HEIGHT - 40 - menu_h, 180)
}

pub fn draw_start_menu(buffer: &mut [u32], mouse_x: usize, mouse_y: usize) {
    let (menu_x, menu_y, menu_w) = start_menu_rects();
    let header_h = 24;
    let menu_h: usize = header_h + MENU_ITEM_COUNT * 36 + 10;

    // Menu body
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_w, menu_h, 0xFF1E1E1E);
    let accent = crate::gui::accent_color();
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_w, accent);
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y + menu_h - 1, menu_w, accent);
    drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_h, accent);
    drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + menu_w - 1, menu_y, menu_h, accent);

    // Header
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_w, header_h, 0xFF252526);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 8, menu_y + 6, "SkyOS Menu", accent);
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y + header_h, menu_w, 0xFF333333);

    for (i, item) in MENU_ITEMS.iter().enumerate() {
        let iy = menu_y + header_h + 5 + i * 36;
        let hovered = mouse_x >= menu_x + 2 && mouse_x < menu_x + menu_w - 2
                   && mouse_y >= iy && mouse_y < iy + 34;

        if hovered {
            drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 2, iy, menu_w - 4, 34, 0xFF2A2D2E);
            drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 2, iy, menu_w - 4, accent);
        }
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 8, iy + 5, 24, 24, MENU_ICONS[i]);
        drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 40, iy + 10, item, 0xFFCCCCCC);
    }
}
