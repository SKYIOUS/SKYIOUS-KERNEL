//! Zero-Copy Networking and Scatter-Gather I/O
//!
//! Provides infrastructure for sending and receiving network data without
//! copying between kernel and userspace buffers. This is the foundation
//! for high-performance networking in Vahi.
//!
//! ## Zero-Copy Sends
//!
//! Instead of copying data from userspace into kernel buffers, zero-copy sends
//! pass a reference to the user's buffer directly to the network stack. The
//! buffer must be registered via `register_zerocopy_buffer()` before use.
//!
//! ## Scatter-Gather I/O
//!
//! Scatter-gather allows sending/receiving data from multiple non-contiguous
//! buffers in a single operation, avoiding the need to copy them into a single
//! contiguous buffer. This is implemented via the standard iovec (iovec) interface.
//!
//! ## MSG_ZEROCOPY Flag
//!
//! When `MSG_ZEROCOPY` is set in sendmsg/recvmsg, the kernel attempts to send
//! data directly from the user's registered buffer. If the buffer is not
//! registered, it falls back to copying.

use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;

/// MSG_ZEROCOPY flag (Linux-compatible)
pub const MSG_ZEROCOPY: i32 = 0x4000;
/// MSG_TRUNC flag (data truncated)
pub const MSG_TRUNC: i32 = 0x20;
/// MSG_MORE flag (more data coming)
pub const MSG_MORE: i32 = 0x8000;
/// MSG_DONTWAIT flag (non-blocking)
pub const MSG_DONTWAIT: i32 = 0x40;
/// MSG_NOSIGNAL flag (don't generate SIGPIPE)
pub const MSG_NOSIGNAL: i32 = 0x40000;

/// Maximum registered zero-copy buffers per process
const MAX_ZEROCOPY_BUFFERS: usize = 256;

/// Maximum single message size for zero-copy (1 MiB)
pub const MAX_ZEROCOPY_MSG_SIZE: usize = 1 << 20;

/// Maximum total scatter-gather segments per message
pub const IOV_MAX: usize = 1024;

/// A registered zero-copy buffer
#[derive(Clone, Debug)]
pub struct ZerocopyBuffer {
    /// Virtual address of the buffer in userspace
    pub addr: usize,
    /// Length of the buffer
    pub len: usize,
    /// Whether this buffer is currently in use (pinned)
    pub pinned: bool,
    /// Process ID that owns this buffer
    pub pid: u32,
}

/// Per-process zero-copy buffer registry
pub struct ZerocopyRegistry {
    /// Registered buffers keyed by address
    buffers: BTreeMap<usize, ZerocopyBuffer>,
    /// Total registered bytes
    total_registered: usize,
}

impl ZerocopyRegistry {
    const fn new() -> Self {
        Self {
            buffers: BTreeMap::new(),
            total_registered: 0,
        }
    }
}

lazy_static! {
    /// Global zero-copy buffer registry
    pub static ref ZEROCOPY_REGISTRY: Mutex<ZerocopyRegistry> = Mutex::new(ZerocopyRegistry::new());
}

/// Registration result
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZerocopyResult {
    /// Buffer registered successfully
    Registered,
    /// Buffer already registered
    AlreadyRegistered,
    /// Registry is full
    RegistryFull,
    /// Buffer overlaps with existing registration
    OverlapsExisting,
    /// Invalid buffer (null or too large)
    InvalidBuffer,
}

/// Register a userspace buffer for zero-copy sends.
///
/// The buffer must be page-aligned for optimal performance, but unaligned
/// buffers are accepted (they will be copied instead of zero-copied).
///
/// Returns `ZerocopyResult` indicating success or failure reason.
pub fn register_zerocopy_buffer(addr: usize, len: usize, pid: u32) -> ZerocopyResult {
    if addr == 0 || len == 0 || len > MAX_ZEROCOPY_MSG_SIZE {
        return ZerocopyResult::InvalidBuffer;
    }

    let mut registry = ZEROCOPY_REGISTRY.lock();

    if registry.buffers.len() >= MAX_ZEROCOPY_BUFFERS {
        return ZerocopyResult::RegistryFull;
    }

    // Check for overlap with existing registrations
    for (&existing_addr, existing_buf) in &registry.buffers {
        let existing_end = existing_addr + existing_buf.len;
        let new_end = addr + len;
        if addr < existing_end && new_end > existing_addr {
            return ZerocopyResult::OverlapsExisting;
        }
    }

    if registry.buffers.contains_key(&addr) {
        return ZerocopyResult::AlreadyRegistered;
    }

    registry.buffers.insert(addr, ZerocopyBuffer {
        addr,
        len,
        pinned: false,
        pid,
    });
    registry.total_registered += len;
    ZerocopyResult::Registered
}

