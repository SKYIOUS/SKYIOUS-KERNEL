//! # vahi-task — Shared task vocabulary
//!
//! The kernel owns process/thread/scheduler implementations. This crate
//! keeps only what must be shared with dependency-free crates (the PTY
//! pipe pair) plus the async task types the kernel's executor runs.

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

pub mod pty;

use core::sync::atomic::{AtomicU64, Ordering};

// Re-export shared types from vahi-types for convenience
pub use vahi_types::{
    Credentials, FdNum, PhysAddr, Pid, SignalNum, Tid, VirtAddr, VmFlags, VmProt, Vma,
};

/// Stub for kernel-local serial output. The real implementation lives in
/// `kernel/src/main.rs`; task uses it only for debug prints.
#[allow(dead_code)]
pub fn serial_write(_msg: &str) {}

// ─── Task / Future Types ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskId(pub u64);

impl TaskId {
    pub fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        TaskId(id)
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Task {
    pub id: TaskId,
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + 'static) -> Task {
        Task {
            id: TaskId::new(),
            future: Box::pin(future),
        }
    }

    pub fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}

pub struct YieldNow(bool);
impl YieldNow {
    pub fn new() -> Self {
        YieldNow(false)
    }
}

impl Default for YieldNow {
    fn default() -> Self {
        Self::new()
    }
}
impl Future for YieldNow {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

// ─── Process States ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ProcessState {
    Running = 0,
    Sleeping = 1,
    Stopped = 2,
    Zombie = 3,
    Traced = 4,
    DiskSleep = 5,
}

// ─── Clone Flags (Linux-compatible) ─────────────────────────────────

bitflags::bitflags! {
    pub struct CloneFlags: u64 {
        const VM             = 0x0000_0100;
        const FS             = 0x0000_0200;
        const FILES          = 0x0000_0400;
        const SIGHAND        = 0x0000_0800;
        const PTRACE         = 0x0000_2000;
        const VFORK          = 0x0000_4000;
        const PARENT         = 0x0000_8000;
        const THREAD         = 0x0001_0000;
        const NEWNS          = 0x0002_0000;
        const SYSVSEM        = 0x0004_0000;
        const SETTLS         = 0x0008_0000;
        const PARENT_SETTID  = 0x0010_0000;
        const CHILD_SETTID   = 0x0020_0000;
        const CHILD_CLEARTID = 0x0040_0000;
        const VFILE          = 0x0080_0000;
    }
}

// ─── Signal Types ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGTRAP = 5,
    SIGABRT = 6,
    SIGBUS = 7,
    SIGFPE = 8,
    SIGKILL = 9,
    SIGUSR1 = 10,
    SIGSEGV = 11,
    SIGUSR2 = 12,
    SIGPIPE = 13,
    SIGALRM = 14,
    SIGTERM = 15,
    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTSTP = 20,
    SIGTTIN = 21,
    SIGTTOU = 22,
    SIGURG = 23,
    SIGXCPU = 24,
    SIGXFSZ = 25,
    SIGVTALRM = 26,
    SIGPROF = 27,
    SIGWINCH = 28,
    SIGIO = 29,
    SIGPWR = 30,
    SIGSYS = 31,
}

impl Signal {
    pub fn from_num(num: u8) -> Option<Self> {
        match num {
            1 => Some(Self::SIGHUP),
            2 => Some(Self::SIGINT),
            3 => Some(Self::SIGQUIT),
            4 => Some(Self::SIGILL),
            5 => Some(Self::SIGTRAP),
            6 => Some(Self::SIGABRT),
            7 => Some(Self::SIGBUS),
            8 => Some(Self::SIGFPE),
            9 => Some(Self::SIGKILL),
            10 => Some(Self::SIGUSR1),
            11 => Some(Self::SIGSEGV),
            12 => Some(Self::SIGUSR2),
            13 => Some(Self::SIGPIPE),
            14 => Some(Self::SIGALRM),
            15 => Some(Self::SIGTERM),
            17 => Some(Self::SIGCHLD),
            18 => Some(Self::SIGCONT),
            19 => Some(Self::SIGSTOP),
            20 => Some(Self::SIGTSTP),
            21 => Some(Self::SIGTTIN),
            22 => Some(Self::SIGTTOU),
            23 => Some(Self::SIGURG),
            24 => Some(Self::SIGXCPU),
            25 => Some(Self::SIGXFSZ),
            26 => Some(Self::SIGVTALRM),
            27 => Some(Self::SIGPROF),
            28 => Some(Self::SIGWINCH),
            29 => Some(Self::SIGIO),
            30 => Some(Self::SIGPWR),
            31 => Some(Self::SIGSYS),
            _ => None,
        }
    }

