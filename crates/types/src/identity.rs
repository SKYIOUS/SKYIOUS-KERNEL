//! Identity vocabulary for the vahi kernel crate boundary.

// ─── Process Identity Types ─────────────────────────────────────────
// Used by both `task` (process ownership) and `vfs`/`net`/`interrupts`
// (accessing process context without depending on `task` directly).

/// Unique process identifier. PID 1 is always init.
pub type Pid = u64;

/// Unique thread identifier (Linux tid).
pub type Tid = u64;

/// Unique file descriptor index.
pub type FdNum = i32;

/// Signal number (Linux standard, e.g., SIGKILL=9).
pub type SignalNum = i32;

/// Virtual address in the kernel address space.
pub type VirtAddr = u64;

/// Physical address in the machine address space.
pub type PhysAddr = u64;

/// Memory page number (page index from physical address 0).
pub type PageFrame = u64;

/// Kernel clock ticks.
pub type TickCount = u64;

// ─── Socket Type ────────────────────────────────────────────────────
// Shared between net (SocketObject) and the kernel's object namespace.

/// Protocol family of a socket object.
#[derive(Clone, Copy, PartialEq)]
pub enum SocketType {
    Tcp,
    Udp,
    Raw,
    Unix,
}

// ─── Credentials ────────────────────────────────────────────────────
// Shared between task (process identity) and syscalls (cred operations).

/// Unix-style credentials for a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
}

impl Credentials {
    /// Root credentials (uid=0, gid=0).
    pub const ROOT: Self = Self {
        uid: 0,
        gid: 0,
        euid: 0,
        egid: 0,
    };

    /// Returns `true` if these are root credentials.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.uid == 0 && self.gid == 0
    }
}

impl Default for Credentials {
    fn default() -> Self {
        Self::ROOT
    }
}
