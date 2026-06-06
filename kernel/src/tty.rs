use pc_keyboard::{Keyboard, ScancodeSet1, HandleControl, layouts, DecodedKey};
use crossbeam_queue::ArrayQueue;
use spin::Mutex;
use lazy_static::lazy_static;

const TTY_BUF_SIZE: usize = 4096;

lazy_static! {
    static ref TTY_KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
        Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore)
    );
    pub static ref TTY_INPUT: ArrayQueue<u8> = ArrayQueue::new(TTY_BUF_SIZE);
}

pub fn feed_scancode(scancode: u8) {
    let mut kbd = TTY_KEYBOARD.lock();
    if let Ok(Some(key_event)) = kbd.add_byte(scancode) {
        let pressed = key_event.state == pc_keyboard::KeyState::Down;
        crate::drivers::input::push_key_event(key_event.code as u16, pressed);

        if let Some(key) = kbd.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(c) => {
                    if c == '\u{3}' {
                        // Ctrl+C — deliver SIGINT to current foreground process
                        let proc = crate::task::process::CURRENT_PROCESS.lock();
                        if let Some(ref p) = *proc {
                            p.signals.lock().raise(crate::syscalls::signal::Signal::SIGINT);
                        }
                        // Also echo ^C to console
                        let _ = TTY_INPUT.push(b'^');
                        let _ = TTY_INPUT.push(b'C');
                        let _ = TTY_INPUT.push(b'\r');
                        return;
                    }
                    if c == '\n' {
                        let _ = TTY_INPUT.push(b'\r');
                    }
                    let _ = TTY_INPUT.push(c as u8);
                }
                DecodedKey::RawKey(_raw) => {}
            }
        }
    }
}
