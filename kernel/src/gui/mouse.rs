//! GUI Mouse Cursor Renderer
//!
//! This module provides the cursor bitmap and drawing logic.
//! The cursor is now rendered by the compositor to the backbuffer.

use crate::gui::drawing;
use crate::gui::{SCREEN_WIDTH, SCREEN_HEIGHT};

// Cursor dimensions
pub const CURSOR_WIDTH: usize = 9;
pub const CURSOR_HEIGHT: usize = 13;

// Arrow bitmap (1 = Draw, 0 = Transparent)
pub const CURSOR_BITMAP: [u16; 13] = [
    0b100000000,
    0b110000000,
    0b111000000,
    0b111100000,
    0b111110000,
    0b111111000,
    0b111111100,
    0b111111110,
    0b111111000,
    0b110111000,
    0b100011000,
    0b000011000,
    0b000001100,
];

pub fn draw_cursor(buffer: &mut [u32], x: usize, y: usize) {
    let cursor_color = 0xFFFFFFFF;
    let border_color = 0xFF000000;

    for dy in 0..CURSOR_HEIGHT {
        let row = CURSOR_BITMAP[dy];
        for dx in 0..CURSOR_WIDTH {
            let bit = (row >> (CURSOR_WIDTH - 1 - dx)) & 1;
            if bit == 1 {
                drawing::draw_pixel(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + dx, y + dy, cursor_color);

                if x + dx > 0 && (dx == 0 || (row >> (CURSOR_WIDTH - dx)) & 1 == 0) {
                    drawing::draw_pixel(buffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + dx - 1, y + dy, border_color);
                }
            }
        }
    }
}
