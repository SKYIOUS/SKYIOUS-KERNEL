#![allow(unused_imports, unused_doc_comments, unused_variables)]
use x86_64::VirtAddr;
use x86_64::registers::model_specific::{LStar, Star, SFMask};
use x86_64::registers::rflags::RFlags;
use crate::gdt;
#[allow(unused_imports)] use crate::interrupts::IrqFmtBuf;
#[allow(unused_imports)] use crate::sync::IrqSafeMutex as Mutex;
use crate::vfs::{VFS, VfsNode, Stat};
use alloc::sync::Arc;
use crate::task::process::Process;
use x86_64::structures::paging::{Page, Size4KiB, Mapper, FrameAllocator, PageTableFlags};

pub mod errno;
pub mod numbers;
pub mod signal;
pub mod futex;
pub mod user_access;
pub mod io_uring;
pub mod shm;
pub mod posix_timers;
pub mod epoll;
pub mod compat;
pub mod procfs;
pub mod inotify;
pub mod mmsg;
pub mod seccomp;
pub mod landlock;
pub mod prctl;
pub mod namespaces;
pub mod cgroup;
pub mod mqueue;

pub mod fs;
pub mod fs_open;
pub mod fs_stat;
pub mod fs_mount;
pub mod fs_io;
pub mod process;
pub mod process_lifecycle;
pub mod process_signal;
pub mod process_creds;
pub mod net;
pub mod net_helpers;
pub mod net_socket;
pub mod net_options;
pub mod ipc;
pub mod gui;
pub mod misc;
pub mod vm;
pub mod eventfd;
pub mod timerfd;
pub mod dispatch;
pub use dispatch::*;
pub mod helpers;
pub use helpers::*;

pub use fs::*;
pub use process::*;
pub use net::*;
pub use signal::*;
pub use ipc::*;
pub use gui::*;
pub use misc::*;

use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;

// Capability constants (matching Linux CAP_* values)
// ponytail: keepping existing CAP_SETUID/CAP_SETGID as-is (swapped vs Linux)


/// Maximum supported CPU count.
#[allow(dead_code)]
pub const MAX_CPUS: usize = 8;
/// Upper bound for a single read/write syscall buffer allocation.
/// Larger requests are clamped (callers must loop Ã¢â‚¬â€ POSIX allows short I/O).

/// Array of per-CPU area pointers for indexed access from non-GS contexts.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct PerCpuPtr(pub *mut PerCpuData);
unsafe impl Send for PerCpuPtr {}
unsafe impl Sync for PerCpuPtr {}

pub static PER_CPU_AREAS: crate::sync::IrqSafeMutex<alloc::vec::Vec<PerCpuPtr>> = crate::sync::IrqSafeMutex::new(alloc::vec::Vec::new());

/// Get the current CPU's per-CPU data via GS segment.


#[repr(C)]
pub struct PerCpuData {
    pub self_ptr:  u64,      // offset 0x00 Ã¢â‚¬â€ pointer to self (gs:0x0 reads this)
    pub cpu_id:    u64,      // offset 0x08
    pub kernel_rsp: u64,      // offset 0x10 Ã¢â‚¬â€ loaded on syscall entry
    pub user_rsp:  u64,      // offset 0x18 Ã¢â‚¬â€ saved on syscall entry
    pub ipi_kind: core::sync::atomic::AtomicU64, // offset 0x20 Ã¢â‚¬â€ IPI discriminant (0=none,1=tlb,2=resched,3=func)
    pub ipi_arg: core::sync::atomic::AtomicU64,  // offset 0x28 Ã¢â‚¬â€ IPI argument / func ptr
    pub idle_count: u64,     // offset 0x30 Ã¢â‚¬â€ idle loop counter
    /// Per-CPU current process pointer (uintptr to Arc<Process>).
    /// Read lock-free on the owning CPU; written only by that CPU's scheduler.
    /// Other CPUs must use the global PROCESS_TABLE for cross-process ops.
    pub current_process: core::sync::atomic::AtomicU64, // offset 0x38
    /// Nesting count of active user-memory copies on this CPU. Read by the
    /// page-fault dispatch trampoline via `gs:0x0 + USER_COPY_NEST_OFFSET`.
    pub user_copy_nest: core::sync::atomic::AtomicU64, // offset 0x40
    /// RSP at page-fault dispatch entry (points at the error code). Written by
    /// the `vahi_pf_dispatch` trampoline so a copy abort can iret to the fixup.
    pub pf_entry_rsp: u64, // offset 0x48
}

