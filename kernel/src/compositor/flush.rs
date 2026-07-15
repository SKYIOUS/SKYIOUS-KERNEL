use crate::drivers::gpu::ring::COMMAND_RING;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Clone, Copy, PartialEq)]
pub enum FlushResult {
    Submitted(u64),
    Completed,
    WouldBlock,
}

static FLUSH_PENDING: AtomicBool = AtomicBool::new(false);
static FLUSH_FENCE: AtomicU64 = AtomicU64::new(0);

pub fn gui_flush_async(window_id: u64) -> FlushResult {
    if FLUSH_PENDING.load(Ordering::Acquire) {
        let fence = FLUSH_FENCE.load(Ordering::Relaxed);
        if COMMAND_RING.poll_completion(fence) {
            FLUSH_PENDING.store(false, Ordering::Release);
            return FlushResult::Completed;
        }
        return FlushResult::WouldBlock;
    }

    // Mark window dirty in compositor
    let mut comp = crate::gui::COMPOSITOR.lock();
    if let Some(win) = comp.windows.iter_mut().find(|w| {
        w.gpu_resource_id.map_or(false, |rid| rid as u64 == window_id)
    }) {
        win.dirty = true;
    }
    drop(comp);

    let cmd = crate::drivers::gpu::ring::GpuCommand {
        opcode: crate::drivers::gpu::ring::GpuOpcode::Flip as u32,
        flags: 0, payload_offset: 0, payload_len: 0,
        fence_id: 0, reserved: [0; 8],
    };
    match COMMAND_RING.submit(&cmd, &[]) {
        Ok(fence) => {
            FLUSH_PENDING.store(true, Ordering::Release);
            FLUSH_FENCE.store(fence, Ordering::Relaxed);
            FlushResult::Submitted(fence)
        }
        Err(()) => FlushResult::WouldBlock,
    }
}

pub fn poll_flush(fence_id: u64) -> bool {
    COMMAND_RING.poll_completion(fence_id)
}
