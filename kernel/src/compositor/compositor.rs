use core::sync::atomic::{AtomicU64, Ordering};
use alloc::vec::Vec;
use crate::drivers::gpu::ring::{GpuCommand, GpuOpcode, COMMAND_RING};
use crate::compositor::vsync::FpsCounter;
use crate::compositor::blend::BlendMode;
use crate::compositor::shadow::ShadowParams;

pub struct WindowSurface {
    pub window_id: u64,
    pub gpu_surface: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
    pub z_order: u32,
    pub opacity: f32,
    pub blur_radius: u32,
    pub shadow: Option<ShadowParams>,
    pub dirty: bool,
    pub texture: Option<u32>,
}

pub struct HwCompositor {
    display_surface: u32,
    window_surfaces: alloc::vec::Vec<WindowSurface>,
    vsync: bool,
    pub frame_count: u64,
    fps_counter: FpsCounter,
    next_window_id: AtomicU64,
}

impl HwCompositor {
    pub fn new() -> Self {
        HwCompositor {
            display_surface: 1,
            window_surfaces: alloc::vec::Vec::new(),
            vsync: true,
            frame_count: 0,
            fps_counter: FpsCounter::new(),
            next_window_id: AtomicU64::new(1),
        }
    }

    pub fn register_window(&mut self, _title: &str, w: u32, h: u32) -> u64 {
        if let Some(ref mut gpu) = *crate::drivers::gpu::virtio_gpu::GPU.lock() {
            let rid = gpu.create_resource(w, h);
            let id = self.next_window_id.fetch_add(1, Ordering::Relaxed);
            self.window_surfaces.push(WindowSurface {
                window_id: id,
                gpu_surface: rid,
                position: (100, 100),
                size: (w, h),
                z_order: self.window_surfaces.len() as u32,
                opacity: 1.0,
                blur_radius: 0,
                shadow: None,
                dirty: true,
                texture: Some(rid),
            });
            id
        } else {
            0
        }
    }

    pub fn unregister_window(&mut self, window_id: u64) {
        self.window_surfaces.retain(|ws| ws.window_id != window_id);
    }

    pub fn set_opacity(&mut self, window_id: u64, opacity: f32) {
        if let Some(ws) = self.window_surfaces.iter_mut().find(|w| w.window_id == window_id) {
            ws.opacity = opacity.clamp(0.0, 1.0);
            ws.dirty = true;
        }
    }

    pub fn set_blur(&mut self, window_id: u64, radius: u32) {
        if let Some(ws) = self.window_surfaces.iter_mut().find(|w| w.window_id == window_id) {
            ws.blur_radius = radius;
            ws.dirty = true;
        }
    }

    pub fn set_shadow(&mut self, window_id: u64, params: ShadowParams) {
        if let Some(ws) = self.window_surfaces.iter_mut().find(|w| w.window_id == window_id) {
            ws.shadow = Some(params);
            ws.dirty = true;
        }
    }

    pub fn move_window(&mut self, window_id: u64, x: i32, y: i32) {
        if let Some(ws) = self.window_surfaces.iter_mut().find(|w| w.window_id == window_id) {
            ws.position = (x, y);
            ws.dirty = true;
        }
    }

    pub fn resize_window(&mut self, window_id: u64, w: u32, h: u32) {
        if let Some(ws) = self.window_surfaces.iter_mut().find(|w| w.window_id == window_id) {
            ws.size = (w, h);
            ws.dirty = true;
        }
    }

    pub fn composite_frame(&mut self) -> Result<(), ()> {
        let now = crate::interrupts::get_ticks();
        // ponytail: single-pass bottom-up composite
        let mut sorted: Vec<usize> = (0..self.window_surfaces.len()).collect();
        sorted.sort_by_key(|&i| self.window_surfaces[i].z_order);

        for &idx in &sorted {
            let ws = &self.window_surfaces[idx];
            if !ws.dirty { continue; }

            // Submit shadow first (behind window)
            if let Some(ref shadow_params) = ws.shadow {
                crate::compositor::shadow::render_shadow(
                    &COMMAND_RING,
                    ws.gpu_surface,
                    shadow_params,
                    ws.position.0, ws.position.1,
                    ws.size.0, ws.size.1,
                )?;
            }

            // Submit blur effect
            if ws.blur_radius > 0 {
                let blur = crate::compositor::blur::GaussianBlur::generate_kernel(ws.blur_radius);
                blur.apply(&COMMAND_RING, ws.gpu_surface, 0, 0, ws.size.0, ws.size.1)?;
            }

            // Submit alpha-blended window content
            if ws.opacity < 1.0 {
                crate::compositor::blend::blend_surface(
                    &COMMAND_RING, self.display_surface, ws.gpu_surface,
                    ws.position, ws.opacity, BlendMode::Normal,
                )?;
            } else {
                // Opaque window: full copy via normal blend w/ opacity=1
                crate::compositor::blend::blend_surface(
                    &COMMAND_RING, self.display_surface, ws.gpu_surface,
                    ws.position, 1.0, BlendMode::Normal,
                )?;
            }
        }

        // Submit flip
        let cmd = GpuCommand {
            opcode: GpuOpcode::Flip as u32,
            flags: 0, payload_offset: 0, payload_len: 0,
            fence_id: 0, reserved: [0; 8],
        };
        let fence = COMMAND_RING.submit(&cmd, &[])?;

        // Wait for fence (simplified — real driver uses async completion IRQ)
        while !COMMAND_RING.poll_completion(fence) {
            core::hint::spin_loop();
        }

        // Clear dirty flags
        for ws in &mut self.window_surfaces {
            ws.dirty = false;
        }

        self.frame_count += 1;
        self.fps_counter.tick(now);
        if self.vsync {
            crate::compositor::vsync::wait_vsync();
        }
        Ok(())
    }

    pub fn composite_window(&mut self, window_id: u64) -> Result<(), ()> {
        let idx = match self.window_surfaces.iter().position(|w| w.window_id == window_id) {
            Some(i) => i,
            None => return Err(()),
        };
        let ws = &self.window_surfaces[idx];

        if let Some(ref shadow_params) = ws.shadow {
            crate::compositor::shadow::render_shadow(
                &COMMAND_RING, ws.gpu_surface, shadow_params,
                ws.position.0, ws.position.1, ws.size.0, ws.size.1,
            )?;
        }
        if ws.blur_radius > 0 {
            let blur = crate::compositor::blur::GaussianBlur::generate_kernel(ws.blur_radius);
            blur.apply(&COMMAND_RING, ws.gpu_surface, 0, 0, ws.size.0, ws.size.1)?;
        }
        if ws.opacity < 1.0 {
            crate::compositor::blend::blend_surface(
                &COMMAND_RING, self.display_surface, ws.gpu_surface,
                ws.position, ws.opacity, BlendMode::Normal,
            )?;
        }
        let cmd = GpuCommand {
            opcode: GpuOpcode::Flip as u32,
            flags: 0, payload_offset: 0, payload_len: 0,
            fence_id: 0, reserved: [0; 8],
        };
        let fence = COMMAND_RING.submit(&cmd, &[])?;
        while !COMMAND_RING.poll_completion(fence) {
            core::hint::spin_loop();
        }
        self.window_surfaces[idx].dirty = false;
        Ok(())
    }

    pub fn fps(&self) -> f32 {
        self.fps_counter.fps()
    }

    pub fn set_vsync(&mut self, enabled: bool) {
        self.vsync = enabled;
    }
}
