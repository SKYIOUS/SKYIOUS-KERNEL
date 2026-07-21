//! PS/2 Mouse Driver
//!
//! This module implements a PS/2 mouse driver that handles mouse input via IRQ12.
//! The driver maintains mouse state (position and button status) and communicates
//! with the mouse controller through ports 0x60 (data) and 0x64 (command).
//!
//! Supports IntelliMouse/scroll wheel via 4-byte packets.

use x86_64::instructions::port::Port;
use core::sync::atomic::{AtomicIsize, AtomicU8, AtomicI8, AtomicU64, Ordering};
use spin::Mutex;

/// Counts how many times the mouse IRQ handler fires. Diagnostic — read from GUI task.
pub static MOUSE_IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
pub static MOUSE_IRQ_BYTES: AtomicU64 = AtomicU64::new(0);

const SCREEN_WIDTH: usize = 800;
const SCREEN_HEIGHT: usize = 600;

/// Lock-free cursor position for ISR-safe reads from the GUI task.
pub static CURSOR_X: AtomicIsize = AtomicIsize::new((SCREEN_WIDTH / 2) as isize);
pub static CURSOR_Y: AtomicIsize = AtomicIsize::new((SCREEN_HEIGHT / 2) as isize);
pub static CURSOR_BUTTONS: AtomicU8 = AtomicU8::new(0);
pub static CURSOR_SCROLL: AtomicI8 = AtomicI8::new(0);

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
static PREV_BUTTONS: AtomicU8 = AtomicU8::new(0);

/// Set scroll wheel mode — called after PS/2 init sequence
pub fn enable_wheel() {
    MOUSE_PACKET.lock().has_wheel = true;
}

/// Feed one byte from the PS/2 data port into the mouse packet state machine.
/// Called by the interrupt dispatcher after verifying (via status bit 5) that
/// the byte belongs to the mouse.
pub fn feed_byte(byte: u8) {
    MOUSE_IRQ_BYTES.fetch_add(1, Ordering::Relaxed);
    static FIRST_CALL: AtomicU8 = AtomicU8::new(0);
    if FIRST_CALL.swap(1, Ordering::Relaxed) == 0 {
        crate::serial_write(&alloc::format!("[MOUSE] feed_byte first call: 0x{:02x}\n", byte));
    }
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
            let new_x = CURSOR_X.load(Ordering::Relaxed)
                .saturating_add(x_rel as isize)
                .min((SCREEN_WIDTH - 1) as isize)
                .max(0);
            let new_y = CURSOR_Y.load(Ordering::Relaxed)
                .saturating_sub(y_rel as isize)
                .min((SCREEN_HEIGHT - 1) as isize)
                .max(0);
            CURSOR_X.store(new_x, Ordering::Relaxed);
            CURSOR_Y.store(new_y, Ordering::Relaxed);

            // Scroll wheel (4th byte, signed)
            if mp.has_wheel {
                let scroll = mp.data[3] as i8;
                if scroll != 0 {
                    CURSOR_SCROLL.store(scroll, Ordering::Relaxed);
                    crate::drivers::input::push_mouse_event(crate::drivers::input::REL_WHEEL, scroll as i32);
                }
            }

            let new_buttons = flags & 0x07;
            let prev = PREV_BUTTONS.load(Ordering::Relaxed);
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
                PREV_BUTTONS.store(new_buttons, Ordering::Relaxed);
            }
            CURSOR_BUTTONS.store(new_buttons, Ordering::Relaxed);

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
