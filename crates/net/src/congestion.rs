//! TCP Reno congestion control.
//!
//! Implements slow start, congestion avoidance, fast retransmit,
//! and fast recovery per RFC 5681.
//!
//! ## Algorithm
//!
//! ```text
//! cwnd < ssthresh  → slow start   (cwnd *= 2 per ACK)
//! cwnd >= ssthresh → congestion avoidance (cwnd += 1/cwnd per ACK)
//! 3 dup ACKs       → fast retransmit + fast recovery (halve cwnd)
//! RTO              → slow start reset (cwnd = 1 MSS)
//! ```
//!
//! ## Invariants
//!
//! - cwnd >= 1 MSS at all times
//! - ssthresh is updated on loss, never increases
//! - FlightSize (bytes in flight) <= cwnd

/// MSS (Maximum Segment Size) in bytes.
pub const MSS: u32 = 1460;

/// Initial congestion window.
pub const INITIAL_CWND: u32 = 10 * MSS;

/// Initial slow-start threshold.
pub const INITIAL_SSTHRESH: u32 = u32::MAX;

/// TCP Reno congestion control state.
pub struct TcpReno {
    pub cwnd: u32,
    pub ssthresh: u32,
    pub recovery: bool,
    pub dup_acks: u32,
}

impl TcpReno {
    /// Create a new connection with initial values.
    pub const fn new() -> Self {
        Self {
            cwnd: INITIAL_CWND,
            ssthresh: INITIAL_SSTHRESH,
            recovery: false,
            dup_acks: 0,
        }
    }

    /// Called for each ACK received.
    pub fn on_ack(&mut self, bytes_acked: u32) {
        if self.cwnd < self.ssthresh {
            // Slow start
            self.cwnd += bytes_acked;
        } else {
            // Congestion avoidance
            self.cwnd += (bytes_acked * bytes_acked) / self.cwnd;
        }
    }

    /// Called when 3 duplicate ACKs are received.
    pub fn on_triple_dup_ack(&mut self) {
        self.ssthresh = core::cmp::max(self.cwnd / 2, 2 * MSS);
        self.cwnd = self.ssthresh + 3 * MSS;
        self.recovery = true;
        self.dup_acks = 0;
    }

    /// Called on retransmission timeout.
    pub fn on_rto(&mut self) {
        self.ssthresh = core::cmp::max(self.cwnd / 2, 2 * MSS);
        self.cwnd = MSS;
        self.recovery = false;
        self.dup_acks = 0;
    }

    /// Called when new data is ACKed exiting recovery.
    pub fn on_exit_recovery(&mut self) {
        self.cwnd = self.ssthresh;
        self.recovery = false;
    }
}
