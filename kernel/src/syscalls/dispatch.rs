#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;
use x86_64::VirtAddr;
use x86_64::registers::model_specific::{LStar, Star, SFMask};
use x86_64::registers::rflags::RFlags;
use x86_64::structures::paging::{Page, Size4KiB, Mapper, FrameAllocator, PageTableFlags};
use crate::gdt;
use crate::interrupts::IrqFmtBuf;
use super::errno;
use super::numbers;
use super::helpers::*;
use super::{fs, process, net, ipc, gui, misc};
use super::{seccomp, landlock, prctl, namespaces, cgroup};
use super::*;
use crate::syscalls::user_access;
use crate::task::process::Process;
extern "C" {
    fn syscall_entry();
}

// ─── Per-CPU data and MSR setup ─────────────────────────────────

pub fn init_syscall_msrs() {
    let selectors = gdt::get_selectors();

    Star::write(
        selectors.user_code_selector,
        selectors.user_data_selector,
        selectors.code_selector,
        selectors.data_selector,
    ).expect("failed to write STAR MSR");

    LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::ALIGNMENT_CHECK);
}

pub fn init() {
    user_access::init_smap();
    init_syscall_msrs();

    unsafe {
        use x86_64::registers::model_specific::Efer;
        Efer::update(|efer| efer.insert(x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS));
        
        init_gs_base(0);
    }
}

pub fn get_per_cpu() -> &'static mut PerCpuData {
    let base: u64;
    unsafe {
        core::arch::asm!("mov {0}, gs:0x0", out(reg) base);
    }
    unsafe { &mut *(base as *mut PerCpuData) }
}

pub fn init_gs_base(cpu_id: usize) {
    use x86_64::registers::model_specific::KernelGsBase;
    use x86_64::registers::model_specific::GsBase;

    let data = alloc::boxed::Box::leak(alloc::boxed::Box::new(PerCpuData {
        self_ptr: 0,
        cpu_id: cpu_id as u64,
        kernel_rsp: crate::gdt::get_kernel_stack().as_u64(),
        user_rsp: 0,
        ipi_kind: core::sync::atomic::AtomicU64::new(0),
        ipi_arg: core::sync::atomic::AtomicU64::new(0),
        idle_count: 0,
        current_process: core::sync::atomic::AtomicU64::new(0),
        user_copy_nest: core::sync::atomic::AtomicU64::new(0),
        pf_entry_rsp: 0,
    }));
    data.self_ptr = data as *mut PerCpuData as u64;
    
    let addr = x86_64::VirtAddr::from_ptr(data as *const _);
    KernelGsBase::write(addr);
    GsBase::write(addr);

    let mut areas = PER_CPU_AREAS.lock();
    if cpu_id >= areas.len() {
        areas.resize(cpu_id + 1, PerCpuPtr(core::ptr::null_mut()));
    }
    areas[cpu_id] = PerCpuPtr(data as *mut PerCpuData);
}

// ─── Typed dispatch ─────────────────────────────────────────────

/// Typed arguments for a syscall, passed to every handler uniformly.
pub struct SyscallArgs {
    pub n: u64,
    pub a1: u64,
    pub a2: u64,
    pub a3: u64,
    pub a4: u64,
    pub a5: u64,
    /// Pointer to the saved user registers (for syscalls that need
    /// to modify the return path, e.g. mmap, clone, execve, futex).
    pub regs: *mut u64,
}

/// Uniform handler signature: every syscall is wrapped to this shape.
pub type SyscallHandler = fn(&SyscallArgs) -> u64;

/// Dispatch table indexed by syscall number. `None` = unknown syscall.
const TABLE_SIZE: usize = 475;