    pub const fn as_num(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::SIGHUP => "SIGHUP",
            Self::SIGINT => "SIGINT",
            Self::SIGQUIT => "SIGQUIT",
            Self::SIGILL => "SIGILL",
            Self::SIGTRAP => "SIGTRAP",
            Self::SIGABRT => "SIGABRT",
            Self::SIGBUS => "SIGBUS",
            Self::SIGFPE => "SIGFPE",
            Self::SIGKILL => "SIGKILL",
            Self::SIGUSR1 => "SIGUSR1",
            Self::SIGSEGV => "SIGSEGV",
            Self::SIGUSR2 => "SIGUSR2",
            Self::SIGPIPE => "SIGPIPE",
            Self::SIGALRM => "SIGALRM",
            Self::SIGTERM => "SIGTERM",
            Self::SIGCHLD => "SIGCHLD",
            Self::SIGCONT => "SIGCONT",
            Self::SIGSTOP => "SIGSTOP",
            Self::SIGTSTP => "SIGTSTP",
            Self::SIGTTIN => "SIGTTIN",
            Self::SIGTTOU => "SIGTTOU",
            Self::SIGURG => "SIGURG",
            Self::SIGXCPU => "SIGXCPU",
            Self::SIGXFSZ => "SIGXFSZ",
            Self::SIGVTALRM => "SIGVTALRM",
            Self::SIGPROF => "SIGPROF",
            Self::SIGWINCH => "SIGWINCH",
            Self::SIGIO => "SIGIO",
            Self::SIGPWR => "SIGPWR",
            Self::SIGSYS => "SIGSYS",
        }
    }

    pub const fn default_action(self) -> SignalAction {
        match self {
            Self::SIGKILL | Self::SIGSTOP => SignalAction::Terminate,
            Self::SIGCONT => SignalAction::Continue,
            Self::SIGCHLD | Self::SIGURG | Self::SIGWINCH => SignalAction::Ignore,
            _ => SignalAction::Terminate,
        }
    }

    pub const fn is_uncatchable(self) -> bool {
        matches!(self, Self::SIGKILL | Self::SIGSTOP)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    Terminate,
    Core,
    Stop,
    Continue,
    Ignore,
    Handler,
}

// ─── Scheduler Policy ───────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedPolicy {
    Normal = 0,
    Batch = 1,
    Idle = 2,
}

#[derive(Debug, Clone, Copy)]
pub struct Priority {
    pub policy: SchedPolicy,
    pub nice: i8,
    pub rt_priority: u8,
}

impl Default for Priority {
    fn default() -> Self {
        Self {
            policy: SchedPolicy::Normal,
            nice: 0,
            rt_priority: 0,
        }
    }
}

impl Priority {
    pub const fn nice(nice: i8) -> Self {
        Self {
            policy: SchedPolicy::Normal,
            nice,
            rt_priority: 0,
        }
    }

    pub const fn rt(priority: u8) -> Self {
        let p = if priority > 99 { 99 } else { priority };
        Self {
            policy: SchedPolicy::Normal,
            nice: -20,
            rt_priority: p,
        }
    }

    pub const fn is_realtime(&self) -> bool {
        self.rt_priority > 0
    }
}

// ─── Tick counter ───────────────────────────────────────────────────
// Single shared clock: the kernel registers its tick source in vahi-types
// (register_tick_fn) during boot; read through vahi_types::get_ticks so
// every consumer agrees on the one clock.

/// Monotonic 100 Hz tick count (delegates to the registered kernel clock).
pub fn get_ticks() -> u64 {
    vahi_types::get_ticks()
}
