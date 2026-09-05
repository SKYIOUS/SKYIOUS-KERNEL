//! Registry vocabulary for the vahi kernel crate boundary.

use crate::identity::{Credentials, Pid, TickCount, VirtAddr};
use crate::provider::{ArchOps, FileDescriptor, PageFaultHandler, ProcessProvider, TimerSource};

// ─── Registry (Runtime Wiring) ─────────────────────────────────────
// The kernel's boot code populates these statics so that extracted
// crates can call through traits without depending on the kernel.

/// Global process provider instance. Populated during boot.
static PROCESS_PROVIDER: spin::Once<&'static dyn ProcessProvider> = spin::Once::new();

/// Global page fault handler. Populated during boot.
static PAGE_FAULT_HANDLER: spin::Once<&'static dyn PageFaultHandler> = spin::Once::new();

/// Global architecture ops. Populated during boot.
static ARCH_OPS: spin::Once<&'static dyn ArchOps> = spin::Once::new();

/// Global timer source. Populated during boot.
static TIMER_SOURCE: spin::Once<&'static dyn TimerSource> = spin::Once::new();

/// Monotonic scheduler tick counter, registered by the kernel's interrupt
/// module. Owned here so extracted crates read ticks without each defining
/// its own stub (per-crate stubs silently disagree with the kernel's clock).
static TICK_FN: spin::Once<fn() -> u64> = spin::Once::new();

/// Register the monotonic tick source. Called once during kernel boot.
pub fn register_tick_fn(f: fn() -> u64) {
    TICK_FN.call_once(|| f);
}

/// Monotonic 100 Hz tick count. Returns 0 until the kernel registers a source.
pub fn get_ticks() -> u64 {
    TICK_FN.get().map_or(0, |f| f())
}

/// Block the calling thread until woken via [`wake_pipe`] (pipe blocking).
/// No-op until the kernel registers a scheduler facade.
static SCHED_FACADE: spin::Once<SchedFacade> = spin::Once::new();

/// Blocking/waking primitives the kernel's scheduler provides to crates
/// that must block on a resource without depending on the scheduler crate.
#[derive(Clone, Copy)]
pub struct SchedFacade {
    pub block_on_pipe: fn(u64),
    pub wake_pipe: fn(u64),
}

/// Register the scheduler facade. Called once during kernel boot.
pub fn register_sched_facade(facade: SchedFacade) {
    SCHED_FACADE.call_once(|| facade);
}

/// Block the current thread on a pipe key. No-op before registration.
pub fn block_on_pipe(key: u64) {
    if let Some(f) = SCHED_FACADE.get() {
        (f.block_on_pipe)(key);
    }
}

/// Wake all threads blocked on a pipe key. No-op before registration.
pub fn wake_pipe(key: u64) {
    if let Some(f) = SCHED_FACADE.get() {
        (f.wake_pipe)(key);
    }
}

/// Sleep the calling thread until tick `target` (thread-context sleep;
/// mirrors sys_nanosleep's mark-Blocked-with-deadline + yield).
/// No-op until the kernel registers the facade.
static SLEEP_FACADE: spin::Once<fn(u64) -> ()> = spin::Once::new();

/// Register the thread-context sleep primitive. Called once during boot.
pub fn register_sleep_facade(f: fn(u64)) {
    SLEEP_FACADE.call_once(|| f);
}

/// Sleep until tick `target`. No-op before registration.
pub fn sleep_until_tick(target: u64) {
    if let Some(f) = SLEEP_FACADE.get() {
        f(target);
    }
}

/// (pid, uid, gid) of the *connecting* process's peer — used by unix-socket
/// SO_PEERCRED. Returns the current process's credentials triple; `None`
/// before the provider is registered.
pub fn current_peer_creds() -> Option<(u32, u32, u32)> {
    let pid = current_pid();
    if pid == 0 {
        return None;
    }
    PROCESS_PROVIDER.get().and_then(|p| p.peer_creds(pid))
}

/// Register the process provider. Called once during kernel boot.
///
/// # Safety
///
/// Must be called exactly once, before any other crate accesses the provider.
pub fn register_process_provider(provider: &'static dyn ProcessProvider) {
    PROCESS_PROVIDER.call_once(|| provider);
}

/// Register the page fault handler. Called once during kernel boot.
///
/// # Panics
///
/// Panics if called more than once (spin::Once behavior).
pub fn register_page_fault_handler(handler: &'static dyn PageFaultHandler) {
    PAGE_FAULT_HANDLER.call_once(|| handler);
}

/// Register the architecture ops. Called once during kernel boot.
///
/// # Panics
///
/// Panics if called more than once (spin::Once behavior).
pub fn register_arch_ops(ops: &'static dyn ArchOps) {
    ARCH_OPS.call_once(|| ops);
}

/// Register the timer source. Called once during kernel boot.
///
/// # Panics
///
/// Panics if called more than once (spin::Once behavior).
pub fn register_timer_source(source: &'static dyn TimerSource) {
    TIMER_SOURCE.call_once(|| source);
}

// ─── Accessor functions ─────────────────────────────────────────────

/// Get the current process PID.
///
/// Returns `0` if no process provider is registered yet.
pub fn current_pid() -> Pid {
    PROCESS_PROVIDER.get().map_or(0, |p| p.current_pid())
}

/// Get the current process credentials.
///
/// Returns root credentials if no process provider is registered yet.
pub fn current_credentials() -> Credentials {
    PROCESS_PROVIDER
        .get()
        .map_or(Credentials::ROOT, |p| p.current_credentials())
}

/// Check if an address is a valid user-space address.
///
/// Defaults to checking against [`USER_ADDR_MAX`] if no provider is registered.
pub fn is_user_addr(addr: VirtAddr) -> bool {
    PROCESS_PROVIDER
        .get()
        .map_or(addr < USER_ADDR_MAX, |p| p.is_user_address(addr))
}

/// Get the current tick count from the registered timer.
///
/// Returns `0` if no timer source is registered yet.
pub fn ticks() -> TickCount {
    TIMER_SOURCE.get().map_or(0, |t| t.ticks())
}

impl core::fmt::Debug for FileDescriptor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::File(_) => f.debug_tuple("File").field(&"...").finish(),
            Self::Socket(h) => f.debug_tuple("Socket").field(h).finish(),
            Self::Pipe(h) => f.debug_tuple("Pipe").field(h).finish(),
            Self::EventFd(v) => f.debug_tuple("EventFd").field(v).finish(),
            Self::Signalfd => write!(f, "Signalfd"),
            Self::Timerfd => write!(f, "Timerfd"),
            Self::EpollFd => write!(f, "EpollFd"),
        }
    }
}

/// Get the architecture ops.
pub fn arch() -> &'static (dyn ArchOps + 'static) {
    *ARCH_OPS.get().expect("vahi-types: ArchOps not registered")
}

// ─── Constants ──────────────────────────────────────────────────────

/// User space upper bound (canonical lower half on x86_64).
pub const USER_ADDR_MAX: VirtAddr = 0x0000_7FFF_FFFF_FFFF;

/// Kernel space lower bound (canonical upper half on x86_64).
pub const KERNEL_ADDR_MIN: VirtAddr = 0xFFFF_8000_0000_0000;

/// Page size (4 KiB).
pub const PAGE_SIZE: usize = 4096;

/// Offset of virtual address above physical address (higher half).
pub const HIGHER_HALF_OFFSET: VirtAddr = 0xFFFF_8000_0000_0000;

/// Maximum number of CPUs supported.
pub const MAX_CPUS: usize = 256;