/// Build the dispatch table. Called once; the result is cached.
const fn build_table() -> [Option<SyscallHandler>; TABLE_SIZE] {
    let mut t: [Option<SyscallHandler>; TABLE_SIZE] = [None; TABLE_SIZE];

    // ── File I/O (0-22) ───────────────────────────────────────
    t[numbers::SYS_READ as usize]        = Some(sys_read);
    t[numbers::SYS_WRITE as usize]       = Some(sys_write);
    t[numbers::SYS_OPEN as usize]        = Some(sys_open);
    t[numbers::SYS_CLOSE as usize]       = Some(sys_close);
    t[numbers::SYS_STAT as usize]        = Some(sys_stat);
    t[numbers::SYS_FSTAT as usize]       = Some(sys_fstat);
    t[numbers::SYS_LSTAT as usize]       = Some(sys_lstat);
    t[numbers::SYS_POLL as usize]        = Some(sys_poll);
    t[numbers::SYS_LSEEK as usize]       = Some(sys_lseek);
    t[numbers::SYS_MMAP as usize]        = Some(sys_mmap);
    t[numbers::SYS_MPROTECT as usize]    = Some(sys_mprotect);
    t[numbers::SYS_MUNMAP as usize]      = Some(sys_munmap);
    t[numbers::SYS_BRK as usize]         = Some(sys_brk);
    t[numbers::SYS_RT_SIGACTION as usize]= Some(sys_rt_sigaction);
    t[numbers::SYS_RT_SIGRETURN as usize]= Some(sys_rt_sigreturn);
    t[numbers::SYS_IOCTL as usize]       = Some(sys_ioctl);
    t[numbers::SYS_ACCESS as usize]      = Some(sys_access);
    t[numbers::SYS_PIPE as usize]        = Some(sys_pipe);
    t[numbers::SYS_SELECT as usize]      = Some(sys_select);
    t[numbers::SYS_SCHED_YIELD as usize] = Some(sys_sched_yield);
    t[numbers::SYS_DUP as usize]         = Some(sys_dup);
    t[numbers::SYS_DUP2 as usize]        = Some(sys_dup2);
    t[numbers::SYS_PAUSE as usize]       = Some(sys_pause);
    t[numbers::SYS_NANOSLEEP as usize]   = Some(sys_nanosleep);
    t[numbers::SYS_SYNC as usize]        = Some(sys_sync);
    t[numbers::SYS_GETPID as usize]      = Some(sys_getpid);
    t[numbers::SYS_SENDFILE as usize]    = Some(sys_sendfile);

    // ── Networking (41-55) ─────────────────────────────────────
    t[numbers::SYS_SOCKET as usize]      = Some(sys_socket);
    t[numbers::SYS_CONNECT as usize]     = Some(sys_connect);
    t[numbers::SYS_ACCEPT as usize]      = Some(sys_accept);
    t[numbers::SYS_SENDTO as usize]      = Some(sys_sendto);
    t[numbers::SYS_RECVFROM as usize]    = Some(sys_recvfrom);
    t[numbers::SYS_SENDMSG as usize]     = Some(sys_sendmsg);
    t[numbers::SYS_RECVMSG as usize]     = Some(sys_recvmsg);
    t[numbers::SYS_BIND as usize]        = Some(sys_bind);
    t[numbers::SYS_LISTEN as usize]      = Some(sys_listen);
    t[numbers::SYS_GETSOCKNAME as usize] = Some(sys_getsockname);
    t[numbers::SYS_GETPEERNAME as usize] = Some(sys_getpeername);
    t[numbers::SYS_SOCKETPAIR as usize]  = Some(sys_socket_pair);
    t[numbers::SYS_SETSOCKOPT as usize]  = Some(sys_setsockopt);
    t[numbers::SYS_GETSOCKOPT as usize]  = Some(sys_getsockopt);

    // ── Process (56-63) ───────────────────────────────────────
    t[numbers::SYS_CLONE as usize]       = Some(sys_clone);
    t[numbers::SYS_FORK as usize]        = Some(sys_fork);
    t[numbers::SYS_EXECVE as usize]      = Some(sys_execve);
    t[numbers::SYS_EXIT as usize]        = Some(sys_exit);
    t[numbers::SYS_WAIT4 as usize]       = Some(sys_wait4);
    t[numbers::SYS_KILL as usize]        = Some(sys_kill);
    t[numbers::SYS_UNAME as usize]       = Some(sys_uname);

    // ── Filesystem (72-95) ────────────────────────────────────
    t[numbers::SYS_FCNTL as usize]       = Some(sys_fcntl);
    t[numbers::SYS_TRUNCATE as usize]    = Some(sys_truncate);
    t[numbers::SYS_FTRUNCATE as usize]   = Some(sys_ftruncate);
    t[numbers::SYS_GETCWD as usize]      = Some(sys_getcwd);
    t[numbers::SYS_CHDIR as usize]       = Some(sys_chdir);
    t[numbers::SYS_RENAME as usize]      = Some(sys_rename);
    t[numbers::SYS_MKDIR as usize]       = Some(sys_mkdir);
    t[numbers::SYS_LINK as usize]        = Some(sys_link);
    t[numbers::SYS_UNLINK as usize]      = Some(sys_unlink);
    t[numbers::SYS_SYMLINK as usize]     = Some(sys_symlink);
    t[numbers::SYS_READLINK as usize]    = Some(sys_readlink);
    t[numbers::SYS_CHMOD as usize]       = Some(sys_chmod);
    t[numbers::SYS_FCHMOD as usize]      = Some(sys_fchmod);
    t[numbers::SYS_CHOWN as usize]       = Some(sys_chown);
    t[numbers::SYS_FCHOWN as usize]      = Some(sys_fchown);
    t[numbers::SYS_UMASK as usize]       = Some(sys_umask);

    // ── Process continued (97-169) ─────────────────────────────
    t[numbers::SYS_GETRLIMIT as usize]   = Some(sys_getrlimit);
    t[numbers::SYS_SETRLIMIT as usize]   = Some(sys_setrlimit);
    t[numbers::SYS_GETPPID as usize]     = Some(sys_getppid);
    t[numbers::SYS_GETPGRP as usize]     = Some(sys_getpgrp);
    t[numbers::SYS_SETSID as usize]      = Some(sys_setsid);
    t[numbers::SYS_GETGROUPS as usize]   = Some(sys_getgroups);
    t[numbers::SYS_SETGROUPS as usize]   = Some(sys_setgroups);
    t[numbers::SYS_GETRESUID as usize]   = Some(sys_getresuid);
    t[numbers::SYS_SETRESUID as usize]   = Some(sys_setresuid);
    t[numbers::SYS_SIGALTSTACK as usize] = Some(sys_sigaltstack);
    t[numbers::SYS_STATFS as usize]      = Some(sys_statfs);
    t[numbers::SYS_SCHED_SETATTR as usize]  = Some(sys_sched_setattr);
    t[numbers::SYS_SCHED_GETATTR as usize]  = Some(sys_sched_getattr);
    t[numbers::SYS_SETPGID as usize]     = Some(sys_setpgid);
    t[numbers::SYS_ARCH_PRCTL as usize]  = Some(sys_arch_prctl);
    t[numbers::SYS_MOUNT as usize]       = Some(sys_mount);
    t[numbers::SYS_UMOUNT2 as usize]     = Some(sys_umount2);
    t[numbers::SYS_REBOOT as usize]      = Some(sys_reboot);

    // ── Misc (200+) ───────────────────────────────────────────
    t[numbers::SYS_RESOLVE as usize]     = Some(sys_resolve);
    t[numbers::SYS_FUTEX as usize]       = Some(sys_futex);
    t[numbers::SYS_SCHED_SETAFFINITY as usize] = Some(sys_sched_setaffinity);
    t[numbers::SYS_SCHED_GETAFFINITY as usize] = Some(sys_sched_getaffinity);
    t[numbers::SYS_SYSINFO as usize]     = Some(sys_sysinfo);
    t[numbers::SYS_OPENPTY as usize]     = Some(sys_openpty);
    t[numbers::SYS_GETDENTS64 as usize]  = Some(sys_getdents64);
    t[numbers::SYS_SET_TID_ADDRESS as usize] = Some(sys_set_tid_address);
    t[numbers::SYS_TIMER_CREATE as usize]    = Some(sys_timer_create);
    t[numbers::SYS_TIMER_SETTIME as usize]   = Some(sys_timer_settime);
    t[numbers::SYS_TIMER_GETTIME as usize]   = Some(sys_timer_gettime);
    t[numbers::SYS_TIMER_GETOVERRUN as usize]= Some(sys_timer_getoverrun);
    t[numbers::SYS_TIMER_DELETE as usize]    = Some(sys_timer_delete);
    t[numbers::SYS_CLOCK_GETTIME as usize]   = Some(sys_clock_gettime);
    t[numbers::SYS_CLOCK_GETRES as usize]    = Some(sys_clock_getres);
    t[numbers::SYS_CLOCK_NANOSLEEP as usize] = Some(sys_clock_nanosleep);
    t[numbers::SYS_EXIT_GROUP as usize]  = Some(sys_exit_group);
    t[numbers::SYS_OPENAT as usize]      = Some(sys_openat);
    t[numbers::SYS_MKDIRAT as usize]     = Some(sys_mkdirat);
    t[numbers::SYS_FSTATAT as usize]     = Some(sys_fstatat);
    t[numbers::SYS_UNLINKAT as usize]    = Some(sys_unlinkat);
    t[numbers::SYS_RENAMEAT as usize]    = Some(sys_renameat);
    t[numbers::SYS_LINKAT as usize]      = Some(sys_linkat);
    t[numbers::SYS_SYMLINKAT as usize]   = Some(sys_symlinkat);
    t[numbers::SYS_READLINKAT as usize]  = Some(sys_readlinkat);
    t[numbers::SYS_FACCESSAT as usize]   = Some(sys_faccessat);
    t[numbers::SYS_UNSHARE as usize]     = Some(sys_unshare);
    t[numbers::SYS_UTIMENSAT as usize]   = Some(sys_utimensat);
    t[numbers::SYS_SIGNALFD as usize]    = Some(sys_signalfd);
    t[numbers::SYS_EVENTFD as usize]     = Some(sys_eventfd);
    t[numbers::SYS_FALLOCATE as usize]   = Some(sys_fallocate);
    t[numbers::SYS_SIGNALFD4 as usize]   = Some(sys_signalfd4);
    t[numbers::SYS_EVENTFD2 as usize]    = Some(sys_eventfd2);
    t[numbers::SYS_GETUID as usize]      = Some(sys_getuid);
    t[numbers::SYS_GETGID as usize]      = Some(sys_getgid);
    t[numbers::SYS_SETUID as usize]      = Some(sys_setuid);
    t[numbers::SYS_SETGID as usize]      = Some(sys_setgid);
    t[numbers::SYS_GETEUID as usize]     = Some(sys_geteuid);
    t[numbers::SYS_GETEGID as usize]     = Some(sys_getegid);
    t[numbers::SYS_CAPGET as usize]      = Some(sys_capget);
    t[numbers::SYS_CAPSET as usize]      = Some(sys_capset);
    t[numbers::SYS_SIGPROCMASK as usize] = Some(sys_rt_sigprocmask);
    t[numbers::SYS_GETRESGID as usize]   = Some(sys_getresgid);
    t[numbers::SYS_SETRESGID as usize]   = Some(sys_setresgid);
    t[numbers::SYS_SECCOMP as usize]     = Some(sys_seccomp);
    t[numbers::SYS_MEMFD_CREATE as usize]= Some(sys_memfd_create);
    t[numbers::SYS_BPF as usize]         = Some(sys_bpf);
    t[numbers::SYS_SWAPON as usize]      = Some(sys_swapon);
    t[numbers::SYS_SWAPOFF as usize]     = Some(sys_swapoff);
    t[numbers::SYS_GETPGID as usize]     = Some(sys_getpgid);
    t[numbers::SYS_GETSID as usize]      = Some(sys_getsid);
    t[numbers::SYS_PRLIMIT64 as usize]   = Some(sys_prlimit64);
    t[numbers::SYS_GETITIMER as usize]   = Some(sys_getitimer);
    t[numbers::SYS_SETITIMER as usize]   = Some(sys_setitimer);
    t[numbers::SYS_TIMES as usize]       = Some(sys_times);
    t[numbers::SYS_GETRUSAGE as usize]   = Some(sys_getrusage);
    t[numbers::SYS_TIMERFD_CREATE as usize]  = Some(sys_timerfd_create);
    t[numbers::SYS_TIMERFD_SETTIME as usize] = Some(sys_timerfd_settime);
    t[numbers::SYS_TIMERFD_GETTIME as usize] = Some(sys_timerfd_gettime);
    t[numbers::SYS_INOTIFY_INIT as usize]       = Some(sys_inotify_init);
    t[numbers::SYS_INOTIFY_ADD_WATCH as usize]  = Some(sys_inotify_add_watch);
    t[numbers::SYS_INOTIFY_RM_WATCH as usize]   = Some(sys_inotify_rm_watch);
    t[numbers::SYS_RECVMMSG as usize]     = Some(sys_recvmmsg);
    t[numbers::SYS_SENDMMSG as usize]     = Some(sys_sendmmsg);
    t[numbers::SYS_OBJMGR_ENUM as usize]= Some(sys_objmgr_enum);
    t[numbers::SYS_OBJMGR_AUDIT as usize]= Some(sys_objmgr_audit);
    t[numbers::SYS_DRMCTL as usize]      = Some(sys_drmctl);
    t[numbers::SYS_HASH as usize]        = Some(sys_hash);
    t[numbers::SYS_GETRANDOM as usize]    = Some(sys_getrandom);
    t[numbers::SYS_IO_URING_SETUP as usize]  = Some(sys_io_uring_setup);
    t[numbers::SYS_IO_URING_ENTER as usize]  = Some(sys_io_uring_enter);
    t[numbers::SYS_IO_URING_REGISTER as usize] = Some(sys_io_uring_register);
    t[numbers::SYS_EPOLL_CREATE1 as usize]   = Some(sys_epoll_create1);
    t[numbers::SYS_EPOLL_CTL as usize]       = Some(sys_epoll_ctl);
    t[numbers::SYS_EPOLL_WAIT as usize]      = Some(sys_epoll_wait);
    t[numbers::SYS_EPOLL_PWAIT as usize]     = Some(sys_epoll_pwait);
    t[numbers::SYS_EPOLL_CREATE as usize]    = Some(sys_epoll_create);
    t[numbers::SYS_READV as usize]       = Some(sys_readv);
    t[numbers::SYS_WRITEV as usize]      = Some(sys_writev);
    t[numbers::SYS_MADVISE as usize]     = Some(sys_madvise);
    t[numbers::SYS_PIPE2 as usize]       = Some(sys_pipe2);
    t[numbers::SYS_DUP3 as usize]        = Some(sys_dup3);
    t[numbers::SYS_PREAD64 as usize]     = Some(sys_pread64);
    t[numbers::SYS_PWRITE64 as usize]    = Some(sys_pwrite64);
    t[numbers::SYS_LANDLOCK_CREATE_RULESET as usize] = Some(sys_landlock_create_ruleset);
    t[numbers::SYS_LANDLOCK_ADD_RULE as usize]      = Some(sys_landlock_add_rule);
    t[numbers::SYS_LANDLOCK_RESTRICT_SELF as usize] = Some(sys_landlock_restrict_self);
    t[numbers::SYS_PRCTL as usize]       = Some(sys_prctl);
    t[numbers::SYS_CGROUP_MKDIR as usize]   = Some(sys_cgroup_mkdir);
    t[numbers::SYS_CGROUP_WRITE as usize]   = Some(sys_cgroup_write);
    t[numbers::SYS_CGROUP_READ as usize]    = Some(sys_cgroup_read);    t[numbers::SYS_SETNS as usize]       = Some(sys_setns);
    t[numbers::SYS_MQ_OPEN as usize]       = Some(sys_mq_open);
    t[numbers::SYS_MQ_CLOSE as usize]      = Some(sys_mq_close);
    t[numbers::SYS_MQ_TIMEDSEND as usize]  = Some(sys_mq_send);
    t[numbers::SYS_MQ_TIMEDRECEIVE as usize]= Some(sys_mq_receive);
    t[numbers::SYS_MQ_UNLINK as usize]     = Some(sys_mq_unlink);

    t[numbers::SYS_SHMGET as usize]      = Some(sys_shmget);
    t[numbers::SYS_SHMAT as usize]       = Some(sys_shmat);
    t[numbers::SYS_SHMCTL as usize]      = Some(sys_shmctl);
    t[numbers::SYS_SHMDT as usize]       = Some(sys_shmdt);

    // ── GUI ────────────────────────────────────────────────────
    t[numbers::SYS_GUI_CREATE_WINDOW as usize]  = Some(sys_gui_create_window);
    t[numbers::SYS_GUI_GET_BUFFER as usize]     = Some(sys_gui_get_buffer);
    t[numbers::SYS_GUI_FLUSH as usize]          = Some(sys_gui_flush);
    t[numbers::SYS_GUI_MAP_BUFFER as usize]     = Some(sys_gui_map_buffer);
    t[numbers::SYS_GUI_GET_KEY as usize]        = Some(sys_gui_get_key);
    t[numbers::SYS_GUI_GET_MOUSE as usize]      = Some(sys_gui_get_mouse);
    t[numbers::SYS_GUI_SET_TITLE as usize]      = Some(sys_gui_set_title);
    t[numbers::SYS_GUI_DESTROY_WINDOW as usize] = Some(sys_gui_destroy_window);
    t[numbers::SYS_GUI_RESIZE_WINDOW as usize]  = Some(sys_gui_resize_window);
    t[numbers::SYS_GUI_MOVE_WINDOW as usize]    = Some(sys_gui_move_window);
    t[numbers::SYS_CLIPBOARD as usize]          = Some(sys_clipboard);
    t[numbers::SYS_NOTIFY as usize]             = Some(sys_notify);
    t[numbers::SYS_BEEP as usize]              = Some(sys_beep);
    t[numbers::SYS_MKFS as usize]              = Some(sys_mkfs);

    t
}

