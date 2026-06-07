use core::{pin::Pin, task::{Context, Poll}};
use futures_util::stream::Stream;
use crossbeam_queue::ArrayQueue;
use crate::println;
use core::task::Waker;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    static ref SCANCODE_QUEUE: ArrayQueue<u8> = ArrayQueue::new(100);
    static ref GUI_SCANCODE_QUEUE: ArrayQueue<u8> = ArrayQueue::new(100);
    static ref WAKER: Mutex<Option<Waker>> = Mutex::new(None);
}

pub fn add_scancode(scancode: u8) {
    // Push to both queues so both shell and GUI get all scancodes
    let _ = GUI_SCANCODE_QUEUE.push(scancode);
    if let Ok(()) = SCANCODE_QUEUE.push(scancode) {
        if let Some(waker) = WAKER.lock().take() {
            waker.wake();
        }
    } else {
        println!("WARNING: scancode queue full; dropping keyboard input");
    }
}

/// Non-blocking try-pop of a scancode for the GUI compositor.
pub fn try_pop_scancode() -> Option<u8> {
    GUI_SCANCODE_QUEUE.pop()
}

pub struct ScancodeStream {
    _private: (),
}

impl ScancodeStream {
    pub fn new() -> Self {
        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        if let Some(scancode) = SCANCODE_QUEUE.pop() {
            return Poll::Ready(Some(scancode));
        }

        WAKER.lock().replace(cx.waker().clone());

        if let Some(scancode) = SCANCODE_QUEUE.pop() {
            WAKER.lock().take();
            return Poll::Ready(Some(scancode));
        }

        Poll::Pending
    }
}
