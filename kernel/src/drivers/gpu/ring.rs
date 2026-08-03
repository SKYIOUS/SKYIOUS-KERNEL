use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use crate::sync::IrqSafeMutex as Mutex;

const RING_SIZE: usize = 256;
const DMA_SIZE: usize = 1024 * 1024;

#[repr(u32)]
pub enum GpuOpcode {
    TransferRect2D = 0x0105,
    FillRect = 0x0200,
    BlendRects = 0x0201,
    BlurRect = 0x0202,
    ShadowRect = 0x0203,
    Flip = 0x0104,
    CreateSurface = 0x0101,
    DestroySurface = 0x0107,
    SetCursor = 0x0300,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GpuCommand {
    pub opcode: u32,
    pub flags: u32,
    pub payload_offset: u32,
    pub payload_len: u32,
    pub fence_id: u64,
    pub reserved: [u8; 8],
}

impl GpuCommand {
    pub const SIZE: usize = 32;
}

pub struct GpuCommandRing {
    ring: Mutex<alloc::vec::Vec<GpuCommand>>,
    head: AtomicU32,
    tail: AtomicU32,
    dma_buf: Mutex<alloc::vec::Vec<u8>>,
    dma_offset: Mutex<u32>,
    next_fence: AtomicU64,
    committed_fence: AtomicU64,
}

impl GpuCommandRing {
    pub const fn new() -> Self {
        GpuCommandRing {
            ring: Mutex::new(alloc::vec::Vec::new()),
            head: AtomicU32::new(0),
            tail: AtomicU32::new(0),
            dma_buf: Mutex::new(alloc::vec::Vec::new()),
            dma_offset: Mutex::new(0),
            next_fence: AtomicU64::new(1),
            committed_fence: AtomicU64::new(0),
        }
    }

    pub fn init_ring(&self) {
        let mut ring = self.ring.lock();
        ring.reserve(RING_SIZE);
        for _ in ring.len()..RING_SIZE {
            ring.push(GpuCommand {
                opcode: 0, flags: 0, payload_offset: 0, payload_len: 0,
                fence_id: 0, reserved: [0; 8],
            });
        }
    }

    pub fn init_dma(&self, size: usize) {
        let mut buf = self.dma_buf.lock();
        buf.resize(size.max(DMA_SIZE), 0);
        let mut dma_off = self.dma_offset.lock();
        *dma_off = 0;
    }

    pub fn submit(&self, cmd: &GpuCommand, payload: &[u8]) -> Result<u64, ()> {
        let tail = self.tail.load(Ordering::Acquire);
        let head = self.head.load(Ordering::Acquire);
        let count = tail.wrapping_sub(head) & (RING_SIZE as u32 - 1);
        if count >= (RING_SIZE as u32 - 1) {
            return Err(());
        }
        let fence = self.next_fence.fetch_add(1, Ordering::Relaxed);
        let mut dma_off = self.dma_offset.lock();
        let mut dma_buf = self.dma_buf.lock();
        let payload_off = *dma_off;
        if (payload_off as usize) + payload.len() > dma_buf.len() {
            return Err(());
        }
        if !payload.is_empty() {
            dma_buf[payload_off as usize..(payload_off as usize + payload.len())]
                .copy_from_slice(payload);
        }
        *dma_off = payload_off + payload.len() as u32;
        drop(dma_buf);
        let mut ring = self.ring.lock();
        let slot = tail as usize & (RING_SIZE - 1);
        ring[slot] = GpuCommand {
            opcode: cmd.opcode,
            flags: cmd.flags,
            payload_offset: payload_off,
            payload_len: payload.len() as u32,
            fence_id: fence,
            reserved: [0; 8],
        };
        drop(ring);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(fence)
    }

    pub fn poll_completion(&self, fence_id: u64) -> bool {
        self.committed_fence.load(Ordering::Acquire) >= fence_id
    }

    pub fn advance_committed(&self, fence_id: u64) {
        let prev = self.committed_fence.load(Ordering::Relaxed);
        if fence_id > prev {
            self.committed_fence.store(fence_id, Ordering::Release);
        }
    }

    pub fn wait_idle(&self) {
        let tail = self.tail.load(Ordering::Acquire);
        loop {
            let head = self.head.load(Ordering::Acquire);
            if head == tail {
                break;
            }
            core::hint::spin_loop();
        }
    }

    pub fn drain(&self) -> Option<(GpuCommand, u32)> {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let slot = head as usize & (RING_SIZE - 1);
        let ring = self.ring.lock();
        let cmd = ring[slot];
        drop(ring);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some((cmd, head))
    }

    pub fn dma_buf(&self) -> &Mutex<alloc::vec::Vec<u8>> {
        &self.dma_buf
    }
}

pub static COMMAND_RING: GpuCommandRing = GpuCommandRing::new();

pub const IORING_OP_GPU_FLIP: u8 = 32;
pub const IORING_OP_GPU_BLEND: u8 = 33;
pub const IORING_OP_GPU_BLUR: u8 = 34;
pub const IORING_OP_GPU_SHADOW: u8 = 35;
pub const IORING_OP_GPU_FILL: u8 = 36;
pub const IORING_OP_GPU_TRANSFER: u8 = 37;

pub fn init() {
    COMMAND_RING.init_ring();
    COMMAND_RING.init_dma(DMA_SIZE);
}