static SYSCALL_TABLE: [Option<SyscallHandler>; TABLE_SIZE] = build_table();

// ─── Handler wrappers ───────────────────────────────────────────
// Each wraps a domain-specific handler into the uniform SyscallHandler signature.

fn sys_read(a: &SyscallArgs) -> u64 { fs::sys_read(a.a1, a.a2 as *mut u8, a.a3 as usize) }
fn sys_write(a: &SyscallArgs) -> u64 { fs::sys_write(a.a1, a.a2 as *const u8, a.a3 as usize) }
fn sys_open(a: &SyscallArgs) -> u64 { fs::sys_open(a.a1 as *const u8, a.a2 as i32, a.a3 as u32) }
fn sys_close(a: &SyscallArgs) -> u64 { fs::sys_close(a.a1) }
fn sys_stat(a: &SyscallArgs) -> u64 { fs::sys_stat(a.a1 as *const u8, a.a2 as *mut crate::vfs::Stat) }
fn sys_lstat(a: &SyscallArgs) -> u64 { fs::sys_lstat(a.a1 as *const u8, a.a2 as *mut crate::vfs::Stat) }
fn sys_fstat(a: &SyscallArgs) -> u64 { fs::sys_fstat(a.a1, a.a2 as *mut crate::vfs::Stat) }
fn sys_lseek(a: &SyscallArgs) -> u64 { fs::sys_lseek(a.a1, a.a2 as i64, a.a3 as i32) }
fn sys_mmap(a: &SyscallArgs) -> u64 {
    let offset = unsafe { *a.regs.add(6) };
    sys_mmap_inner(a.a1, a.a2, a.a3, a.a4, a.a5, offset)
}
fn sys_mprotect(a: &SyscallArgs) -> u64 { fs::sys_mprotect(a.a1, a.a2, a.a3) }
fn sys_munmap(a: &SyscallArgs) -> u64 { fs::sys_munmap(a.a1, a.a2) }
fn sys_brk(a: &SyscallArgs) -> u64 { fs::sys_brk(a.a1) }
fn sys_exit(a: &SyscallArgs) -> u64 { process::sys_exit(a.a1) }
fn sys_clone(a: &SyscallArgs) -> u64 {
    process::sys_clone(a.a1, a.a2, a.a3 as *mut u32, a.a4, a.a5 as *mut u32, a.regs)
}
fn sys_fork(a: &SyscallArgs) -> u64 { process::sys_fork(a.regs) }
fn sys_getpid(a: &SyscallArgs) -> u64 { process::sys_getpid() }
fn sys_getppid(a: &SyscallArgs) -> u64 { process::sys_getppid() }
fn sys_setpgid(a: &SyscallArgs) -> u64 { process::sys_setpgid(a.a1, a.a2) }
fn sys_getpgid(a: &SyscallArgs) -> u64 { process::sys_getpgid(a.a1) }
fn sys_getpgrp(a: &SyscallArgs) -> u64 { process::sys_getpgrp() }
fn sys_setsid(a: &SyscallArgs) -> u64 { process::sys_setsid() }
fn sys_getsid(a: &SyscallArgs) -> u64 { process::sys_getsid(a.a1) }
fn sys_dup(a: &SyscallArgs) -> u64 { fs::sys_dup(a.a1) }
fn sys_dup2(a: &SyscallArgs) -> u64 { fs::sys_dup2(a.a1, a.a2) }
fn sys_access(a: &SyscallArgs) -> u64 { fs::sys_access(a.a1 as *const u8, a.a2 as i32) }
fn sys_openat(a: &SyscallArgs) -> u64 { fs::sys_openat(a.a1 as i64, a.a2 as *const u8, a.a3 as i32, a.a4 as u32) }
fn sys_mkdirat(a: &SyscallArgs) -> u64 { fs::sys_mkdirat(a.a1 as i64, a.a2 as *const u8, a.a3 as u32) }
fn sys_fstatat(a: &SyscallArgs) -> u64 { fs::sys_fstatat(a.a1 as i64, a.a2 as *const u8, a.a3 as *mut crate::vfs::Stat, a.a4 as i32) }
fn sys_unlinkat(a: &SyscallArgs) -> u64 { fs::sys_unlinkat(a.a1 as i64, a.a2 as *const u8, a.a3 as i32) }
fn sys_renameat(a: &SyscallArgs) -> u64 { fs::sys_renameat(a.a1 as i64, a.a2 as *const u8, a.a3 as i64, a.a4 as *const u8) }
fn sys_linkat(a: &SyscallArgs) -> u64 { fs::sys_linkat(a.a1 as i64, a.a2 as *const u8, a.a3 as i64, a.a4 as *const u8, a.a5 as i32) }
fn sys_symlinkat(a: &SyscallArgs) -> u64 { fs::sys_symlinkat(a.a2 as *const u8, a.a1 as i64, a.a3 as *const u8) }
fn sys_readlinkat(a: &SyscallArgs) -> u64 { fs::sys_readlinkat(a.a1 as i64, a.a2 as *const u8, a.a3 as *mut u8, a.a4) }
fn sys_faccessat(a: &SyscallArgs) -> u64 { fs::sys_faccessat(a.a1 as i64, a.a2 as *const u8, a.a3 as i32, a.a4 as i32) }
fn sys_fcntl(a: &SyscallArgs) -> u64 { fs::sys_fcntl(a.a1, a.a2 as i32, a.a3) }
fn sys_pipe(a: &SyscallArgs) -> u64 { fs::sys_pipe(a.a1 as *mut u32) }
fn sys_uname(a: &SyscallArgs) -> u64 { process::sys_uname(a.a1 as *mut UtsName) }
fn sys_wait4(a: &SyscallArgs) -> u64 { process::sys_wait4(a.a1 as i64, a.a2 as *mut i32, a.a3 as i32, a.a4 as *mut u8) }
fn sys_execve(a: &SyscallArgs) -> u64 { process::sys_execve(a.a1 as *const u8, a.a2 as *const *const u8, a.a3 as *const *const u8, a.regs) }
fn sys_socket(a: &SyscallArgs) -> u64 { net::sys_socket(a.a1, a.a2, a.a3) }
fn sys_bind(a: &SyscallArgs) -> u64 { net::sys_bind(a.a1, a.a2 as *const u8, a.a3) }
fn sys_connect(a: &SyscallArgs) -> u64 { net::sys_connect(a.a1, a.a2 as *const u8, a.a3) }
fn sys_listen(a: &SyscallArgs) -> u64 { net::sys_listen(a.a1, a.a2) }
fn sys_accept(a: &SyscallArgs) -> u64 { net::sys_accept(a.a1, a.a2 as *mut u8, a.a3 as *mut u32) }
fn sys_sendto(a: &SyscallArgs) -> u64 { net::sys_sendto(a.a1, a.a2 as *const u8, a.a3, a.a4 as *const u8, a.a5) }
fn sys_recvfrom(a: &SyscallArgs) -> u64 { net::sys_recvfrom(a.a1, a.a2 as *mut u8, a.a3, a.a4 as *mut u8, a.a5 as *mut u32) }
fn sys_setsockopt(a: &SyscallArgs) -> u64 { net::sys_setsockopt(a.a1, a.a2 as i32, a.a3 as i32, a.a4 as *const u8, a.a5) }
fn sys_getsockname(a: &SyscallArgs) -> u64 { net::sys_getsockname(a.a1, a.a2 as *mut u8, a.a3 as *mut u32) }
fn sys_getpeername(a: &SyscallArgs) -> u64 { misc::sys_getpeername(a.a1, a.a2 as *mut u8, a.a3 as *mut u32) }
fn sys_getsockopt(a: &SyscallArgs) -> u64 { net::sys_getsockopt(a.a1, a.a2 as i32, a.a3 as i32, a.a4 as *mut u8, a.a5 as *mut u32) }
fn sys_socket_pair(a: &SyscallArgs) -> u64 { net::sys_socketpair(a.a1, a.a2, a.a3, a.a4 as *mut i32) }
fn sys_nanosleep(a: &SyscallArgs) -> u64 { process::sys_nanosleep(a.a1, a.a2) }
fn sys_getcwd(a: &SyscallArgs) -> u64 { fs::sys_getcwd(a.a1 as *mut u8, a.a2 as usize) }
fn sys_chdir(a: &SyscallArgs) -> u64 { fs::sys_chdir(a.a1 as *const u8) }
fn sys_mkdir(a: &SyscallArgs) -> u64 { fs::sys_mkdir(a.a1 as *const u8, a.a2 as u32) }
fn sys_unlink(a: &SyscallArgs) -> u64 { fs::sys_unlink(a.a1 as *const u8) }
fn sys_link(a: &SyscallArgs) -> u64 { fs::sys_link(a.a1 as *const u8, a.a2 as *const u8) }
fn sys_symlink(a: &SyscallArgs) -> u64 { fs::sys_symlink(a.a1 as *const u8, a.a2 as *const u8) }
fn sys_readlink(a: &SyscallArgs) -> u64 { fs::sys_readlink(a.a1 as *const u8, a.a2 as *mut u8, a.a3) }
fn sys_rename(a: &SyscallArgs) -> u64 { fs::sys_rename(a.a1 as *const u8, a.a2 as *const u8) }
fn sys_kill(a: &SyscallArgs) -> u64 { process::sys_kill(a.a1 as i64, a.a2 as u32) }
fn sys_futex(a: &SyscallArgs) -> u64 {
    let val3 = unsafe { *a.regs.add(6) as u32 };
    crate::syscalls::futex::sys_futex(a.a1 as *mut u32, a.a2 as u32, a.a3 as u32, a.a4 as u32, a.a5 as *mut u32, val3)
}
fn sys_sysinfo(a: &SyscallArgs) -> u64 { process::sys_sysinfo(a.a1 as *mut u64) }
fn sys_rt_sigaction(a: &SyscallArgs) -> u64 { process::sys_rt_sigaction(a.a1, a.a2 as *const u64, a.a3 as *mut u64, a.a4) }
fn sys_rt_sigreturn(a: &SyscallArgs) -> u64 { process::sys_rt_sigreturn(a.regs) }
fn sys_sched_yield(a: &SyscallArgs) -> u64 { process::sys_sched_yield() }
fn sys_sched_setattr(a: &SyscallArgs) -> u64 { process::sys_sched_setattr(a.a1 as i64, a.a2 as *const u8, a.a3) }
fn sys_sched_getattr(a: &SyscallArgs) -> u64 { process::sys_sched_getattr(a.a1 as i64, a.a2 as *mut u8, a.a3, a.a4) }
fn sys_sched_setaffinity(a: &SyscallArgs) -> u64 { process::sys_sched_setaffinity(a.a1 as i64, a.a2, a.a3) }
fn sys_sched_getaffinity(a: &SyscallArgs) -> u64 { process::sys_sched_getaffinity(a.a1 as i64, a.a2, a.a3) }
fn sys_getdents64(a: &SyscallArgs) -> u64 { fs::sys_getdents64(a.a1, a.a2 as *mut u8, a.a3 as usize) }
fn sys_ioctl(a: &SyscallArgs) -> u64 { fs::sys_ioctl(a.a1, a.a2, a.a3 as *mut u8) }
fn sys_clock_gettime(a: &SyscallArgs) -> u64 { misc::sys_clock_gettime(a.a1, a.a2 as *mut Timespec) }
fn sys_clock_getres(a: &SyscallArgs) -> u64 { misc::sys_clock_getres(a.a1, a.a2 as *mut Timespec) }
fn sys_clock_nanosleep(a: &SyscallArgs) -> u64 { misc::sys_clock_nanosleep(a.a1, a.a2, a.a3 as *const Timespec, a.a4 as *mut Timespec) }
fn sys_mount(a: &SyscallArgs) -> u64 { fs::sys_mount(a.a1 as *const u8, a.a2 as *const u8, a.a3 as *const u8, a.a4, a.a5 as *const u8) }
fn sys_umount2(a: &SyscallArgs) -> u64 { fs::sys_umount2(a.a1 as *const u8, a.a2) }
fn sys_mkfs(a: &SyscallArgs) -> u64 { fs::sys_mkfs(a.a1 as *const u8, a.a2) }
fn sys_utimensat(a: &SyscallArgs) -> u64 { fs::sys_utimensat(a.a1 as i64, a.a2 as *const u8, a.a3 as *const u8, a.a4 as i32) }
fn sys_fallocate(a: &SyscallArgs) -> u64 { fs::sys_fallocate(a.a1, a.a2 as i32, a.a3 as i64, a.a4 as i64) }
fn sys_chmod(a: &SyscallArgs) -> u64 { fs::sys_chmod(a.a1 as *const u8, a.a2 as u32) }
fn sys_fchmod(a: &SyscallArgs) -> u64 { fs::sys_fchmod(a.a1, a.a2 as u32) }
fn sys_chown(a: &SyscallArgs) -> u64 { fs::sys_chown(a.a1 as *const u8, a.a2 as u32, a.a3 as u32) }
fn sys_fchown(a: &SyscallArgs) -> u64 { fs::sys_fchown(a.a1, a.a2 as u32, a.a3 as u32) }
fn sys_umask(a: &SyscallArgs) -> u64 { fs::sys_umask(a.a1 as u32) }
fn sys_getrlimit(a: &SyscallArgs) -> u64 { process::sys_getrlimit(a.a1, a.a2 as *mut u8) }
fn sys_setrlimit(a: &SyscallArgs) -> u64 { process::sys_setrlimit(a.a1, a.a2 as *const u8) }
fn sys_prlimit64(a: &SyscallArgs) -> u64 { process::sys_prlimit64(a.a1, a.a2, a.a3 as *const u8, a.a4 as *mut u8) }
fn sys_getrusage(a: &SyscallArgs) -> u64 { process::sys_getrusage(a.a1, a.a2 as *mut u8) }
fn sys_arch_prctl(a: &SyscallArgs) -> u64 { process::sys_arch_prctl(a.a1, a.a2) }
fn sys_select(a: &SyscallArgs) -> u64 { misc::sys_select(a.a1, a.a2 as *mut u64, a.a3 as *mut u64, a.a4 as *mut u64, a.a5 as *const u64) }
fn sys_poll(a: &SyscallArgs) -> u64 { misc::sys_poll(a.a1 as *const u8, a.a2 as usize, a.a3 as i32) }
fn sys_getuid(a: &SyscallArgs) -> u64 { process::sys_getuid() }
fn sys_getgid(a: &SyscallArgs) -> u64 { process::sys_getgid() }
fn sys_setuid(a: &SyscallArgs) -> u64 { process::sys_setuid(a.a1) }
fn sys_setgid(a: &SyscallArgs) -> u64 { process::sys_setgid(a.a1) }
fn sys_geteuid(a: &SyscallArgs) -> u64 { process::sys_geteuid() }
fn sys_getegid(a: &SyscallArgs) -> u64 { process::sys_getegid() }
fn sys_getresuid(a: &SyscallArgs) -> u64 { process::sys_getresuid(a.a1 as *mut u32, a.a2 as *mut u32, a.a3 as *mut u32) }
fn sys_setresuid(a: &SyscallArgs) -> u64 { process::sys_setresuid(a.a1 as u32, a.a2 as u32, a.a3 as u32) }
fn sys_getresgid(a: &SyscallArgs) -> u64 { process::sys_getresgid(a.a1 as *mut u32, a.a2 as *mut u32, a.a3 as *mut u32) }
fn sys_setresgid(a: &SyscallArgs) -> u64 { process::sys_setresgid(a.a1 as u32, a.a2 as u32, a.a3 as u32) }
fn sys_getgroups(a: &SyscallArgs) -> u64 { process::sys_getgroups(a.a1 as i32, a.a2 as *mut u32) }
fn sys_setgroups(a: &SyscallArgs) -> u64 { process::sys_setgroups(a.a1 as i64, a.a2 as *const u32) }
fn sys_capget(a: &SyscallArgs) -> u64 { process::sys_capget(a.a1 as *mut u8, a.a2 as *mut u8) }
fn sys_capset(a: &SyscallArgs) -> u64 { process::sys_capset(a.a1 as *const u8, a.a2 as *const u8) }
fn sys_rt_sigprocmask(a: &SyscallArgs) -> u64 { process::sys_sigprocmask(a.a1 as i32, a.a2 as *const u64, a.a3 as *mut u64) }
fn sys_io_uring_setup(a: &SyscallArgs) -> u64 { io_uring::sys_io_uring_setup(a.a1, a.a2) }
fn sys_io_uring_enter(a: &SyscallArgs) -> u64 { io_uring::sys_io_uring_enter(a.a1, a.a2 as u32, a.a3 as u32, a.a4 as u32, a.a5) }
fn sys_io_uring_register(a: &SyscallArgs) -> u64 { io_uring::sys_io_uring_register(a.a1, a.a2 as u32, a.a3, a.a4 as u32) }
fn sys_bpf(a: &SyscallArgs) -> u64 { crate::ebpf::sys_bpf(a.a1 as u32, a.a2, a.a3, a.a4) }
fn sys_sync(a: &SyscallArgs) -> u64 { fs::sys_sync() }
fn sys_reboot(a: &SyscallArgs) -> u64 { misc::sys_reboot(a.a1, a.a2) }
fn sys_drmctl(a: &SyscallArgs) -> u64 { gui::sys_drmctl(a.a1, a.a2, a.a3 as *mut u8) }
fn sys_hash(a: &SyscallArgs) -> u64 { misc::sys_hash(a.a1, a.a2 as *const u8, a.a3, a.a4 as *mut u8, a.a5) }
fn sys_getrandom(a: &SyscallArgs) -> u64 { misc::sys_getrandom(a.a1 as *mut u8, a.a2 as usize, a.a3) }
fn sys_statfs(a: &SyscallArgs) -> u64 { fs::sys_statfs(a.a1 as *const u8, a.a2 as *mut u8) }
fn sys_openpty(a: &SyscallArgs) -> u64 { gui::sys_openpty() }
fn sys_set_tid_address(a: &SyscallArgs) -> u64 { process::sys_set_tid_address(a.a1 as *const u32) }
fn sys_exit_group(a: &SyscallArgs) -> u64 { process::sys_exit_group(a.a1) }
fn sys_truncate(a: &SyscallArgs) -> u64 { fs::sys_truncate(a.a1 as *const u8, a.a2 as i64) }
fn sys_ftruncate(a: &SyscallArgs) -> u64 { fs::sys_ftruncate(a.a1, a.a2 as i64) }
fn sys_sendfile(a: &SyscallArgs) -> u64 { fs::sys_sendfile(a.a1, a.a2, a.a3 as *mut u64, a.a4) }
fn sys_sigaltstack(a: &SyscallArgs) -> u64 { process::sys_sigaltstack(a.a1 as *const u8, a.a2 as *mut u8) }
fn sys_getitimer(a: &SyscallArgs) -> u64 { process::sys_getitimer(a.a1, a.a2 as *mut u8) }
fn sys_setitimer(a: &SyscallArgs) -> u64 { process::sys_setitimer(a.a1, a.a2 as *const u8, a.a3 as *mut u8) }
fn sys_times(a: &SyscallArgs) -> u64 { process::sys_times(a.a1 as *mut u8) }
fn sys_signalfd(a: &SyscallArgs) -> u64 { process::sys_signalfd(a.a1, a.a2 as *const u64, a.a3) }
fn sys_signalfd4(a: &SyscallArgs) -> u64 { process::sys_signalfd4(a.a1, a.a2 as *const u64, a.a3, a.a4 as i32) }
fn sys_eventfd(a: &SyscallArgs) -> u64 { eventfd::sys_eventfd2(a.a1 as u32, 0) }
fn sys_eventfd2(a: &SyscallArgs) -> u64 { eventfd::sys_eventfd2(a.a1 as u32, a.a2 as i32) }
fn sys_pause(a: &SyscallArgs) -> u64 { process::sys_pause() }
fn sys_timer_create(a: &SyscallArgs) -> u64 { posix_timers::sys_timer_create(a.a1 as i32, a.a2 as *const posix_timers::sigevent, a.a3 as *mut i32) }
fn sys_timer_settime(a: &SyscallArgs) -> u64 { posix_timers::sys_timer_settime(a.a1 as i32, a.a2 as i32, a.a3 as *const posix_timers::itimerspec, a.a4 as *mut posix_timers::itimerspec) }
fn sys_timer_gettime(a: &SyscallArgs) -> u64 { posix_timers::sys_timer_gettime(a.a1 as i32, a.a2 as *mut posix_timers::itimerspec) }
fn sys_timer_getoverrun(a: &SyscallArgs) -> u64 { posix_timers::sys_timer_getoverrun(a.a1 as i32) }
fn sys_timer_delete(a: &SyscallArgs) -> u64 { posix_timers::sys_timer_delete(a.a1 as i32) }
fn sys_timerfd_create(a: &SyscallArgs) -> u64 { timerfd::sys_timerfd_create(a.a1, a.a2) }
fn sys_timerfd_settime(a: &SyscallArgs) -> u64 { timerfd::sys_timerfd_settime(a.a1, a.a2, a.a3 as *const u8, a.a4 as *mut u8) }
fn sys_timerfd_gettime(a: &SyscallArgs) -> u64 { timerfd::sys_timerfd_gettime(a.a1, a.a2 as *mut u8) }
fn sys_inotify_init(a: &SyscallArgs) -> u64 { inotify::sys_inotify_init(a.a1) }
fn sys_inotify_add_watch(a: &SyscallArgs) -> u64 { inotify::sys_inotify_add_watch(a.a1, a.a2 as *const u8, a.a3 as u32) }
fn sys_inotify_rm_watch(a: &SyscallArgs) -> u64 { inotify::sys_inotify_rm_watch(a.a1, a.a3 as u32) }
fn sys_recvmmsg(a: &SyscallArgs) -> u64 { mmsg::sys_recvmmsg(a.a1, a.a2 as *mut mmsg::mmsghdr, a.a3, a.a4, a.a5 as *const u8) }
fn sys_sendmmsg(a: &SyscallArgs) -> u64 { mmsg::sys_sendmmsg(a.a1, a.a2 as *mut mmsg::mmsghdr, a.a3, a.a4) }
fn sys_shmget(a: &SyscallArgs) -> u64 { shm::sys_shmget(a.a1 as i32, a.a2 as usize, a.a3 as i32) }
fn sys_shmat(a: &SyscallArgs) -> u64 { shm::sys_shmat(a.a1 as i32, a.a2 as *const u8, a.a3 as i32) }
fn sys_shmctl(a: &SyscallArgs) -> u64 { shm::sys_shmctl(a.a1 as i32, a.a2 as i32, a.a3 as *mut u8) }
fn sys_shmdt(a: &SyscallArgs) -> u64 { shm::sys_shmdt(a.a1 as *const u8) }
fn sys_memfd_create(a: &SyscallArgs) -> u64 { shm::sys_memfd_create(a.a1 as *const u8, a.a2 as u32) }
fn sys_swapon(a: &SyscallArgs) -> u64 { fs::sys_swapon(a.a1 as *const u8, a.a2 as i32) }
fn sys_swapoff(a: &SyscallArgs) -> u64 { fs::sys_swapoff(a.a1 as *const u8) }
fn sys_epoll_create(a: &SyscallArgs) -> u64 { epoll::sys_epoll_create(a.a1 as i32) }
fn sys_epoll_create1(a: &SyscallArgs) -> u64 { epoll::sys_epoll_create1(a.a1 as i32) }
fn sys_epoll_ctl(a: &SyscallArgs) -> u64 { epoll::sys_epoll_ctl(a.a1, a.a2 as i32, a.a3 as i32, a.a4 as *const u8) }
fn sys_epoll_wait(a: &SyscallArgs) -> u64 { epoll::sys_epoll_wait(a.a1, a.a2 as *mut u8, a.a3 as i32, a.a4 as i32) }
fn sys_epoll_pwait(a: &SyscallArgs) -> u64 { epoll::sys_epoll_pwait(a.a1, a.a2 as *mut u8, a.a3 as i32, a.a4 as i32, a.a5 as *const u8, 0) }
fn sys_readv(a: &SyscallArgs) -> u64 { compat::sys_readv(a.a1, a.a2 as *const u8, a.a3 as i64) }
fn sys_writev(a: &SyscallArgs) -> u64 { compat::sys_writev(a.a1, a.a2 as *const u8, a.a3 as i64) }
fn sys_madvise(a: &SyscallArgs) -> u64 { compat::sys_madvise(a.a1, a.a2, a.a3) }
fn sys_pipe2(a: &SyscallArgs) -> u64 { compat::sys_pipe2(a.a1 as *mut u32, a.a2 as i32) }
fn sys_dup3(a: &SyscallArgs) -> u64 { compat::sys_dup3(a.a1, a.a2, a.a3 as i32) }
fn sys_pread64(a: &SyscallArgs) -> u64 { compat::sys_pread64(a.a1, a.a2 as *mut u8, a.a3 as usize, a.a4) }
fn sys_pwrite64(a: &SyscallArgs) -> u64 { compat::sys_pwrite64(a.a1, a.a2 as *const u8, a.a3 as usize, a.a4) }
fn sys_prctl(a: &SyscallArgs) -> u64 { prctl::sys_prctl(a.a1, a.a2, a.a3, a.a4, a.a5) }
fn sys_seccomp(a: &SyscallArgs) -> u64 { seccomp::sys_seccomp(a.a1 as u32, a.a2 as u32, a.a3 as *const u8) }
fn sys_landlock_create_ruleset(a: &SyscallArgs) -> u64 { landlock::sys_landlock_create_ruleset(a.a1 as *const u8, a.a2 as usize, a.a3 as u32) }
fn sys_landlock_add_rule(a: &SyscallArgs) -> u64 { landlock::sys_landlock_add_rule(a.a1, a.a2 as u32, a.a3 as *const u8, a.a4 as u32) }
fn sys_landlock_restrict_self(a: &SyscallArgs) -> u64 { landlock::sys_landlock_restrict_self(a.a1, a.a2 as u32) }
fn sys_unshare(a: &SyscallArgs) -> u64 { namespaces::sys_unshare(a.a1) }fn sys_setns(a: &SyscallArgs) -> u64 { namespaces::sys_setns(a.a1, a.a2) }
fn sys_mq_open(a: &SyscallArgs) -> u64 { mqueue::mq_open(a.a1 as *const u8, a.a2 as i32, a.a3 as i32, a.a4 as *mut u8) }
fn sys_mq_close(a: &SyscallArgs) -> u64 { mqueue::mq_close(a.a1 as i32) }
fn sys_mq_send(a: &SyscallArgs) -> u64 { mqueue::mq_send(a.a1 as i32, a.a2 as *const u8, a.a3 as usize, a.a4 as u32) }
fn sys_mq_receive(a: &SyscallArgs) -> u64 { mqueue::mq_receive(a.a1 as i32, a.a2 as *mut u8, a.a3 as usize, a.a4 as *mut u32) }
fn sys_mq_unlink(a: &SyscallArgs) -> u64 { mqueue::mq_unlink(a.a1 as *const u8) }

