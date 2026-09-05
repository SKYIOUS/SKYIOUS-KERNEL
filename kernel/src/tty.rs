use crate::sync::IrqSafeMutex as Mutex;
use crossbeam_queue::ArrayQueue;
use lazy_static::lazy_static;
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

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
                        // Ctrl+C — deliver SIGINT to current foreground process.
                        // Keyboard IRQ context (I4): resolve per-CPU without
                        // blocking and try-lock every acquisition — a blocking
                        // lock here could freeze the CPU.
                        let proc = crate::syscalls::helpers::try_get_current_process();
                        if let Some(p) = proc {
                            if let Some(mut sig) = p.signals.try_lock() {
                                sig.raise(crate::syscalls::signal::Signal::SIGINT);
                            }
                            // Route SIGINT to signalfd instances
                            crate::task::process::route_signal_to_signalfd_for(
                                &p,
                                2,
                                crate::task::process::SI_USER,
                                0,
                                0,
                                0,
                            );
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
