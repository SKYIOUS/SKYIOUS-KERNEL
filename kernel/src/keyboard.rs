//! Keyboard Driver
//!
//! This module provides keyboard input handling by passing scancodes
//! to the async task keyboard handler for processing.

pub fn handle_scancode(scancode: u8) {
    crate::task::keyboard::add_scancode(scancode);
}