fn sys_cgroup_mkdir(a: &SyscallArgs) -> u64 { cgroup::sys_cgroup_mkdir(a.a1 as *const u8) }
fn sys_cgroup_write(a: &SyscallArgs) -> u64 { cgroup::sys_cgroup_write(a.a1 as *const u8, a.a2 as *const u8, a.a3 as *const u8) }
fn sys_cgroup_read(a: &SyscallArgs) -> u64 { cgroup::sys_cgroup_read(a.a1 as *const u8, a.a2 as *const u8, a.a3 as *mut u8) }
fn sys_resolve(a: &SyscallArgs) -> u64 { misc::sys_resolve(a.a1 as *const u8, a.a2 as *mut u8) }
fn sys_objmgr_enum(a: &SyscallArgs) -> u64 { misc::sys_objmgr_enum(a.a1 as *mut u8, a.a2 as usize) }
fn sys_objmgr_audit(a: &SyscallArgs) -> u64 { misc::sys_objmgr_audit(a.a1, a.a2 as *mut u8, a.a3 as usize) }
fn sys_beep(a: &SyscallArgs) -> u64 { gui::sys_beep(a.a1 as u32, a.a2 as u32) }
fn sys_sendmsg(a: &SyscallArgs) -> u64 { net::sys_sendmsg(a.a1 as i64, a.a2 as *const msghdr, a.a3 as i32) }
fn sys_recvmsg(a: &SyscallArgs) -> u64 { net::sys_recvmsg(a.a1 as i64, a.a2 as *mut msghdr, a.a3 as i32) }
fn sys_gui_create_window(a: &SyscallArgs) -> u64 { gui::sys_gui_create_window(a.a1 as *const u8, a.a2 as usize, a.a3 as usize) }
fn sys_gui_get_buffer(a: &SyscallArgs) -> u64 { gui::sys_gui_get_buffer(a.a1) }
fn sys_gui_flush(a: &SyscallArgs) -> u64 { gui::sys_gui_flush(a.a1, a.a2 as *const u32) }
fn sys_gui_map_buffer(a: &SyscallArgs) -> u64 { gui::sys_gui_map_buffer(a.a1) }
fn sys_gui_get_key(a: &SyscallArgs) -> u64 { gui::sys_gui_get_key(a.a1) }
fn sys_gui_get_mouse(a: &SyscallArgs) -> u64 { gui::sys_gui_get_mouse(a.a1) }
fn sys_gui_set_title(a: &SyscallArgs) -> u64 { gui::sys_gui_set_title(a.a1, a.a2 as *const u8) }
fn sys_gui_destroy_window(a: &SyscallArgs) -> u64 { gui::sys_gui_destroy_window(a.a1) }
fn sys_gui_resize_window(a: &SyscallArgs) -> u64 { gui::sys_gui_resize_window(a.a1, a.a2, a.a3) }
fn sys_gui_move_window(a: &SyscallArgs) -> u64 { gui::sys_gui_move_window(a.a1, a.a2, a.a3) }
fn sys_clipboard(a: &SyscallArgs) -> u64 { gui::sys_clipboard(a.a1, a.a2 as *mut u8, a.a3) }
fn sys_notify(a: &SyscallArgs) -> u64 { gui::sys_notify(a.a1 as *const u8, a.a2, a.a3) }

