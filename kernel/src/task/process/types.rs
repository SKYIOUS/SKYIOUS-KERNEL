//! Self-contained type definitions extracted from process.rs.
//!
//! These types have no dependency on `Process` and can be imported
//! independently by any module that needs them.

// ─── sigaltstack / itimerval / tms types ─────────────────────────

pub const SS_DISABLE: i32 = 2;
pub const SS_ONSTACK: i32 = 1;
pub const SIGSTKSZ: usize = 8192;
pub const MINSIGSTKSZ: usize = 2048;

#[repr(C)]
pub struct stack_t {
    pub ss_sp: *mut u8,
    pub ss_flags: i32,
    pub ss_size: usize,
}

// ponytail: stack_t holds a raw pointer used only for signal altstack storage,
// never dereferenced from another thread. Marking Send is safe here.
unsafe impl Send for stack_t {}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
pub struct itimerval {
    pub it_interval: timeval,
    pub it_value: timeval,
}

#[repr(C)]
pub struct tms {
    pub tms_utime: i64,
    pub tms_stime: i64,
    pub tms_cutime: i64,
    pub tms_cstime: i64,
}

// ─── signalfd types ──────────────────────────────────────────────

pub struct SignalFdData {
    pub mask: u64,
    pub pending: alloc::collections::VecDeque<SignalFdInfo>,
}

/// Linux-compatible signalfd_siginfo (128 bytes).
/// Matches the kernel's struct signalfd_siginfo layout exactly.
#[repr(C)]
pub struct SignalFdInfo {
    pub ssi_signo: u32,
    pub ssi_errno: i32,
    pub ssi_code: i32,
    pub ssi_pid: u32,
    pub ssi_uid: u32,
    pub ssi_fd: i32,
    pub ssi_tid: u32,
    pub ssi_band: u32,
    pub ssi_overrun: u32,
    pub ssi_sigval: u64,
    pub ssi_status: i32,
    pub ssi_int: i32,
    pub ssi_ptr: u64,
    pub ssi_utime: u64,
    pub ssi_stime: u64,
    pub ssi_addr: u64,
    pub ssi_addr_lsb: u16,
    pub _pad1: u16,
    pub ssi_sys_private: u32,
    pub ssi_call_addr: u64,
    pub ssi_sys_call: u32,
    pub ssi_arch: u32,
    pub ssi_pad: [u8; 24],
}

// Default intentionally absent: construction is via new(); nothing calls Default.
#[allow(clippy::new_without_default)]
impl SignalFdInfo {
    pub fn new() -> Self {
        Self {
            ssi_signo: 0,
            ssi_errno: 0,
            ssi_code: 0,
            ssi_pid: 0,
            ssi_uid: 0,
            ssi_fd: 0,
            ssi_tid: 0,
            ssi_band: 0,
            ssi_overrun: 0,
            ssi_sigval: 0,
            ssi_status: 0,
            ssi_int: 0,
            ssi_ptr: 0,
            ssi_utime: 0,
            ssi_stime: 0,
            ssi_addr: 0,
            ssi_addr_lsb: 0,
            _pad1: 0,
            ssi_sys_private: 0,
            ssi_call_addr: 0,
            ssi_sys_call: 0,
            ssi_arch: 0,
            ssi_pad: [0u8; 24],
        }
    }
}

// ─── eventfd types ──────────────────────────────────────────────

pub const EFD_SEMAPHORE: i32 = 1;
pub const EFD_NONBLOCK: i32 = 0x800;
pub const EFD_CLOEXEC: i32 = 0x40000;
pub const EFD_MAX: u64 = 0xFFFF_FFFF_FFFF_FFFE;

pub struct EventFdData {
    pub counter: u64,
    pub semaphore: bool,
    pub nonblock: bool,
    /// Unique key for blocking/wake via the scheduler's block_queue.
    pub key: u64,
}

/// TimerFd state: a file descriptor that becomes readable when a timer fires.
pub struct TimerFdData {
    /// Clock ID: 0 = CLOCK_REALTIME, 1 = CLOCK_MONOTONIC, 4 = CLOCK_BOOTTIME
    pub clock_id: u32,
    pub nonblock: bool,
    /// Initial interval (nanoseconds). 0 = one-shot.
    pub it_interval_ns: u64,
    /// Current expiration time relative to clock (nanoseconds).
    pub it_value_ns: u64,
    /// Number of times the timer has fired since last read.
    pub expirations: u64,
    /// Absolute tick at which the timer next fires.
    pub wake_tick: u64,
    pub armed: bool,
    /// Unique key for blocking/wake via the scheduler's block_queue.
    pub key: u64,
}

// ─── POSIX signal info constants ────────────────────────────────

pub const SI_USER: i32 = 0;
pub const SI_KERNEL: i32 = 0x80;
pub const SI_TIMER: i32 = -2;
pub const SI_CHILD: i32 = -11;
pub const SI_ASYNCIO: i32 = -4;
pub const SI_SIGIO: i32 = -5;
pub const SI_TKILL: i32 = -6;
pub const SI_DETHREAD: i32 = -7;
pub const SI_ASYNCNL: i32 = -60;
pub const SI_MESGQ: i32 = -13;

// Note: Credentials is defined in process.rs because it has extra fields
// (cap_effective, cap_permitted, cap_inheritable, umask) and is tightly
// coupled with the Process impl blocks.
