//! USB HID boot-protocol handling.
//!
//! This module decodes the fixed-format HID *boot* reports (8-byte keyboard,
//! up-to-8-byte mouse) into the kernel input subsystem, mirroring what the
//! PS/2 path produces. Devices are switched into boot protocol at enumeration
//! time so the report layout is well-defined regardless of their report
//! descriptor.
//!
//! Key code space: the kernel's input subsystem keys off
//! `pc_keyboard::KeyCode as u16` (see `tty.rs::feed_scancode`). We therefore
//! map HID Keyboard usage IDs to `KeyCode` variants — never to literal
//! discriminants — so the mapping survives `pc_keyboard` version bumps.

use pc_keyboard::KeyCode;

use crate::drivers::input;

// ─── HID class requests (USB HID 7.2) ────────────────────────────────────────

pub const HID_REQ_GET_REPORT: u8 = 0x01;
pub const HID_REQ_SET_REPORT: u8 = 0x09;
pub const HID_REQ_GET_IDLE: u8 = 0x02;
pub const HID_REQ_SET_IDLE: u8 = 0x0A;
pub const HID_REQ_GET_PROTOCOL: u8 = 0x03;
pub const HID_REQ_SET_PROTOCOL: u8 = 0x0B;

/// Boot-protocol device kind, inferred from the HID interface's bInterfaceProtocol
/// (1 = keyboard, 2 = mouse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidKind {
    Keyboard,
    Mouse,
}

// ─── Keyboard report decoder ─────────────────────────────────────────────────

/// Decode an 8-byte HID boot keyboard report and emit key events.
///
/// Layout: `[modifiers, reserved, keys[0..6]]`. `keys[]` holds the currently
/// pressed non-modifier usage IDs; pressing/releasing shows as a key entering
/// or leaving the array. We diff against `prev` to emit press/release edges.
/// Modifier changes are diffed out of the modifier byte directly.
pub fn decode_keyboard_boot(report: &[u8; 8], prev: &mut [u8; 8]) {
    let new_mods = report[0];
    let old_mods = prev[0];
    let changed = new_mods ^ old_mods;

    // Modifier byte bit assignment (HID 1.11 Appendix B):
    //   0=LCtrl 1=LShift 2=LAlt 3=LGUI 4=RCtrl 5=RShift 6=RAlt 7=RGUI
    if changed != 0 {
        for bit in 0..8u32 {
            if changed & (1 << bit) != 0 {
                let pressed = new_mods & (1 << bit) != 0;
                if let Some(kc) = modifier_to_keycode(bit as u8) {
                    input::push_key_event(kc as u16, pressed);
                }
            }
        }
    }

    // Diff the key array: anything in `report` not in `prev` is a new press;
    // anything in `prev` not in `report` is a release.
    let new_keys = &report[2..8];
    let old_keys = &prev[2..8];

    for &k in new_keys {
        if k == 0 {
            continue;
        }
        if !old_keys.contains(&k) {
            if let Some(kc) = hid_usage_to_keycode(k) {
                input::push_key_event(kc as u16, true);
            }
        }
    }
    for &k in old_keys {
        if k == 0 {
            continue;
        }
        if !new_keys.contains(&k) {
            if let Some(kc) = hid_usage_to_keycode(k) {
                input::push_key_event(kc as u16, false);
            }
        }
    }

    *prev = *report;
}

/// Map a modifier bit (0..7) to its `KeyCode`.
fn modifier_to_keycode(bit: u8) -> Option<KeyCode> {
    Some(match bit {
        0 => KeyCode::ControlLeft,
        1 => KeyCode::ShiftLeft,
        2 => KeyCode::AltLeft,
        3 => KeyCode::WindowsLeft,
        4 => KeyCode::ControlRight,
        5 => KeyCode::ShiftRight,
        6 => KeyCode::AltRight,
        7 => KeyCode::WindowsRight,
        _ => return None,
    })
}

// ─── Mouse report decoder ────────────────────────────────────────────────────

