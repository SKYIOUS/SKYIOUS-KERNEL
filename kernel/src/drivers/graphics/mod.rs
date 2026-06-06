pub mod bga;
pub mod console;

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub static FRAMEBUFFER: AtomicPtr<u32> = AtomicPtr::new(core::ptr::null_mut());
pub static WIDTH: AtomicUsize = AtomicUsize::new(0);
pub static HEIGHT: AtomicUsize = AtomicUsize::new(0);
pub static STRIDE: AtomicUsize = AtomicUsize::new(0);

pub fn init(framebuffer: Option<&mut bootloader_api::info::FrameBuffer>) {
    if let Some(fb) = framebuffer {
        let info = fb.info();
        WIDTH.store(info.width, Ordering::SeqCst);
        HEIGHT.store(info.height, Ordering::SeqCst);
        STRIDE.store(info.stride, Ordering::SeqCst);

        let ptr = fb.buffer_mut().as_mut_ptr() as *mut u32;
        FRAMEBUFFER.store(ptr, Ordering::SeqCst);
        
        // Clear screen initially
        console::WRITER.lock().clear_screen();
    }
}

pub fn is_active() -> bool {
    !FRAMEBUFFER.load(Ordering::Relaxed).is_null()
}
