#![allow(dead_code)]

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color(0xFF000000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32))
    }
    pub const fn from_u32(val: u32) -> Self { Color(val) }
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const RED: Color = Color::rgb(255, 0, 0);
    pub const GREEN: Color = Color::rgb(0, 255, 0);
    pub const BLUE: Color = Color::rgb(0, 0, 255);
    pub const CYAN: Color = Color::rgb(0, 255, 255);
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);
    pub const YELLOW: Color = Color::rgb(255, 255, 0);
    pub const GRAY: Color = Color::rgb(128, 128, 128);
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);
    pub const ORANGE: Color = Color::rgb(255, 165, 0);
    pub const NAVY: Color = Color::rgb(0, 0, 128);
    pub const TEAL: Color = Color::rgb(0, 128, 128);
    pub const MAROON: Color = Color::rgb(128, 0, 0);
    pub const PURPLE: Color = Color::rgb(128, 0, 128);
    pub const OLIVE: Color = Color::rgb(128, 128, 0);
}
