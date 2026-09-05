//! Provider vocabulary for the vahi kernel crate boundary.

use crate::identity::{Credentials, Pid, TickCount};
use crate::vm::{VirtAddr, Vma};

use alloc::boxed::Box;

// ─── Process Provider Trait ─────────────────────────────────────────
// Breaks the memory ↔ task cycle: memory calls through this trait
// instead of directly accessing `task::process::CURRENT_PROCESS`.

/// Trait for accessing process context without depending on the task crate.
///
/// Implemented by the kernel's task module and registered during boot.
pub trait ProcessProvider: Send + Sync {
    /// Get the PID of the currently running process.
    fn current_pid(&self) -> Pid;

    /// Check if a process exists with the given PID.
    fn process_exists(&self, pid: Pid) -> bool;

    /// Check if the current process is the init process (PID 1).
    fn is_init(&self) -> bool;

    /// Get the credentials of the current process.
    fn current_credentials(&self) -> Credentials;

    /// Get the virtual memory areas of a process.
    fn vmas(&self, pid: Pid) -> Option<&[Vma]>;

    /// Check if an address is a valid user-space address (< 0x800000000000).
    fn is_user_address(&self, addr: VirtAddr) -> bool;

    /// Credentials of the process with the given PID (for SO_PEERCRED).
    ///
    /// The full kernel `Credentials` (capabilities, saved ids) is not part of
    /// the shared vocabulary; only the unix peer-cred triple is needed.
    fn peer_creds(&self, pid: Pid) -> Option<(u32, u32, u32)>;
}

// ─── Page Fault Handler Trait ───────────────────────────────────────
// Breaks the interrupts ↔ memory cycle: interrupts dispatches page
// faults through this trait instead of calling memory::paging directly.

/// Trait for handling page faults without depending on the memory crate.
///
/// Implemented by the kernel's memory/paging module and registered during boot.
pub trait PageFaultHandler: Send + Sync {
    /// Handle a page fault at the given virtual address.
    ///
    /// Returns `true` if the fault was handled (page fault on valid VMA),
    /// `false` if the fault is an error (segfault).
    fn handle(&self, fault_addr: VirtAddr, error_code: u64) -> bool;

    /// Check if CoW is needed for a page at the given address.
    fn needs_cow(&self, addr: VirtAddr) -> bool;
}

// ─── File Descriptor Trait ──────────────────────────────────────────
// Breaks the task ↔ vfs cycle: task owns FD table but accesses
// filesystem operations through this trait.

/// Operations that can be performed on an open file descriptor.
pub trait FileOps: Send + Sync {
    /// Read data from the file at the given offset.
    fn read(&self, buf: &mut [u8], offset: u64) -> Result<usize, i32>;

    /// Write data to the file at the given offset.
    fn write(&self, buf: &[u8], offset: u64) -> Result<usize, i32>;

    /// Seek to a position in the file.
    fn seek(&self, offset: u64, whence: u32) -> Result<u64, i32>;

    /// Close the file and release resources.
    fn close(&self) -> Result<(), i32>;

    /// Get file metadata (size, mode, etc.).
    fn stat(&self) -> Result<FileStat, i32>;

    /// Memory-map the file (for mmap support).
    fn mmap(&self, offset: u64, len: usize, prot: u32, flags: u32) -> Result<VirtAddr, i32>;
}

/// File metadata returned by [`FileOps::stat()`].
#[derive(Debug, Clone)]
pub struct FileStat {
    /// Inode number.
    pub inode: u64,
    /// File size in bytes.
    pub size: u64,
    /// File type and permissions (POSIX mode_t).
    pub mode: u32,
    /// Number of hard links.
    pub nlink: u32,
    /// Owner user ID.
    pub uid: u32,
    /// Owner group ID.
    pub gid: u32,
}

/// File descriptor type stored in a process's FD table.
///
/// Each variant wraps the appropriate backend — regular files go through
/// `FileOps`, sockets through `SocketOps`, pipes through `PipeHandle`.
pub enum FileDescriptor {
    /// Regular file (disk, tmpfs, etc.) backed by [`FileOps`].
    File(Box<dyn FileOps>),
    /// Network or Unix domain socket.
    Socket(SocketHandle),
    /// Pipe (read/write ends share a buffer).
    Pipe(PipeHandle),
    /// eventfd — event notification between processes.
    EventFd(u64),
    /// signalfd — accept signals as readable FDs.
    Signalfd,
    /// timerfd — accept timer expiration as readable FDs.
    Timerfd,
    /// epoll FD — event multiplexer.
    EpollFd,
}

