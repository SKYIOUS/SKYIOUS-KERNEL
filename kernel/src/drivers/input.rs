use crossbeam_queue::ArrayQueue;
use lazy_static::lazy_static;

/// Input event types (Linux-compatible subset)
pub const EV_SYN: u16 = 0x00;
pub const EV_KEY: u16 = 0x01;
pub const EV_REL: u16 = 0x02;

/// Event codes for EV_REL
pub const REL_X: u16 = 0x00;
pub const REL_Y: u16 = 0x01;
pub const REL_WHEEL: u16 = 0x08;

/// Synchronization events
pub const SYN_REPORT: u16 = 0x00;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InputEvent {
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

const EVENT_QUEUE_CAPACITY: usize = 256;

lazy_static! {
    pub static ref KEYBOARD_EVENTS: ArrayQueue<InputEvent> = ArrayQueue::new(EVENT_QUEUE_CAPACITY);
    pub static ref MOUSE_EVENTS: ArrayQueue<InputEvent> = ArrayQueue::new(EVENT_QUEUE_CAPACITY);
}

pub fn push_key_event(code: u16, pressed: bool) {
    let value = if pressed { 1 } else { 0 };
    let _ = KEYBOARD_EVENTS.push(InputEvent { kind: EV_KEY, code, value });
    let _ = KEYBOARD_EVENTS.push(InputEvent { kind: EV_SYN, code: SYN_REPORT, value: 0 });
}

pub fn push_mouse_event(code: u16, delta: i32) {
    let _ = MOUSE_EVENTS.push(InputEvent { kind: EV_REL, code, value: delta });
}

pub fn push_mouse_button(button: u16, pressed: bool) {
    let value = if pressed { 1 } else { 0 };
    let _ = MOUSE_EVENTS.push(InputEvent { kind: EV_KEY, code: button, value });
}

pub fn sync_mouse() {
    let _ = MOUSE_EVENTS.push(InputEvent { kind: EV_SYN, code: SYN_REPORT, value: 0 });
}
