//! PS/2 Mouse Driver
//!
//! This module implements a PS/2 mouse driver that handles mouse input via IRQ12.
//! The driver maintains mouse state (position and button status) and communicates
//! with the mouse controller through ports 0x60 (data) and 0x64 (command).
//!
//! # Protocol
//! The PS/2 mouse sends data in 3-byte packets:
//! - Byte 0: Flags (Y overflow, X overflow, Y sign, X sign, Always 1, Middle, Right, Left)
//! - Byte 1: X movement (signed 8-bit)
//! - Byte 2: Y movement (signed 8-bit)

use x86_64::instructions::port::Port;
use spin::Mutex;
use lazy_static::lazy_static;

const SCREEN_WIDTH: usize = 800;
const SCREEN_HEIGHT: usize = 600;

pub struct MouseState {
    pub x: usize,
    pub y: usize,
    pub buttons: u8,
}

lazy_static! {
    pub static ref MOUSE: Mutex<MouseState> = Mutex::new(MouseState {
        x: SCREEN_WIDTH / 2,
        y: SCREEN_HEIGHT / 2,
        buttons: 0,
    });
}

pub fn init() {
    // Hardware init moved to drivers::ps2::init()
}

struct MousePacket {
    data: [u8; 3],
    index: usize,
}

static MOUSE_PACKET: Mutex<MousePacket> = Mutex::new(MousePacket {
    data: [0; 3],
    index: 0,
});

// Track previous button state to only push EV_KEY on changes
static PREV_BUTTONS: Mutex<u8> = Mutex::new(0);

/// Feed one byte from the PS/2 data port into the mouse packet state machine.
/// Called by the interrupt dispatcher after verifying (via status bit 5) that
/// the byte belongs to the mouse.
pub fn feed_byte(byte: u8) {
    let mut mp = MOUSE_PACKET.lock();
    let idx = mp.index;
    mp.data[idx] = byte;
    mp.index += 1;

    if mp.index == 3 {
        let flags = mp.data[0];
        // 9-bit two's complement reconstruction (sign bits in flags bits 4/5)
        let x_raw = mp.data[1] as i32;
        let y_raw = mp.data[2] as i32;
        let x_sign = (flags as i32 >> 4) & 1;
        let y_sign = (flags as i32 >> 5) & 1;
        let x_rel: i32 = if x_sign == 0 { x_raw } else { x_raw - 256 };
        let y_rel: i32 = if y_sign == 0 { y_raw } else { y_raw - 256 };

        // Validate packet: overflow bits (6-7) must be clear
        if flags & 0xC0 == 0 {
            let mut mouse = MOUSE.lock();

            if x_rel > 0 {
                mouse.x = (mouse.x + x_rel as usize).min(SCREEN_WIDTH - 1);
            } else {
                mouse.x = mouse.x.saturating_sub((-x_rel) as usize);
            }

            if y_rel > 0 {
                mouse.y = (mouse.y + y_rel as usize).min(SCREEN_HEIGHT - 1);
            } else {
                mouse.y = mouse.y.saturating_sub((-y_rel) as usize);
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