// ─── Socket Types ───────────────────────────────────────────────────

/// Opaque socket handle (real implementation in vahi-net).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketHandle(pub u64);

/// Opaque pipe handle (real implementation in vahi-task or vahi-vfs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PipeHandle(pub u64);

// ─── I/O Vector for scatter/gather ──────────────────────────────────

/// POSIX iovec for `readv`/`writev` syscalls.
#[derive(Debug, Clone, Copy)]
pub struct IoVec {
    /// Virtual address of the buffer.
    pub base: VirtAddr,
    /// Length of the buffer in bytes.
    pub len: usize,
}

// ─── Architecture Trait ─────────────────────────────────────────────
// Breaks the boot ↔ arch cycle: boot calls through this trait
// instead of directly depending on arch-specific code.

/// Architecture-specific operations.
pub trait ArchOps: Send + Sync {
    /// Initialize the architecture (GDT, IDT, etc.)
    fn init(&self);

    /// Initialize a secondary CPU (AP).
    fn init_ap(&self);

    /// Read the timestamp counter.
    fn rdtsc(&self) -> u64;

    /// Enable/disable interrupts.
    fn enable_interrupts(&self);
    fn disable_interrupts(&self);

    /// Halt the CPU until the next interrupt.
    fn halt(&self);

    /// Get the current CPU ID.
    fn cpu_id(&self) -> u32;

    /// Get the total number of CPUs.
    fn cpu_count(&self) -> u32;

    /// Shutdown the machine.
    fn shutdown(&self) -> !;

    /// Reboot the machine.
    fn reboot(&self) -> !;
}

// ─── Timer Trait ────────────────────────────────────────────────────
// Used by interrupts/timer to abstract over timer implementations.

/// High-resolution timer source.
pub trait TimerSource: Send + Sync {
    /// Read the current tick count.
    fn ticks(&self) -> TickCount;

    /// Calibrate the timer (measure frequency).
    fn calibrate(&self);

    /// Set the timer interval (for periodic mode).
    fn set_interval(&self, interval_ticks: TickCount);

    /// Enable/disable one-shot mode.
    fn set_oneshot(&self, ticks: TickCount);

    /// Check if the timer has fired.
    fn has_fired(&self) -> bool;
}

// ─── Driver Trait ───────────────────────────────────────────────────
// Used by the driver subsystem to register and manage drivers.

/// Common trait for all device drivers.
pub trait Driver: Send + Sync {
    /// Human-readable driver name.
    fn name(&self) -> &str;

    /// Probe for the device and initialize if found.
    fn probe(&self) -> Result<(), DriverError>;

    /// Shut down the driver and release resources.
    fn shutdown(&self);

    /// Handle a hardware interrupt (called from IRQ context).
    ///
    /// # Safety
    ///
    /// Must only be called from the correct IRQ handler.
    fn handle_irq(&self);
}

/// Driver error types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverError {
    /// No device matching the PCI ID or compatible string was found.
    DeviceNotFound,
    /// Device found but initialization failed (wrong firmware, bad config).
    InitFailed,
    /// Not enough memory to initialize (DMA buffer, page tables).
    NoMemory,
    /// Device did not respond within the timeout.
    Timeout,
    /// Invalid configuration (bad IRQ, wrong BAR, etc.).
    InvalidConfig,
}

// ─── Network Types ──────────────────────────────────────────────────
// Used by net, vfs (Unix sockets), and task (socket FDs).

/// Network socket operations.
pub trait SocketOps: Send + Sync {
    /// Bind the socket to a local address and port.
    fn bind(&self, addr: &[u8], port: u16) -> Result<(), i32>;
    /// Mark the socket as passive (listening for connections).
    fn listen(&self, backlog: i32) -> Result<(), i32>;
    /// Accept a new connection. Returns (handle, remote addr, remote port).
    fn accept(&self) -> Result<(SocketHandle, [u8; 16], u16), i32>;
    /// Initiate a connection to a remote address.
    fn connect(&self, addr: &[u8], port: u16) -> Result<(), i32>;
    /// Send data. Returns the number of bytes sent.
    fn send(&self, buf: &[u8], flags: i32) -> Result<usize, i32>;
    /// Receive data. Returns the number of bytes received.
    fn recv(&self, buf: &mut [u8], flags: i32) -> Result<usize, i32>;
    /// Close the socket and release resources.
    fn close(&self) -> Result<(), i32>;
}