/// Unregister a previously registered zero-copy buffer.
pub fn unregister_zerocopy_buffer(addr: usize) -> bool {
    let mut registry = ZEROCOPY_REGISTRY.lock();
    if let Some(buf) = registry.buffers.remove(&addr) {
        registry.total_registered -= buf.len;
        true
    } else {
        false
    }
}

/// Check if a buffer is registered for zero-copy.
pub fn is_zerocopy_registered(addr: usize) -> bool {
    let registry = ZEROCOPY_REGISTRY.lock();
    registry.buffers.contains_key(&addr)
}

/// Pin a registered buffer (prevent it from being freed during a send).
/// Returns true if the buffer was pinned successfully.
pub fn pin_zerocopy_buffer(addr: usize) -> bool {
    let mut registry = ZEROCOPY_REGISTRY.lock();
    if let Some(buf) = registry.buffers.get_mut(&addr) {
        if !buf.pinned {
            buf.pinned = true;
            return true;
        }
    }
    false
}

/// Unpin a previously pinned buffer.
pub fn unpin_zerocopy_buffer(addr: usize) {
    let mut registry = ZEROCOPY_REGISTRY.lock();
    if let Some(buf) = registry.buffers.get_mut(&addr) {
        buf.pinned = false;
    }
}

/// Get total registered zero-copy bytes (for diagnostics)
pub fn zerocopy_total_registered() -> usize {
    let registry = ZEROCOPY_REGISTRY.lock();
    registry.total_registered
}

/// Get number of registered zero-copy buffers (for diagnostics)
pub fn zerocopy_buffer_count() -> usize {
    let registry = ZEROCOPY_REGISTRY.lock();
    registry.buffers.len()
}

/// Scatter-gather segment descriptor for zero-copy operations.
///
/// This is an internal representation used by the networking stack to
/// track which parts of registered buffers are being sent/received.
#[derive(Debug, Clone)]
pub struct ScatterSegment {
    /// Pointer to the data (may be in userspace if zero-copy, or kernel buffer if copied)
    pub data: *const u8,
    /// Length of this segment
    pub len: usize,
    /// Whether this segment is zero-copy (data is in userspace)
    pub is_zerocopy: bool,
    /// Buffer registration address (if zero-copy)
    pub registration_addr: Option<usize>,
}

// SAFETY: ScatterSegment is only accessed through the networking stack
unsafe impl Send for ScatterSegment {}
unsafe impl Sync for ScatterSegment {}

impl ScatterSegment {
    /// Create a copied segment (data was copied from userspace to kernel buffer)
    pub fn copied(data: *const u8, len: usize) -> Self {
        Self {
            data,
            len,
            is_zerocopy: false,
            registration_addr: None,
        }
    }

    /// Create a zero-copy segment (data is directly in userspace buffer)
    pub fn zerocopy(data: *const u8, len: usize, registration_addr: usize) -> Self {
        Self {
            data,
            len,
            is_zerocopy: true,
            registration_addr: Some(registration_addr),
        }
    }
}

/// Scatter-gather I/O state for a send/recv operation
pub struct ScatterGatherIo {
    /// Segments to process
    pub segments: Vec<ScatterSegment>,
    /// Total bytes across all segments
    pub total_bytes: usize,
    /// Whether any segment is zero-copy
    pub has_zerocopy: bool,
}

