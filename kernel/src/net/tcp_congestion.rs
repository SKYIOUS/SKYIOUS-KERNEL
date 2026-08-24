//! TCP Reno congestion control for the Vahi kernel.
//!
//! Sits between the application send path and smoltcp's TCP socket.
//! Tracks per-connection congestion state (cwnd, ssthresh, RTT) and
//! rate-limits how much data is fed into smoltcp's send buffer.
//!
//! smoltcp handles retransmission and the state machine; this module
//! handles the congestion window and RTT estimation.

use alloc::collections::BTreeMap;
use crate::sync::IrqSafeMutex as Mutex;

/// Default maximum segment size (bytes).
const DEFAULT_MSS: u32 = 1460;

/// Initial congestion window (segments).
const INITIAL_CWND_SEGMENTS: u32 = 10;

/// Maximum congestion window (segments, ~16 MB at 1460 MSS).
const MAX_CWND_BYTES: u32 = 10_000 * DEFAULT_MSS;

/// Duplicate ACK threshold for fast retransmit trigger.
const DUP_ACK_THRESHOLD: u32 = 3;

/// RTT smoothing factor (1/8 = 0.125, per Jacobson/Karels).
const ALPHA: u32 = 8;
/// RTT variance smoothing factor (1/4 = 0.25).
const BETA: u32 = 4;
/// Initial RTT estimate (ms) — conservative default.
const INITIAL_RTO_MS: u64 = 1000;

/// Congestion control state for one TCP connection.
pub struct TcpCongestionState {
    /// Congestion window in bytes.
    pub cwnd: u32,
    /// Slow-start threshold in bytes.
    pub ssthresh: u32,
    /// MSS in bytes.
    pub mss: u32,

    // ── RTT estimation (Jacobson/Karels) ──
    /// Smoothed RTT in microseconds.
    pub srtt_us: u64,
    /// RTT variance in microseconds.
    pub rttvar_us: u64,
    /// Retransmission timeout in milliseconds.
    pub rto_ms: u64,
    /// Whether we're waiting for an ACK to sample RTT.
    pub rtt_sample_pending: bool,

    // ── Loss detection ──
    /// Number of duplicate ACKs received since last new ACK.
    pub dup_ack_count: u32,
    /// Bytes in flight (estimated).
    pub bytes_in_flight: u32,
    /// Whether we're in fast recovery (NewReno).
    pub in_recovery: bool,
    /// Retransmission timeout counter.
    pub rto_count: u32,
}

impl TcpCongestionState {
    pub fn new(mss: u32) -> Self {
        TcpCongestionState {
            cwnd: mss * INITIAL_CWND_SEGMENTS,
            ssthresh: u32::MAX,
            mss,
            srtt_us: 0,
            rttvar_us: 0,
            rto_ms: INITIAL_RTO_MS,
            rtt_sample_pending: false,
            dup_ack_count: 0,
            bytes_in_flight: 0,
            in_recovery: false,
            rto_count: 0,
        }
    }

    /// Max bytes that can be sent without violating cwnd.
    pub fn send_budget(&self) -> u32 {
        self.cwnd.saturating_sub(self.bytes_in_flight)
    }
}

// ── Global connection table ──────────────────────────────────────────

lazy_static::lazy_static! {
    static ref CONN_STATES: Mutex<BTreeMap<usize, TcpCongestionState>> =
        Mutex::new(BTreeMap::new());
}

/// Max bytes the application can send on this connection.
/// Unknown connections get initial cwnd (conservative default).
pub fn send_budget(handle_id: usize) -> u32 {
    CONN_STATES
        .lock()
        .get(&handle_id)
        .map_or(DEFAULT_MSS * INITIAL_CWND_SEGMENTS, |s| s.send_budget())
}

/// Record that bytes were sent (enqueued into smoltcp).
pub fn on_send(handle_id: usize, bytes: u32) {
    if let Some(s) = CONN_STATES.lock().get_mut(&handle_id) {
        s.bytes_in_flight = s.bytes_in_flight.saturating_add(bytes);
        s.rtt_sample_pending = true;
    }
}