#[cfg(feature = "ash")]
fn sys_ash_register(a: &SyscallArgs) -> u64 { crate::ash::syscalls::sys_ash_register(a.a1 as *const u8, a.a2 as usize, a.a3) }
#[cfg(feature = "ash")]
fn sys_ash_unregister(a: &SyscallArgs) -> u64 { crate::ash::syscalls::sys_ash_unregister(a.a1) }
#[cfg(feature = "ash")]
fn sys_ash_stats(a: &SyscallArgs) -> u64 { crate::ash::syscalls::sys_ash_stats(a.a1, a.a2 as *mut crate::ash::AshStats) }
#[cfg(feature = "ash")]
fn sys_ash_control(a: &SyscallArgs) -> u64 { crate::ash::syscalls::sys_ash_control(a.a1) }

#[cfg(feature = "hypervisor")]
fn sys_vm_create(a: &SyscallArgs) -> u64 { misc::sys_vm_create(a.a1 as *const u8, a.a2) }
#[cfg(feature = "hypervisor")]
fn sys_vm_destroy(a: &SyscallArgs) -> u64 { misc::sys_vm_destroy(a.a1) }
#[cfg(feature = "hypervisor")]
fn sys_vm_start(a: &SyscallArgs) -> u64 { misc::sys_vm_start(a.a1) }
#[cfg(feature = "hypervisor")]
fn sys_vm_stop(a: &SyscallArgs) -> u64 { misc::sys_vm_stop(a.a1) }
#[cfg(feature = "hypervisor")]
fn sys_vm_pause(a: &SyscallArgs) -> u64 { process::sys_vm_pause(a.a1) }
#[cfg(feature = "hypervisor")]
fn sys_vm_resume(a: &SyscallArgs) -> u64 { misc::sys_vm_resume(a.a1) }
#[cfg(feature = "hypervisor")]
fn sys_vm_load_kernel(a: &SyscallArgs) -> u64 { misc::sys_vm_load_kernel(a.a1, a.a2 as *const u8) }
#[cfg(feature = "hypervisor")]
fn sys_vm_get_info(a: &SyscallArgs) -> u64 { misc::sys_vm_get_info(a.a1, a.a2 as *mut u8, a.a3 as usize) }
#[cfg(feature = "hypervisor")]
fn sys_vm_set_memory(a: &SyscallArgs) -> u64 { misc::sys_vm_set_memory(a.a1, a.a2, a.a3) }
#[cfg(feature = "hypervisor")]
fn sys_vm_inject_irq(a: &SyscallArgs) -> u64 { misc::sys_vm_inject_irq(a.a1, a.a2 as u8) }