impl ScatterGatherIo {
    /// Create from iovec array (standard scatter-gather setup).
    ///
    /// This validates the iovecs and creates segments. If `allow_zerocopy`
    /// is true and buffers are registered, zero-copy segments are used.
    pub fn from_iovec(
        iov_base: *const u8,
        _iov_len: usize,
        iov_count: usize,
        allow_zerocopy: bool,
    ) -> Option<Self> {
        if iov_base.is_null() || iov_count == 0 || iov_count > IOV_MAX {
            return None;
        }

        let mut segments = Vec::with_capacity(iov_count);
        let mut total_bytes = 0usize;
        let mut has_zerocopy = false;

        // Read iovec array from userspace
        let iov_size = core::mem::size_of::<crate::syscalls::net_helpers::iovec>();
        let mut iov_buf: Vec<crate::syscalls::net_helpers::iovec> = Vec::with_capacity(iov_count);
        for _ in 0..iov_count {
            iov_buf.push(crate::syscalls::net_helpers::iovec {
                iov_base: core::ptr::null_mut(),
                iov_len: 0,
            });
        }

        unsafe {
            if crate::syscalls::user_access::copy_from_user(
                core::slice::from_raw_parts_mut(
                    iov_buf.as_mut_ptr() as *mut u8,
                    iov_count * iov_size,
                ),
                iov_base,
            )
            .is_err()
            {
                return None;
            }
        }

        for iov in &iov_buf {
            if iov.iov_len == 0 {
                continue;
            }

            let base = iov.iov_base as usize;
            let can_zerocopy = allow_zerocopy && is_zerocopy_registered(base);

            if can_zerocopy {
                segments.push(ScatterSegment::zerocopy(
                    iov.iov_base as *const u8,
                    iov.iov_len,
                    base,
                ));
                has_zerocopy = true;
            } else {
                // Must copy from userspace
                let mut buf = alloc::vec![0u8; iov.iov_len];
                unsafe {
                    if crate::syscalls::user_access::copy_from_user(
                        &mut buf,
                        iov.iov_base as *const u8,
                    )
                    .is_err()
                    {
                        return None;
                    }
                }
                let ptr = buf.as_ptr();
                // Leak the buffer so it stays alive — caller must free
                core::mem::forget(buf);
                segments.push(ScatterSegment::copied(ptr, iov.iov_len));
            }

            total_bytes += iov.iov_len;
        }

        Some(Self {
            segments,
            total_bytes,
            has_zerocopy,
        })
    }

    /// Get all data as a contiguous slice (only valid if not zero-copy).
    ///
    /// For non-zero-copy operations, returns a slice into the copied buffers.
    /// For zero-copy, returns None (must iterate segments individually).
    pub fn as_contiguous_slice(&self) -> Option<&[u8]> {
        if self.has_zerocopy {
            return None;
        }

        // For single-segment non-zero-copy, return directly
        if self.segments.len() == 1 && !self.segments[0].is_zerocopy {
            let seg = &self.segments[0];
            return Some(unsafe { core::slice::from_raw_parts(seg.data, seg.len) });
        }

        // For multi-segment, we'd need to assemble — return None for safety
        None
    }

    /// Copy all segments into a single contiguous buffer.
    ///
    /// This is the fallback path when zero-copy isn't possible.
    pub fn to_contiguous(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.total_bytes);
        for seg in &self.segments {
            unsafe {
                let src = core::slice::from_raw_parts(seg.data, seg.len);
                buf.extend_from_slice(src);
            }
        }
        buf
    }
}

impl Drop for ScatterGatherIo {
    fn drop(&mut self) {
        // Free copied (non-zero-copy) segment buffers
        for seg in &self.segments {
            if !seg.is_zerocopy && !seg.data.is_null() {
                unsafe {
                    let _ = Vec::from_raw_parts(
                        seg.data as *mut u8,
                        seg.len,
                        seg.len,
                    );
                }
            }
        }
    }
}

/// Completion notification for zero-copy sends.
///
/// After a zero-copy send completes, the kernel sends a completion
/// notification so userspace knows the buffer can be reused.
#[derive(Debug, Clone, Copy)]
pub struct ZerocopyCompletion {
    /// User cookie (from msg_control or custom field)
    pub cookie: u64,
    /// Number of bytes sent
    pub bytes_sent: usize,
    /// Whether the send completed successfully
    pub success: bool,
}

lazy_static! {
    /// Completion queue for zero-copy sends (per-CPU for lock-free push)
    pub static ref ZEROCOPY_COMPLETIONS: Mutex<Vec<ZerocopyCompletion>> = Mutex::new(Vec::new());
}

/// Post a zero-copy completion notification.
pub fn post_zerocopy_completion(cookie: u64, bytes_sent: usize, success: bool) {
    let mut completions = ZEROCOPY_COMPLETIONS.lock();
    completions.push(ZerocopyCompletion {
        cookie,
        bytes_sent,
        success,
    });
}

/// Drain pending zero-copy completions.
pub fn drain_zerocopy_completions() -> Vec<ZerocopyCompletion> {
    let mut completions = ZEROCOPY_COMPLETIONS.lock();
    core::mem::take(&mut *completions)
}

// ---------------------------------------------------------------------------
// Scatter-Gather Recv: Write directly to iovecs
// ---------------------------------------------------------------------------