/// Decode a HID boot mouse report and emit mouse events.
///
/// Layout: `[buttons, dx, dy, dwheel?]`. Button bits: 0=left, 1=right,
/// 2=middle (matching the Linux BTN_LEFT/BTN_RIGHT/BTN_MIDDLE codes used by
/// the PS/2 path). Y is positive-up in HID, but the PS/2 path emits
/// positive-down and lets the GUI invert, so we negate Y here to stay
/// consistent with what consumers expect.
pub fn decode_mouse_boot(report: &[u8], prev: &mut [u8; 8]) {
    if report.len() < 3 {
        return;
    }
    let buttons = report[0] & 0x07;
    let dx = report[1] as i8 as i32;
    let dy = report[2] as i8 as i32;

    let prev_buttons = prev[0] & 0x07;
    let changed = buttons ^ prev_buttons;
    if changed != 0 {
        if changed & 1 != 0 {
            input::push_mouse_button(0x110, buttons & 1 != 0);
        }
        if changed & 2 != 0 {
            input::push_mouse_button(0x111, buttons & 2 != 0);
        }
        if changed & 4 != 0 {
            input::push_mouse_button(0x112, buttons & 4 != 0);
        }
    }

    if dx != 0 {
        input::push_mouse_event(input::REL_X, dx);
    }
    if dy != 0 {
        // Negate: HID Y is up-positive, consumers expect down-positive.
        input::push_mouse_event(input::REL_Y, -dy);
    }

    if report.len() >= 4 {
        let wheel = report[3] as i8 as i32;
        if wheel != 0 {
            input::push_mouse_event(input::REL_WHEEL, wheel);
        }
    }

    input::sync_mouse();

    // Remember button state for the next diff.
    prev[0] = buttons;
}

// ─── HID Keyboard usage → KeyCode table ──────────────────────────────────────

/// Map a USB HID Keyboard/Keypad usage ID (HID Usage Tables 1.21, section 10)
/// to a `pc_keyboard::KeyCode`. Covers the common 0x04–0xA4 range (A–Z,
/// digits, function keys, navigation, editing, punctuation). Returns `None`
/// for unmapped usages so unknown keys are silently ignored rather than
/// mis-delivered.
pub fn hid_usage_to_keycode(usage: u8) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match usage {
        // a–z (0x04–0x1D)
        0x04 => A, 0x05 => B, 0x06 => C, 0x07 => D, 0x08 => E, 0x09 => F,
        0x0A => G, 0x0B => H, 0x0C => I, 0x0D => J, 0x0E => K, 0x0F => L,
        0x10 => M, 0x11 => N, 0x12 => O, 0x13 => P, 0x14 => Q, 0x15 => R,
        0x16 => S, 0x17 => T, 0x18 => U, 0x19 => V, 0x1A => W, 0x1B => X,
        0x1C => Y, 0x1D => Z,
        // 1–9, 0 (0x1E–0x27)
        0x1E => Key1, 0x1F => Key2, 0x20 => Key3, 0x21 => Key4, 0x22 => Key5,
        0x23 => Key6, 0x24 => Key7, 0x25 => Key8, 0x26 => Key9, 0x27 => Key0,
        0x28 => Enter,
        0x29 => Escape,
        0x2A => Backspace,
        0x2B => Tab,
        0x2C => Spacebar,
        0x2D => Minus,
        0x2E => Equals,
        0x2F => BracketSquareLeft,
        0x30 => BracketSquareRight,
        0x31 => BackSlash,
        0x33 => SemiColon,
        0x34 => Quote,
        0x35 => BackTick,
        0x36 => Comma,
        0x37 => Fullstop,
        0x38 => Slash,
        // CapsLock 0x39
        0x39 => CapsLock,
        // F1–F12 (0x3A–0x45)
        0x3A => F1, 0x3B => F2, 0x3C => F3, 0x3D => F4, 0x3E => F5, 0x3F => F6,
        0x40 => F7, 0x41 => F8, 0x42 => F9, 0x43 => F10, 0x44 => F11, 0x45 => F12,
        // NumLock / ScrollLock
        0x53 => NumpadLock,
        0x47 => ScrollLock,
        // Navigation cluster
        0x49 => Insert,
        0x4A => Home,
        0x4B => PageUp,
        0x4C => Delete,
        0x4D => End,
        0x4E => PageDown,
        // Arrows
        0x4F => ArrowRight,
        0x50 => ArrowLeft,
        0x51 => ArrowDown,
        0x52 => ArrowUp,
        // Numpad digits
        0x54 => NumpadSlash,
        0x55 => NumpadStar,
        0x56 => NumpadMinus,
        0x57 => NumpadPlus,
        0x58 => NumpadEnter,
        0x59 => Numpad1, 0x5A => Numpad2, 0x5B => Numpad3, 0x5C => Numpad4,
        0x5D => Numpad5, 0x5E => Numpad6, 0x5F => Numpad7, 0x60 => Numpad8,
        0x61 => Numpad9, 0x62 => Numpad0,
        0x63 => NumpadPeriod,
        _ => return None,
    })
}
