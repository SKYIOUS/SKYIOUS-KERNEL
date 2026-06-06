//! Vahi Kernel Shell
//!
//! Handles the rendering of the desktop, taskbar, and start menu.

use crate::gui::{drawing, SCREEN_WIDTH, SCREEN_HEIGHT};

pub fn draw_background(buffer: &mut [u32]) {
    let bg_color = 0x001A237E; // Deep Blue
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, 0, SCREEN_WIDTH, SCREEN_HEIGHT, bg_color);
}

pub fn draw_taskbar(buffer: &mut [u32]) {
    let taskbar_height = 40;
    let taskbar_color = 0xFF303030; // Dark Gray
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, SCREEN_HEIGHT - taskbar_height, SCREEN_WIDTH, taskbar_height, taskbar_color);

    // Start Button
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 5, SCREEN_HEIGHT - 35, 60, 30, 0xFF008000); // Green Button
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 12, SCREEN_HEIGHT - 28, "START", 0xFFFFFFFF);
}

pub fn draw_icons(buffer: &mut [u32]) {
    // Computer Icon
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 20, 20, 48, 48, 0xFFFFFFFF); 
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, 75, "SYSTEM", 0xFFFFFFFF);
    
    // Folder Icon
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 20, 110, 48, 48, 0xFFFFFFFF); 
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, 165, "FILES", 0xFFFFFFFF);
}

pub fn draw_start_menu(buffer: &mut [u32]) {
    drawing::draw_rect(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 5, SCREEN_HEIGHT - 240, 150, 200, 0xFFE0E0E0);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, SCREEN_HEIGHT - 230, "File Manager", 0xFF000000);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, SCREEN_HEIGHT - 210, "Terminal", 0xFF000000);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, SCREEN_HEIGHT - 190, "Settings", 0xFF000000);
    drawing::draw_string(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, 15, SCREEN_HEIGHT - 60, "Shutdown", 0xFF000000);
}
