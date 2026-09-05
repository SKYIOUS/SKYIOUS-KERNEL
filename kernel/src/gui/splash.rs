//! Boot Splash Screen
//!
//! Renders the SkyOS logo during kernel boot, hiding debug output.
//! The splash is cleared when the GUI compositor initializes.

use core::sync::atomic::{AtomicBool, Ordering};

/// Whether splash mode is active (hides println output)
pub static SPLASH_ACTIVE: AtomicBool = AtomicBool::new(true);

/// Draw the SkyOS splash screen on the framebuffer.
/// Called right after framebuffer hardware init.
pub fn init() {
    let fb_ptr = crate::drivers::graphics::FRAMEBUFFER.load(Ordering::Relaxed);
    if fb_ptr.is_null() {
        return;
    }

    let width = crate::drivers::graphics::WIDTH.load(core::sync::atomic::Ordering::Relaxed);
    let height = crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed);
    if width == 0 || height == 0 {
        return;
    }

    let stride = crate::drivers::graphics::STRIDE.load(core::sync::atomic::Ordering::Relaxed);

    // ponytail: solid fill only — gradient + glyph rendering is ~100x slower
    // under TCG. Restore the full splash when KVM acceleration is available.
    let bg = 0xFF1A237Eu32; // deep navy (0x001A237E with alpha)
    for y in 0..height {
        let row = unsafe { fb_ptr.add(y * stride) };
        for x in 0..width {
            unsafe {
                row.add(x).write_volatile(bg);
            }
        }
    }

    // Flip to display if VirtIO GPU is active
    crate::drivers::gpu::virtio_gpu::flip();
}

/// Clear the splash screen (called when GUI compositor initializes)
pub fn clear() {
    SPLASH_ACTIVE.store(false, Ordering::Relaxed);
}
