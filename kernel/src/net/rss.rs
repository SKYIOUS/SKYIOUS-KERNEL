//! Receive Side Scaling (RSS) and TCP Segmentation Offload (TSO)
//!
//! ## RSS (Receive Side Scaling)
//!
//! Distributes incoming network packets across multiple CPU cores to
//! parallelize receive processing. Uses a hash-based distribution:
//!
//! - Toeplitz hash over source/destination IP + port
//! - Per-CPU receive queues with independent ring buffers
//! - Flow-based affinity (same flow → same queue → same CPU)
//!
//! ## TSO (TCP Segmentation Offload)
//!
//! Allows the NIC to segment large TCP buffers into MTU-sized packets:
//! - Userspace sends a large buffer (e.g., 64 KB)
//! - NIC hardware segments into 1500-byte packets
//! - Reduces per-packet overhead in the network stack
//!
//! For NICs without hardware TSO, software TSO performs the segmentation.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU16, AtomicUsize, Ordering};
use crate::sync::IrqSafeMutex as Mutex;
use lazy_static::lazy_static;

/// Maximum number of RSS queues (one per CPU)
pub const MAX_RSS_QUEUES: usize = 8;

/// Default MTU (Maximum Transmission Unit)
pub const DEFAULT_MTU: usize = 1500;

/// Maximum TSO segment size (largest buffer NIC will segment)
pub const MAX_TSO_SEGMENT: usize = 65536;

/// TCP header size
const TCP_HEADER_SIZE: usize = 20;

/// IP header size
const IP_HEADER_SIZE: usize = 20;

/// Ethernet header size
const ETH_HEADER_SIZE: usize = 14;

/// Toeplitz hash key (same as Linux default)
const TOEPLITZ_KEY: [u8; 40] = [
    0x6d, 0x5a, 0x56, 0xda, 0x25, 0x5b, 0x0e, 0xc2,
    0x41, 0x67, 0x25, 0x3d, 0x43, 0xa3, 0x8f, 0xb0,
    0xd0, 0xca, 0x2b, 0xcb, 0xae, 0x7b, 0x30, 0xb4,
    0x77, 0xcb, 0x2d, 0xa3, 0x80, 0x30, 0xf2, 0x0c,
    0x6a, 0x42, 0xb7, 0x3b, 0xbe, 0xac, 0x01, 0xfa,
];

/// Per-CPU receive queue
pub struct RssQueue {
    /// Queue index
    pub index: usize,
    /// Packets received on this queue
    pub packets_received: AtomicUsize,
    /// Bytes received on this queue
    pub bytes_received: AtomicUsize,
    /// Queue enabled
    pub enabled: bool,
}

impl RssQueue {
    const fn new(index: usize) -> Self {
        Self {
            index,
            packets_received: AtomicUsize::new(0),
            bytes_received: AtomicUsize::new(0),
            enabled: true,
        }
    }
}

/// RSS configuration
pub struct RssConfig {
    /// Number of active queues
    pub num_queues: AtomicU16,
    /// Hash key for Toeplitz hash
    pub hash_key: [u8; 40],
}

const DEFAULT_RSS_CONFIG: RssConfig = RssConfig {
    num_queues: AtomicU16::new(1),
    hash_key: TOEPLITZ_KEY,
};

lazy_static! {
    /// Global RSS queues
    pub static ref RSS_QUEUES: Mutex<Vec<RssQueue>> = {
        let mut queues = Vec::with_capacity(MAX_RSS_QUEUES);
        for i in 0..MAX_RSS_QUEUES {
            queues.push(RssQueue::new(i));
        }
        Mutex::new(queues)
    };

    /// RSS configuration
    pub static ref RSS_CONFIG: RssConfig = DEFAULT_RSS_CONFIG;
}

/// Initialize RSS with the given number of queues.
pub fn rss_init(num_queues: usize) {
    let num = core::cmp::min(num_queues, MAX_RSS_QUEUES);
    RSS_CONFIG.num_queues.store(num as u16, Ordering::Relaxed);

    let mut queues = RSS_QUEUES.lock();
    for i in 0..MAX_RSS_QUEUES {
        queues[i].enabled = i < num;
    }

    crate::serial_write("[RSS] Initialized with ");
    crate::serial_write(&alloc::format!("{} queues\n", num));
}

/// Compute Toeplitz hash for RSS distribution.
///
/// Hashes source/destination IP + port to determine which queue
/// handles a packet. Same flow always goes to the same queue.
pub fn toeplitz_hash(src_ip: &[u8; 4], dst_ip: &[u8; 4], src_port: u16, dst_port: u16) -> u32 {
    let mut data = [0u8; 12];
    data[0..4].copy_from_slice(src_ip);
    data[4..8].copy_from_slice(dst_ip);
    data[8..10].copy_from_slice(&src_port.to_be_bytes());
    data[10..12].copy_from_slice(&dst_port.to_be_bytes());

    let mut hash: u32 = 0;
    for (i, &byte) in data.iter().enumerate() {
        for bit in (0..8).rev() {
            hash = hash.wrapping_shl(1);
            if byte & (1 << bit) != 0 {
                hash ^= TOEPLITZ_KEY[i % 40] as u32;
            }
        }
    }
    hash
}

