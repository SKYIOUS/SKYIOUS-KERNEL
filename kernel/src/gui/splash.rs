//! Boot Splash Screen
//!
//! Renders the SkyOS logo during kernel boot, hiding debug output.
//! The splash is cleared when the GUI compositor initializes.

use core::sync::atomic::{AtomicBool, Ordering};

/// Whether splash mode is active (hides println output)
pub static SPLASH_ACTIVE: AtomicBool = AtomicBool::new(true);

/// Draw a single pixel directly to the framebuffer (no bounds check for speed)
#[inline(always)]
unsafe fn put_pixel(fb: *mut u32, stride: usize, x: u32, y: u32, color: u32) {
    fb.add(y as usize * stride + x as usize).write_volatile(color);
}

/// Draw a filled rectangle directly to the framebuffer
unsafe fn fill_rect(fb: *mut u32, stride: usize, x: u32, y: u32, w: u32, h: u32, color: u32, screen_w: u32, screen_h: u32) {
    for dy in 0..h {
        let py = y + dy;
        if py >= screen_h { break; }
        for dx in 0..w {
            let px = x + dx;
            if px >= screen_w { break; }
            put_pixel(fb, stride, px, py, color);
        }
    }
}

/// Draw the SkyOS splash screen on the framebuffer.
/// Called right after framebuffer hardware init.
pub fn init() {
    let fb_ptr = crate::drivers::graphics::FRAMEBUFFER.load(Ordering::Relaxed);
    if fb_ptr.is_null() { return; }

    let width = crate::drivers::graphics::WIDTH.load(core::sync::atomic::Ordering::Relaxed);
    let height = crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed);
    if width == 0 || height == 0 { return; }

    let stride = crate::drivers::graphics::STRIDE.load(core::sync::atomic::Ordering::Relaxed);
    let w = width as u32;
    let h = height as u32;

    // Fill background: deep navy gradient
    for y in 0..height {
        let r = 15u32 + (25u32 * y as u32 / h);
        let g = 20u32 + (35u32 * y as u32 / h);
        let b = 50u32 + (100u32 * y as u32 / h);
        let color = 0xFF000000 | (r << 16) | (g << 8) | b;
        for x in 0..width {
            unsafe { put_pixel(fb_ptr, stride, x as u32, y as u32, color); }
        }
    }

    // Draw a large centered "S" as the SkyOS logo mark
    let logo_size = 80u32;
    let logo_x = (w - logo_size) / 2;
    let logo_y = h / 2 - logo_size - 30;
    let accent = 0xFF0078D4;

    // "S" from the 5x7 font, scaled up
    let s_glyph = [0x3Cu8, 0x40, 0x40, 0x3C, 0x02, 0x02, 0x7C];
    let scale = logo_size / 5;
    for (gy, &row) in s_glyph.iter().enumerate() {
        for gx in 0..5u32 {
            if row & (0x20 >> gx) != 0 {
                unsafe {
                    fill_rect(fb_ptr, stride,
                        logo_x + gx * scale, logo_y + gy as u32 * scale,
                        scale, scale, accent, w, h);
                }
            }
        }
    }

    // Draw "SkyOS" text below the logo
    let text = "SkyOS";
    let char_w = 5u32;
    let char_h = 7u32;
    let text_scale = 4u32;
    let text_total_w = text.len() as u32 * char_w * text_scale + (text.len() as u32 - 1) * text_scale;
    let text_x = (w - text_total_w) / 2;
    let text_y = logo_y + logo_size + 20;

    let glyphs: &[[u8; 7]] = &[
        [0x3C, 0x40, 0x40, 0x3C, 0x02, 0x02, 0x7C], // S
        [0x44, 0x48, 0x50, 0x60, 0x50, 0x48, 0x44], // k
        [0x44, 0x44, 0x44, 0x3C, 0x04, 0x08, 0x70], // y
        [0x38, 0x44, 0x44, 0x44, 0x44, 0x44, 0x38], // O
        [0x3C, 0x40, 0x40, 0x3C, 0x02, 0x02, 0x7C], // S
    ];

    for (i, glyph) in glyphs.iter().enumerate() {
        let cx = text_x + i as u32 * (char_w * text_scale + text_scale);
        for (gy, &row) in glyph.iter().enumerate() {
            for gx in 0..char_w {
                if row & (0x20 >> gx) != 0 {
                    unsafe {
                        fill_rect(fb_ptr, stride,
                            cx + gx * text_scale, text_y + gy as u32 * text_scale,
                            text_scale, text_scale, 0xFFFFFFFF, w, h);
                    }
                }
            }
        }
    }

    // Draw subtitle
    let subtitle = "A modern OS in Rust";
    let sub_char_w = 5u32;
    let sub_scale = 1u32;
    let sub_total_w = subtitle.len() as u32 * (sub_char_w + 1) * sub_scale;
    let sub_x = (w - sub_total_w) / 2;
    let sub_y = text_y + char_h * text_scale + 24;

    // Simple 3x5 font for subtitle (only lowercase + space + common chars)
    // We'll use a minimal approach: draw small rectangles for readable text
    let sub_color = 0xFF999999;
    for (i, &ch) in subtitle.as_bytes().iter().enumerate() {
        let cx = sub_x + i as u32 * (sub_char_w + 1) * sub_scale;
        // Draw a simple placeholder character (small filled rect with gap)
        if ch != b' ' {
            unsafe {
                fill_rect(fb_ptr, stride, cx, sub_y, sub_char_w * sub_scale, 5 * sub_scale, sub_color, w, h);
            }
        }
    }

    // Draw loading bar
    let bar_w = 200u32;
    let bar_h = 4u32;
    let bar_x = (w - bar_w) / 2;
    let bar_y = sub_y + 20;

    // Bar background
    unsafe {
        fill_rect(fb_ptr, stride, bar_x, bar_y, bar_w, bar_h, 0xFF333333, w, h);
    }

    // Flip to display if VirtIO GPU is active
    crate::drivers::gpu::virtio_gpu::flip();
}

/// Clear the splash screen (called when GUI compositor initializes)
pub fn clear() {
    SPLASH_ACTIVE.store(false, Ordering::Relaxed);
}
