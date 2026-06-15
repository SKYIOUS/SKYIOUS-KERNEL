pub mod virtio_gpu;

/// DRM/KMS abstraction entry point for all GPU backends.

pub fn width() -> u32 {
    if let Some(gpu) = virtio_gpu::GPU.lock().as_ref() {
        gpu.width
    } else {
        crate::drivers::graphics::WIDTH.load(core::sync::atomic::Ordering::Relaxed) as u32
    }
}

pub fn height() -> u32 {
    if let Some(gpu) = virtio_gpu::GPU.lock().as_ref() {
        gpu.height
    } else {
        crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed) as u32
    }
}

pub fn set_mode(w: u32, h: u32) {
    if let Some(ref mut gpu) = *virtio_gpu::GPU.lock() {
        gpu.width = w;
        gpu.height = h;
    }
}
