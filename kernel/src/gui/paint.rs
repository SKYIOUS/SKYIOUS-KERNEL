//! Rendering pipeline — damage-tracked compositing and framebuffer upload.

use super::*;
use crate::drivers::graphics::FRAMEBUFFER;
use super::surface::{DirtyRect};

#[cfg(feature = "gpu")]
use crate::compositor::{GuiScene, GuiWindow, DirtyRect as CompDirtyRect};
#[cfg(feature = "gpu")]
use crate::compositor::HwCompositor;
#[cfg(feature = "gpu")]
use core::sync::atomic::Ordering;

#[cfg(feature = "gpu")]
pub(crate) fn software_flip(scene: &GuiScene) {
    let fb_ptr = FRAMEBUFFER.load(Ordering::Relaxed);
    if !fb_ptr.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(scene.backbuffer, fb_ptr, (scene.width * scene.height) as usize);
        }
    }
    crate::drivers::gpu::virtio_gpu::flip();
}

impl Compositor {
    pub fn render(&mut self, mouse_x: usize, mouse_y: usize) {
        // Load wallpaper if dirty
        self.load_wallpaper();

        // Decay notifications
        self.notifications.retain(|n| n.ticks_remaining > 0);
        for notif in &mut self.notifications {
            notif.ticks_remaining = notif.ticks_remaining.saturating_sub(1);
        }

        // Full background + overlay composite (always correct, keep simple)
        // ponytail: always full-composite, only framebuffer copy is delta
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.background_cache.as_ptr(),
                self.backbuffer.as_mut_ptr(),
                SCREEN_WIDTH * SCREEN_HEIGHT,
            );
        }
        shell::draw_taskbar(&mut self.backbuffer);

        let taskbar_y = SCREEN_HEIGHT - 40;
        for (i, win) in self.windows.iter().enumerate() {
            let bx = 70 + i * 120;
            let is_active = i == self.windows.len() - 1;
            let btn_color = if win.minimized { 0xFF3A3A3A } else if is_active { 0xFF2D2D2D } else { 0xFF252526 };
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, taskbar_y + 5, 115, 30, btn_color);
            if is_active {
                drawing::draw_line_h(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, taskbar_y + 5, 115, crate::gui::accent_color());
            }
            let display = if win.title.len() > 13 { &win.title[..13] } else { &*win.title };
            drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx + 5, taskbar_y + 10, display, 0xFFFFFFFF);
        }

        shell::draw_icons(&mut self.backbuffer);

        for win in &mut self.windows {
            if let Some(ref mut term) = win.terminal {
                if term.is_monitor {
                    term.refresh_monitor();
                }
            }
        }

        for window in &self.windows {
            if !window.minimized { window.render(&mut self.backbuffer, mouse_x, mouse_y); }
        }

        self.animations.retain(|anim| anim.frame < anim.total);
        for anim in &mut self.animations {
            anim.frame += 1;
            if anim.window_idx >= self.windows.len() { continue; }
            let win = &self.windows[anim.window_idx];
            if win.minimized { continue; }
            if anim.fade_out {
                let a = (255 * anim.frame / anim.total) as u32;
                let overlay = a.min(255) << 24;
                drawing::draw_rect_alpha(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                    win.x, win.y, win.width, win.height, overlay);
            } else {
                let a = 255 - (255 * anim.frame / anim.total) as u32;
                let overlay = a.min(255) << 24;
                drawing::draw_rect_alpha(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                    win.x, win.y, win.width, win.height, overlay);
            }
        }

        if self.start_menu_open {
            shell::draw_start_menu(&mut self.backbuffer, mouse_x, mouse_y);
        }

        if self.context_menu.open {
            let item_h = 24;
            let menu_w = 160;
            let menu_h = self.context_menu.items.len() * item_h + 8;
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                self.context_menu.x, self.context_menu.y, menu_w, menu_h, 0xE02D2D2D);
            drawing::draw_line_h(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                self.context_menu.x, self.context_menu.y, menu_w, accent_color());
            for (i, (name, _action)) in self.context_menu.items.iter().enumerate() {
                if *name == "---" {
                    let sep_y = self.context_menu.y + 4 + i * item_h + item_h / 2;
                    drawing::draw_line_h(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                        self.context_menu.x + 8, sep_y, menu_w - 16, 0xFF555555);
                    continue;
                }
                let iy = self.context_menu.y + 4 + i * item_h;
                let hover = mouse_x >= self.context_menu.x + 2
                    && mouse_x < self.context_menu.x + menu_w - 2
                    && mouse_y >= iy && mouse_y < iy + item_h;
                if hover {
                    drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                        self.context_menu.x + 2, iy, menu_w - 4, item_h, 0xFF3A3A3A);
                }
                drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT,
                    self.context_menu.x + 12, iy + 5, name, 0xFFCCCCCC);
            }
        }

        if let Some(_idx) = self.drag_index {
            if mouse_x < 5 {
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, 0, SCREEN_WIDTH / 2, SCREEN_HEIGHT - 40, 0x300078D4);
            } else if mouse_x > SCREEN_WIDTH - 5 {
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, SCREEN_WIDTH / 2, 0, SCREEN_WIDTH / 2, SCREEN_HEIGHT - 40, 0x300078D4);
            } else if mouse_y < 5 {
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, 0, 0, SCREEN_WIDTH, SCREEN_HEIGHT - 40, 0x300078D4);
            }
        }

        if self.alt_tab_active && self.windows.len() > 1 {
            let overlay_w = (self.windows.len() as u32) * 130 + 20;
            let overlay_h: u32 = 60;
            let overlay_x = (SCREEN_WIDTH as u32 - overlay_w) / 2;
            let overlay_y = (SCREEN_HEIGHT as u32 - overlay_h) / 2;
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, overlay_x as usize, overlay_y as usize, overlay_w as usize, overlay_h as usize, 0xE01E1E1E);

            for (i, win) in self.windows.iter().enumerate() {
                let bx = overlay_x as usize + 10 + i * 130;
                let by = overlay_y as usize + 10;
                let bg = if i == self.alt_tab_index { 0xFF0078D4 } else { 0xFF3A3A3A };
                drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx, by, 120, 40, bg);
                let display = if win.title.len() > 14 { &win.title[..14] } else { &*win.title };
                drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, bx + 4, by + 14, display, 0xFFFFFFFF);
            }
        }

        let mut notif_y = 50usize;
        for notif in &self.notifications {
            let text_w = notif.text.len() * 8 + 36;
            let x = SCREEN_WIDTH - text_w - 10;
            let color = notif.notif_color();
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, notif_y, text_w, 30, 0xE0252526);
            drawing::draw_rect(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x, notif_y, 4, 30, color);
            let icon = match notif.kind {
                crate::gui::NotifKind::Info => "i",
                crate::gui::NotifKind::Warning => "!",
                crate::gui::NotifKind::Error => "x",
            };
            drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + 8, notif_y + 8, icon, color);
            drawing::draw_string(&mut self.backbuffer, SCREEN_WIDTH, SCREEN_HEIGHT, x + 20, notif_y + 8, &notif.text, 0xFFCCCCCC);
            notif_y += 36;
        }

        mouse::draw_cursor(&mut self.backbuffer, mouse_x, mouse_y);

        #[cfg(feature = "gpu")]
        {
            // Build GuiScene and delegate to HwCompositor
            let rects = self.damage.drain();
            let damage = if rects.is_empty() {
                None
            } else {
                let mut merge = DirtyRect::from_xywh(usize::MAX, usize::MAX, 0, 0);
                for r in &rects {
                    let old_right = merge.x + merge.w;
                    let old_bottom = merge.y + merge.h;
                    merge.x = merge.x.min(r.x);
                    merge.y = merge.y.min(r.y);
                    merge.w = (r.x + r.w).max(old_right) - merge.x;
                    merge.h = (r.y + r.h).max(old_bottom) - merge.y;
                }
                Some(CompDirtyRect { x: merge.x, y: merge.y, w: merge.w, h: merge.h })
            };

            let scene = GuiScene {
                backbuffer: self.backbuffer.as_ptr(),
                width: SCREEN_WIDTH as u32,
                height: SCREEN_HEIGHT as u32,
                windows: self.windows.iter().filter(|w| !w.minimized).map(|w| GuiWindow {
                    gpu_surface: w.gpu_surface.unwrap_or(0),
                    position: (w.x as i32, w.y as i32),
                    size: (w.width as u32, w.height as u32),
                    opacity: w.opacity,
                    blur_radius: w.blur_radius,
                    shadow: w.shadow.clone(),
                    z_order: w.z_order,
                }).collect(),
                damage,
            };

            let mut hw_comp = HwCompositor::new();
            // Register any new windows (first frame only)
            for ws in &scene.windows {
                if ws.gpu_surface == 0 {
                    hw_comp.register_window("", ws.size.0, ws.size.1);
                }
            }
            if hw_comp.compose(&scene).is_err() {
                // Fallback to software path on compose failure
                software_flip(&scene);
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            // Software fallback: original direct copy + flip (drain-and-clear semantics)
            let fb_ptr = FRAMEBUFFER.load(core::sync::atomic::Ordering::Relaxed);
            let rects = self.damage.drain();
            if rects.is_empty() {
                if !fb_ptr.is_null() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(self.backbuffer.as_ptr(), fb_ptr, SCREEN_WIDTH * SCREEN_HEIGHT);
                    }
                }
                crate::drivers::gpu::virtio_gpu::flip();
            } else {
                let mut merge = DirtyRect::from_xywh(usize::MAX, usize::MAX, 0, 0);
                for r in &rects {
                    let old_right = merge.x + merge.w;
                    let old_bottom = merge.y + merge.h;
                    merge.x = merge.x.min(r.x);
                    merge.y = merge.y.min(r.y);
                    merge.w = (r.x + r.w).max(old_right) - merge.x;
                    merge.h = (r.y + r.h).max(old_bottom) - merge.y;
                    if !fb_ptr.is_null() {
                        for row in r.y..r.y + r.h {
                            if row >= SCREEN_HEIGHT { break; }
                            let src = &self.backbuffer[row * SCREEN_WIDTH + r.x..][..r.w];
                            let dst = unsafe { &mut *fb_ptr.add(row * SCREEN_WIDTH + r.x) };
                            unsafe {
                                core::ptr::copy_nonoverlapping(src.as_ptr(), dst, r.w);
                            }
                        }
                    }
                }
                crate::drivers::gpu::virtio_gpu::flip_rect(merge.x as u32, merge.y as u32, merge.w as u32, merge.h as u32);
            }
        }
    }
}
