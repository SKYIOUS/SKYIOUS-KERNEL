pub mod bga;
pub mod console;

use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

pub static FRAMEBUFFER: AtomicPtr<u32> = AtomicPtr::new(core::ptr::null_mut());
pub static WIDTH: AtomicUsize = AtomicUsize::new(0);
pub static HEIGHT: AtomicUsize = AtomicUsize::new(0);
pub static STRIDE: AtomicUsize = AtomicUsize::new(0);

pub fn init_limine(framebuffer: Option<&limine::framebuffer::Framebuffer>) {
    if let Some(fb) = framebuffer {
        WIDTH.store(fb.width as usize, Ordering::SeqCst);
        HEIGHT.store(fb.height as usize, Ordering::SeqCst);
        STRIDE.store(fb.pitch as usize, Ordering::SeqCst);

        let ptr = fb.address() as *mut u32;
        FRAMEBUFFER.store(ptr, Ordering::SeqCst);

        // Clear screen initially
        console::WRITER.lock().clear_screen();
    }
}

pub fn is_active() -> bool {
    !FRAMEBUFFER.load(Ordering::Relaxed).is_null()
}