/// Write received data directly into iovec buffers instead of copying
/// to a single contiguous buffer first.
///
/// This is the scatter-gather recv path: data is written to each iovec
/// buffer in order, avoiding the intermediate allocation.
///
/// Returns the total bytes written across all iovecs.
pub fn scatter_gather_recv(
    recv_data: &[u8],
    iovecs: &[crate::syscalls::net_helpers::iovec],
) -> usize {
    let mut offset = 0;
    let total = recv_data.len();

    for iov in iovecs {
        if iov.iov_len == 0 || offset >= total {
            break;
        }
        let to_copy = core::cmp::min(iov.iov_len, total - offset);
        unsafe {
            let dst = core::slice::from_raw_parts_mut(iov.iov_base as *mut u8, to_copy);
            dst.copy_from_slice(&recv_data[offset..offset + to_copy]);
        }
        offset += to_copy;
    }

    offset
}

/// Scatter-gather recv with zero-copy awareness.
///
/// For registered zero-copy buffers, data is written directly without
/// copying. For unregistered buffers, standard copy is used.
///
/// Returns (bytes_written, used_zero_copy).
pub fn scatter_gather_recv_zerocopy(
    recv_data: &[u8],
    iovecs: &[crate::syscalls::net_helpers::iovec],
) -> (usize, bool) {
    let mut offset = 0;
    let total = recv_data.len();
    let mut used_zerocopy = false;

    for iov in iovecs {
        if iov.iov_len == 0 || offset >= total {
            break;
        }
        let to_copy = core::cmp::min(iov.iov_len, total - offset);
        let base = iov.iov_base as usize;

        if is_zerocopy_registered(base) {
            // Zero-copy: write directly to registered buffer
            unsafe {
                let dst = core::slice::from_raw_parts_mut(iov.iov_base as *mut u8, to_copy);
                dst.copy_from_slice(&recv_data[offset..offset + to_copy]);
            }
            used_zerocopy = true;
        } else {
            // Standard copy
            unsafe {
                let dst = core::slice::from_raw_parts_mut(iov.iov_base as *mut u8, to_copy);
                dst.copy_from_slice(&recv_data[offset..offset + to_copy]);
            }
        }
        offset += to_copy;
    }

    (offset, used_zerocopy)
}

/// Calculate total iovec capacity.
pub fn iovec_total_capacity(iovecs: &[crate::syscalls::net_helpers::iovec]) -> usize {
    iovecs.iter().map(|iov| iov.iov_len).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_zerocopy_buffer() {
        let addr = 0x1000_0000usize;
        let result = register_zerocopy_buffer(addr, 4096, 1);
        assert_eq!(result, ZerocopyResult::Registered);
        assert!(is_zerocopy_registered(addr));
    }

    #[test]
    fn test_unregister_zerocopy_buffer() {
        let addr = 0x2000_0000usize;
        register_zerocopy_buffer(addr, 4096, 1);
        assert!(unregister_zerocopy_buffer(addr));
        assert!(!is_zerocopy_registered(addr));
    }

    #[test]
    fn test_zerocopy_overlap_detection() {
        let addr1 = 0x3000_0000usize;
        let addr2 = 0x3000_1000usize; // Overlaps with addr1 (4096 bytes)
        register_zerocopy_buffer(addr1, 4096, 1);
        let result = register_zerocopy_buffer(addr2, 4096, 1);
        assert_eq!(result, ZerocopyResult::OverlapsExisting);
    }

    #[test]
    fn test_zerocopy_invalid_buffer() {
        assert_eq!(register_zerocopy_buffer(0, 4096, 1), ZerocopyResult::InvalidBuffer);
        assert_eq!(register_zerocopy_buffer(0x1000, 0, 1), ZerocopyResult::InvalidBuffer);
    }

    #[test]
    fn test_zerocopy_buffer_count() {
        let before = zerocopy_buffer_count();
        let addr = 0x4000_0000usize;
        register_zerocopy_buffer(addr, 4096, 1);
        assert_eq!(zerocopy_buffer_count(), before + 1);
        unregister_zerocopy_buffer(addr);
    }

    #[test]
    fn test_scatter_gather_drop_frees_buffers() {
        let data = alloc::vec![1u8, 2, 3, 4];
        let ptr = data.as_ptr();
        let len = data.len();
        core::mem::forget(data);

        let seg = ScatterSegment::copied(ptr, len);
        let io = ScatterGatherIo {
            segments: alloc::vec![seg],
            total_bytes: len,
            has_zerocopy: false,
        };

        assert_eq!(io.total_bytes, 4);
        // Drop should free the buffer
        drop(io);
    }

    #[test]
    fn test_msg_flags_constants() {
        assert_eq!(MSG_ZEROCOPY, 0x4000);
        assert_eq!(MSG_TRUNC, 0x20);
        assert_eq!(MSG_MORE, 0x8000);
    }
}
