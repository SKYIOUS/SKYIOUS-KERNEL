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

        // fb.address() returns an HHDM-mapped virtual pointer (Limine maps it).
        let ptr = fb.address() as *mut u32;
        FRAMEBUFFER.store(ptr, Ordering::SeqCst);
        // ponytail: skip early clear_screen — Limine already shows a clean
        // framebuffer. Pixel-by-pixel clear is slow under TCG and risks
        // faulting if the fb physical pages aren't fully mapped yet. The
        // console clears on first write.
    }
}

pub fn is_active() -> bool {
    !FRAMEBUFFER.load(Ordering::Relaxed).is_null()
}