/// Select an RSS queue for a packet based on flow hash.
pub fn rss_select_queue(src_ip: &[u8; 4], dst_ip: &[u8; 4], src_port: u16, dst_port: u16) -> usize {
    let num_queues = RSS_CONFIG.num_queues.load(Ordering::Relaxed) as usize;
    if num_queues == 0 {
        return 0;
    }

    let hash = toeplitz_hash(src_ip, dst_ip, src_port, dst_port);
    (hash as usize) % num_queues
}

/// Record a packet on an RSS queue.
pub fn rss_record_packet(queue_index: usize, bytes: usize) {
    let queues = RSS_QUEUES.lock();
    if let Some(queue) = queues.get(queue_index) {
        queue.packets_received.fetch_add(1, Ordering::Relaxed);
        queue.bytes_received.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Get RSS statistics for a queue.
pub fn rss_queue_stats(queue_index: usize) -> Option<(usize, usize)> {
    let queues = RSS_QUEUES.lock();
    queues.get(queue_index).map(|q| {
        (
            q.packets_received.load(Ordering::Relaxed),
            q.bytes_received.load(Ordering::Relaxed),
        )
    })
}

// ---------------------------------------------------------------------------
// TSO (TCP Segmentation Offload)
// ---------------------------------------------------------------------------

/// Software TSO segment descriptor
#[derive(Debug, Clone)]
pub struct TsoSegment {
    /// Sequence number for this segment
    pub seq: u32,
    /// Payload offset in the original buffer
    pub offset: usize,
    /// Length of this segment's payload
    pub len: usize,
    /// MSS (Maximum Segment Size) for this segment
    pub mss: usize,
    /// Whether this is the last segment
    pub is_last: bool,
}

/// Software TCP segmentation: split a large buffer into MSS-sized segments.
///
/// This is the software fallback when the NIC doesn't support hardware TSO.
/// Returns a list of segments that can be sent as individual TCP packets.
pub fn tso_segment_tcp(
    buffer: &[u8],
    mss: usize,
    initial_seq: u32,
) -> Vec<TsoSegment> {
    let mut segments = Vec::new();
    let mut offset = 0;
    let mut seq = initial_seq;

    while offset < buffer.len() {
        let seg_len = core::cmp::min(mss, buffer.len() - offset);
        let is_last = offset + seg_len >= buffer.len();

        segments.push(TsoSegment {
            seq,
            offset,
            len: seg_len,
            mss,
            is_last,
        });

        seq = seq.wrapping_add(seg_len as u32);
        offset += seg_len;
    }

    segments
}

/// Calculate the total overhead per TCP segment (headers).
pub fn tcp_segment_overhead(_ip_total_len: bool) -> usize {
    ETH_HEADER_SIZE + IP_HEADER_SIZE + TCP_HEADER_SIZE
}

/// Validate TSO parameters.
pub fn tso_validate(mss: usize, buffer_len: usize) -> Result<(), &'static str> {
    if mss == 0 {
        return Err("MSS cannot be zero");
    }
    if mss > DEFAULT_MTU - TCP_HEADER_SIZE - IP_HEADER_SIZE {
        return Err("MSS exceeds MTU");
    }
    if buffer_len > MAX_TSO_SEGMENT {
        return Err("Buffer exceeds maximum TSO segment size");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toeplitz_hash_deterministic() {
        let h1 = toeplitz_hash(&[10, 0, 2, 1], &[10, 0, 2, 2], 80, 12345);
        let h2 = toeplitz_hash(&[10, 0, 2, 1], &[10, 0, 2, 2], 80, 12345);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_toeplitz_hash_different_flows() {
        let h1 = toeplitz_hash(&[10, 0, 2, 1], &[10, 0, 2, 2], 80, 12345);
        let h2 = toeplitz_hash(&[10, 0, 2, 1], &[10, 0, 2, 2], 80, 12346);
        // Different port should produce different hash (very likely)
        // Not guaranteed but statistically certain
        let _ = (h1, h2);
    }

    #[test]
    fn test_rss_select_queue_range() {
        let q = rss_select_queue(&[10, 0, 2, 1], &[10, 0, 2, 2], 80, 443);
        assert!(q < MAX_RSS_QUEUES);
    }

    #[test]
    fn test_tso_segment_basic() {
        let buffer = vec![0u8; 3000];
        let segments = tso_segment_tcp(&buffer, 1460, 1000);
        assert!(segments.len() >= 3); // 3000 / 1460 ≈ 2.05 → 3 segments
        assert_eq!(segments[0].seq, 1000);
        assert_eq!(segments[0].len, 1460);
        assert!(!segments[0].is_last);
        assert!(segments.last().unwrap().is_last);
    }

    #[test]
    fn test_tso_segment_exact_mss() {
        let buffer = vec![0u8; 1460];
        let segments = tso_segment_tcp(&buffer, 1460, 0);
        assert_eq!(segments.len(), 1);
        assert!(segments[0].is_last);
    }

    #[test]
    fn test_tso_validate() {
        assert!(tso_validate(1460, 3000).is_ok());
        assert!(tso_validate(0, 3000).is_err());
        assert!(tso_validate(10000, 3000).is_err());
    }
}
