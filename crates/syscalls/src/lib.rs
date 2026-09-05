//! # vahi-syscalls — System Call Interface
//!
//! Syscall numbers, dispatch table, and implementations.
//! Extracted from `kernel/src/syscalls/`.
//!
//! ## Status
//!
//! This crate is scaffolded. The full syscall implementations remain in
//! `kernel/src/syscalls/` until the dependent crates (vahi-vfs, vahi-task,
//! vahi-net, vahi-drivers) are fully extracted.

#![no_std]

extern crate alloc;

pub mod cgroup;
pub mod errno;
pub mod futex;
pub mod inotify;
pub mod io_uring;
pub mod landlock;
pub mod mqueue;
pub mod namespaces;
pub mod net_helpers;
pub mod numbers;
pub mod posix_timers;
pub mod process_lifecycle;
pub mod ptrace;
pub mod seccomp;
pub mod shm;
pub mod signal;
pub mod timerfd;
pub mod user_access;

use core::sync::atomic::AtomicU64;

/// Syscall capability bits (Linux-compatible).
pub const CAP_SYS_ADMIN: u32 = 21;

/// Per-CPU data structure.
#[repr(C)]
pub struct PerCpuData {
    pub self_ptr: u64,
    pub cpu_id: u64,
    pub kernel_rsp: u64,
    pub user_rsp: u64,
    pub ipi_kind: AtomicU64,
    pub ipi_arg: AtomicU64,
    pub idle_count: u64,
    pub current_process: AtomicU64,
    pub user_copy_nest: AtomicU64,
    pub pf_entry_rsp: u64,
}

/// Stub: get per-CPU data.
#[allow(dead_code, static_mut_refs)]
pub fn get_per_cpu() -> &'static mut PerCpuData {
    static mut PER_CPU: PerCpuData = PerCpuData {
        self_ptr: 0,
        cpu_id: 0,
        kernel_rsp: 0,
        user_rsp: 0,
        ipi_kind: AtomicU64::new(0),
        ipi_arg: AtomicU64::new(0),
        idle_count: 0,
        current_process: AtomicU64::new(0),
        user_copy_nest: AtomicU64::new(0),
        pf_entry_rsp: 0,
    };
    unsafe { &mut PER_CPU }
}

/// Stub: set kernel stack for current CPU.
#[allow(dead_code)]
pub fn set_kernel_stack(_stack_top: u64) {}

/// Stub: check if current process has pending signals.
#[allow(dead_code)]
pub fn check_signal_interrupt() -> bool {
    false
}

/// Check if the current process has the given capability.
pub fn has_capability(_cap: u32) -> bool {
    false
}

/// Get the effective UID of the current process.
pub fn get_current_euid() -> u32 {
    vahi_types::current_credentials().euid
}

/// Stub: get the current process.
pub fn get_current_process() -> Option<&'static str> {
    None
}