/// Record that an ACK acknowledged new data. Updates cwnd (slow start / CA).
pub fn on_ack(handle_id: usize, bytes_acked: u32, rtt_sample_ms: Option<u64>) {
    if let Some(s) = CONN_STATES.lock().get_mut(&handle_id) {
        // RTT estimate.
        if let Some(rtt_ms) = rtt_sample_ms {
            update_rtt(s, rtt_ms);
            s.rtt_sample_pending = false;
        }

        s.bytes_in_flight = s.bytes_in_flight.saturating_sub(bytes_acked);
        s.dup_ack_count = 0;

        if s.in_recovery {
            // Fast recovery (NewReno): inflate cwnd by 1 MSS per ACK.
            // Exit when cumulative ACK advances past recovery point
            // (detected by bytes_in_flight dropping to ssthresh level).
            if s.bytes_in_flight <= s.ssthresh {
                s.in_recovery = false;
                s.cwnd = s.ssthresh;
            } else {
                s.cwnd = s.cwnd.saturating_add(s.mss);
            }
            return;
        }

        if s.cwnd < s.ssthresh {
            // Slow start: cwnd += 1 MSS per ACK.
            s.cwnd = (s.cwnd + s.mss).min(MAX_CWND_BYTES);
        } else {
            // Congestion avoidance: cwnd += MSS²/cwnd per ACK.
            let inc = (s.mss * s.mss) / s.cwnd;
            s.cwnd = (s.cwnd + inc.max(1)).min(MAX_CWND_BYTES);
        }
    }
}

/// Record a duplicate ACK. Returns true if fast retransmit should trigger.
pub fn on_dup_ack(handle_id: usize) -> bool {
    let mut map = CONN_STATES.lock();
    if let Some(s) = map.get_mut(&handle_id) {
        s.dup_ack_count += 1;
        s.dup_ack_count >= DUP_ACK_THRESHOLD
    } else {
        false
    }
}

/// Enter fast retransmit / fast recovery (Reno).
pub fn enter_fast_recovery(handle_id: usize) {
    if let Some(s) = CONN_STATES.lock().get_mut(&handle_id) {
        s.ssthresh = (s.cwnd / 2).max(s.mss * 2);
        s.cwnd = s.ssthresh + 3 * s.mss;
        s.in_recovery = true;
        s.dup_ack_count = 0;
    }
}

/// Handle retransmission timeout. Returns the new RTO.
pub fn on_timeout(handle_id: usize) -> u64 {
    CONN_STATES
        .lock()
        .get_mut(&handle_id)
        .map_or(INITIAL_RTO_MS, |s| {
            s.ssthresh = (s.cwnd / 2).max(s.mss * 2);
            s.cwnd = s.mss;
            s.dup_ack_count = 0;
            s.in_recovery = false;
            s.rto_count += 1;
            if s.rto_count > 1 {
                // Exponential backoff with overflow protection.
                s.rto_ms = s.rto_ms.saturating_mul(2).min(60_000);
            }
            s.rto_ms
        })
}

/// Create congestion state for a new connection.
pub fn create(handle_id: usize, mss: u32) {
    CONN_STATES.lock().insert(handle_id, TcpCongestionState::new(mss));
}

/// Remove congestion state when a connection closes.
pub fn remove(handle_id: usize) {
    CONN_STATES.lock().remove(&handle_id);
}

/// Jacobson/Karels RTT update.
fn update_rtt(s: &mut TcpCongestionState, rtt_ms: u64) {
    // Clamp RTT to 30s to prevent overflow in us conversion.
    let rtt_ms = rtt_ms.min(30_000);
    let rtt_us = rtt_ms.saturating_mul(1000);
    if s.srtt_us == 0 {
        // First measurement — seed the estimator.
        s.srtt_us = rtt_us;
        s.rttvar_us = rtt_us / 2;
    } else {
        let diff = if rtt_us > s.srtt_us {
            rtt_us - s.srtt_us
        } else {
            s.srtt_us - rtt_us
        };
        // RTTVAR = (3/4) * RTTVAR + (1/4) * |diff|
        s.rttvar_us = (((BETA - 1) as u64) * s.rttvar_us + diff) / (BETA as u64);
        // SRTT = (7/8) * SRTT + (1/8) * R
        s.srtt_us = (((ALPHA - 1) as u64) * s.srtt_us + rtt_us) / (ALPHA as u64);
    }
    // RTO = SRTT + 4 * RTTVAR, clamped to [200ms, 60s].
    let rto_us = s.srtt_us.saturating_add(4 * s.rttvar_us);
    s.rto_ms = (rto_us / 1000).max(200).min(60_000);
    s.rto_count = 0;
}


