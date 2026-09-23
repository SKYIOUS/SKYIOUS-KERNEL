use super::errno;
use super::helpers::*;
use super::numbers;
use super::*;
use super::{cgroup, landlock, namespaces, prctl, seccomp};
use super::{fs, gui, ipc, misc, net, process};
use crate::gdt;
use crate::interrupts::IrqFmtBuf;
use crate::objects::KernelObject;
use crate::sync::IrqSafeMutex as Mutex;
use crate::syscalls::user_access;
use crate::task::process::Process;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::vfs::{Stat, VfsNode, VFS};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use x86_64::registers::model_specific::{LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB};
use x86_64::VirtAddr;
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
    )
    .expect("failed to write STAR MSR");

    LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::ALIGNMENT_CHECK);
}

pub fn init() {
    user_access::init_smap();
    init_syscall_msrs();

    unsafe {
        use x86_64::registers::model_specific::Efer;
        Efer::update(|efer| {
            efer.insert(x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS)
        });

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

/// GS-aware CPU-ID provider installed for vahi-sync's `IrqSafeMutex`.
///
/// Invariant: every CPU that executes this function either has a valid GS
/// base (it has run `init_gs_base`) or has GS base == 0 (an AP between
/// trampoline entry and `init_gs_base`, or any CPU before the BSP's own
/// `init_gs_base`). The naive implementation read `gs:0x0` directly, which
/// dereferences linear address 0 while GS base is still 0 — with no IDT
/// loaded yet that page fault became a double fault and a triple fault
/// (the K-02 `-smp 2` boot failure, faulting PC inside the provider
/// closure). Here the base is obtained from IA32_GS_BASE — an MSR read,
/// which never faults and performs no memory access — and the CPUID
/// fallback (leaf 1 EBX[31:24] = initial APIC ID, valid from reset) is
/// used while the base is still 0.
fn current_cpu_id() -> u16 {
    const IA32_GS_BASE: u32 = 0xC000_0101;
    // SAFETY: `rdmsr` on IA32_GS_BASE is architecturally defined on every
    // long-mode CPU (the only target of this kernel) and does not access
    // memory or the stack. Only ecx/edx/eax are touched.
    let base = unsafe {
        let (lo, hi): (u32, u32);
        core::arch::asm!(
            "rdmsr",
            in("ecx") IA32_GS_BASE,
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem)
        );
        ((hi as u64) << 32) | lo as u64
    };
    if base == 0 {
        // GS not yet initialized on this CPU (pre-per-CPU window).
        // __cpuid(1) never faults and EBX[31:24] is this CPU's initial
        // APIC ID — unique per CPU from reset.
        let r = core::arch::x86_64::__cpuid(1);
        return ((r.ebx >> 24) & 0xFF) as u16;
    }
    // SAFETY: base != 0 and is the address at which this CPU's
    // `init_gs_base` leaked its `PerCpuData`; `self_ptr` (offset 0) was
    // written before GS base was installed, so the struct is fully
    // initialized. Only the owning CPU writes its own PerCpuData, and this
    // read is of `cpu_id`, which never changes after init.
    let data = unsafe { &*(base as *const PerCpuData) };
    data.cpu_id as u16
}

pub fn init_gs_base(cpu_id: usize) {
    use x86_64::registers::model_specific::GsBase;
    use x86_64::registers::model_specific::KernelGsBase;

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
        pf_callee_saved: [0; 6],
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

    // Register the GS-aware CPU-ID provider for vahi-sync's IrqSafeMutex.
    // `current_cpu_id` is safe on pre-per-CPU CPUs (CPUID fallback while
    // GS base is 0), so this registration is correct even though APs run
    // it before their own GS is live. Every CPU registers the identical
    // fn item, so cross-CPU registration is value-idempotent.
    // SAFETY: the provider contract requires: safe with interrupts
    // disabled (rdmsr/cpuid are), unique per CPU (initial APIC id / stored
    // cpu_id), and no panic or allocation (none present).
    unsafe {
        vahi_sync::set_cpu_id_provider(current_cpu_id);
    }
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

/// Declare one syscall: generates the `&SyscallArgs` adapter and registers it
/// in the dispatch table. Every syscall is one line — the table is the map.
macro_rules! sys_handler {
    ($table:ident, $args:ident, $num:expr, $family:ident :: $fn:ident ($($arg:expr),* $(,)?)) => {{
        fn handler($args: &SyscallArgs) -> u64 {
            let _ = $args;
            $family::$fn($($arg),*)
        }
        $table[$num as usize] = Some(handler as SyscallHandler);
    }};
    ($table:ident, $args:ident, $num:expr, crate :: $mod:ident :: $fn:ident ($($arg:expr),* $(,)?)) => {{
        fn handler($args: &SyscallArgs) -> u64 {
            let _ = $args;
            crate::$mod::$fn($($arg),*)
        }
        $table[$num as usize] = Some(handler as SyscallHandler);
    }};
    ($table:ident, $args:ident, $num:expr, $fn:ident ($($arg:expr),* $(,)?)) => {{
        fn handler($args: &SyscallArgs) -> u64 {
            let _ = $args;
            $fn($($arg),*)
        }
        $table[$num as usize] = Some(handler as SyscallHandler);
    }};
}

// Stack-passed 6th argument (x86_64 ABI): read from the saved user registers.
fn mmap_entry(args: &SyscallArgs) -> u64 {
    let offset = unsafe { *args.regs.add(6) };
    sys_mmap_inner(args.a1, args.a2, args.a3, args.a4, args.a5, offset)
}

fn futex_entry(args: &SyscallArgs) -> u64 {
    let val3 = unsafe { *args.regs.add(6) as u32 };
    crate::syscalls::futex::sys_futex(
        args.a1 as *mut u32,
        args.a2 as u32,
        args.a3 as u32,
        args.a4 as u32,
        args.a5 as *mut u32,
        val3,
    )
}

/// Dispatch table indexed by syscall number. `None` = unknown syscall.
const TABLE_SIZE: usize = 475;

/// Build the dispatch table. Called once; the result is cached.
const fn build_table() -> [Option<SyscallHandler>; TABLE_SIZE] {
    let mut t: [Option<SyscallHandler>; TABLE_SIZE] = [None; TABLE_SIZE];
    sys_handler!(
        t,
        args,
        numbers::SYS_READ,
        fs::sys_read(args.a1, args.a2 as *mut u8, args.a3 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_WRITE,
        fs::sys_write(args.a1, args.a2 as *const u8, args.a3 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_OPEN,
        fs::sys_open(args.a1 as *const u8, args.a2 as i32, args.a3 as u32)
    );
    sys_handler!(t, args, numbers::SYS_CLOSE, fs::sys_close(args.a1));
    sys_handler!(
        t,
        args,
        numbers::SYS_STAT,
        fs::sys_stat(args.a1 as *const u8, args.a2 as *mut crate::vfs::Stat)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FSTAT,
        fs::sys_fstat(args.a1, args.a2 as *mut crate::vfs::Stat)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LSTAT,
        fs::sys_lstat(args.a1 as *const u8, args.a2 as *mut crate::vfs::Stat)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_POLL,
        misc::sys_poll(args.a1 as *const u8, args.a2 as usize, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LSEEK,
        fs::sys_lseek(args.a1, args.a2 as i64, args.a3 as i32)
    );
    sys_handler!(t, args, numbers::SYS_MMAP, mmap_entry(args));
    sys_handler!(
        t,
        args,
        numbers::SYS_MPROTECT,
        fs::sys_mprotect(args.a1, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MUNMAP,
        fs::sys_munmap(args.a1, args.a2)
    );
    sys_handler!(t, args, numbers::SYS_BRK, fs::sys_brk(args.a1));
    sys_handler!(
        t,
        args,
        numbers::SYS_RT_SIGACTION,
        process::sys_rt_sigaction(args.a1, args.a2 as *const u64, args.a3 as *mut u64, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RT_SIGRETURN,
        process::sys_rt_sigreturn(args.regs)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_IOCTL,
        fs::sys_ioctl(args.a1, args.a2, args.a3 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_ACCESS,
        fs::sys_access(args.a1 as *const u8, args.a2 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_PIPE,
        fs::sys_pipe(args.a1 as *mut u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SELECT,
        misc::sys_select(
            args.a1,
            args.a2 as *mut u64,
            args.a3 as *mut u64,
            args.a4 as *mut u64,
            args.a5 as *const u64,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SCHED_YIELD,
        process::sys_sched_yield()
    );
    sys_handler!(t, args, numbers::SYS_DUP, fs::sys_dup(args.a1));
    sys_handler!(t, args, numbers::SYS_DUP2, fs::sys_dup2(args.a1, args.a2));
    sys_handler!(t, args, numbers::SYS_PAUSE, process::sys_pause());
    sys_handler!(
        t,
        args,
        numbers::SYS_NANOSLEEP,
        process::sys_nanosleep(args.a1, args.a2)
    );
    sys_handler!(t, args, numbers::SYS_SYNC, fs::sys_sync());
    sys_handler!(t, args, numbers::SYS_GETPID, process::sys_getpid());
    sys_handler!(
        t,
        args,
        numbers::SYS_SENDFILE,
        fs::sys_sendfile(args.a1, args.a2, args.a3 as *mut u64, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SOCKET,
        net::sys_socket(args.a1, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CONNECT,
        net::sys_connect(args.a1, args.a2 as *const u8, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_ACCEPT,
        net::sys_accept(args.a1, args.a2 as *mut u8, args.a3 as *mut u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SENDTO,
        net::sys_sendto(
            args.a1,
            args.a2 as *const u8,
            args.a3,
            args.a4 as *const u8,
            args.a5
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RECVFROM,
        net::sys_recvfrom(
            args.a1,
            args.a2 as *mut u8,
            args.a3,
            args.a4 as *mut u8,
            args.a5 as *mut u32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SENDMSG,
        net::sys_sendmsg(args.a1 as i64, args.a2 as *const msghdr, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RECVMSG,
        net::sys_recvmsg(args.a1 as i64, args.a2 as *mut msghdr, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_BIND,
        net::sys_bind(args.a1, args.a2 as *const u8, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LISTEN,
        net::sys_listen(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETSOCKNAME,
        net::sys_getsockname(args.a1, args.a2 as *mut u8, args.a3 as *mut u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETPEERNAME,
        misc::sys_getpeername(args.a1, args.a2 as *mut u8, args.a3 as *mut u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SOCKETPAIR,
        net::sys_socketpair(args.a1, args.a2, args.a3, args.a4 as *mut i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETSOCKOPT,
        net::sys_setsockopt(
            args.a1,
            args.a2 as i32,
            args.a3 as i32,
            args.a4 as *const u8,
            args.a5
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETSOCKOPT,
        net::sys_getsockopt(
            args.a1,
            args.a2 as i32,
            args.a3 as i32,
            args.a4 as *mut u8,
            args.a5 as *mut u32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CLONE,
        process::sys_clone(
            args.a1,
            args.a2,
            args.a3 as *mut u32,
            args.a4,
            args.a5 as *mut u32,
            args.regs
        )
    );
    sys_handler!(t, args, numbers::SYS_FORK, process::sys_fork(args.regs));
    sys_handler!(
        t,
        args,
        numbers::SYS_EXECVE,
        process::sys_execve(
            args.a1 as *const u8,
            args.a2 as *const *const u8,
            args.a3 as *const *const u8,
            args.regs,
        )
    );
    sys_handler!(t, args, numbers::SYS_EXIT, process::sys_exit(args.a1));
    sys_handler!(
        t,
        args,
        numbers::SYS_WAIT4,
        process::sys_wait4(
            args.a1 as i64,
            args.a2 as *mut i32,
            args.a3 as i32,
            args.a4 as *mut u8
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_KILL,
        process::sys_kill(args.a1 as i64, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_UNAME,
        process::sys_uname(args.a1 as *mut UtsName)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FCNTL,
        fs::sys_fcntl(args.a1, args.a2 as i32, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TRUNCATE,
        fs::sys_truncate(args.a1 as *const u8, args.a2 as i64)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FTRUNCATE,
        fs::sys_ftruncate(args.a1, args.a2 as i64)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETCWD,
        fs::sys_getcwd(args.a1 as *mut u8, args.a2 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CHDIR,
        fs::sys_chdir(args.a1 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RENAME,
        fs::sys_rename(args.a1 as *const u8, args.a2 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MKDIR,
        fs::sys_mkdir(args.a1 as *const u8, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LINK,
        fs::sys_link(args.a1 as *const u8, args.a2 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_UNLINK,
        fs::sys_unlink(args.a1 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SYMLINK,
        fs::sys_symlink(args.a1 as *const u8, args.a2 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_READLINK,
        fs::sys_readlink(args.a1 as *const u8, args.a2 as *mut u8, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CHMOD,
        fs::sys_chmod(args.a1 as *const u8, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FCHMOD,
        fs::sys_fchmod(args.a1, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CHOWN,
        fs::sys_chown(args.a1 as *const u8, args.a2 as u32, args.a3 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FCHOWN,
        fs::sys_fchown(args.a1, args.a2 as u32, args.a3 as u32)
    );
    sys_handler!(t, args, numbers::SYS_UMASK, fs::sys_umask(args.a1 as u32));
    sys_handler!(
        t,
        args,
        numbers::SYS_GETRLIMIT,
        process::sys_getrlimit(args.a1, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETRLIMIT,
        process::sys_setrlimit(args.a1, args.a2 as *const u8)
    );
    sys_handler!(t, args, numbers::SYS_GETPPID, process::sys_getppid());
    sys_handler!(t, args, numbers::SYS_GETPGRP, process::sys_getpgrp());
    sys_handler!(t, args, numbers::SYS_SETSID, process::sys_setsid());
    sys_handler!(
        t,
        args,
        numbers::SYS_GETGROUPS,
        process::sys_getgroups(args.a1 as i32, args.a2 as *mut u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETGROUPS,
        process::sys_setgroups(args.a1 as i64, args.a2 as *const u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETRESUID,
        process::sys_getresuid(
            args.a1 as *mut u32,
            args.a2 as *mut u32,
            args.a3 as *mut u32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETRESUID,
        process::sys_setresuid(args.a1 as u32, args.a2 as u32, args.a3 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SIGALTSTACK,
        process::sys_sigaltstack(args.a1 as *const u8, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_STATFS,
        fs::sys_statfs(args.a1 as *const u8, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SCHED_SETATTR,
        process::sys_sched_setattr(args.a1 as i64, args.a2 as *const u8, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SCHED_GETATTR,
        process::sys_sched_getattr(args.a1 as i64, args.a2 as *mut u8, args.a3, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETPGID,
        process::sys_setpgid(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_ARCH_PRCTL,
        process::sys_arch_prctl(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MOUNT,
        fs::sys_mount(
            args.a1 as *const u8,
            args.a2 as *const u8,
            args.a3 as *const u8,
            args.a4,
            args.a5 as *const u8,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_UMOUNT2,
        fs::sys_umount2(args.a1 as *const u8, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_REBOOT,
        misc::sys_reboot(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RESOLVE,
        misc::sys_resolve(args.a1 as *const u8, args.a2 as *mut u8)
    );
    sys_handler!(t, args, numbers::SYS_FUTEX, futex_entry(args));
    sys_handler!(
        t,
        args,
        numbers::SYS_SCHED_SETAFFINITY,
        process::sys_sched_setaffinity(args.a1 as i64, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SCHED_GETAFFINITY,
        process::sys_sched_getaffinity(args.a1 as i64, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SYSINFO,
        process::sys_sysinfo(args.a1 as *mut u64)
    );
    sys_handler!(t, args, numbers::SYS_OPENPTY, gui::sys_openpty());
    sys_handler!(
        t,
        args,
        numbers::SYS_GETDENTS64,
        fs::sys_getdents64(args.a1, args.a2 as *mut u8, args.a3 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SET_TID_ADDRESS,
        process::sys_set_tid_address(args.a1 as *const u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMER_CREATE,
        posix_timers::sys_timer_create(
            args.a1 as i32,
            args.a2 as *const posix_timers::sigevent,
            args.a3 as *mut i32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMER_SETTIME,
        posix_timers::sys_timer_settime(
            args.a1 as i32,
            args.a2 as i32,
            args.a3 as *const posix_timers::itimerspec,
            args.a4 as *mut posix_timers::itimerspec,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMER_GETTIME,
        posix_timers::sys_timer_gettime(args.a1 as i32, args.a2 as *mut posix_timers::itimerspec)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMER_GETOVERRUN,
        posix_timers::sys_timer_getoverrun(args.a1 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMER_DELETE,
        posix_timers::sys_timer_delete(args.a1 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CLOCK_GETTIME,
        misc::sys_clock_gettime(args.a1, args.a2 as *mut Timespec)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CLOCK_GETRES,
        misc::sys_clock_getres(args.a1, args.a2 as *mut Timespec)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CLOCK_NANOSLEEP,
        misc::sys_clock_nanosleep(
            args.a1,
            args.a2,
            args.a3 as *const Timespec,
            args.a4 as *mut Timespec
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EXIT_GROUP,
        process::sys_exit_group(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_OPENAT,
        fs::sys_openat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as i32,
            args.a4 as u32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MKDIRAT,
        fs::sys_mkdirat(args.a1 as i64, args.a2 as *const u8, args.a3 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FSTATAT,
        fs::sys_fstatat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as *mut crate::vfs::Stat,
            args.a4 as i32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_UNLINKAT,
        fs::sys_unlinkat(args.a1 as i64, args.a2 as *const u8, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RENAMEAT,
        fs::sys_renameat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as i64,
            args.a4 as *const u8,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LINKAT,
        fs::sys_linkat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as i64,
            args.a4 as *const u8,
            args.a5 as i32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SYMLINKAT,
        fs::sys_symlinkat(args.a2 as *const u8, args.a1 as i64, args.a3 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_READLINKAT,
        fs::sys_readlinkat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as *mut u8,
            args.a4
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FACCESSAT,
        fs::sys_faccessat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as i32,
            args.a4 as i32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_UNSHARE,
        namespaces::sys_unshare(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_UTIMENSAT,
        fs::sys_utimensat(
            args.a1 as i64,
            args.a2 as *const u8,
            args.a3 as *const u8,
            args.a4 as i32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SIGNALFD,
        process::sys_signalfd(args.a1, args.a2 as *const u64, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EVENTFD,
        eventfd::sys_eventfd2(args.a1 as u32, 0)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_FALLOCATE,
        fs::sys_fallocate(args.a1, args.a2 as i32, args.a3 as i64, args.a4 as i64)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SIGNALFD4,
        process::sys_signalfd4(args.a1, args.a2 as *const u64, args.a3, args.a4 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EVENTFD2,
        eventfd::sys_eventfd2(args.a1 as u32, args.a2 as i32)
    );
    sys_handler!(t, args, numbers::SYS_GETUID, process::sys_getuid());
    sys_handler!(t, args, numbers::SYS_GETGID, process::sys_getgid());
    sys_handler!(t, args, numbers::SYS_SETUID, process::sys_setuid(args.a1));
    sys_handler!(t, args, numbers::SYS_SETGID, process::sys_setgid(args.a1));
    sys_handler!(t, args, numbers::SYS_GETEUID, process::sys_geteuid());
    sys_handler!(t, args, numbers::SYS_GETEGID, process::sys_getegid());
    sys_handler!(
        t,
        args,
        numbers::SYS_CAPGET,
        process::sys_capget(args.a1 as *mut u8, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CAPSET,
        process::sys_capset(args.a1 as *const u8, args.a2 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SIGPROCMASK,
        process::sys_sigprocmask(args.a1 as i32, args.a2 as *const u64, args.a3 as *mut u64)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETRESGID,
        process::sys_getresgid(
            args.a1 as *mut u32,
            args.a2 as *mut u32,
            args.a3 as *mut u32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETRESGID,
        process::sys_setresgid(args.a1 as u32, args.a2 as u32, args.a3 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SECCOMP,
        seccomp::sys_seccomp(args.a1 as u32, args.a2 as u32, args.a3 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MEMFD_CREATE,
        shm::sys_memfd_create(args.a1 as *const u8, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_BPF,
        crate::ebpf::sys_bpf(args.a1 as u32, args.a2, args.a3, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SWAPON,
        fs::sys_swapon(args.a1 as *const u8, args.a2 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SWAPOFF,
        fs::sys_swapoff(args.a1 as *const u8)
    );
    sys_handler!(t, args, numbers::SYS_GETPGID, process::sys_getpgid(args.a1));
    sys_handler!(t, args, numbers::SYS_GETSID, process::sys_getsid(args.a1));
    sys_handler!(
        t,
        args,
        numbers::SYS_PRLIMIT64,
        process::sys_prlimit64(args.a1, args.a2, args.a3 as *const u8, args.a4 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETITIMER,
        process::sys_getitimer(args.a1, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETITIMER,
        process::sys_setitimer(args.a1, args.a2 as *const u8, args.a3 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMES,
        process::sys_times(args.a1 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETRUSAGE,
        process::sys_getrusage(args.a1, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMERFD_CREATE,
        timerfd::sys_timerfd_create(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMERFD_SETTIME,
        timerfd::sys_timerfd_settime(args.a1, args.a2, args.a3 as *const u8, args.a4 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_TIMERFD_GETTIME,
        timerfd::sys_timerfd_gettime(args.a1, args.a2 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_INOTIFY_INIT,
        inotify::sys_inotify_init(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_INOTIFY_ADD_WATCH,
        inotify::sys_inotify_add_watch(args.a1, args.a2 as *const u8, args.a3 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_INOTIFY_RM_WATCH,
        inotify::sys_inotify_rm_watch(args.a1, args.a3 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_RECVMMSG,
        mmsg::sys_recvmmsg(
            args.a1,
            args.a2 as *mut mmsg::mmsghdr,
            args.a3,
            args.a4,
            args.a5 as *const u8,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SENDMMSG,
        mmsg::sys_sendmmsg(args.a1, args.a2 as *mut mmsg::mmsghdr, args.a3, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_OBJMGR_ENUM,
        misc::sys_objmgr_enum(args.a1 as *mut u8, args.a2 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_OBJMGR_AUDIT,
        misc::sys_objmgr_audit(args.a1, args.a2 as *mut u8, args.a3 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_DRMCTL,
        gui::sys_drmctl(args.a1, args.a2, args.a3 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_HASH,
        misc::sys_hash(
            args.a1,
            args.a2 as *const u8,
            args.a3,
            args.a4 as *mut u8,
            args.a5
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GETRANDOM,
        misc::sys_getrandom(args.a1 as *mut u8, args.a2 as usize, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_IO_URING_SETUP,
        io_uring::sys_io_uring_setup(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_IO_URING_ENTER,
        io_uring::sys_io_uring_enter(
            args.a1,
            args.a2 as u32,
            args.a3 as u32,
            args.a4 as u32,
            args.a5
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_IO_URING_REGISTER,
        io_uring::sys_io_uring_register(args.a1, args.a2 as u32, args.a3, args.a4 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EPOLL_CREATE1,
        epoll::sys_epoll_create1(args.a1 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EPOLL_CTL,
        epoll::sys_epoll_ctl(
            args.a1,
            args.a2 as i32,
            args.a3 as i32,
            args.a4 as *const u8
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EPOLL_WAIT,
        epoll::sys_epoll_wait(args.a1, args.a2 as *mut u8, args.a3 as i32, args.a4 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EPOLL_PWAIT,
        epoll::sys_epoll_pwait(
            args.a1,
            args.a2 as *mut u8,
            args.a3 as i32,
            args.a4 as i32,
            args.a5 as *const u8,
            0,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_EPOLL_CREATE,
        epoll::sys_epoll_create(args.a1 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_READV,
        compat::sys_readv(args.a1, args.a2 as *const u8, args.a3 as i64)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_WRITEV,
        compat::sys_writev(args.a1, args.a2 as *const u8, args.a3 as i64)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MADVISE,
        compat::sys_madvise(args.a1, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_PIPE2,
        compat::sys_pipe2(args.a1 as *mut u32, args.a2 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_DUP3,
        compat::sys_dup3(args.a1, args.a2, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_PREAD64,
        compat::sys_pread64(args.a1, args.a2 as *mut u8, args.a3 as usize, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_PWRITE64,
        compat::sys_pwrite64(args.a1, args.a2 as *const u8, args.a3 as usize, args.a4)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LANDLOCK_CREATE_RULESET,
        landlock::sys_landlock_create_ruleset(
            args.a1 as *const u8,
            args.a2 as usize,
            args.a3 as u32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LANDLOCK_ADD_RULE,
        landlock::sys_landlock_add_rule(
            args.a1,
            args.a2 as u32,
            args.a3 as *const u8,
            args.a4 as u32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_LANDLOCK_RESTRICT_SELF,
        landlock::sys_landlock_restrict_self(args.a1, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_PRCTL,
        prctl::sys_prctl(args.a1, args.a2, args.a3, args.a4, args.a5)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CGROUP_MKDIR,
        cgroup::sys_cgroup_mkdir(args.a1 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CGROUP_WRITE,
        cgroup::sys_cgroup_write(
            args.a1 as *const u8,
            args.a2 as *const u8,
            args.a3 as *const u8
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CGROUP_READ,
        cgroup::sys_cgroup_read(
            args.a1 as *const u8,
            args.a2 as *const u8,
            args.a3 as *mut u8
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SETNS,
        namespaces::sys_setns(args.a1, args.a2)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MQ_OPEN,
        mqueue::mq_open(
            args.a1 as *const u8,
            args.a2 as i32,
            args.a3 as i32,
            args.a4 as *mut u8
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MQ_CLOSE,
        mqueue::mq_close(args.a1 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MQ_TIMEDSEND,
        mqueue::mq_send(
            args.a1 as i32,
            args.a2 as *const u8,
            args.a3 as usize,
            args.a4 as u32
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MQ_TIMEDRECEIVE,
        mqueue::mq_receive(
            args.a1 as i32,
            args.a2 as *mut u8,
            args.a3 as usize,
            args.a4 as *mut u32,
        )
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MQ_UNLINK,
        mqueue::mq_unlink(args.a1 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SHMGET,
        shm::sys_shmget(args.a1 as i32, args.a2 as usize, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SHMAT,
        shm::sys_shmat(args.a1 as i32, args.a2 as *const u8, args.a3 as i32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SHMCTL,
        shm::sys_shmctl(args.a1 as i32, args.a2 as i32, args.a3 as *mut u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_SHMDT,
        shm::sys_shmdt(args.a1 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_CREATE_WINDOW,
        gui::sys_gui_create_window(args.a1 as *const u8, args.a2 as usize, args.a3 as usize)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_GET_BUFFER,
        gui::sys_gui_get_buffer(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_FLUSH,
        gui::sys_gui_flush(args.a1, args.a2 as *const u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_MAP_BUFFER,
        gui::sys_gui_map_buffer(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_GET_KEY,
        gui::sys_gui_get_key(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_GET_MOUSE,
        gui::sys_gui_get_mouse(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_SET_TITLE,
        gui::sys_gui_set_title(args.a1, args.a2 as *const u8)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_DESTROY_WINDOW,
        gui::sys_gui_destroy_window(args.a1)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_RESIZE_WINDOW,
        gui::sys_gui_resize_window(args.a1, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_GUI_MOVE_WINDOW,
        gui::sys_gui_move_window(args.a1, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_CLIPBOARD,
        gui::sys_clipboard(args.a1, args.a2 as *mut u8, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_NOTIFY,
        gui::sys_notify(args.a1 as *const u8, args.a2, args.a3)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_BEEP,
        gui::sys_beep(args.a1 as u32, args.a2 as u32)
    );
    sys_handler!(
        t,
        args,
        numbers::SYS_MKFS,
        fs::sys_mkfs(args.a1 as *const u8, args.a2)
    );
    t
}

static SYSCALL_TABLE: [Option<SyscallHandler>; TABLE_SIZE] = build_table();

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
        lock.as_ref()
            .map(|p| *p.emulation.lock() == crate::task::process::EmulationMode::Linux)
            .unwrap_or(false)
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

    let args = SyscallArgs {
        n,
        a1: arg1,
        a2: arg2,
        a3: arg3,
        a4: arg4,
        a5: arg5,
        regs: regs_ptr,
    };

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
            if !signals.has_unmasked_pending(signals.blocked) {
                return result;
            }

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
            let k_ptr =
                (crate::memory::physical_memory_offset() + phys.as_u64()) as *mut SignalFrame;

            unsafe {
                (*k_ptr).r15 = *regs_ptr.add(0);
                (*k_ptr).r14 = *regs_ptr.add(1);
                (*k_ptr).r13 = *regs_ptr.add(2);
                (*k_ptr).r12 = *regs_ptr.add(3);
                (*k_ptr).r11 = *regs_ptr.add(4);
                (*k_ptr).r10 = *regs_ptr.add(5);
                (*k_ptr).r9 = *regs_ptr.add(6);
                (*k_ptr).r8 = *regs_ptr.add(7);
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
                let ret_phys =
                    match crate::memory::virt_to_phys(x86_64::VirtAddr::new(ret_addr_rsp)) {
                        Some(p) => p,
                        None => {
                            crate::serial_write(
                                "[SIGNAL] invalid user return stack, killing process\n",
                            );
                            sys_exit_inner(128 + sig_num as u64);
                            unreachable!();
                        }
                    };
                let ret_kptr =
                    (crate::memory::physical_memory_offset() + ret_phys.as_u64()) as *mut u64;
                if restorer != crate::syscalls::signal::SIGNAL_RESTORER.as_ptr() as u64 {
                    // App provided its own restorer — write the pointer.
                    unsafe {
                        *ret_kptr = restorer;
                    }
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
                    r8: unsafe { *regs_ptr.add(7) },
                    r9: unsafe { *regs_ptr.add(6) },
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
fn sys_exit_inner(code: u64) {
    process::sys_exit(code);
}
fn sys_mmap_inner(a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, offset: u64) -> u64 {
    super::sys_mmap(a1, a2, a3, a4, a5, offset)
}
