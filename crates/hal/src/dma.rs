//! DMA buffer abstractions.
//!
//! Provides `DmaBuf` for single allocations and `PooledDma` for pooled
//! hot-path DMA buffers. These are stubs extracted from `kernel/src/hal/dma.rs`.

extern crate alloc;

use alloc::alloc::{alloc, dealloc, Layout};
use core::ptr::NonNull;

/// A single DMA-safe buffer, physically contiguous and cache-line aligned.
pub struct DmaBuf {
    ptr: NonNull<u8>,
    size: usize,
    phys: u64,
}

impl DmaBuf {
    /// Allocate a DMA-safe buffer of `size` bytes.
    pub fn new(size: usize) -> Option<Self> {
        let layout = Layout::from_size_align(size, 4096).ok()?;
        // SAFETY: layout is non-zero (size >= 1, align = 4096). The global
        // allocator is initialized before any DMA allocation occurs.
        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            None
        } else {
            let phys = ptr as u64;
            Some(DmaBuf {
                ptr: NonNull::new(ptr)?,
                size,
                phys,
            })
        }
    }

    /// Physical address of the buffer.
    pub fn phys(&self) -> u64 {
        self.phys
    }

    /// Virtual address of the buffer.
    pub fn virt(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }

    /// Size of the buffer in bytes.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Const pointer to the buffer.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }

    /// Mutable pointer to the buffer.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        let layout = Layout::from_size_align(self.size, 4096).unwrap();
        // SAFETY: ptr was allocated by alloc() in DmaBuf::new() with the
        // same layout. No double-free risk — Drop runs once per instance.
        unsafe { dealloc(self.ptr.as_ptr(), layout) };
    }
}

/// RAII wrapper: allocates a DMA buffer from the pool, returns it on drop.
pub struct PooledDma {
    phys_addr: u64,
    virt_addr: *mut u8,
    size: usize,
    from_pool: bool,
}

impl PooledDma {
    /// Allocate a pooled DMA buffer of at least `min_size` bytes.
    pub fn alloc(min_size: usize, _bdf: u16) -> Option<Self> {
        let buf = DmaBuf::new(min_size)?;
        let phys = buf.phys();
        let virt = buf.virt();
        let sz = buf.size();
        core::mem::forget(buf);
        Some(PooledDma {
            phys_addr: phys,
            virt_addr: virt,
            size: sz,
            from_pool: false,
        })
    }

    pub fn phys(&self) -> u64 {
        self.phys_addr
    }
    pub fn virt(&self) -> *mut u8 {
        self.virt_addr
    }
    pub fn size(&self) -> usize {
        self.size
    }
    pub fn as_ptr(&self) -> *const u8 {
        self.virt_addr
    }
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.virt_addr
    }
}

impl Drop for PooledDma {
    fn drop(&mut self) {
        if !self.from_pool {
            let layout = Layout::from_size_align(self.size, 4096).unwrap();
            // SAFETY: virt_addr was allocated by alloc() in PooledDma::alloc()
            // with the same layout. Pooled buffers are returned to the pool.
            unsafe { dealloc(self.virt_addr, layout) };
        }
    }
}