// ─── Main dispatch entry ────────────────────────────────────────

#[no_mangle]
pub extern "sysv64" fn syscall_handler(
    n: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    regs_ptr: *mut u64,
) -> u64 {
    let is_linux = {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| *p.emulation.lock() == crate::task::process::EmulationMode::Linux).unwrap_or(false)
    };
    if is_linux {
        return crate::emulation::dispatch_linux_syscall(n, arg1, arg2, arg3, arg4, arg5, regs_ptr);
    }

    do_syscall(n, arg1, arg2, arg3, arg4, arg5, regs_ptr)
}

pub fn do_syscall(
    n: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    regs_ptr: *mut u64,
) -> u64 {
    #[cfg(feature = "ash")]
    {
        match crate::ash::hooks::syscall::hook_syscall_entry(n, arg1, arg2, arg3) {
            crate::ash::AshResult::Drop | crate::ash::AshResult::Handled => {
                return errno::Errno::EPERM as u64;
            }
            _ => {}
        }
    }

    if !seccomp::check_syscall(n, &[arg1, arg2, arg3, arg4, arg5, 0]) {
        return errno::Errno::EPERM as u64;
    }

    let args = SyscallArgs { n, a1: arg1, a2: arg2, a3: arg3, a4: arg4, a5: arg5, regs: regs_ptr };

    let result = if (n as usize) < TABLE_SIZE {
        match SYSCALL_TABLE[n as usize] {
            Some(handler) => handler(&args),
            None => {
                crate::println!("[SYSCALL] Unknown syscall: {} (0x{:x})", n, n);
                errno::Errno::ENOSYS as u64
            }
        }
    } else {
        crate::println!("[SYSCALL] Unknown syscall: {} (0x{:x})", n, n);
        errno::Errno::ENOSYS as u64
    };

    // ─── Signal delivery (unchanged) ──────────────────────────
    {
        let process_arc = match get_current_process() {
            Some(p) => p,
            None => return result,
        };
        let (handler, restorer, sig_num, sig_bit) = {
            let mut signals = process_arc.signals.lock();
            if !signals.has_unmasked_pending(signals.blocked) { return result; }

            let available = signals.pending & !signals.blocked;
            let sig_bit = available.trailing_zeros();
            let sig_num = sig_bit + 1;
            let handler = process_arc.signal_handlers.lock()[sig_bit as usize];
            let sa_restorer = process_arc.signal_restorers.lock()[sig_bit as usize];
            let restorer = super::signal::get_restorer(handler, sa_restorer);

            if handler == 1 {
                signals.pending &= !(1 << sig_bit);
                return result;
            }

            drop(signals);
            (handler, restorer, sig_num, sig_bit)
        };

        if handler == 0 {
            sys_exit_inner(128 + sig_num as u64);
        } else {
            let old_rsp = unsafe { *regs_ptr.add(17) };
            let old_rip = unsafe { *regs_ptr.add(15) };
            let old_rflags = unsafe { *regs_ptr.add(16) };

            let ret_addr_rsp = old_rsp - 8;
            let frame_size = core::mem::size_of::<SignalFrame>();
            let new_rsp = (ret_addr_rsp - frame_size as u64) & !0xF;

            let phys = match crate::memory::virt_to_phys(x86_64::VirtAddr::new(new_rsp)) {
                Some(p) => p,
                None => {
                    crate::serial_write("[SIGNAL] invalid user stack, killing process\n");
                    sys_exit_inner(128 + sig_num as u64);
                    unreachable!();
                }
            };
            let k_ptr = (crate::memory::physical_memory_offset() + phys.as_u64()) as *mut SignalFrame;

            unsafe {
                (*k_ptr).r15 = *regs_ptr.add(0);
                (*k_ptr).r14 = *regs_ptr.add(1);
                (*k_ptr).r13 = *regs_ptr.add(2);
                (*k_ptr).r12 = *regs_ptr.add(3);
                (*k_ptr).r11 = *regs_ptr.add(4);
                (*k_ptr).r10 = *regs_ptr.add(5);
                (*k_ptr).r9  = *regs_ptr.add(6);
                (*k_ptr).r8  = *regs_ptr.add(7);
                (*k_ptr).rdi = *regs_ptr.add(8);
                (*k_ptr).rsi = *regs_ptr.add(9);
                (*k_ptr).rbp = *regs_ptr.add(10);
                (*k_ptr).rbx = *regs_ptr.add(11);
                (*k_ptr).rdx = *regs_ptr.add(12);
                (*k_ptr).rcx = *regs_ptr.add(13);
                (*k_ptr).rax = *regs_ptr.add(14);
                (*k_ptr).rip = old_rip;
                (*k_ptr).rflags = old_rflags;
                (*k_ptr).rsp = old_rsp;
            }

            // Copy the restorer trampoline code to the user stack.
            // If sa_restorer was set by the app, we write the pointer.
            // If using the default trampoline, we copy the actual bytes.
            {
                let ret_phys = match crate::memory::virt_to_phys(x86_64::VirtAddr::new(ret_addr_rsp)) {
                    Some(p) => p,
                    None => {
                        crate::serial_write("[SIGNAL] invalid user return stack, killing process\n");
                        sys_exit_inner(128 + sig_num as u64);
                        unreachable!();
                    }
                };
                let ret_kptr = (crate::memory::physical_memory_offset() + ret_phys.as_u64()) as *mut u64;
                if restorer != crate::syscalls::signal::SIGNAL_RESTORER.as_ptr() as u64 {
                    // App provided its own restorer — write the pointer.
                    unsafe { *ret_kptr = restorer; }
                } else {
                    // Kernel default trampoline — copy code bytes to user stack.
                    let code_ptr = super::signal::SIGNAL_RESTORER.as_ptr() as *const u8;
                    let code_len = super::signal::SIGNAL_RESTORER.len();
                    let dst = ret_kptr as *mut u8;
                    unsafe {
                        core::ptr::copy_nonoverlapping(code_ptr, dst, code_len);
                    }
                }
            }

            // Save FPU state before entering the signal handler.
            let saved_fpu = super::signal::save_fpu_state_for_signal();

            // Block signals during handler execution (SA_MASK semantics).
            {
                let mut signals = process_arc.signals.lock();
                signals.pending &= !(1 << sig_bit);
                // Block the delivered signal + SA_MASK during handler.
                signals.blocked |= 1 << sig_bit;
                signals.saved_context = Some(crate::syscalls::signal::SignalContext {
                    rip: old_rip,
                    rsp: new_rsp,
                    rbp: unsafe { *regs_ptr.add(10) },
                    rax: unsafe { *regs_ptr.add(14) },
                    rbx: unsafe { *regs_ptr.add(11) },
                    rcx: unsafe { *regs_ptr.add(13) },
                    rdx: unsafe { *regs_ptr.add(12) },
                    rsi: unsafe { *regs_ptr.add(9) },
                    rdi: unsafe { *regs_ptr.add(8) },
                    r8:  unsafe { *regs_ptr.add(7) },
                    r9:  unsafe { *regs_ptr.add(6) },
                    r10: unsafe { *regs_ptr.add(5) },
                    r11: unsafe { *regs_ptr.add(4) },
                    r12: unsafe { *regs_ptr.add(3) },
                    r13: unsafe { *regs_ptr.add(2) },
                    r14: unsafe { *regs_ptr.add(1) },
                    r15: unsafe { *regs_ptr.add(0) },
                    rflags: old_rflags,
                    fpu_state: None,
                });
            }

            // Store FPU state in saved context for rt_sigreturn to restore.
            if let Some(fpu) = saved_fpu {
                if let Some(ref mut ctx) = process_arc.signals.lock().saved_context {
                    ctx.fpu_state = Some(fpu);
                }
            }

            unsafe {
                *regs_ptr.add(17) = new_rsp;
                *regs_ptr.add(15) = handler;
                *regs_ptr.add(8) = sig_num as u64;
            }
        }
    }

    result
}

// Thin wrappers for signal delivery path
fn sys_exit_inner(code: u64) { process::sys_exit(code); }
fn sys_mmap_inner(a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, offset: u64) -> u64 {
    super::sys_mmap(a1, a2, a3, a4, a5, offset)
}
