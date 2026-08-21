#![allow(dead_code)]

pub mod verifier;
pub mod runtime;
pub mod manager;
pub mod syscalls;
pub mod hooks;

use alloc::vec::Vec;

/// Hook point identifiers — where an ASH handler is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookPoint {
    NetReceive { interface: u8, port: u16, protocol: Protocol },
    NetTransmit { interface: u8, port: u16, protocol: Protocol },
    SyscallEntry { syscall_num: u64 },
    SyscallExit { syscall_num: u64 },
    TimerFired { timer_id: u64 },
    SignalDelivery { signal: u32 },
    MessageReceive { channel: u64 },
}

/// Network protocols filterable by ASH handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
    Icmp,
    Raw,
}

impl Protocol {
    pub fn to_u8(self) -> u8 {
        match self {
            Protocol::Tcp => 6,
            Protocol::Udp => 17,
            Protocol::Icmp => 1,
            Protocol::Raw => 0,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            6 => Some(Protocol::Tcp),
            17 => Some(Protocol::Udp),
            1 => Some(Protocol::Icmp),
            0 => Some(Protocol::Raw),
            _ => None,
        }
    }
}

/// Result returned by an ASH handler after execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AshResult {
    Continue,
    Handled,
    Drop,
    Modified,
    Error(AshError),
}

/// Runtime error codes for ASH execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AshError {
    MemoryViolation,
    CycleLimitExceeded,
    InvalidInstruction,
    VerifierRejected,
    NotFound,
    Unknown,
}

/// A registered (unverified) ASH handler as submitted by userspace.
#[derive(Debug, Clone)]
pub struct AshHandler {
    pub id: u64,
    pub pid: u64,
    pub bytecode: Vec<u8>,
    pub hook_point: HookPoint,
    pub context_mask: u32,
    pub max_insns: u32,
    pub expiry: Option<u64>,
}

/// A verified ASH handler ready for execution.
#[derive(Debug, Clone)]
pub struct VerifiedAsh {
    pub bytecode: Vec<u8>,
    pub insns: Vec<crate::ebpf::vm::EbpfInsn>,
    pub jited: Vec<u8>,
    pub max_cycles: u32,
    pub context_size: usize,
    pub memory_budget: usize,
}

/// Statistics for ASH usage accounting.
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct AshStats {
    pub total_insns: u64,
    pub total_events: u64,
    pub total_dropped: u64,
    pub total_handled: u64,
    pub total_modified: u64,
    pub total_errors: u64,
}
