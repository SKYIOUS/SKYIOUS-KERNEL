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
        let r = (13 + (26 - 13) * y / SCREEN_HEIGHT) as u32;
        let g = (27 + (35 - 27) * y / SCREEN_HEIGHT) as u32;
        let b = (62 + (126 - 62) * y / SCREEN_HEIGHT) as u32;
        let color = 0xFF000000 | (r << 16) | (g << 8) | b;
        let row_start = y * SCREEN_WIDTH;
        // Fill row
        let row = &mut buffer[row_start..row_start + SCREEN_WIDTH];
        for px in row.iter_mut() {
            *px = color;
        }
    }
}

pub fn draw_taskbar(buffer: &mut [u32]) {
    let taskbar_height = 40;
    let taskbar_color = 0xFF303030; // Dark Gray
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, SCREEN_HEIGHT - taskbar_height, SCREEN_WIDTH, taskbar_height, taskbar_color);

    // Start Button
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 5, SCREEN_HEIGHT - 35, 60, 30, 0xFF008000); // Green Button
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 12, SCREEN_HEIGHT - 28, "START", 0xFFFFFFFF);

    // Clock - right side of taskbar
    let time_str = format_time();
    let time_slice = core::str::from_utf8(&time_str[..5]).unwrap_or("00:00");
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH - 55, SCREEN_HEIGHT - 28, time_slice, 0xFFE0E0E0);
}

pub fn draw_icons(buffer: &mut [u32]) {
    // Computer Icon
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 20, 20, 48, 48, 0xFFFFFFFF); 
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, 75, "SYSTEM", 0xFFFFFFFF);
    
    // Folder Icon
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 20, 110, 48, 48, 0xFFFFFFFF); 
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, 165, "FILES", 0xFFFFFFFF);
}

pub const MENU_ITEM_COUNT: usize = 5;
pub const MENU_ITEMS: [&str; MENU_ITEM_COUNT] = ["File Manager", "Terminal", "Settings", "About", "Shutdown"];
pub const MENU_ICONS: [u32; MENU_ITEM_COUNT] = [0xFFFFD700, 0xFF00C853, 0xFF2979FF, 0xFF7B1FA2, 0xFFD50000];

fn start_menu_rects() -> (usize, usize, usize) {
    (5, SCREEN_HEIGHT - 250, 180)
}

pub fn draw_start_menu(buffer: &mut [u32], mouse_x: usize, mouse_y: usize) {
    let (menu_x, menu_y, menu_w) = start_menu_rects();
    let menu_h: usize = MENU_ITEM_COUNT * 36 + 10;

    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_w, menu_h, 0xFF2D2D2D);
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_w, 0xFF555555);
    drawing::draw_line_h(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y + menu_h - 1, menu_w, 0xFF555555);
    drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x, menu_y, menu_h, 0xFF555555);
    drawing::draw_line_v(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + menu_w - 1, menu_y, menu_h, 0xFF555555);

    for (i, item) in MENU_ITEMS.iter().enumerate() {
        let iy = menu_y + 5 + i * 36;
        let hovered = mouse_x >= menu_x + 2 && mouse_x < menu_x + menu_w - 2
                   && mouse_y >= iy && mouse_y < iy + 34;

        if hovered {
            drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 2, iy, menu_w - 4, 34, 0xFF3A3A5C);
        }
        drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 8, iy + 5, 24, 24, MENU_ICONS[i]);
        drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, menu_x + 40, iy + 10, item, 0xFFFFFFFF);
    }
}
