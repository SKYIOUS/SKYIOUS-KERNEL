//! PS/2 Mouse Driver
//!
//! This module implements a PS/2 mouse driver that handles mouse input via IRQ12.
//! The driver maintains mouse state (position and button status) and communicates
//! with the mouse controller through ports 0x60 (data) and 0x64 (command).
//!
//! Supports IntelliMouse/scroll wheel via 4-byte packets.

use x86_64::instructions::port::Port;
use spin::Mutex;
use lazy_static::lazy_static;

const SCREEN_WIDTH: usize = 800;
const SCREEN_HEIGHT: usize = 600;

pub struct MouseState {
    pub x: usize,
    pub y: usize,
    pub buttons: u8,
    pub scroll: i8,
}

lazy_static! {
    pub static ref MOUSE: Mutex<MouseState> = Mutex::new(MouseState {
        x: SCREEN_WIDTH / 2,
        y: SCREEN_HEIGHT / 2,
        buttons: 0,
        scroll: 0,
    });
}

/// Whether scroll wheel was detected
pub static HAS_WHEEL: spin::Mutex<bool> = spin::Mutex::new(false);

pub fn init() {
    // Hardware init moved to drivers::ps2::init()
}

struct MousePacket {
    data: [u8; 4],
    index: usize,
    has_wheel: bool,
}

static MOUSE_PACKET: Mutex<MousePacket> = Mutex::new(MousePacket {
    data: [0; 4],
    index: 0,
    has_wheel: false,
});

// Track previous button state to only push EV_KEY on changes
static PREV_BUTTONS: Mutex<u8> = Mutex::new(0);

/// Set scroll wheel mode — called after PS/2 init sequence
pub fn enable_wheel() {
    MOUSE_PACKET.lock().has_wheel = true;
    *HAS_WHEEL.lock() = true;
}

/// Feed one byte from the PS/2 data port into the mouse packet state machine.
/// Called by the interrupt dispatcher after verifying (via status bit 5) that
/// the byte belongs to the mouse.
pub fn feed_byte(byte: u8) {
    let mut mp = MOUSE_PACKET.lock();
    let pkt_size = if mp.has_wheel { 4 } else { 3 };

    // Validate first byte of a new packet: bit 3 must be set (always 1 for mouse)
    if mp.index == 0 && (byte & 0x08) == 0 {
        // Not a valid mouse start byte — discard and resync
        return;
    }

    let idx = mp.index;
    mp.data[idx] = byte;
    mp.index += 1;

    if mp.index >= pkt_size {
        let flags = mp.data[0];

        // Validate: bit 3 must be 1 (mouse packet signature)
        if flags & 0x08 == 0 {
            mp.index = 0;
            return;
        }
        let x_raw = mp.data[1] as i32;
        let y_raw = mp.data[2] as i32;
        let x_sign = (flags as i32 >> 4) & 1;
        let y_sign = (flags as i32 >> 5) & 1;
        let x_rel: i32 = if x_sign == 0 { x_raw } else { x_raw - 256 };
        let y_rel: i32 = if y_sign == 0 { y_raw } else { y_raw - 256 };

        if flags & 0xC0 == 0 {
            let mut mouse = MOUSE.lock();

            if x_rel > 0 {
                mouse.x = (mouse.x + x_rel as usize).min(SCREEN_WIDTH - 1);
            } else {
                mouse.x = mouse.x.saturating_sub((-x_rel) as usize);
            }

            let y_rel = -y_rel; // Invert Y axis (PS/2 positive=down, screen positive=up)
            if y_rel > 0 {
                mouse.y = (mouse.y + y_rel as usize).min(SCREEN_HEIGHT - 1);
            } else {
                mouse.y = mouse.y.saturating_sub((-y_rel) as usize);
            }

            // Scroll wheel (4th byte, signed)
            if mp.has_wheel {
                let scroll = mp.data[3] as i8;
                if scroll != 0 {
                    mouse.scroll = scroll;
                    crate::drivers::input::push_mouse_event(crate::drivers::input::REL_WHEEL, scroll as i32);
                }
            }

            let new_buttons = flags & 0x07;
            let prev = *PREV_BUTTONS.lock();
            if new_buttons != prev {
                if (new_buttons & 1) != (prev & 1) {
                    crate::drivers::input::push_mouse_button(0x110, new_buttons & 1 != 0);
                }
                if (new_buttons & 2) != (prev & 2) {
                    crate::drivers::input::push_mouse_button(0x111, new_buttons & 2 != 0);
                }
                if (new_buttons & 4) != (prev & 4) {
                    crate::drivers::input::push_mouse_button(0x112, new_buttons & 4 != 0);
                }
                *PREV_BUTTONS.lock() = new_buttons;
            }
            mouse.buttons = new_buttons;

            if x_rel != 0 {
                crate::drivers::input::push_mouse_event(crate::drivers::input::REL_X, x_rel);
            }
            if y_rel != 0 {
                crate::drivers::input::push_mouse_event(crate::drivers::input::REL_Y, y_rel);
            }
            crate::drivers::input::sync_mouse();
        }

        mp.index = 0;
    }
}

pub fn handle_interrupt() {
    let mut port = Port::<u8>::new(0x60);
    let byte = unsafe { port.read() };
    feed_byte(byte);
}
