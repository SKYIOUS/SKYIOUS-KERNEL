//! 2D Drawing Primitives
//!
//! This module provides basic functions for drawing on a buffer.
//! All functions operate on a generic `[u32]` buffer to allow
//! drawing to both backbuffers and the physical framebuffer.

/// Draws a single pixel to the buffer.
pub fn draw_pixel(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, color: u32) {
    if x < width && y < height {
        buffer[y * width + x] = color;
    }
}

/// Draws a filled rectangle to the buffer.
pub fn draw_rect(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        let py = y + dy;
        if py >= height { break; }
        for dx in 0..w {
            let px = x + dx;
            if px >= width { break; }
            buffer[py * width + px] = color;
        }
    }
}

/// Draws a horizontal line to the buffer.
pub fn draw_line_h(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, w: usize, color: u32) {
    if y >= height { return; }
    for dx in 0..w {
        let px = x + dx;
        if px >= width { break; }
        buffer[y * width + px] = color;
    }
}

/// Draws a vertical line to the buffer.
pub fn draw_line_v(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, h: usize, color: u32) {
    if x >= width { return; }
    for dy in 0..h {
        let py = y + dy;
        if py >= height { break; }
        buffer[py * width + x] = color;
    }
}

use font8x8::{UnicodeFonts, BASIC_FONTS};

/// Draws a character using the font8x8 crate with scaling.
pub fn draw_char_scaled(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, c: char, color: u32, scale: usize) {
    if let Some(glyph) = BASIC_FONTS.get(c) {
        for (dy, row) in glyph.iter().enumerate() {
            for dx in 0..8 {
                if (row >> dx) & 1 != 0 {
                    draw_rect(buffer, width, height, x + dx * scale, y + dy * scale, scale, scale, color);
                }
            }
        }
    }
}

/// Draws a string of characters using the bitmap font.
pub fn draw_string(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, s: &str, color: u32) {
    draw_string_scaled(buffer, width, height, x, y, s, color, 1);
}

/// Draws a string of characters using the bitmap font with scaling.
pub fn draw_string_scaled(buffer: &mut [u32], width: usize, height: usize, x: usize, y: usize, s: &str, color: u32, scale: usize) {
    let mut curr_x = x;
    for c in s.chars() {
        draw_char_scaled(buffer, width, height, curr_x, y, c, color, scale);
        curr_x += 8 * scale;
        if curr_x >= width { break; }
    }
}
