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

    // Basics
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

    // Modern dark theme palette
    pub const ACCENT: Color = Color::rgb(0, 120, 212);
    pub const ACCENT_HOVER: Color = Color::rgb(0, 140, 240);
    pub const ACCENT_ACTIVE: Color = Color::rgb(0, 90, 180);
    pub const BG_DARK: Color = Color::rgb(30, 30, 30);
    pub const BG_SURFACE: Color = Color::rgb(37, 37, 38);
    pub const BG_TITLE: Color = Color::rgb(45, 45, 45);
    pub const BG_HOVER: Color = Color::rgb(42, 45, 46);
    pub const BG_INPUT: Color = Color::rgb(60, 60, 60);
    pub const BORDER: Color = Color::rgb(60, 60, 60);
    pub const TEXT: Color = Color::rgb(204, 204, 204);
    pub const TEXT_BRIGHT: Color = Color::rgb(240, 240, 240);
    pub const TEXT_SUBTLE: Color = Color::rgb(136, 136, 136);
    pub const SHADOW: Color = Color::rgb(10, 10, 10);
    pub const CLOSE_RED: Color = Color::rgb(232, 17, 35);
    pub const CLOSE_HOVER: Color = Color::rgb(255, 40, 55);
    pub const ICON_TERM: Color = Color::rgb(0, 200, 83);
    pub const ICON_CALC: Color = Color::rgb(255, 215, 0);
    pub const ICON_FILES: Color = Color::rgb(0, 180, 255);
    pub const ICON_MONITOR: Color = Color::rgb(255, 111, 0);
    pub const ICON_SETTINGS: Color = Color::rgb(41, 121, 255);
    pub const ICON_ABOUT: Color = Color::rgb(123, 31, 162);
}
