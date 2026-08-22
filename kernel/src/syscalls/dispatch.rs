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
    // Detect SMAP and enable if available (must be done before any user access)
    user_access::init_smap();

    init_syscall_msrs();

    unsafe {
        use x86_64::registers::model_specific::Efer;
        Efer::update(|efer| efer.insert(x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS));
        
        // Setup GS base for BSP (CPU 0)
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

    // Allocate per-CPU data (leaked intentionally Ã¢â‚¬â€ lives forever)
    let data = alloc::boxed::Box::leak(alloc::boxed::Box::new(PerCpuData {
        self_ptr: 0, // will be set after allocation
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
    data.self_ptr = data as *mut PerCpuData as u64; // self-referential pointer
    
    let addr = x86_64::VirtAddr::from_ptr(data as *const _);
    KernelGsBase::write(addr);
    GsBase::write(addr); // Also set GS base for kernel-mode access if needed

    // Register in the global area table
    let mut areas = PER_CPU_AREAS.lock();
    if cpu_id >= areas.len() {
        areas.resize(cpu_id + 1, PerCpuPtr(core::ptr::null_mut()));
    }
    areas[cpu_id] = PerCpuPtr(data as *mut PerCpuData);
}

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
    // Check if the current process is in Linux emulation mode
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

    // Seccomp filter check — must run before the syscall is executed
    if !seccomp::check_syscall(n, &[arg1, arg2, arg3, arg4, arg5, 0]) {
        return errno::Errno::EPERM as u64;
    }

    let result = match n {
        numbers::SYS_READ => fs::sys_read(arg1, arg2 as *mut u8, arg3 as usize),
        numbers::SYS_WRITE => fs::sys_write(arg1, arg2 as *const u8, arg3 as usize),
        numbers::SYS_OPEN => fs::sys_open(arg1 as *const u8, arg2 as i32, arg3 as u32),
        numbers::SYS_CLOSE => fs::sys_close(arg1),
        numbers::SYS_STAT => fs::sys_stat(arg1 as *const u8, arg2 as *mut crate::vfs::Stat),
        numbers::SYS_LSTAT => fs::sys_lstat(arg1 as *const u8, arg2 as *mut crate::vfs::Stat),
        numbers::SYS_FSTAT => fs::sys_fstat(arg1, arg2 as *mut crate::vfs::Stat),
        numbers::SYS_LSEEK => fs::sys_lseek(arg1, arg2 as i64, arg3 as i32),
        numbers::SYS_MMAP => {
            let offset = unsafe { *regs_ptr.add(6) };
            sys_mmap(arg1, arg2, arg3, arg4, arg5, offset)
        }
        numbers::SYS_MPROTECT => fs::sys_mprotect(arg1, arg2, arg3),
        numbers::SYS_MUNMAP => fs::sys_munmap(arg1, arg2),
        numbers::SYS_BRK => fs::sys_brk(arg1),
        numbers::SYS_EXIT => process::sys_exit(arg1),
        numbers::SYS_CLONE => process::sys_clone(arg1, arg2, arg3 as *mut u32, arg4, arg5 as *mut u32, regs_ptr),
        numbers::SYS_FORK => process::sys_fork(regs_ptr),
        numbers::SYS_GETPID => process::sys_getpid(),
        numbers::SYS_GETPPID => process::sys_getppid(),
        numbers::SYS_SETPGID => process::sys_setpgid(arg1, arg2),
        numbers::SYS_GETPGID => process::sys_getpgid(arg1),
        numbers::SYS_GETPGRP => process::sys_getpgrp(),
        numbers::SYS_SETSID => process::sys_setsid(),
        numbers::SYS_GETSID => process::sys_getsid(arg1),
        numbers::SYS_DUP => fs::sys_dup(arg1),
        numbers::SYS_DUP2 => fs::sys_dup2(arg1, arg2),
        numbers::SYS_ACCESS => fs::sys_access(arg1 as *const u8, arg2 as i32),
        numbers::SYS_OPENAT => fs::sys_openat(arg1 as i64, arg2 as *const u8, arg3 as i32, arg4 as u32),
        numbers::SYS_MKDIRAT => fs::sys_mkdirat(arg1 as i64, arg2 as *const u8, arg3 as u32),
        numbers::SYS_FSTATAT => fs::sys_fstatat(arg1 as i64, arg2 as *const u8, arg3 as *mut crate::vfs::Stat, arg4 as i32),
        numbers::SYS_UNLINKAT => fs::sys_unlinkat(arg1 as i64, arg2 as *const u8, arg3 as i32),
        numbers::SYS_RENAMEAT => fs::sys_renameat(arg1 as i64, arg2 as *const u8, arg3 as i64, arg4 as *const u8),
        numbers::SYS_LINKAT => fs::sys_linkat(arg1 as i64, arg2 as *const u8, arg3 as i64, arg4 as *const u8, arg5 as i32),
        numbers::SYS_SYMLINKAT => fs::sys_symlinkat(arg1 as *const u8, arg2 as i64, arg3 as *const u8),
        numbers::SYS_READLINKAT => fs::sys_readlinkat(arg1 as i64, arg2 as *const u8, arg3 as *mut u8, arg4),
        numbers::SYS_FACCESSAT => fs::sys_faccessat(arg1 as i64, arg2 as *const u8, arg3 as i32, arg4 as i32),
        numbers::SYS_FCNTL => fs::sys_fcntl(arg1, arg2 as i32, arg3),
        numbers::SYS_PIPE => fs::sys_pipe(arg1 as *mut u32),
        numbers::SYS_UNAME => process::sys_uname(arg1 as *mut UtsName),
        numbers::SYS_WAIT4 => process::sys_wait4(arg1 as i64, arg2 as *mut i32, arg3 as i32, arg4 as *mut u8),
        numbers::SYS_EXECVE => process::sys_execve(arg1 as *const u8, arg2 as *const *const u8, arg3 as *const *const u8, regs_ptr),
        numbers::SYS_SOCKET => net::sys_socket(arg1, arg2, arg3),
        numbers::SYS_BIND => net::sys_bind(arg1, arg2 as *const u8, arg3),
        numbers::SYS_CONNECT => net::sys_connect(arg1, arg2 as *const u8, arg3),
        numbers::SYS_LISTEN => net::sys_listen(arg1, arg2),
        numbers::SYS_ACCEPT => net::sys_accept(arg1, arg2 as *mut u8, arg3 as *mut u32),
        numbers::SYS_SENDTO => net::sys_sendto(arg1, arg2 as *const u8, arg3, arg4 as *const u8, arg5),
        numbers::SYS_RECVFROM => net::sys_recvfrom(arg1, arg2 as *mut u8, arg3, arg4 as *mut u8, arg5 as *mut u32),
        numbers::SYS_SETSOCKOPT => net::sys_setsockopt(arg1, arg2 as i32, arg3 as i32, arg4 as *const u8, arg5),
        numbers::SYS_SENDMSG => net::sys_sendmsg(arg1 as i64, arg2 as *const msghdr, arg3 as i32),
        numbers::SYS_RECVMSG => net::sys_recvmsg(arg1 as i64, arg2 as *mut msghdr, arg3 as i32),
        numbers::SYS_GETSOCKNAME => net::sys_getsockname(arg1, arg2 as *mut u8, arg3 as *mut u32),
        numbers::SYS_GETPEERNAME => misc::sys_getpeername(arg1, arg2 as *mut u8, arg3 as *mut u32),
        numbers::SYS_GETSOCKOPT => net::sys_getsockopt(arg1, arg2 as i32, arg3 as i32, arg4 as *mut u8, arg5 as *mut u32),
        numbers::SYS_SOCKETPAIR => net::sys_socketpair(arg1, arg2, arg3, arg4 as *mut i32),
        
        numbers::SYS_GUI_CREATE_WINDOW => gui::sys_gui_create_window(arg1 as *const u8, arg2 as usize, arg3 as usize),
        numbers::SYS_GUI_GET_BUFFER => gui::sys_gui_get_buffer(arg1),
        numbers::SYS_GUI_FLUSH => gui::sys_gui_flush(arg1, arg2 as *const u32),
        numbers::SYS_GUI_MAP_BUFFER => gui::sys_gui_map_buffer(arg1),
        numbers::SYS_GUI_GET_KEY => gui::sys_gui_get_key(arg1),
        numbers::SYS_GUI_GET_MOUSE => gui::sys_gui_get_mouse(arg1),
        numbers::SYS_GUI_SET_TITLE => gui::sys_gui_set_title(arg1, arg2 as *const u8),
        numbers::SYS_GUI_DESTROY_WINDOW => gui::sys_gui_destroy_window(arg1),
        numbers::SYS_GUI_RESIZE_WINDOW => gui::sys_gui_resize_window(arg1, arg2, arg3),
        numbers::SYS_GUI_MOVE_WINDOW => gui::sys_gui_move_window(arg1, arg2, arg3),
        numbers::SYS_CLIPBOARD => gui::sys_clipboard(arg1, arg2 as *mut u8, arg3),
        numbers::SYS_NOTIFY => gui::sys_notify(arg1 as *const u8, arg2, arg3),
        numbers::SYS_NANOSLEEP => process::sys_nanosleep(arg1, arg2),
        
        numbers::SYS_GETCWD => fs::sys_getcwd(arg1 as *mut u8, arg2 as usize),
        numbers::SYS_CHDIR => fs::sys_chdir(arg1 as *const u8),
        numbers::SYS_MKDIR => fs::sys_mkdir(arg1 as *const u8, arg2 as u32),
        numbers::SYS_UNLINK => fs::sys_unlink(arg1 as *const u8),
        numbers::SYS_RESOLVE => misc::sys_resolve(arg1 as *const u8, arg2 as *mut u8),
        numbers::SYS_KILL => process::sys_kill(arg1 as i64, arg2 as u32),
        numbers::SYS_FUTEX => {
            // val3 = saved r9 at regs_ptr offset 6 (between r10 at +5 and r8 at +7)
            let val3 = unsafe { *regs_ptr.add(6) as u32 };
            crate::syscalls::futex::sys_futex(
                arg1 as *mut u32, arg2 as u32, arg3 as u32,
                arg4 as u32, arg5 as *mut u32, val3,
            )
        }
        numbers::SYS_SYSINFO => process::sys_sysinfo(arg1 as *mut u64),
        numbers::SYS_RT_SIGACTION => process::sys_rt_sigaction(arg1, arg2 as *const u64, arg3 as *mut u64, arg4),
        numbers::SYS_RT_SIGRETURN => process::sys_rt_sigreturn(regs_ptr),
        numbers::SYS_SCHED_YIELD => process::sys_sched_yield(),
        numbers::SYS_SCHED_SETATTR => process::sys_sched_setattr(arg1 as i64, arg2 as *const u8, arg3),
        numbers::SYS_SCHED_GETATTR => process::sys_sched_getattr(arg1 as i64, arg2 as *mut u8, arg3, arg4),
        numbers::SYS_GETDENTS64 => fs::sys_getdents64(arg1, arg2 as *mut u8, arg3 as usize),
        numbers::SYS_IOCTL => fs::sys_ioctl(arg1, arg2, arg3 as *mut u8),
        numbers::SYS_CLOCK_GETTIME => misc::sys_clock_gettime(arg1, arg2 as *mut Timespec),
        numbers::SYS_MOUNT => fs::sys_mount(arg1 as *const u8, arg2 as *const u8, arg3 as *const u8, arg4, arg5 as *const u8),
        numbers::SYS_UMOUNT2 => fs::sys_umount2(arg1 as *const u8, arg2),
        numbers::SYS_MKFS => fs::sys_mkfs(arg1 as *const u8, arg2),
        numbers::SYS_UTIMENSAT => fs::sys_utimensat(arg1 as i64, arg2 as *const u8, arg3 as *const u8, arg4 as i32),
        numbers::SYS_FALLOCATE => fs::sys_fallocate(arg1, arg2 as i32, arg3 as i64, arg4 as i64),
        numbers::SYS_CHMOD => fs::sys_chmod(arg1 as *const u8, arg2 as u32),
        numbers::SYS_FCHMOD => fs::sys_fchmod(arg1, arg2 as u32),
        numbers::SYS_CHOWN => fs::sys_chown(arg1 as *const u8, arg2 as u32, arg3 as u32),
        numbers::SYS_FCHOWN => fs::sys_fchown(arg1, arg2 as u32, arg3 as u32),
        numbers::SYS_UMASK => fs::sys_umask(arg1 as u32),
        numbers::SYS_GETRLIMIT => process::sys_getrlimit(arg1, arg2 as *mut u8),
        numbers::SYS_SETRLIMIT => process::sys_setrlimit(arg1, arg2 as *const u8),
        numbers::SYS_PRLIMIT64 => process::sys_prlimit64(arg1, arg2, arg3 as *const u8, arg4 as *mut u8),
        numbers::SYS_LINK => fs::sys_link(arg1 as *const u8, arg2 as *const u8),
        numbers::SYS_SYMLINK => fs::sys_symlink(arg1 as *const u8, arg2 as *const u8),
        numbers::SYS_READLINK => fs::sys_readlink(arg1 as *const u8, arg2 as *mut u8, arg3),
        numbers::SYS_RENAME => fs::sys_rename(arg1 as *const u8, arg2 as *const u8),
        numbers::SYS_ARCH_PRCTL => process::sys_arch_prctl(arg1, arg2),
        numbers::SYS_BEEP => gui::sys_beep(arg1 as u32, arg2 as u32),
        numbers::SYS_SELECT => misc::sys_select(arg1, arg2 as *mut u64, arg3 as *mut u64, arg4 as *mut u64, arg5 as *const u64),
        numbers::SYS_POLL => misc::sys_poll(arg1 as *const u8, arg2 as usize, arg3 as i32),
        numbers::SYS_GETUID => process::sys_getuid(),
        numbers::SYS_GETGID => process::sys_getgid(),
        numbers::SYS_SETUID => process::sys_setuid(arg1),
        numbers::SYS_SETGID => process::sys_setgid(arg1),
        numbers::SYS_GETEUID => process::sys_geteuid(),
        numbers::SYS_GETEGID => process::sys_getegid(),
        numbers::SYS_GETRESUID => process::sys_getresuid(arg1 as *mut u32, arg2 as *mut u32, arg3 as *mut u32),
        numbers::SYS_SETRESUID => process::sys_setresuid(arg1 as u32, arg2 as u32, arg3 as u32),
        numbers::SYS_GETRESGID => process::sys_getresgid(arg1 as *mut u32, arg2 as *mut u32, arg3 as *mut u32),
        numbers::SYS_SETRESGID => process::sys_setresgid(arg1 as u32, arg2 as u32, arg3 as u32),
        numbers::SYS_GETGROUPS => process::sys_getgroups(arg1 as i32, arg2 as *mut u32),
        numbers::SYS_SETGROUPS => process::sys_setgroups(arg1 as i64, arg2 as *const u32),
        numbers::SYS_CAPGET => process::sys_capget(arg1 as *mut u8, arg2 as *mut u8),
        numbers::SYS_CAPSET => process::sys_capset(arg1 as *const u8, arg2 as *const u8),
        numbers::SYS_SIGPROCMASK => process::sys_sigprocmask(arg1 as i32, arg2 as *const u64, arg3 as *mut u64),
        numbers::SYS_IO_URING_SETUP => io_uring::sys_io_uring_setup(arg1),
        numbers::SYS_IO_URING_ENTER => io_uring::sys_io_uring_enter(arg1, arg2, arg3, arg4, arg5),
        numbers::SYS_BPF => {
            crate::ebpf::sys_bpf(arg1 as u32, arg2, arg3, arg4)
        }
        numbers::SYS_SYNC => fs::sys_sync(),
        numbers::SYS_REBOOT => misc::sys_reboot(arg1, arg2),
        numbers::SYS_DRMCTL => gui::sys_drmctl(arg1, arg2, arg3 as *mut u8),
        numbers::SYS_HASH => misc::sys_hash(arg1, arg2 as *const u8, arg3, arg4 as *mut u8, arg5),
        numbers::SYS_STATFS => fs::sys_statfs(arg1 as *const u8, arg2 as *mut u8),
        numbers::SYS_OPENPTY => gui::sys_openpty(),
        numbers::SYS_SET_TID_ADDRESS => process::sys_set_tid_address(arg1 as *const u32),
        numbers::SYS_EXIT_GROUP => process::sys_exit_group(arg1),
        numbers::SYS_TRUNCATE => fs::sys_truncate(arg1 as *const u8, arg2 as i64),
        numbers::SYS_FTRUNCATE => fs::sys_ftruncate(arg1, arg2 as i64),
        numbers::SYS_SENDFILE => fs::sys_sendfile(arg1, arg2, arg3 as *mut u64, arg4),
        #[cfg(feature = "ash")]
        numbers::SYS_ASH_REGISTER => crate::ash::syscalls::sys_ash_register(arg1 as *const u8, arg2 as usize, arg3),
        #[cfg(feature = "ash")]
        numbers::SYS_ASH_UNREGISTER => crate::ash::syscalls::sys_ash_unregister(arg1),
        #[cfg(feature = "ash")]
        numbers::SYS_ASH_STATS => crate::ash::syscalls::sys_ash_stats(arg1, arg2 as *mut crate::ash::AshStats),
        #[cfg(feature = "ash")]
        numbers::SYS_ASH_CONTROL => crate::ash::syscalls::sys_ash_control(arg1),
        numbers::SYS_OBJMGR_ENUM => misc::sys_objmgr_enum(arg1 as *mut u8, arg2 as usize),
        numbers::SYS_OBJMGR_AUDIT => misc::sys_objmgr_audit(arg1, arg2 as *mut u8, arg3 as usize),
        numbers::SYS_PAUSE => process::sys_pause(),
        numbers::SYS_SIGALTSTACK => process::sys_sigaltstack(arg1 as *const u8, arg2 as *mut u8),
        numbers::SYS_GETITIMER => process::sys_getitimer(arg1, arg2 as *mut u8),
        numbers::SYS_SETITIMER => process::sys_setitimer(arg1, arg2 as *const u8, arg3 as *mut u8),
        numbers::SYS_TIMER_CREATE => posix_timers::sys_timer_create(arg1 as i32, arg2 as *const posix_timers::sigevent, arg3 as *mut i32),
        numbers::SYS_TIMER_SETTIME => posix_timers::sys_timer_settime(arg1 as i32, arg2 as i32, arg3 as *const posix_timers::itimerspec, arg4 as *mut posix_timers::itimerspec),
        numbers::SYS_TIMER_GETTIME => posix_timers::sys_timer_gettime(arg1 as i32, arg2 as *mut posix_timers::itimerspec),
        numbers::SYS_TIMER_GETOVERRUN => posix_timers::sys_timer_getoverrun(arg1 as i32),
        numbers::SYS_TIMER_DELETE => posix_timers::sys_timer_delete(arg1 as i32),
        numbers::SYS_TIMES => process::sys_times(arg1 as *mut u8),
        numbers::SYS_SIGNALFD => process::sys_signalfd4(arg1, arg2 as *const u64, 0),
        numbers::SYS_SIGNALFD4 => process::sys_signalfd4(arg1, arg2 as *const u64, arg3 as i32),
        numbers::SYS_EVENTFD => misc::sys_eventfd2(arg1 as u32, 0),
        numbers::SYS_EVENTFD2 => misc::sys_eventfd2(arg1 as u32, arg2 as i32),
        // Hypervisor syscalls
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_CREATE => misc::sys_vm_create(arg1 as *const u8, arg2),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_DESTROY => misc::sys_vm_destroy(arg1),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_START => misc::sys_vm_start(arg1),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_STOP => misc::sys_vm_stop(arg1),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_PAUSE => process::sys_vm_pause(arg1),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_RESUME => misc::sys_vm_resume(arg1),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_LOAD_KERNEL => misc::sys_vm_load_kernel(arg1, arg2 as *const u8),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_GET_INFO => misc::sys_vm_get_info(arg1, arg2 as *mut u8, arg3 as usize),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_SET_MEMORY => misc::sys_vm_set_memory(arg1, arg2, arg3),
        #[cfg(feature = "hypervisor")]
        numbers::SYS_VM_INJECT_IRQ => misc::sys_vm_inject_irq(arg1, arg2 as u8),

        // SysV shared memory
        numbers::SYS_SHMGET => shm::sys_shmget(arg1 as i32, arg2 as usize, arg3 as i32),
        numbers::SYS_SHMAT => shm::sys_shmat(arg1 as i32, arg2 as *const u8, arg3 as i32),
        numbers::SYS_SHMCTL => shm::sys_shmctl(arg1 as i32, arg2 as i32, arg3 as *mut u8),
        numbers::SYS_SHMDT => shm::sys_shmdt(arg1 as *const u8),
        numbers::SYS_MEMFD_CREATE => shm::sys_memfd_create(arg1 as *const u8, arg2 as u32),
        numbers::SYS_SWAPON => fs::sys_swapon(arg1 as *const u8, arg2 as i32),
        numbers::SYS_SWAPOFF => fs::sys_swapoff(arg1 as *const u8),
        
        // Phase 2: Linux-compatible syscalls
        numbers::SYS_EPOLL_CREATE1 => epoll::sys_epoll_create1(arg1 as i32),
        numbers::SYS_EPOLL_CTL => epoll::sys_epoll_ctl(arg1, arg2 as i32, arg3 as i32, arg4 as *const u8),
        numbers::SYS_EPOLL_WAIT => epoll::sys_epoll_wait(arg1, arg2 as *mut u8, arg3 as i32, arg4 as i32),
        numbers::SYS_EPOLL_PWAIT => epoll::sys_epoll_pwait(arg1, arg2 as *mut u8, arg3 as i32, arg4 as i32, arg5 as *const u8, 0),
        numbers::SYS_EPOLL_CREATE => epoll::sys_epoll_create(arg1 as i32),
        numbers::SYS_READV => compat::sys_readv(arg1, arg2 as *const u8, arg3 as i64),
        numbers::SYS_WRITEV => compat::sys_writev(arg1, arg2 as *const u8, arg3 as i64),
        numbers::SYS_MADVISE => compat::sys_madvise(arg1, arg2, arg3),
        numbers::SYS_PIPE2 => compat::sys_pipe2(arg1 as *mut u32, arg2 as i32),
        numbers::SYS_DUP3 => compat::sys_dup3(arg1, arg2, arg3 as i32),
        numbers::SYS_PREAD64 => compat::sys_pread64(arg1, arg2 as *mut u8, arg3 as usize, arg4),
        numbers::SYS_PWRITE64 => compat::sys_pwrite64(arg1, arg2 as *const u8, arg3 as usize, arg4),

        // Phase 5: Security syscalls
        numbers::SYS_PRCTL => prctl::sys_prctl(arg1, arg2, arg3, arg4, arg5),
        numbers::SYS_SECCOMP => seccomp::sys_seccomp(arg1 as u32, arg2 as u32, arg3 as *const u8),
        numbers::SYS_LANDLOCK_CREATE_RULESET => landlock::sys_landlock_create_ruleset(arg1 as *const u8, arg2 as usize, arg3 as u32),
        numbers::SYS_LANDLOCK_ADD_RULE => landlock::sys_landlock_add_rule(arg1, arg2 as u32, arg3 as *const u8, arg4 as u32),
        numbers::SYS_LANDLOCK_RESTRICT_SELF => landlock::sys_landlock_restrict_self(arg1, arg2 as u32),

        // Phase 6: Container syscalls
        numbers::SYS_UNSHARE => namespaces::sys_unshare(arg1),
        numbers::SYS_SETNS => namespaces::sys_setns(arg1, arg2),
        numbers::SYS_CGROUP_MKDIR => cgroup::sys_cgroup_mkdir(arg1 as *const u8),
        numbers::SYS_CGROUP_WRITE => cgroup::sys_cgroup_write(arg1 as *const u8, arg2 as *const u8, arg3 as *const u8),
        numbers::SYS_CGROUP_READ => cgroup::sys_cgroup_read(arg1 as *const u8, arg2 as *const u8, arg3 as *mut u8),

        _ => {
            crate::println!("[SYSCALL] Unknown syscall: {} (0x{:x})", n, n);
            errno::Errno::ENOSYS as u64
        }
    };

    {
        let process_arc = match get_current_process() {
            Some(p) => p,
            None => return result,
        };
        let (handler, restorer, sig_num, sig_bit) = {
            let mut signals = process_arc.signals.lock();
            // Only deliver unmasked signals
            if !signals.has_unmasked_pending(signals.blocked) { return result; }

            let available = signals.pending & !signals.blocked;
            let sig_bit = available.trailing_zeros();
            let sig_num = sig_bit + 1;
            let handler = process_arc.signal_handlers.lock()[sig_bit as usize];
            let restorer = process_arc.signal_restorers.lock()[sig_bit as usize];

            if handler == 1 {
                signals.pending &= !(1 << sig_bit);
                return result;
            }

            drop(signals);
            (handler, restorer, sig_num, sig_bit)
        };

        if handler == 0 {
            sys_exit(128 + sig_num as u64);
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
                    sys_exit(128 + sig_num as u64);
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

            let ret_phys = match crate::memory::virt_to_phys(x86_64::VirtAddr::new(ret_addr_rsp)) {
                Some(p) => p,
                None => {
                    crate::serial_write("[SIGNAL] invalid user return stack, killing process\n");
                    sys_exit(128 + sig_num as u64);
                    unreachable!();
                }
            };
            let ret_kptr = (crate::memory::physical_memory_offset() + ret_phys.as_u64()) as *mut u64;
            unsafe { *ret_kptr = restorer; }

            {
                let mut signals = process_arc.signals.lock();
                signals.pending &= !(1 << sig_bit);
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
                });
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