/// Offset of `PerCpuData::pf_entry_rsp`, written directly by the page-fault
/// dispatch trampoline (asm). Kept in lock-step via this assertion.
pub const PF_ENTRY_RSP_OFFSET: usize = 0x48;
const _: () = assert!(core::mem::offset_of!(PerCpuData, pf_entry_rsp) == PF_ENTRY_RSP_OFFSET);

#[repr(C, packed)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}







const F_DUPFD: i32 = 0;
const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;




/// Sets the kernel stack for the current CPU. 
/// Called by the scheduler on context switch.
pub fn set_kernel_stack(stack_top: u64) {
    let data = get_per_cpu();
    data.kernel_rsp = stack_top;
}

extern "C" {
    fn syscall_entry();
}


/// Inner dispatch without emulation redirect Ã¢â‚¬â€ called by both the public entry
/// point and the Linux emulation layer to avoid infinite recursion.












pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;









/// Check if current process has pending unmasked signals; return EINTR if so
pub(crate) fn check_signal_interrupt() -> bool {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let sig = p.signals.lock();
        sig.has_unmasked_pending(sig.blocked)
    } else {
        false
    }
}












#[repr(C)]
pub struct Timespec {
    pub tv_sec: i64,
    pub tv_nsec: i64,
}





/// Check whether the caller may change ownership of a file to `new_uid`/`new_gid`.
/// Returns true if allowed.


















































#[allow(dead_code)] /// Read: copies clipboard to user buffer, returns bytes copied
/// Write: copies user buffer to clipboard











core::arch::global_asm!(
    r#"
    .global syscall_entry
    syscall_entry:
        endbr64             # CET IBT landing pad
        swapgs              # Switch to kernel GS base
        mov gs:[0x18], rsp  # Save user RSP to PerCpuData.user_rsp (offset 0x18)
        mov rsp, gs:[0x10]  # Load kernel RSP from PerCpuData.kernel_rsp (offset 0x10)

        # Save registers (to match TaskContext layout for easy fork)
        push gs:[0x18]      # user_rsp
        push r11           # user_rflags
        push rcx           # user_rip
        push rax
        push rcx           # rcx again (for sysv64 arg matching if needed)
        push rdx
        push rbx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11           # r11 again
        push r12
        push r13
        push r14
        push r15

        # Set up syscall_handler(n, arg1, arg2, arg3, arg4, arg5, regs_ptr)
        # Stack offsets (bytes from current RSP):
        # +112 = rax  (syscall number n)
        # +64  = rdi  (arg1)
        # +72  = rsi  (arg2)
        # +96  = rdx  (arg3)
        # +40  = r10  (arg4)
        # +56  = r8   (arg5)
        mov rdi, [rsp+112]      # n = syscall number
        mov rsi, [rsp+64]       # arg1 = saved rdi
        mov rdx, [rsp+72]       # arg2 = saved rsi
        mov rcx, [rsp+96]       # arg3 = saved rdx
        mov r8,  [rsp+40]       # arg4 = saved r10
        mov r9,  [rsp+56]       # arg5 = saved r8
        push rsp                # regs_ptr (7th arg on stack)
        
        call syscall_handler
        
        add rsp, 8              # Pop the regs_ptr we pushed

        # Restore registers
        pop r15
        pop r14
        pop r13
        pop r12
        add rsp, 8              # Skip scratch r11 Ã¢â‚¬â€ real RFLAGS is loaded later
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rbx
        pop rdx
        pop rcx
        mov r11, [rsp+16]       # Load user RFLAGS (saved at [rsp+16]) into R11 for sysretq
        # Skip saved rax (syscall number) Ã¢â‚¬â€ return value from handler is already in RAX
        add rsp, 8
        # Drop saved user_rip, rflags, rsp (they are restored via sysret and mov rsp)
        add rsp, 24

        mov rsp, gs:[0x18]     # Restore user RSP
        swapgs              # Switch back to user GS base
        sysretq
    "#
);













/// Linux-compatible capget: read capability sets

/// Linux-compatible capset: set capability sets (root only)

/// sigprocmask: examine/change signal mask

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ pause Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬


// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ sigaltstack Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

use crate::task::process::stack_t;
use crate::task::process::{SS_DISABLE, SS_ONSTACK, MINSIGSTKSZ};


// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ signalfd4 Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

use crate::task::process::{SignalFdData, SignalFdInfo, SIGNAL_FDS, SFD_NONBLOCK, SFD_CLOEXEC};
use crate::task::process::{EventFdData, EFD_SEMAPHORE, EFD_NONBLOCK, EFD_MAX};


// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ eventfd2 Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬


// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ itimer Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

use crate::task::process::itimerval;
use crate::task::process::timeval;



// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ times Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

use crate::task::process::tms;


















// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ *at syscall variants Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬















