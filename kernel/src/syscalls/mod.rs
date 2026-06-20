use x86_64::VirtAddr;
use x86_64::registers::model_specific::{LStar, Star, SFMask};
use x86_64::registers::rflags::RFlags;
use crate::gdt;
use spin::Mutex;
use crate::vfs::{VFS, VfsNode, Stat};
use alloc::sync::Arc;
use x86_64::structures::paging::{Page, Size4KiB, Mapper, FrameAllocator, PageTableFlags};

pub mod errno;
pub mod numbers;
pub mod signal;
pub mod user_access;
pub mod io_uring;

use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;

// Capability constants (matching Linux CAP_* values)
pub const CAP_SYS_ADMIN: u64 = 1 << 21;
pub const CAP_SYS_BOOT: u64 = 1 << 22;
pub const CAP_KILL: u64 = 1 << 5;
pub const CAP_SETUID: u64 = 1 << 6;
pub const CAP_SETGID: u64 = 1 << 7;

/// Check if the current process has the given capability in its effective set.
fn has_capability(cap_bit: u64) -> bool {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(false, |p| (*p.cap_effective.lock() & cap_bit) != 0)
}

/// Log a security-relevant event to serial for audit trail.
fn audit_log(event: &str, detail: &str) {
    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id).unwrap_or(0)
    };
    crate::serial_write("[AUDIT] ");
    crate::serial_write(event);
    crate::serial_write(" pid=");
    let pid_str = alloc::format!("{}", pid);
    crate::serial_write(&pid_str);
    crate::serial_write(" ");
    crate::serial_write(detail);
    crate::serial_write("\n");
}

/// Get euid for the current process. Returns 0 (root) if no process.
pub fn get_current_euid() -> u32 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(0, |p| *p.euid.lock())
}

/// Get egid for the current process. Returns 0 (root) if no process.
fn get_current_egid() -> u32 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(0, |p| *p.egid.lock())
}

/// Check if the current process can access a file with given mode/uid/gid.
/// `need` is the access bits required (4=read, 2=write, 1=execute).
/// Returns true if access is granted.
fn check_file_permission(st_mode: u32, st_uid: u32, st_gid: u32, need: u32) -> bool {
    let euid = get_current_euid();
    let egid = get_current_egid();
    // Root can access anything
    if euid == 0 { return true; }
    let bits = if euid == st_uid { (st_mode >> 6) & 7 }
               else if egid == st_gid { (st_mode >> 3) & 7 }
               else { st_mode & 7 };
    (bits & need) == need
}

/// Check if current process can access a VfsNode with the given required permission bits.
fn check_node_permission(node: &Arc<dyn VfsNode>, need: u32) -> bool {
    if let Ok(stat) = node.stat() {
        check_file_permission(stat.st_mode, stat.st_uid, stat.st_gid, need)
    } else {
        true // If we can't stat, allow (compatibility with special filesystems)
    }
}

/// Check if current process owns the given file (euid matches st_uid or is root).
fn check_file_owner(node: &Arc<dyn VfsNode>) -> bool {
    let euid = get_current_euid();
    if euid == 0 { return true; }
    if let Ok(stat) = node.stat() {
        euid == stat.st_uid
    } else {
        true
    }
}

pub fn init() {
    // Detect SMAP and enable if available (must be done before any user access)
    user_access::init_smap();

    let selectors = gdt::get_selectors();

    Star::write(
        selectors.user_code_selector,
        selectors.user_data_selector,
        selectors.code_selector,
        selectors.data_selector,
    ).expect("failed to write STAR MSR");

    LStar::write(VirtAddr::new(syscall_entry as *const () as u64));
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG | RFlags::ALIGNMENT_CHECK);

    unsafe {
        use x86_64::registers::model_specific::Efer;
        Efer::update(|efer| efer.insert(x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS));
        
        // Setup GS base for BSP (CPU 0)
        init_gs_base(0);
    }
}

/// Maximum supported CPU count.
#[allow(dead_code)]
pub const MAX_CPUS: usize = 8;

/// Array of per-CPU area pointers for indexed access from non-GS contexts.
#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct PerCpuPtr(pub *mut PerCpuData);
unsafe impl Send for PerCpuPtr {}
unsafe impl Sync for PerCpuPtr {}

pub static PER_CPU_AREAS: spin::Mutex<alloc::vec::Vec<PerCpuPtr>> = spin::Mutex::new(alloc::vec::Vec::new());

/// Get the current CPU's per-CPU data via GS segment.
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

    // Allocate per-CPU data (leaked intentionally — lives forever)
    let data = alloc::boxed::Box::leak(alloc::boxed::Box::new(PerCpuData {
        self_ptr: 0, // will be set after allocation
        cpu_id: cpu_id as u64,
        kernel_rsp: crate::gdt::get_kernel_stack().as_u64(),
        user_rsp: 0,
        ipi_pending: 0,
        ipi_arg: 0,
        idle_count: 0,
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

#[derive(Clone, Copy)]
#[repr(C)]
pub struct PerCpuData {
    pub self_ptr:  u64,      // offset 0x00 — pointer to self (gs:0x0 reads this)
    pub cpu_id:    u64,      // offset 0x08
    pub kernel_rsp: u64,      // offset 0x10 — loaded on syscall entry
    pub user_rsp:  u64,      // offset 0x18 — saved on syscall entry
    pub ipi_pending: u64,    // offset 0x20 — IPI function pointer
    pub ipi_arg: u64,        // offset 0x28 — IPI argument
    pub idle_count: u64,     // offset 0x30 — idle loop counter
}

#[repr(C)]
pub struct UtsName {
    pub sysname: [u8; 65],
    pub nodename: [u8; 65],
    pub release: [u8; 65],
    pub version: [u8; 65],
    pub machine: [u8; 65],
    pub domainname: [u8; 65],
}


pub fn sys_open_path(path: &str) -> Result<u64, errno::Errno> {
    let path_c = alloc::format!("{}\0", path);
    let fd = syscall_handler(numbers::SYS_OPEN, path_c.as_ptr() as u64, 0x1, 0, 0, 0, core::ptr::null_mut()); // O_RDONLY=0x1
    if (fd as i64) < 0 {
        Err(unsafe { core::mem::transmute::<i64, errno::Errno>(fd as i64) })
    } else {
        Ok(fd)
    }
}

fn sys_getppid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        p.parent_id.unwrap_or(0)
    } else {
        0
    }
}

fn sys_dup2(old_fd: u64, new_fd: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.fd_table.lock();
        if old_fd as usize >= fd_table.len() || fd_table[old_fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
        
        let old_desc = fd_table[old_fd as usize].clone();
        let old_flags = {
            let flags = p.fd_flags.lock();
            if (old_fd as usize) < flags.len() { flags[old_fd as usize] } else { 0 }
        };
        
        if new_fd as usize >= fd_table.len() {
            fd_table.resize(new_fd as usize + 1, None);
            p.fd_flags.lock().resize(new_fd as usize + 1, 0);
        }
        
        fd_table[new_fd as usize] = old_desc;
        p.fd_flags.lock()[new_fd as usize] = old_flags;
        return new_fd;
    }
    errno::Errno::ESRCH as u64
}

fn sys_dup(old_fd: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.fd_table.lock();
        if old_fd as usize >= fd_table.len() || fd_table[old_fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
        let old_desc = fd_table[old_fd as usize].clone().unwrap();
        // Find lowest available fd
        for (i, slot) in fd_table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(old_desc);
                return i as u64;
            }
        }
        fd_table.push(Some(old_desc));
        return (fd_table.len() - 1) as u64;
    }
    errno::Errno::ESRCH as u64
}

fn sys_access(path_ptr: *const u8, mode: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    let node = match VFS.lock().resolve_path(&path_str) {
        Some(n) => n,
        None => return errno::Errno::ENOENT as u64,
    };
    // Convert F_OK/R_OK/W_OK/X_OK to permission bits
    let need = match mode & 7 {
        0 => 0,          // F_OK: just check existence
        1 => 1,          // W_OK
        2 => 4,          // R_OK
        3 => 5,          // R_OK | W_OK
        4 => 4,          // X_OK
        5 => 5,          // X_OK | W_OK
        6 => 5,          // X_OK | R_OK
        7 => 7,          // R_OK | W_OK | X_OK
        _ => 0,
    };
    if need == 0 { return 0; } // F_OK: file exists
    if check_node_permission(&node, need) { 0 } else { errno::Errno::EACCES as u64 }
}

const F_DUPFD: i32 = 0;
const F_GETFD: i32 = 1;
const F_SETFD: i32 = 2;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;

fn sys_fcntl(fd: u64, cmd: i32, arg: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.fd_table.lock();
        if fd as usize >= fd_table.len() || fd_table[fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
        match cmd {
            F_DUPFD => {
                let desc = fd_table[fd as usize].clone().unwrap();
                for (i, slot) in fd_table.iter_mut().enumerate() {
                    if slot.is_none() && i as u64 > arg {
                        *slot = Some(desc);
                        return i as u64;
                    }
                }
                fd_table.push(Some(desc));
                (fd_table.len() - 1) as u64
            }
            F_GETFD => 0, // No close-on-exec flag tracked yet
            F_SETFD => 0, // Accept and ignore
            F_GETFL => {
                let flags = p.fd_flags.lock();
                if (fd as usize) < flags.len() { flags[fd as usize] } else { 0 }
            }
            F_SETFL => {
                let mut flags = p.fd_flags.lock();
                if fd as usize >= flags.len() {
                    flags.resize(fd as usize + 1, 0);
                }
                flags[fd as usize] = arg & 0xFFFF; // O_NONBLOCK=0x800, etc.
                0
            }
            _ => errno::Errno::EINVAL as u64,
        }
    } else {
        errno::Errno::ESRCH as u64
    }
}

fn sys_pipe(fds_ptr: *mut u32) -> u64 {
    let (reader, writer) = crate::vfs::pipe::Pipe::new();
    
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.fd_table.lock();
        
        let find_slot = |table: &mut Vec<Option<FileDescriptor>>| {
            for (i, slot) in table.iter_mut().enumerate() {
                if slot.is_none() { return Some(i); }
            }
            None
        };

        let r_fd = if let Some(i) = find_slot(&mut fd_table) {
            fd_table[i] = Some(FileDescriptor::File { node: reader, offset: 0 });
            i
        } else {
            fd_table.push(Some(FileDescriptor::File { node: reader, offset: 0 }));
            fd_table.len() - 1
        };

        let w_fd = if let Some(i) = find_slot(&mut fd_table) {
            fd_table[i] = Some(FileDescriptor::File { node: writer, offset: 0 });
            i
        } else {
            fd_table.push(Some(FileDescriptor::File { node: writer, offset: 0 }));
            fd_table.len() - 1
        };

        unsafe {
            if user_access::copy_to_user(fds_ptr as *mut u8, &[r_fd as u8, 0, 0, 0, w_fd as u8, 0, 0, 0]).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        return 0;
    }
    errno::Errno::ESRCH as u64
}

fn sys_uname(buf: *mut UtsName) -> u64 {
    let mut uts = UtsName {
        sysname: [0; 65],
        nodename: [0; 65],
        release: [0; 65],
        version: [0; 65],
        machine: [0; 65],
        domainname: [0; 65],
    };

    let fill = |dest: &mut [u8; 65], src: &str| {
        let bytes = src.as_bytes();
        let len = core::cmp::min(bytes.len(), 64);
        dest[..len].copy_from_slice(&bytes[..len]);
    };

    fill(&mut uts.sysname, "Vahi");
    fill(&mut uts.nodename, "sarga-os");
    fill(&mut uts.release, "0.3.0");
    fill(&mut uts.version, "SARGA OS — Vahi V5.0 Roadmap Implementation");
    fill(&mut uts.machine, "x86_64");

    if unsafe { user_access::copy_to_user(buf as *mut u8, core::slice::from_raw_parts(&uts as *const _ as *const u8, core::mem::size_of::<UtsName>())) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

/// Sets the kernel stack for the current CPU. 
/// Called by the scheduler on context switch.
pub fn set_kernel_stack(stack_top: u64) {
    let data = get_per_cpu();
    data.kernel_rsp = stack_top;
}

extern "C" {
    fn syscall_entry();
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

/// Inner dispatch without emulation redirect — called by both the public entry
/// point and the Linux emulation layer to avoid infinite recursion.
pub(crate) fn do_syscall(
    n: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    regs_ptr: *mut u64,
) -> u64 {
    let result = match n {
        numbers::SYS_READ => sys_read(arg1, arg2 as *mut u8, arg3 as usize),
        numbers::SYS_WRITE => sys_write(arg1, arg2 as *const u8, arg3 as usize),
        numbers::SYS_OPEN => sys_open(arg1 as *const u8, arg2 as i32),
        numbers::SYS_CLOSE => sys_close(arg1),
        numbers::SYS_STAT => sys_stat(arg1 as *const u8, arg2 as *mut crate::vfs::Stat),
        numbers::SYS_FSTAT => sys_fstat(arg1, arg2 as *mut crate::vfs::Stat),
        numbers::SYS_LSEEK => sys_lseek(arg1, arg2 as i64, arg3 as i32),
        numbers::SYS_MMAP => sys_mmap(arg1, arg2, arg3, arg4, arg5, 0), // arg6 (offset) not passed in this simple handler yet
        numbers::SYS_MUNMAP => sys_munmap(arg1, arg2),
        numbers::SYS_BRK => sys_brk(arg1),
        numbers::SYS_EXIT => sys_exit(arg1),
        numbers::SYS_CLONE => sys_clone(arg1, arg2, arg3 as *mut u32, arg4, arg5 as *mut u32, regs_ptr),
        numbers::SYS_FORK => sys_fork(regs_ptr),
        numbers::SYS_GETPID => sys_getpid(),
        numbers::SYS_GETPPID => sys_getppid(),
        numbers::SYS_DUP => sys_dup(arg1),
        numbers::SYS_DUP2 => sys_dup2(arg1, arg2),
        numbers::SYS_ACCESS => sys_access(arg1 as *const u8, arg2 as i32),
        numbers::SYS_FCNTL => sys_fcntl(arg1, arg2 as i32, arg3),
        numbers::SYS_PIPE => sys_pipe(arg1 as *mut u32),
        numbers::SYS_UNAME => sys_uname(arg1 as *mut UtsName),
        numbers::SYS_WAIT4 => sys_wait4(arg1 as i64, arg2 as *mut i32, arg3 as i32, arg4 as *mut u8),
        numbers::SYS_EXECVE => sys_execve(arg1 as *const u8, arg2 as *const *const u8, arg3 as *const *const u8, regs_ptr),
        numbers::SYS_SOCKET => sys_socket(arg1, arg2, arg3),
        numbers::SYS_BIND => sys_bind(arg1, arg2 as *const u8, arg3),
        numbers::SYS_CONNECT => sys_connect(arg1, arg2 as *const u8, arg3),
        numbers::SYS_LISTEN => sys_listen(arg1, arg2),
        numbers::SYS_ACCEPT => sys_accept(arg1, arg2 as *mut u8, arg3 as *mut u32),
        numbers::SYS_SENDTO => sys_sendto(arg1, arg2 as *const u8, arg3, arg4 as *const u8, arg5),
        numbers::SYS_RECVFROM => sys_recvfrom(arg1, arg2 as *mut u8, arg3, arg4 as *mut u8, arg5 as *mut u32),
        
        numbers::SYS_GUI_CREATE_WINDOW => sys_gui_create_window(arg1 as *const u8, arg2 as usize, arg3 as usize),
        numbers::SYS_GUI_GET_BUFFER => sys_gui_get_buffer(arg1),
        numbers::SYS_GUI_FLUSH => sys_gui_flush(arg1, arg2 as *const u32),
        numbers::SYS_GUI_MAP_BUFFER => sys_gui_map_buffer(arg1),
        numbers::SYS_GUI_GET_KEY => sys_gui_get_key(arg1),
        numbers::SYS_GUI_GET_MOUSE => sys_gui_get_mouse(arg1),
        numbers::SYS_GUI_SET_TITLE => sys_gui_set_title(arg1, arg2 as *const u8),
        numbers::SYS_GUI_DESTROY_WINDOW => sys_gui_destroy_window(arg1),
        numbers::SYS_GUI_RESIZE_WINDOW => sys_gui_resize_window(arg1, arg2, arg3),
        numbers::SYS_GUI_MOVE_WINDOW => sys_gui_move_window(arg1, arg2, arg3),
        numbers::SYS_CLIPBOARD => sys_clipboard(arg1, arg2 as *mut u8, arg3),
        numbers::SYS_NOTIFY => sys_notify(arg1 as *const u8, arg2, arg3),
        numbers::SYS_NANOSLEEP => sys_nanosleep(arg1, arg2),
        
        numbers::SYS_GETCWD => sys_getcwd(arg1 as *mut u8, arg2 as usize),
        numbers::SYS_CHDIR => sys_chdir(arg1 as *const u8),
        numbers::SYS_MKDIR => sys_mkdir(arg1 as *const u8, arg2 as u32),
        numbers::SYS_UNLINK => sys_unlink(arg1 as *const u8),
        numbers::SYS_VAHIAI => sys_vahiai(arg1 as *const u8, arg2 as *const *const u8, arg3, arg4 as *mut u8, arg5),
        numbers::SYS_RESOLVE => sys_resolve(arg1 as *const u8, arg2 as *mut u8),
        numbers::SYS_KILL => sys_kill(arg1 as i64, arg2 as u32),
        numbers::SYS_KORLANG => sys_korlang(arg1, arg2, arg3, arg4, arg5),
        numbers::SYS_FUTEX => sys_futex(arg1 as *mut u32, arg2 as u32, arg3 as u32),
        numbers::SYS_SYSINFO => sys_sysinfo(arg1 as *mut u64),
        numbers::SYS_RT_SIGACTION => sys_rt_sigaction(arg1, arg2 as *const u64, arg3 as *mut u64),
        numbers::SYS_RT_SIGRETURN => sys_rt_sigreturn(regs_ptr),
        numbers::SYS_SCHED_YIELD => sys_sched_yield(),
        numbers::SYS_SCHED_SETATTR => sys_sched_setattr(arg1 as i64, arg2 as *const u8, arg3),
        numbers::SYS_SCHED_GETATTR => sys_sched_getattr(arg1 as i64, arg2 as *mut u8, arg3, arg4),
        numbers::SYS_GETDENTS64 => sys_getdents64(arg1, arg2 as *mut u8, arg3 as usize),
        numbers::SYS_IOCTL => sys_ioctl(arg1, arg2, arg3 as *mut u8),
        numbers::SYS_CLOCK_GETTIME => sys_clock_gettime(arg1, arg2 as *mut Timespec),
        numbers::SYS_MOUNT => sys_mount(arg1 as *const u8, arg2 as *const u8, arg3 as *const u8, arg4, arg5 as *const u8),
        numbers::SYS_UMOUNT2 => sys_umount2(arg1 as *const u8, arg2),
        numbers::SYS_MKFS => sys_mkfs(arg1 as *const u8, arg2),
        numbers::SYS_FCHMOD => sys_fchmod(arg1, arg2 as u32),
        numbers::SYS_FCHOWN => sys_fchown(arg1, arg2 as u32, arg3 as u32),
        numbers::SYS_SYMLINK => sys_symlink(arg1 as *const u8, arg2 as *const u8),
        numbers::SYS_READLINK => sys_readlink(arg1 as *const u8, arg2 as *mut u8, arg3),
        numbers::SYS_RENAME => sys_rename(arg1 as *const u8, arg2 as *const u8),
        numbers::SYS_ARCH_PRCTL => sys_arch_prctl(arg1, arg2),
        numbers::SYS_BEEP => sys_beep(arg1 as u32, arg2 as u32),
        numbers::SYS_SELECT => sys_select(arg1, arg2 as *mut u64, arg3 as *mut u64, arg4 as *mut u64, arg5 as *const u64),
        numbers::SYS_POLL => sys_poll(arg1 as *const u8, arg2 as usize, arg3 as i32),
        numbers::SYS_GETUID => sys_getuid(),
        numbers::SYS_GETGID => sys_getgid(),
        numbers::SYS_SETUID => sys_setuid(arg1),
        numbers::SYS_SETGID => sys_setgid(arg1),
        numbers::SYS_GETEUID => sys_geteuid(),
        numbers::SYS_GETEGID => sys_getegid(),
        numbers::SYS_IO_URING_SETUP => io_uring::sys_io_uring_setup(arg1),
        numbers::SYS_IO_URING_ENTER => io_uring::sys_io_uring_enter(arg1, arg2, arg3, arg4, arg5),
        numbers::SYS_BPF => {
            let ret = crate::ebpf::sys_bpf(arg1 as u32, arg2, arg3, arg4);
            ret as u64
        }
        numbers::SYS_SYNC => sys_sync(),
        numbers::SYS_REBOOT => sys_reboot(arg1, arg2),
        numbers::SYS_DRMCTL => sys_drmctl(arg1, arg2, arg3 as *mut u8),
        numbers::SYS_HASH => sys_hash(arg1, arg2 as *const u8, arg3, arg4 as *mut u8, arg5),
        numbers::SYS_STATFS => sys_statfs(arg1 as *const u8, arg2 as *mut u8),
        numbers::SYS_OPENPTY => sys_openpty(),
        numbers::SYS_SET_TID_ADDRESS => sys_set_tid_address(arg1 as *const u32),
        numbers::SYS_EXIT_GROUP => sys_exit_group(arg1),
        _ => {
            crate::println!("[SYSCALL] Unknown syscall: {} (0x{:x})", n, n);
            errno::Errno::ENOSYS as u64
        }
    };

    {
        let (handler, restorer, sig_num, sig_bit) = {
            let proc_lock = CURRENT_PROCESS.lock();
            let proc = match *proc_lock {
                Some(ref p) => p,
                None => return result,
            };
            let mut signals = proc.signals.lock();
            if !signals.has_pending() { return result; }

            let sig_bit = signals.pending.trailing_zeros();
            let sig_num = sig_bit + 1;
            let handler = proc.signal_handlers.lock()[sig_bit as usize];
            let restorer = proc.signal_restorers.lock()[sig_bit as usize];

            if handler == 1 {
                signals.pending &= !(1 << sig_bit);
                return result;
            }

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
            let k_ptr = (*crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + phys.as_u64()) as *mut SignalFrame;

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
            let ret_kptr = (*crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + ret_phys.as_u64()) as *mut u64;
            unsafe { *ret_kptr = restorer; }

            {
                let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
                if let Some(ref proc) = *proc_lock {
                    let mut signals = proc.signals.lock();
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

#[repr(C)]
struct SignalFrame {
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    r11: u64,
    r10: u64,
    r9: u64,
    r8: u64,
    rdi: u64,
    rsi: u64,
    rbp: u64,
    rbx: u64,
    rdx: u64,
    rcx: u64,
    rax: u64,
    rip: u64,
    rflags: u64,
    rsp: u64,
}

fn sys_rt_sigaction(sig: u64, act: *const u64, oldact: *mut u64) -> u64 {
    if sig == 0 || sig > 32 { return errno::Errno::EINVAL as u64; }
    let proc_lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let mut handlers = proc.signal_handlers.lock();
        let idx = (sig - 1) as usize;

        if !oldact.is_null() {
            let old_handler = handlers[idx];
            unsafe {
                if user_access::copy_to_user(oldact as *mut u8, core::slice::from_raw_parts(&old_handler as *const _ as *const u8, 8)).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
        }

        if !act.is_null() {
            let mut new_handler = 0u64;
            unsafe {
                if user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut new_handler as *mut _ as *mut u8, 8), act as *const u8).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
            handlers[idx] = new_handler;
        }
        return 0;
    }
    errno::Errno::ESRCH as u64
}

fn sys_rt_sigreturn(regs_ptr: *mut u64) -> u64 {
    let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
    let proc = match *proc_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let saved = proc.signals.lock().restore_context();
    let ctx = match saved {
        Some(c) => c,
        None => return errno::Errno::EINVAL as u64,
    };
    drop(proc_lock);

    // Restore registers from saved context
    unsafe {
        *regs_ptr.add(0)  = ctx.r15;
        *regs_ptr.add(1)  = ctx.r14;
        *regs_ptr.add(2)  = ctx.r13;
        *regs_ptr.add(3)  = ctx.r12;
        *regs_ptr.add(4)  = ctx.r11;
        *regs_ptr.add(5)  = ctx.r10;
        *regs_ptr.add(6)  = ctx.r9;
        *regs_ptr.add(7)  = ctx.r8;
        *regs_ptr.add(8)  = ctx.rdi;
        *regs_ptr.add(9)  = ctx.rsi;
        *regs_ptr.add(10) = ctx.rbp;
        *regs_ptr.add(11) = ctx.rbx;
        *regs_ptr.add(12) = ctx.rdx;
        *regs_ptr.add(13) = ctx.rcx;
        *regs_ptr.add(14) = ctx.rax;
        *regs_ptr.add(15) = ctx.rip;
        *regs_ptr.add(16) = ctx.rflags;
        *regs_ptr.add(17) = ctx.rsp;
    }
    ctx.rax
}

fn sys_read(fd: u64, buf: *mut u8, count: usize) -> u64 {
    let process = {
        let process_lock = CURRENT_PROCESS.lock();
        match *process_lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    let mut fd_table = process.fd_table.lock();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, ref mut offset }) => {
            // Reset offset for streaming devices (character, pipe) since
            // each read() returns a fresh snapshot. Detect via stat mode.
            if let Ok(stat) = node.stat() {
                let is_regular = (stat.st_mode & 0o170000) == 0o100000;
                if !is_regular { *offset = 0; }
            }
            match node.read(count) {
                Ok(data) => {
                    if *offset >= data.len() {
                        0
                    } else {
                        let available = data.len() - *offset;
                        let len = core::cmp::min(available, count);
                        if unsafe { user_access::copy_to_user(buf, &data[*offset..*offset + len]) }.is_err() {
                            return errno::Errno::EFAULT as u64;
                        }
                        *offset += len;
                        len as u64
                    }
                }
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::PtyMaster { _idx: _, ref pair }) => {
            let mut data = alloc::vec![0u8; count];
            match crate::pty::pty_read_master(pair, &mut data) {
                Ok(n) if n > 0 => {
                    if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n as u64
                },
                Ok(_) => 0,
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::PtySlave { _idx: _, ref pair }) => {
            let ldisc = crate::pty::PtyLineDiscipline::default();
            let mut data = alloc::vec![0u8; count];
            match crate::pty::pty_read_slave(pair, &mut data, &ldisc) {
                Ok(n) if n > 0 => {
                    if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_err() {
                        return errno::Errno::EFAULT as u64;
                    }
                    n as u64
                },
                Ok(_) => 0,
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::Socket(handle, _stype)) => {
            #[cfg(not(feature = "net"))]
            return errno::Errno::ENOSYS as u64;
            #[cfg(feature = "net")]
            {
                let mut sockets = crate::net::SOCKETS.lock();
                // Try TCP
                if let Some(n) = with_tcp_mut(&mut *sockets, handle, |socket| {
                    if socket.may_recv() {
                        let mut n = 0usize;
                        let result = socket.recv(|slice| {
                            n = core::cmp::min(slice.len(), count);
                            let ok = unsafe { user_access::copy_to_user(buf, &slice[..n]) }.is_ok();
                            (n, ok)
                        });
                        if result.unwrap_or(false) { return n as u64; }
                    }
                    0u64
                }) { return n; }
                // Try UDP
                if let Some(n) = with_udp_mut(&mut *sockets, handle, |socket| {
                    let mut data = vec![0u8; count];
                    if let Ok((n, _ep)) = socket.recv_slice(&mut data) {
                        if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_ok() {
                            return n as u64;
                        }
                        return errno::Errno::EFAULT as u64;
                    }
                    errno::Errno::EAGAIN as u64
                }) { return n; }
                errno::Errno::EAGAIN as u64
            }
        },
        None => errno::Errno::EBADF as u64,
    }
}

fn sys_write(fd: u64, buf: *const u8, count: usize) -> u64 {
    // Clone Arc and drop CURRENT_PROCESS early to avoid deadlock with timer ISR
    let process = {
        let process_lock = CURRENT_PROCESS.lock();
        match *process_lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    let mut fd_table = process.fd_table.lock();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, ref mut offset }) => {
            let mut data = vec![0u8; count];
            if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() {
                 return errno::Errno::EFAULT as u64;
            }
            match node.write(&data) {
                Ok(_) => {
                    *offset += count;
                    count as u64
                },
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::PtyMaster { _idx: _, ref pair }) => {
            let mut data = vec![0u8; count];
            if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            match crate::pty::pty_write_master(pair, &data) {
                Ok(n) => n as u64,
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::PtySlave { _idx: _, ref pair }) => {
            let mut data = vec![0u8; count];
            if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            match crate::pty::pty_write_slave(pair, &data) {
                Ok(n) => n as u64,
                Err(_) => errno::Errno::EIO as u64,
            }
        },
        Some(FileDescriptor::Socket(handle, _stype)) => {
            #[cfg(not(feature = "net"))]
            return errno::Errno::ENOSYS as u64;
            #[cfg(feature = "net")]
            {
                let mut write_data = vec![0u8; count];
                if unsafe { user_access::copy_from_user(&mut write_data, buf) }.is_err() {
                    return errno::Errno::EFAULT as u64;
                }

                let mut sockets = crate::net::SOCKETS.lock();
                // Try TCP
                if let Some(v) = with_tcp_mut(&mut *sockets, handle, |socket| {
                    if socket.may_send() {
                        let result = socket.send(|slice| {
                            let n = core::cmp::min(slice.len(), write_data.len());
                            slice[..n].copy_from_slice(&write_data[..n]);
                            (n, true)
                        });
                        if result.unwrap_or(false) { return count as u64; }
                    }
                    errno::Errno::EAGAIN as u64
                }) { return v; }
                // Try UDP — write() only works on connected sockets
                // For UDP, use sendto() instead
                errno::Errno::ENOSYS as u64
            }
        },
        None => errno::Errno::EBADF as u64,
    }
}

fn sys_open(path_ptr: *const u8, flags: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    const O_CREAT: i32 = 0x40;

    let vfs = VFS.lock();
    if let Some(node) = vfs.resolve_path(&path_str) {
        // Permission check: determine access needed from flags
        let acc_mode = (flags & 3) as u32; // O_RDONLY=0, O_WRONLY=1, O_RDWR=2
        let need = match acc_mode {
            1 => 2u32, // write
            2 => 6u32, // read+write
            _ => 4u32, // read
        };
        if !check_node_permission(&node, need) {
            return errno::Errno::EACCES as u64;
        }
        // LSM hook: file permission check
        let perm = match acc_mode {
            1 => "write",
            2 => "read",
            _ => "read",
        };
        let subj = crate::security::current_subject();
        if !crate::security::hook_file_perm(&subj, &path_str, perm) {
            return errno::Errno::EACCES as u64;
        }
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let mut fd_table = process.fd_table.lock();
            for (i, slot) in fd_table.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(FileDescriptor::File { node: node.clone() as Arc<dyn VfsNode>, offset: 0 });
                    return i as u64;
                }
            }
            fd_table.push(Some(FileDescriptor::File { node, offset: 0 }));
            return (fd_table.len() - 1) as u64;
        }
    } else if (flags & O_CREAT) != 0 {
        let last_slash = path_str.rfind('/').unwrap_or(0);
        let (parent_path, name) = if last_slash == 0 && !path_str.starts_with('/') {
            (".", path_str.as_str())
        } else if last_slash == 0 {
            ("/", &path_str[1..])
        } else {
            (&path_str[..last_slash], &path_str[last_slash+1..])
        };

        // For O_CREAT, check write+execute on parent directory
        if let Some(parent_node) = vfs.resolve_path(parent_path) {
            if !check_node_permission(&parent_node, 3) { // w+x
                return errno::Errno::EACCES as u64;
            }
            if let Ok(new_node) = parent_node.create(name) {
                let process_lock = CURRENT_PROCESS.lock();
                if let Some(ref process) = *process_lock {
                    let mut fd_table = process.fd_table.lock();
                    fd_table.push(Some(FileDescriptor::File { node: new_node, offset: 0 }));
                    return (fd_table.len() - 1) as u64;
                }
            }
        }
    }
    errno::Errno::ENOENT as u64
}

fn sys_close(fd: u64) -> u64 {
    let process = {
        let process_lock = CURRENT_PROCESS.lock();
        match *process_lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };
    let mut fd_table = process.fd_table.lock();
    if (fd as usize) < fd_table.len() {
        if let Some(ref desc) = fd_table[fd as usize] {
            // Clean up sockets to prevent fd leak
            if let FileDescriptor::Socket(handle, _stype) = desc {
                #[cfg(feature = "net")]
                {
                    crate::net::SOCKETS.lock().remove(*handle);
                }
            }
        }
        fd_table[fd as usize] = None;
        return 0;
    }
    errno::Errno::EBADF as u64
}

fn sys_stat(path_ptr: *const u8, stat_buf: *mut Stat) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    if let Some(node) = VFS.lock().resolve_path(&path_str) {
        if let Ok(stat) = node.stat() {
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}

fn sys_statfs(path_ptr: *const u8, statfs_buf: *mut u8) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    let vfs = VFS.lock();
    if let Some(node) = vfs.resolve_path(&path_str) {
        if let Ok(statfs) = node.statfs() {
            let slice = unsafe {
                core::slice::from_raw_parts(&statfs as *const _ as *const u8, core::mem::size_of::<crate::vfs::StatFs>())
            };
            if unsafe { user_access::copy_to_user(statfs_buf, slice) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            return 0;
        }
    }
    if let Some(root) = vfs.statfs_mount(&path_str) {
        if let Ok(statfs) = root.statfs() {
            let slice = unsafe {
                core::slice::from_raw_parts(&statfs as *const _ as *const u8, core::mem::size_of::<crate::vfs::StatFs>())
            };
            if unsafe { user_access::copy_to_user(statfs_buf, slice) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}

fn sys_fstat(fd: u64, stat_buf: *mut Stat) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let fd_table = process.fd_table.lock();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            if let Ok(stat) = node.stat() {
                if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() {
                     return errno::Errno::EFAULT as u64;
                }
                return 0;
            }
            errno::Errno::EIO as u64
        },
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => {
            let mut stat = Stat::default();
            stat.st_mode = 0o020000 | 0o620; // character device, rw-rw----
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            0
        },
        Some(FileDescriptor::Socket(_, _)) => {
            let mut stat = Stat::default();
            stat.st_mode = 0o140000 | 0o666; // socket, rw-rw-rw-
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            0
        },
        None => errno::Errno::EBADF as u64,
    }
}

pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;

fn sys_lseek(fd: u64, offset: i64, whence: i32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let mut fd_table = process.fd_table.lock();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }

    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, offset: ref mut fd_offset }) => {
            let file_size = if let Ok(stat) = node.stat() {
                stat.st_size as i64
            } else {
                return errno::Errno::EIO as u64;
            };

            let new_offset = match whence {
                SEEK_SET => offset,
                SEEK_CUR => (*fd_offset as i64) + offset,
                SEEK_END => file_size + offset,
                _ => return errno::Errno::EINVAL as u64,
            };

            if new_offset < 0 {
                return errno::Errno::EINVAL as u64;
            }

            *fd_offset = new_offset as usize;
            *fd_offset as u64
        },
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => {
            errno::Errno::ESPIPE as u64
        },
        Some(FileDescriptor::Socket(_, _)) => {
            errno::Errno::ESPIPE as u64
        },
        None => errno::Errno::EBADF as u64,
    }
}

fn sys_brk(addr: u64) -> u64 {
    let process = {
        let process_lock = CURRENT_PROCESS.lock();
        match *process_lock {
            Some(ref p) => p.clone(),
            None => return 0,
        }
    };

    let mut brk = process.brk.lock();
    if addr == 0 {
        return *brk;
    }

    if addr > *brk {
        // Demand-paged expansion: just update the brk value.
        // The page fault handler will map pages on demand.
        *brk = addr;
    }
    *brk
}

fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, _fd: u64, _offset: u64) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return -(errno::Errno::ESRCH as i64) as u64,
    };

    const _MAP_PRIVATE: u64 = 0x02;
    const MAP_ANONYMOUS: u64 = 0x20;

    if (flags & MAP_ANONYMOUS) == 0 {
        return -(errno::Errno::ENOSYS as i64) as u64;
    }

    let mut mmap_addr = addr;
    if mmap_addr == 0 {
        // ASLR: pick a random mmap base in [MMAP_MIN, MMAP_MAX) page-aligned.
        // Sequential fallback ensures forward progress if RDTSC is somehow predictable.
        const MMAP_MIN: u64 = 0x4000_0000_0000;
        const MMAP_MAX: u64 = 0x7F00_0000_0000;
        static MMAP_NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(MMAP_MIN);
        let lo: u32;
        let hi: u32;
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags)); }
        let entropy = ((hi as u64) << 32) | (lo as u64);
        let range = MMAP_MAX - MMAP_MIN;
        // Mix entropy with sequential counter for spacing
        let rand_offset = (entropy.wrapping_mul(len + 1)) & (range - 1);
        mmap_addr = MMAP_MIN + (rand_offset & !0xFFF);
        if mmap_addr < MMAP_MIN { mmap_addr = MMAP_MIN; }
        if mmap_addr >= MMAP_MAX { mmap_addr = MMAP_MIN; }
        // Advance sequential cursor past this allocation
        MMAP_NEXT.store(mmap_addr.wrapping_add((len + 4095) & !4095), core::sync::atomic::Ordering::Relaxed);
    }

    let len_aligned = (len + 4095) & !4095;

    use crate::memory::buddy::BuddyFrameAllocator;
    let mut frame_allocator = BuddyFrameAllocator;
    let mut mapper = if let Some(m) = unsafe { process.address_space.mapper() } { m } else { return -(errno::Errno::ENOMEM as i64) as u64; };

    let start_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(mmap_addr));
    let end_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(mmap_addr + len_aligned - 1));

    let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if (prot & 0x2) != 0 {
        page_flags |= PageTableFlags::WRITABLE;
    }
    if (prot & 0x4) == 0 {
        page_flags |= PageTableFlags::NO_EXECUTE;
    }

    for page in Page::range_inclusive(start_page, end_page) {
        if let Some(frame) = frame_allocator.allocate_frame() {
            unsafe {
                match mapper.map_to(page, frame, page_flags, &mut frame_allocator) {
                    Ok(t) => { t.flush(); }
                    Err(_e) => { return -(errno::Errno::ENOMEM as i64) as u64; }
                }
            }
            crate::memory::frame_info::increment(frame.start_address());
        } else {
            return -(errno::Errno::ENOMEM as i64) as u64;
        }
    }

    process.add_vma(crate::task::process::Vma {
        start: mmap_addr,
        end: mmap_addr + len_aligned,
        flags: page_flags,
        _name: "mmap",
    });

    mmap_addr
}

fn sys_munmap(addr: u64, len: u64) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let len_aligned = (len + 4095) & !4095;
    let start_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(addr));
    let end_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(addr + len_aligned - 1));

    let mut mapper = if let Some(m) = unsafe { process.address_space.mapper() } { m } else { return errno::Errno::EINVAL as u64; };

    for page in Page::range_inclusive(start_page, end_page) {
        if let Ok((frame, t)) = mapper.unmap(page) {
            t.flush();
            crate::memory::frame_info::decrement(frame.start_address());
        }
    }

    // Clean up VMA entries
    process.remove_vma_range(addr, addr + len_aligned);

    0
}

fn sys_exit(status: u64) -> u64 {
    let (parent_pid, clear_tid) = {
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            *process.exit_code.lock() = Some(status as i32);
            if status != 42 {
                crate::println!("[PROCESS] Pid {} exited with status {}", process.id, status);
            }
            (process.parent_id, *process.clear_child_tid.lock())
        } else {
            (None, 0)
        }
    };
    
    // Clear child tid and wake futex (for pthread_join)
    if clear_tid != 0 {
        let zero = 0u32;
        let _ = unsafe { user_access::copy_to_user(clear_tid as *mut u8, core::slice::from_raw_parts(&zero as *const _ as *const u8, 4)) };
        let _ = sys_futex(clear_tid as *mut u32, 1, 1);
    }
    
    // Send SIGCHLD to parent process
    if let Some(ppid) = parent_pid {
        let table = crate::task::process::PROCESS_TABLE.lock();
        if let Some(parent) = table.get(&ppid) {
            parent.signals.lock().raise(crate::syscalls::signal::Signal::SIGCHLD);
        }
        drop(table);
    }
    
    // Mark current thread as exited
    if let Some(mut thread) = crate::task::scheduler::current_thread() {
        thread.status = crate::task::thread::ThreadStatus::Exited;
        crate::task::scheduler::set_current_thread(thread);
    }
    crate::task::scheduler::schedule();
}

fn sys_set_tid_address(tidptr: *const u32) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        *proc.clear_child_tid.lock() = tidptr as u64;
        proc.id
    } else {
        0
    }
}

fn sys_exit_group(status: u64) -> u64 {
    crate::println!("[PROCESS] Thread group exited with {}", status);
    sys_exit(status)
}

fn sys_nanosleep(seconds: u64, nanoseconds: u64) -> u64 {
    // 1 tick = 1 timer interrupt. Assuming 100Hz = 10ms per tick.
    // This is a rough estimation for now.
    let ms = (seconds * 1000) + (nanoseconds / 1_000_000);
    let sleep_ticks = core::cmp::max(1, ms / 10); // Minimum 1 tick
    
    let target_tick = crate::interrupts::get_ticks() + sleep_ticks;

    if let Some(mut current_thread) = crate::task::scheduler::current_thread() {
        current_thread.status = crate::task::thread::ThreadStatus::Blocked;
        current_thread.sleep_until = Some(target_tick);
        crate::task::scheduler::add_sleeping_thread(*current_thread);
    }
    
    crate::task::scheduler::schedule();
}

fn sys_futex(uaddr: *mut u32, op: u32, val: u32) -> u64 {
    const FUTEX_WAIT: u32 = 0;
    const FUTEX_WAKE: u32 = 1;

    match op {
        FUTEX_WAIT => {
            let current_val = unsafe { core::ptr::read_volatile(uaddr) };
            if current_val != val {
                return errno::Errno::EAGAIN as u64;
            }
            if let Some(mut current_thread) = crate::task::scheduler::current_thread() {
                current_thread.status = crate::task::thread::ThreadStatus::Blocked;
                current_thread.futex_wake_addr = Some(uaddr as u64);
                crate::task::scheduler::add_futex_thread(*current_thread);
            }
            crate::task::scheduler::schedule();
        }
        FUTEX_WAKE => {
            crate::task::scheduler::wake_futex(uaddr as u64, val) as u64
        }
        _ => errno::Errno::ENOSYS as u64,
    }
}

fn sys_sysinfo(buf: *mut u64) -> u64 {
    let uptime_ticks = crate::interrupts::get_ticks();
    let uptime_secs = uptime_ticks / 100;
    let info = [
        0u64,                            // total_ram (pages)
        0u64,                            // free_ram (pages)
        uptime_secs,                     // uptime_seconds
        0u64,                            // processes
        1u64,                            // load_avg_1m (1<<16 fixed point)
    ];
    if unsafe { crate::syscalls::user_access::copy_to_user(
        buf as *mut u8,
        core::slice::from_raw_parts(
            info.as_ptr() as *const u8,
            info.len() * 8,
        ),
    ) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

fn sys_arch_prctl(code: u64, addr: u64) -> u64 {
    use x86_64::instructions::segmentation::Segment64;
    const ARCH_SET_FS: u64 = 0x1002;
    const ARCH_GET_FS: u64 = 0x1003;

    match code {
        ARCH_SET_FS => {
            unsafe {
                x86_64::registers::segmentation::FS::write_base(x86_64::VirtAddr::new(addr));
            }
            0
        }
        ARCH_GET_FS => {
            let base = x86_64::registers::segmentation::FS::read_base();
            if addr != 0 {
                let val = base.as_u64();
                if unsafe { user_access::copy_to_user(addr as *mut u8, core::slice::from_raw_parts(&val as *const _ as *const u8, 8)) }.is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
            0
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

fn sys_beep(freq_hz: u32, duration_ms: u32) -> u64 {
    crate::drivers::audio::pcspeaker::beep(freq_hz, duration_ms);
    0
}

fn sys_sched_yield() -> u64 {
    use crate::task::scheduler;
    let switch = {
        let mut sched = scheduler::this_cpu_sched().lock();
        sched.prepare_switch_tls()
    };
    if let Some((old_ptr, new_sp, new_fs)) = switch {
        crate::task::thread::switch_thread(old_ptr, new_sp, new_fs);
    }
    0
}

fn sys_sched_setattr(pid: i64, attr_ptr: *const u8, _flags: u64) -> u64 {
    let proc = if pid == 0 {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        match *lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    } else {
        let table = crate::task::process::PROCESS_TABLE.lock();
        match table.get(&(pid as u64)) {
            Some(p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    if attr_ptr.is_null() { return errno::Errno::EFAULT as u64; }

    let size = unsafe { *(attr_ptr as *const u32) };
    if size < 8 { return errno::Errno::EINVAL as u64; }

    let policy = unsafe { *(attr_ptr.add(4) as *const u32) };
    if policy != 0 { return errno::Errno::EINVAL as u64; } // Only SCHED_OTHER

    let nice = if size >= 12 {
        unsafe { *(attr_ptr.add(8) as *const i32) }
    } else {
        0
    };

    // Map nice [-20..19] to priority [0..7]
    let priority = if nice <= -15 { 7u8 }
        else if nice <= -10 { 6u8 }
        else if nice <= -5  { 5u8 }
        else if nice <= 0   { 4u8 }
        else if nice <= 5   { 3u8 }
        else if nice <= 10  { 2u8 }
        else if nice <= 15  { 1u8 }
        else { 0u8 };

    // Update current thread priority if it belongs to the target process
    let mut sched = crate::task::scheduler::this_cpu_sched().lock();
    if let Some(ref mut cur) = sched.current_thread {
        if let Some(ref p) = cur.process {
            if p.id == proc.id {
                cur.priority = priority;
            }
        }
    }
    drop(sched);

    // Update global pending queue threads
    let mut global = crate::task::scheduler::GLOBAL.lock();
    for t in global.pending_queue.iter_mut() {
        if let Some(ref p) = t.process {
            if p.id == proc.id {
                t.priority = priority;
            }
        }
    }
    drop(global);
    0
}

fn sys_sched_getattr(pid: i64, attr_ptr: *mut u8, size: u64, _flags: u64) -> u64 {
    let target = if pid == 0 {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        match *lock {
            Some(ref p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    } else {
        let table = crate::task::process::PROCESS_TABLE.lock();
        match table.get(&(pid as u64)) {
            Some(p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    if attr_ptr.is_null() { return errno::Errno::EFAULT as u64; }
    let out_size = if size == 0 { 12u32 } else { size as u32 };

    // Get current thread priority if it belongs to target process
    let priority = {
        let sched = crate::task::scheduler::this_cpu_sched().lock();
        if let Some(ref cur) = sched.current_thread {
            if let Some(ref p) = cur.process {
                if p.id == target.id { cur.priority } else { 3u8 }
            } else { 3u8 }
        } else { 3u8 }
    };

    let nice = match priority {
        7 => -20, 6 => -10, 5 => -5, 4 => 0,
        3 => 5, 2 => 10, 1 => 15, _ => 19,
    };

    if unsafe { user_access::copy_to_user(attr_ptr as *mut u8, core::slice::from_raw_parts(&out_size as *const _ as *const u8, 4)) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    if out_size >= 8 {
        let zero = 0u32;
        if unsafe { user_access::copy_to_user(attr_ptr.add(4) as *mut u8, core::slice::from_raw_parts(&zero as *const _ as *const u8, 4)) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    if out_size >= 12 {
        let nice_le = nice as u32;
        if unsafe { user_access::copy_to_user(attr_ptr.add(8) as *mut u8, core::slice::from_raw_parts(&nice_le as *const _ as *const u8, 4)) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    0
}

fn sys_getdents64(fd: u64, buf: *mut u8, len: usize) -> u64 {
    use crate::vfs::VFS;
    let _vfs = VFS.lock();
    let proc = CURRENT_PROCESS.lock();
    let node = if let Some(ref p) = *proc {
        let fd_table = p.fd_table.lock();
        if let Some(entry) = fd_table.get(fd as usize) {
            match entry {
                Some(crate::task::process::FileDescriptor::File { node, .. }) => node.clone(),
                _ => return errno::Errno::EBADF as u64,
            }
        } else {
            return errno::Errno::EBADF as u64;
        }
    } else {
        return errno::Errno::EBADF as u64;
    };
    if !node.is_dir() { return errno::Errno::ENOTDIR as u64; }
    drop(proc);

    let children = match node.children() {
        Ok(c) => c,
        Err(_) => return errno::Errno::EIO as u64,
    };
    let mut written: usize = 0;
    for child in &children {
        let name = child.name();
        let name_bytes = name.as_bytes();
        let reclen = ((core::mem::size_of::<u64>() * 3) + name_bytes.len() + 1 + 7) & !7;
        if written + reclen > len { break; }

        #[repr(C)]
        struct LinuxDirent64 {
            d_ino: u64,
            d_off: u64,
            d_reclen: u16,
            d_type: u8,
        }

        let entry_offset = written;
        let d_type = if child.is_dir() { 4u8 } else { 8u8 };
        let dirent = LinuxDirent64 {
            d_ino: 1,
            d_off: (written + reclen) as u64,
            d_reclen: reclen as u16,
            d_type,
        };

        let dirent_bytes = unsafe {
            core::slice::from_raw_parts(
                &dirent as *const _ as *const u8,
                core::mem::size_of::<LinuxDirent64>(),
            )
        };

        if unsafe { buf.add(entry_offset) }.is_null() { return errno::Errno::EFAULT as u64; }
        unsafe {
            if user_access::copy_to_user(buf.add(entry_offset), dirent_bytes).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }

        let name_offset = entry_offset + core::mem::size_of::<LinuxDirent64>();
        unsafe {
            if user_access::copy_to_user(buf.add(name_offset), name_bytes).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }

        if name_offset + name_bytes.len() < entry_offset + reclen {
            let null_byte = [0u8];
            unsafe {
                if user_access::copy_to_user(buf.add(name_offset + name_bytes.len()), &null_byte).is_err() {
                    return errno::Errno::EFAULT as u64;
                }
            }
        }

        written += reclen;
    }

    written as u64
}

fn sys_ioctl(fd: u64, request: u64, argp: *mut u8) -> u64 {
    const TIOCGWINSZ: u64 = 0x5413;
    const TCGETS: u64 = 0x5401;
    const TCSETS: u64 = 0x5402;
    const FIONBIO: u64 = 0x5421;

    #[repr(C)]
    struct Winsize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    #[repr(C)]
    struct Termios {
        c_iflag: u32,
        c_oflag: u32,
        c_cflag: u32,
        c_lflag: u32,
        c_cc: [u8; 19],
    }

    match request {
        TIOCGWINSZ => {
            let cols = crate::drivers::graphics::WIDTH.load(core::sync::atomic::Ordering::Relaxed) / 8;
            let rows = crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed) / 16;
            let ws = Winsize {
                ws_row: rows as u16,
                ws_col: cols as u16,
                ws_xpixel: (cols * 8) as u16,
                ws_ypixel: (rows * 16) as u16,
            };
            if unsafe { user_access::copy_to_user(argp, core::slice::from_raw_parts(
                &ws as *const _ as *const u8, core::mem::size_of::<Winsize>(),
            )) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            0
        }
        TCGETS => {
            let t = Termios {
                c_iflag: 0,
                c_oflag: 0,
                c_cflag: 0xBF, // CLOCAL | CREAD | CS8
                c_lflag: 0x5,  // ICANON | ECHO
                c_cc: [0; 19],
            };
            if unsafe { user_access::copy_to_user(argp, core::slice::from_raw_parts(
                &t as *const _ as *const u8, core::mem::size_of::<Termios>(),
            )) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            0
        }
        TCSETS => 0,
        FIONBIO => 0,
        _ => {
            // Try node-specific ioctl (block devices, etc.)
            let process = {
                let process_lock = CURRENT_PROCESS.lock();
                match *process_lock {
                    Some(ref p) => p.clone(),
                    None => return errno::Errno::ESRCH as u64,
                }
            };
            let fd_table = process.fd_table.lock();
            if (fd as usize) >= fd_table.len() {
                return errno::Errno::EBADF as u64;
            }
            match fd_table[fd as usize] {
                Some(crate::task::process::FileDescriptor::File { ref node, .. }) => {
                    match node.ioctl(request, argp) {
                        Ok(ret) => ret,
                        Err(_) => errno::Errno::ENOTTY as u64,
                    }
                }
                _ => errno::Errno::ENOTTY as u64,
            }
        }
    }
}

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn sys_clock_gettime(clock_id: u64, tp: *mut Timespec) -> u64 {
    if tp.is_null() { return errno::Errno::EFAULT as u64; }
    const CLOCK_REALTIME: u64 = 0;
    const CLOCK_MONOTONIC: u64 = 1;
    let ts = match clock_id {
        CLOCK_REALTIME => {
            let (sec, nsec) = crate::drivers::rtc::read_realtime();
            Timespec { tv_sec: sec, tv_nsec: nsec }
        }
        CLOCK_MONOTONIC => {
            let ticks = crate::interrupts::get_ticks();
            let total_ms = ticks * 10;
            Timespec {
                tv_sec: (total_ms / 1000) as i64,
                tv_nsec: ((total_ms % 1000) * 1_000_000) as i64,
            }
        }
        _ => return errno::Errno::EINVAL as u64,
    };
    if unsafe { user_access::copy_to_user(tp as *mut u8, core::slice::from_raw_parts(
        &ts as *const _ as *const u8, core::mem::size_of::<Timespec>(),
    )) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

fn sys_mount(source: *const u8, target: *const u8, fstype: *const u8, _flags: u64, _data: *const u8) -> u64 {
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_ADMIN) {
        audit_log("CAP_SYS_ADMIN", "mount DENIED");
        return errno::Errno::EPERM as u64;
    }
    let mut src_buf = [0u8; 256];
    let mut tgt_buf = [0u8; 256];
    let mut fs_buf = [0u8; 32];

    if unsafe { user_access::copy_from_user(&mut src_buf[..255], source).is_err() } { return errno::Errno::EFAULT as u64; }
    if unsafe { user_access::copy_from_user(&mut tgt_buf[..255], target).is_err() } { return errno::Errno::EFAULT as u64; }
    if unsafe { user_access::copy_from_user(&mut fs_buf[..31], fstype).is_err() } { return errno::Errno::EFAULT as u64; }

    let _src_str = match core::ffi::CStr::from_bytes_until_nul(&src_buf) {
        Ok(c) => match c.to_str() { Ok(s) => s, Err(_) => return errno::Errno::EINVAL as u64 },
        Err(_) => return errno::Errno::EINVAL as u64,
    };
    let tgt_str = match core::ffi::CStr::from_bytes_until_nul(&tgt_buf) {
        Ok(c) => match c.to_str() { Ok(s) => s, Err(_) => return errno::Errno::EINVAL as u64 },
        Err(_) => return errno::Errno::EINVAL as u64,
    };
    let fs_str = match core::ffi::CStr::from_bytes_until_nul(&fs_buf) {
        Ok(c) => match c.to_str() { Ok(s) => s, Err(_) => return errno::Errno::EINVAL as u64 },
        Err(_) => return errno::Errno::EINVAL as u64,
    };

    // For filesystems that need a block device, iterate registered devices
    let devices = crate::drivers::block::BLOCK_DEVICES.lock();

    let fs: Option<alloc::sync::Arc<dyn crate::vfs::FileSystem>> = match fs_str {
        "tmpfs" => Some(alloc::sync::Arc::new(crate::vfs::ramfs::Tmpfs::new())),
        "devfs" => Some(alloc::sync::Arc::new(crate::vfs::devfs::DevFs::new())),
        "ctlfs" => Some(alloc::sync::Arc::new(crate::vfs::ctlfs::CtlFs::new())),
        "ext2" => {
            // Try each block device for ext2
            let mut found = None;
            for dev in devices.iter() {
                if let Ok(ext2fs) = crate::vfs::ext2::mount(dev.clone()) {
                    found = Some(ext2fs as alloc::sync::Arc<dyn crate::vfs::FileSystem>);
                    break;
                }
            }
            found
        }
        "skyfs" => {
            let mut found = None;
            for dev in devices.iter() {
                if let Ok(skyfs) = crate::vfs::skyfs::SkyFSHandle::mount(dev.clone()) {
                    found = Some(skyfs as alloc::sync::Arc<dyn crate::vfs::FileSystem>);
                    break;
                }
            }
            found
        }
        _ => None,
    };
    drop(devices);

    let fs = match fs {
        Some(f) => f,
        None => return errno::Errno::ENODEV as u64,
    };

    let mut vfs = crate::vfs::VFS.lock();
    vfs.mount(tgt_str, fs);
    0
}

fn sys_fchmod(fd: u64, mode: u32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.fd_table.lock();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            if !check_file_owner(node) {
                return errno::Errno::EACCES as u64;
            }
            if node.chmod(mode).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
        },
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => errno::Errno::ENOSYS as u64,
        Some(FileDescriptor::Socket(_, _)) => errno::Errno::ENOSYS as u64,
        None => errno::Errno::EBADF as u64,
    }
}

fn sys_fchown(fd: u64, uid: u32, gid: u32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.fd_table.lock();
    if (fd as usize) >= fd_table.len() {
        return errno::Errno::EBADF as u64;
    }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            if !check_file_owner(node) {
                return errno::Errno::EACCES as u64;
            }
            if node.chown(uid, gid).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
        },
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => errno::Errno::ENOSYS as u64,
        Some(FileDescriptor::Socket(_, _)) => errno::Errno::ENOSYS as u64,
        None => errno::Errno::EBADF as u64,
    }
}

fn sys_umount2(target: *const u8, _flags: u64) -> u64 {
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_ADMIN) {
        audit_log("CAP_SYS_ADMIN", "umount DENIED");
        return errno::Errno::EPERM as u64;
    }
    let path_str = match unsafe { user_access::read_user_string(target, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    match VFS.lock().umount(&path_str) {
        Ok(_) => 0,
        Err(_) => errno::Errno::EINVAL as u64,
    }
}

fn sys_mkfs(fstype: *const u8, device: u64) -> u64 {
    if get_current_euid() != 0 && !has_capability(CAP_SYS_ADMIN) {
        audit_log("CAP_SYS_ADMIN", "mkfs DENIED");
        return errno::Errno::EPERM as u64;
    }
    let fs_type = match unsafe { user_access::read_user_string(fstype, 32) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    let blk = crate::drivers::block::BLOCK_DEVICES.lock();
    let dev = match blk.get(device as usize) {
        Some(d) => d.clone(),
        None => return errno::Errno::ENODEV as u64,
    };
    drop(blk);

    match fs_type.as_str() {
        "skyfs" => {
            if crate::vfs::skyfs::SkyFSHandle::format(dev).is_ok() {
                0
            } else {
                errno::Errno::EIO as u64
            }
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

fn sys_symlink(target: *const u8, linkpath: *const u8) -> u64 {
    let target_str = match unsafe { user_access::read_user_string(target, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    let linkpath_str = match unsafe { user_access::read_user_string(linkpath, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    let vfs = crate::vfs::VFS.lock();
    let (parent_path, name) = split_parent(&linkpath_str);
    if let Some(parent) = vfs.resolve_path(&parent_path) {
        if !check_node_permission(&parent, 2) {
            return errno::Errno::EACCES as u64;
        }
        if parent.symlink(&name, &target_str).is_ok() {
            0
        } else {
            errno::Errno::EPERM as u64
        }
    } else {
        errno::Errno::ENOENT as u64
    }
}

fn sys_readlink(pathname: *const u8, buf: *mut u8, bufsize: u64) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(pathname, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    let vfs = crate::vfs::VFS.lock();
    if let Some(node) = vfs.resolve_path(&path_str) {
        match node.readlink() {
            Ok(target) => {
                let len = core::cmp::min(target.len(), bufsize as usize);
                unsafe {
                    core::ptr::copy_nonoverlapping(target.as_ptr(), buf, len);
                }
                len as u64
            }
            Err(_) => errno::Errno::EINVAL as u64,
        }
    } else {
        errno::Errno::ENOENT as u64
    }
}

fn split_parent(path: &str) -> (String, String) {
    let trimmed = path.trim_end_matches('/');
    if let Some(pos) = trimmed.rfind('/') {
        if pos == 0 {
            (String::from("/"), String::from(&trimmed[1..]))
        } else {
            (String::from(&trimmed[..pos]), String::from(&trimmed[pos + 1..]))
        }
    } else {
        (String::from("/"), String::from(trimmed))
    }
}

fn sys_fork(regs_ptr: *mut u64) -> u64 {
    use crate::task::process::{Process, CURRENT_PROCESS};
    use crate::memory::buddy::BuddyFrameAllocator;

    let parent_lock = CURRENT_PROCESS.lock();
    if let Some(ref parent) = *parent_lock {
        let parent_id = parent.id;
        
        // 1. Clone Address Space with CoW
        let mut frame_allocator = BuddyFrameAllocator;
        let child_as = match parent.address_space.clone_cow(&mut frame_allocator) {
            Some(as_space) => as_space,
            None => return errno::Errno::ENOMEM as u64,
        };

        // 2. Create new Process
        let child_pid = Process::next_id();
        let mut child_process = Process::new(child_pid, Some(parent_id), child_as);
        {
            let parent_vmas = parent.vmas.lock();
            child_process.vmas = Mutex::new(parent_vmas.clone());
        }                    child_process.entry_point = parent.entry_point;
            *child_process.fd_table.lock() = parent.fd_table.lock().clone();
            *child_process.fd_flags.lock() = parent.fd_flags.lock().clone();
        let child_arc = Arc::new(child_process);
        
        // Track child in parent and global table
        parent.children.lock().push(child_pid);
        crate::task::process::Process::register(child_arc.clone());

        // 3. Clone current thread (deep copy stack)
        if let Some(ref current_thread) = crate::task::scheduler::this_cpu_sched().lock().current_thread {
            let child_thread: crate::task::thread::Thread = current_thread.clone_fork(child_arc, regs_ptr);
            
            // 4. Add to scheduler
            crate::task::scheduler::spawn_thread(child_thread);
            
            return child_pid;
        }
    }
    
    errno::Errno::EPERM as u64 
}

fn sys_clone(flags: u64, child_stack: u64, parent_tid: *mut u32, child_tls: u64, child_tidptr: *mut u32, regs_ptr: *mut u64) -> u64 {
    use crate::task::process::{Process, CURRENT_PROCESS};
    use crate::memory::buddy::BuddyFrameAllocator;

    const CLONE_SETTLS: u64 = 0x80000;
    const CLONE_PARENT_SETTID: u64 = 0x00100000;
    const CLONE_CHILD_SETTID: u64 = 0x02000000;
    const CLONE_CHILD_CLEARTID: u64 = 0x00200000;

    let parent_lock = CURRENT_PROCESS.lock();
    if let Some(ref parent) = *parent_lock {
        let child_pid = Process::next_id();

        let child_as = match parent.address_space.clone_cow(&mut BuddyFrameAllocator) {
            Some(as_space) => as_space,
            None => return errno::Errno::ENOMEM as u64,
        };

        let mut child_process = Process::new(child_pid, Some(parent.id), child_as);
        {
            let parent_vmas = parent.vmas.lock();
            child_process.vmas = Mutex::new(parent_vmas.clone());
        }
        child_process.entry_point = parent.entry_point;
        *child_process.fd_table.lock() = parent.fd_table.lock().clone();
        *child_process.fd_flags.lock() = parent.fd_flags.lock().clone();
        *child_process.signal_handlers.lock() = parent.signal_handlers.lock().clone();

        if flags & CLONE_CHILD_CLEARTID != 0 && !child_tidptr.is_null() {
            *child_process.clear_child_tid.lock() = child_tidptr as u64;
        }

        if flags & CLONE_CHILD_SETTID != 0 && !child_tidptr.is_null() {
            unsafe { core::ptr::write_unaligned(child_tidptr, child_pid as u32); }
        }

        if flags & CLONE_PARENT_SETTID != 0 && !parent_tid.is_null() {
            unsafe { core::ptr::write_unaligned(parent_tid, child_pid as u32); }
        }

        let child_arc = Arc::new(child_process);

        parent.children.lock().push(child_pid);
        crate::task::process::Process::register(child_arc.clone());

        if let Some(ref current_thread) = crate::task::scheduler::this_cpu_sched().lock().current_thread {
            let mut child_thread = current_thread.clone_thread(child_arc, regs_ptr, child_stack);

            if flags & CLONE_SETTLS != 0 {
                child_thread.fs_base = child_tls;
            }

            crate::task::scheduler::spawn_thread(child_thread);
            return child_pid;
        }
    }

    errno::Errno::EPERM as u64
}

fn sys_wait4(pid: i64, status_ptr: *mut i32, _options: i32, _rusage: *mut u8) -> u64 {
    let parent_id = {
        let lock = CURRENT_PROCESS.lock();
        if let Some(ref p) = *lock { p.id } else { return errno::Errno::ESRCH as u64; }
    };

    let mut child_to_reap = None;
    loop {
        // Find an exited child
        {
            let process_table = crate::task::process::PROCESS_TABLE.lock();
            let parent = match process_table.get(&parent_id) {
                Some(p) => p,
                None => { return 0; }
            };
            let children_pids = parent.children.lock();

            for (index, &child_pid) in children_pids.iter().enumerate() {
                if pid != -1 && child_pid != pid as u64 {
                    continue;
                }
                
                if let Some(child) = process_table.get(&child_pid) {
                    let exit_status = child.exit_code.lock();
                    if let Some(status) = *exit_status {
                        child_to_reap = Some((child_pid, status, index));
                        break;
                    }
                }
            }
        }

        if let Some((child_pid, status, index)) = child_to_reap.take() {
            if !status_ptr.is_null() {
                unsafe { *status_ptr = status; }
            }
            
            {
                let process_table = crate::task::process::PROCESS_TABLE.lock();
                let parent = process_table.get(&parent_id).unwrap();
                parent.children.lock().remove(index);
            }
            crate::task::process::PROCESS_TABLE.lock().remove(&child_pid);
            return child_pid;
        }

        // No child exited yet — yield to other threads (child gets a chance to run)
        crate::task::scheduler::try_schedule();
    }
}

fn sys_kill(pid: i64, sig: u32) -> u64 {
    let sig_enum = match sig {
        1 => crate::syscalls::signal::Signal::SIGHUP,
        2 => crate::syscalls::signal::Signal::SIGINT,
        9 => crate::syscalls::signal::Signal::_SIGKILL,
        10 => crate::syscalls::signal::Signal::_SIGUSR1,
        11 => crate::syscalls::signal::Signal::_SIGSEGV,
        15 => crate::syscalls::signal::Signal::_SIGTERM,
        _ => return errno::Errno::EINVAL as u64,
    };

    let euid = get_current_euid();
    let table = crate::task::process::PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&(pid as u64)) {
        // Only root or same user (or CAP_KILL) can send signals
        let target_uid = *proc.uid.lock();
        if euid != 0 && euid != target_uid && !has_capability(CAP_KILL) {
            audit_log("CAP_KILL", &alloc::format!("kill({},{}) DENIED", pid, sig));
            return errno::Errno::EPERM as u64;
        }
        // LSM hook: process kill check
        let subj = crate::security::current_subject();
        if !crate::security::hook_file_perm(&subj, &alloc::format!("pid:{}", pid), "kill") {
            return errno::Errno::EPERM as u64;
        }
        proc.signals.lock().raise(sig_enum);
        return 0;
    }
    errno::Errno::ESRCH as u64
}

fn sys_getpid() -> u64 {
    use crate::task::process::CURRENT_PROCESS;
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        p.id
    } else {
        0
    }
}

use smoltcp::socket::{Socket, tcp, udp};

/// Safely access a TCP socket by handle without panicking on type mismatch.
fn with_tcp_mut<R>(sockets: &mut smoltcp::iface::SocketSet, handle: smoltcp::iface::SocketHandle, f: impl FnOnce(&mut tcp::Socket) -> R) -> Option<R> {
    for (h, socket) in sockets.iter_mut() {
        if h == handle {
            if let Socket::Tcp(ref mut s) = socket {
                return Some(f(s));
            }
            return None;
        }
    }
    None
}

/// Safely access a UDP socket by handle without panicking on type mismatch.
fn with_udp_mut<R>(sockets: &mut smoltcp::iface::SocketSet, handle: smoltcp::iface::SocketHandle, f: impl FnOnce(&mut udp::Socket) -> R) -> Option<R> {
    for (h, socket) in sockets.iter_mut() {
        if h == handle {
            if let Socket::Udp(ref mut s) = socket {
                return Some(f(s));
            }
            return None;
        }
    }
    None
}

fn sys_socket(domain: u64, ty: u64, _protocol: u64) -> u64 {
    if domain != 2 {
        return errno::Errno::EAFNOSUPPORT as u64;
    }

    #[cfg(not(feature = "net"))]
    {
        let _ = ty;
        return errno::Errno::ENOSYS as u64;
    }

    #[cfg(feature = "net")]
    {
        let handle = if ty == 2 { // SOCK_DGRAM
            let rx_buffer = smoltcp::socket::udp::PacketBuffer::new(vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16], vec![0; 4096]);
            let tx_buffer = smoltcp::socket::udp::PacketBuffer::new(vec![smoltcp::socket::udp::PacketMetadata::EMPTY; 16], vec![0; 4096]);
            let socket = smoltcp::socket::udp::Socket::new(rx_buffer, tx_buffer);
            crate::net::SOCKETS.lock().add(socket)
        } else if ty == 1 { // SOCK_STREAM
            let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0; 4096]);
            let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0; 4096]);
            let socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
            crate::net::SOCKETS.lock().add(socket)
        } else {
            return errno::Errno::EINVAL as u64;
        };

        let socket_type = if ty == 1 { crate::task::process::SocketType::Tcp } else { crate::task::process::SocketType::Udp };
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let mut fd_table = process.fd_table.lock();
            let fd_obj = FileDescriptor::Socket(handle, socket_type);
            for (i, slot) in fd_table.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Some(fd_obj);
                    return i as u64;
                }
            }
            fd_table.push(Some(fd_obj));
            return (fd_table.len() - 1) as u64;
        }
        errno::Errno::ESRCH as u64
    }
}

use alloc::collections::BTreeMap;
use smoltcp::wire::IpEndpoint;
use smoltcp::iface::SocketHandle;

lazy_static::lazy_static! {
    static ref TCP_BIND_ENDPOINTS: spin::Mutex<BTreeMap<(u64, SocketHandle), IpEndpoint>> =
        spin::Mutex::new(BTreeMap::new());
}

fn sys_bind(sockfd: u64, addr_ptr: *const u8, addrlen: u64) -> u64 {
    if addrlen < 8 { return errno::Errno::EINVAL as u64; }
    let mut addr_buf = [0u8; 16];
    let copy_len = core::cmp::min(addrlen as usize, 16);
    if unsafe { user_access::copy_from_user(&mut addr_buf[..copy_len], addr_ptr) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }

    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let family = unsafe { *(addr_buf.as_ptr() as *const u16) };
        if family != 2 { return errno::Errno::EAFNOSUPPORT as u64; }
        let port = u16::from_be(unsafe { *(addr_buf.as_ptr().add(2) as *const u16) });
        let ip_bytes = unsafe { *(addr_buf.as_ptr().add(4) as *const [u8; 4]) };
        let ip = smoltcp::wire::Ipv4Address::from_bytes(&ip_bytes);
        let endpoint = smoltcp::wire::IpEndpoint::new(smoltcp::wire::IpAddress::Ipv4(ip), port);

        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.fd_table.lock();
            if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
            if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
                let pid = process.id;
                match stype {
                    crate::task::process::SocketType::Udp => {
                        let mut sockets = crate::net::SOCKETS.lock();
                        let success = with_udp_mut(&mut *sockets, handle, |socket| {
                            socket.bind(endpoint).is_ok()
                        }).unwrap_or(false);
                        if !success { return errno::Errno::EADDRINUSE as u64; }
                    }
                    crate::task::process::SocketType::Tcp => {
                        TCP_BIND_ENDPOINTS.lock().insert((pid, handle), endpoint);
                    }
                }
                return 0;
            }
        }
        errno::Errno::EBADF as u64
    }
}

fn sys_connect(sockfd: u64, addr_ptr: *const u8, addrlen: u64) -> u64 {
    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        if addrlen < 8 { return errno::Errno::EINVAL as u64; }
        let mut addr_buf = [0u8; 16];
        let copy_len = core::cmp::min(addrlen as usize, 16);
        if unsafe { user_access::copy_from_user(&mut addr_buf[..copy_len], addr_ptr) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }

        let family = unsafe { *(addr_buf.as_ptr() as *const u16) };
        if family != 2 { return errno::Errno::EAFNOSUPPORT as u64; }
        let port = u16::from_be(unsafe { *(addr_buf.as_ptr().add(2) as *const u16) });
        let ip_bytes = unsafe { *(addr_buf.as_ptr().add(4) as *const [u8; 4]) };
        let endpoint = smoltcp::wire::IpEndpoint::new(
            smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(&ip_bytes)),
            port,
        );

        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let fd_table = process.fd_table.lock();
            if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
            if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
                let mut sockets = crate::net::SOCKETS.lock();
                match stype {
                    crate::task::process::SocketType::Tcp => {
                        let mut iface_lock = crate::net::NETWORK_INTERFACE.lock();
                        let result = iface_lock.as_mut().map(|iface| {
                            let cx = iface.context();
                            with_tcp_mut(&mut *sockets, handle, |socket| {
                                if !socket.is_active() {
                                    let local_endpoint = smoltcp::wire::IpListenEndpoint {
                                        addr: None,
                                        port: 0,
                                    };
                                    if socket.connect(cx, endpoint, local_endpoint).is_err() {
                                        Err(errno::Errno::ECONNREFUSED)
                                    } else {
                                        Ok(0u64)
                                    }
                                } else if socket.may_send() {
                                    Ok(0u64)
                                } else {
                                    Err(errno::Errno::EALREADY)
                                }
                            })
                        });
                        match result {
                            Some(Some(Ok(v))) => return v,
                            Some(Some(Err(e))) => return e as u64,
                            _ => return errno::Errno::EIO as u64,
                        }
                    }
                    crate::task::process::SocketType::Udp => {
                        return 0;
                    }
                }
            }
        }
        errno::Errno::EBADF as u64
    }
}

fn sys_listen(sockfd: u64, _backlog: u64) -> u64 {
    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.fd_table.lock();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
        if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
            if stype != crate::task::process::SocketType::Tcp {
                return errno::Errno::EOPNOTSUPP as u64;
            }
            let pid = process.id;
            let bind_ep = TCP_BIND_ENDPOINTS.lock().get(&(pid, handle)).copied();
            let port = bind_ep.map(|ep| ep.port).unwrap_or(0);
            if port == 0 { return errno::Errno::EINVAL as u64; }
            let mut sockets = crate::net::SOCKETS.lock();
            let success = with_tcp_mut(&mut *sockets, handle, |socket| {
                let listen_ep = smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port,
                };
                socket.listen(listen_ep).is_ok()
            }).unwrap_or(false);
            if !success { return errno::Errno::EADDRINUSE as u64; }
            return 0;
        }
        errno::Errno::EBADF as u64
    }
}

fn sys_accept(sockfd: u64, addr_ptr: *mut u8, addrlen_ptr: *mut u32) -> u64 {
    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        crate::net::poll();

        let process = {
            let process_lock = CURRENT_PROCESS.lock();
            match *process_lock { Some(ref p) => p.clone(), None => return errno::Errno::ESRCH as u64 }
        };
        let mut fd_table = process.fd_table.lock();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }

        let (handle, local_port) = match fd_table[sockfd as usize] {
            Some(FileDescriptor::Socket(h, stype)) => {
                if stype != crate::task::process::SocketType::Tcp {
                    return errno::Errno::EOPNOTSUPP as u64;
                }
                let mut sockets = crate::net::SOCKETS.lock();
                let result = with_tcp_mut(&mut *sockets, h, |socket| {
                    if socket.is_listening() || !socket.is_open() {
                        return Err(errno::Errno::EAGAIN);
                    }
                    let remote = socket.remote_endpoint();
                    let local_port = socket.local_endpoint().map(|ep| ep.port).unwrap_or(0);
                    Ok((remote, local_port))
                });
                match result {
                    Some(Ok((remote, lp))) => {
                        match remote {
                            Some(ep) => {
                                if !addr_ptr.is_null() && !addrlen_ptr.is_null() {
                                    let family: u16 = 2;
                                    let port_be = ep.port.to_be();
                                    let mut ip_bytes = [0u8; 4];
                                    let smoltcp::wire::IpAddress::Ipv4(ipv4) = ep.addr;
                                    ip_bytes.copy_from_slice(ipv4.as_bytes());
                                    let mut sockaddr = [0u8; 16];
                                    sockaddr[..2].copy_from_slice(&family.to_ne_bytes());
                                    sockaddr[2..4].copy_from_slice(&port_be.to_ne_bytes());
                                    sockaddr[4..8].copy_from_slice(&ip_bytes);
                                    let _ = unsafe { user_access::copy_to_user(addr_ptr, &sockaddr) };
                                    let addr_len: u32 = 16;
                                    let _ = unsafe { user_access::copy_to_user(addrlen_ptr as *mut u8, &addr_len.to_ne_bytes()) };
                                }
                                (h, lp)
                            }
                            None => return errno::Errno::EINVAL as u64,
                        }
                    }
                    Some(Err(e)) => return e as u64,
                    None => return errno::Errno::EINVAL as u64,
                }
            }
            _ => return errno::Errno::EBADF as u64,
        };

        if local_port == 0 { return errno::Errno::EINVAL as u64; }

        let rx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0u8; 4096]);
        let tx_buffer = smoltcp::socket::tcp::SocketBuffer::new(vec![0u8; 4096]);
        let mut new_socket = smoltcp::socket::tcp::Socket::new(rx_buffer, tx_buffer);
        let listen_addr = smoltcp::wire::IpListenEndpoint {
            addr: None,
            port: local_port,
        };
        if new_socket.listen(listen_addr).is_err() {
            return errno::Errno::EADDRINUSE as u64;
        }

        let mut sockets = crate::net::SOCKETS.lock();
        let new_handle = sockets.add(new_socket);
        fd_table[sockfd as usize] = Some(FileDescriptor::Socket(new_handle, crate::task::process::SocketType::Tcp));

        let pid = process.id;
        if let Some(ep) = TCP_BIND_ENDPOINTS.lock().get(&(pid, handle)).copied() {
            TCP_BIND_ENDPOINTS.lock().insert((pid, new_handle), ep);
        }

        for (i, slot) in fd_table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(FileDescriptor::Socket(handle, crate::task::process::SocketType::Tcp));
                return i as u64;
            }
        }
        fd_table.push(Some(FileDescriptor::Socket(handle, crate::task::process::SocketType::Tcp)));
        (fd_table.len() - 1) as u64
    }
}

fn sys_sendto(sockfd: u64, buf: *const u8, len: u64, addr_ptr: *const u8, addrlen: u64) -> u64 {
    #[cfg(not(feature = "net"))]
    return errno::Errno::ENOSYS as u64;

    #[cfg(feature = "net")]
    {
        let process_lock = CURRENT_PROCESS.lock();
        let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.fd_table.lock();
        if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
        
        if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
            let mut data = vec![0u8; len as usize];
            if unsafe { user_access::copy_from_user(&mut data, buf) }.is_err() { return errno::Errno::EFAULT as u64; }

            let dest_endpoint = if !addr_ptr.is_null() && addrlen >= 8 {
                 let mut addr_buf = [0u8; 16];
                 let clen = core::cmp::min(addrlen as usize, 16);
                 if unsafe { user_access::copy_from_user(&mut addr_buf[..clen], addr_ptr) }.is_err() {
                     return errno::Errno::EFAULT as u64;
                 }
                 {
                     let port = u16::from_be(unsafe { *(addr_buf.as_ptr().add(2) as *const u16) });
                     let ip_bytes = unsafe { *(addr_buf.as_ptr().add(4) as *const [u8; 4]) };
                     Some(smoltcp::wire::IpEndpoint::new(smoltcp::wire::IpAddress::Ipv4(smoltcp::wire::Ipv4Address::from_bytes(&ip_bytes)), port))
                 }
            } else {
                 None
            };

            let mut sockets = crate::net::SOCKETS.lock();
            match stype {
                crate::task::process::SocketType::Udp => {
                    if let Some(endpoint) = dest_endpoint {
                        if with_udp_mut(&mut *sockets, handle, |socket| {
                            socket.send_slice(&data, endpoint).is_ok()
                        }).unwrap_or(false) { return len; }
                    }
                }
                _ => return errno::Errno::ENOSYS as u64,
            }
            return errno::Errno::EIO as u64;
        }
        errno::Errno::EBADF as u64
    }
}

#[cfg(not(feature = "net"))]
fn sys_recvfrom(_sockfd: u64, _buf: *mut u8, _len: u64, _addr_ptr: *mut u8, _addrlen_ptr: *mut u32) -> u64 {
    errno::Errno::ENOSYS as u64
}

#[cfg(feature = "net")]
fn sys_recvfrom(sockfd: u64, buf: *mut u8, len: u64, _addr_ptr: *mut u8, _addrlen_ptr: *mut u32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.fd_table.lock();
    if (sockfd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    
    if let Some(FileDescriptor::Socket(handle, stype)) = fd_table[sockfd as usize] {
        let mut sockets = crate::net::SOCKETS.lock();
        let mut data = vec![0u8; len as usize];
        match stype {
            crate::task::process::SocketType::Udp => {
                if let Some(n) = with_udp_mut(&mut *sockets, handle, |socket| {
                    if let Ok((n, _ep)) = socket.recv_slice(&mut data) {
                        if unsafe { user_access::copy_to_user(buf, &data[..n]) }.is_ok() {
                            return n as u64;
                        }
                    }
                    0u64
                }) { return n; }
            }
            _ => return errno::Errno::ENOSYS as u64,
        }
        return 0;
    }
    errno::Errno::EBADF as u64
}

fn sys_execve(path_ptr: *const u8, argv_ptr: *const *const u8, _envp_ptr: *const *const u8, _regs_ptr: *mut u64) -> u64 {
    use crate::syscalls::user_access;
    
    // 1. Copy path and argv from user space
    let path = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    // LSM hook: exec permission check
    let subj = crate::security::current_subject();
    if !crate::security::hook_file_perm(&subj, &path, "exec") {
        return errno::Errno::EACCES as u64;
    }

    let mut argv = Vec::new();
    if !argv_ptr.is_null() {
        let mut i = 0;
        loop {
            let mut ptr: *const u8 = core::ptr::null();
            unsafe {
                if user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut ptr as *mut _ as *mut u8, 8), argv_ptr.add(i) as *const u8).is_err() {
                    break;
                }
            }
            if ptr.is_null() { break; }
            if let Ok(s) = unsafe { user_access::read_user_string(ptr, 256) } {
                argv.push(s);
            } else {
                break;
            }
            i += 1;
            if i > 64 { break; } // Limit args
        }
    }

    // 2. Resolve path and check permissions
    let node = match crate::vfs::VFS.lock().resolve_path(&path) {
        Some(n) => n,
        None => return errno::Errno::ENOENT as u64,
    };

    // Require execute permission on the binary
    if !check_node_permission(&node, 1) {
        return errno::Errno::EACCES as u64;
    }

    let elf_data = match node.read(usize::MAX) {
        Ok(d) => d,
        Err(_) => return errno::Errno::EIO as u64,
    };

    // 3. Copy fd table and flags from old process
    let (old_fd_table, old_fd_flags) = crate::task::process::CURRENT_PROCESS.lock()
        .as_ref().map(|p| (p.fd_table.lock().clone(), p.fd_flags.lock().clone()))
        .unwrap_or_default();

    // 4. Load ELF into new AddressSpace
    use crate::memory::paging::AddressSpace;
    let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
    let new_as = AddressSpace::new(&mut frame_allocator).expect("Failed to create new AddressSpace");
    
    let process = match crate::task::process::Process::load_elf(&elf_data, new_as) {
        Ok(p) => p,
        Err(_) => return errno::Errno::ENOEXEC as u64,
    };

    // Detect emulation mode based on ELF header
    crate::emulation::set_emulation(&process, &elf_data);
    if *process.emulation.lock() == crate::task::process::EmulationMode::Linux {
        crate::println!("[EMULATION] Running Linux binary: {}", path);
    }

    // Restore fd table and flags
    *process.fd_table.lock() = old_fd_table;
    *process.fd_flags.lock() = old_fd_flags;

    let entry = process.entry_point;
    let process_arc = Arc::new(process);

    // Activate new address space BEFORE setting up user stack
    // so virt_to_phys can find the freshly-mapped pages.
    unsafe { process_arc.address_space.activate(); }

    // 4. Setup user stack
    let user_rsp = match process_arc.setup_user_stack(&argv) {
        Ok(rsp) => rsp,
        Err(()) => {
            crate::serial_write("[EXEC] OOM: failed to allocate user stack\n");
            return errno::Errno::ENOMEM as u64;
        }
    };

    // 5. Update CURRENT_PROCESS
    {
        let mut cur = CURRENT_PROCESS.lock();
        *cur = Some(process_arc.clone());
    }
    
    // Update current thread's process
    {
        if let Some(mut thread) = crate::task::scheduler::current_thread() {
            thread.process = Some(process_arc.clone());
            crate::task::scheduler::set_current_thread(thread);
        }
    }

    unsafe {
        crate::task::thread::jump_to_usermode(entry, user_rsp);
    }
}

fn sys_gui_create_window(title_ptr: *const u8, width: usize, height: usize) -> u64 {
    use crate::gui::{COMPOSITOR, window::Window};
    let mut comp = COMPOSITOR.lock();
    
    // Leak the title string so it can be &'static str
    let title_str = if title_ptr.is_null() {
        "User App"
    } else {
        let mut len = 0;
        unsafe {
            while *title_ptr.add(len) != 0 { len += 1; }
        }
        let title_slice = unsafe { core::slice::from_raw_parts(title_ptr, len) };
        let s = core::str::from_utf8(title_slice).unwrap_or("User App");
        
        let boxed = alloc::string::String::from(s).into_boxed_str();
        let leaked: &'static str = unsafe { core::mem::transmute(&*boxed) };
        core::mem::forget(boxed);
        leaked
    };

    let mut win = Window::new(100, 100, width + 2, height + 22, title_str); // Add borders/titlebar
    
    // PHASE G3: Allocate shared physical memory for high-performance rendering
    let content_len = width * height;
    let size_bytes = content_len * 4;
    
    use crate::memory::buddy::BUDDY_ALLOCATOR;
    // Simple integer log2 for power-of-2 allocation
    let mut order = 0;
    while (4096 << order) < size_bytes && order < crate::memory::buddy::MAX_ORDER {
        order += 1;
    }

    if let Some(phys_addr) = BUDDY_ALLOCATOR.lock().allocate_contiguous(order) {
        win.phys_addr = Some(phys_addr.as_u64());
        
        // Zero the memory
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
        let k_ptr = (offset + phys_addr.as_u64()) as *mut u8;
        unsafe { core::ptr::write_bytes(k_ptr, 0, (4096 << order) as usize); }
    } else {
        // Fallback to kernel box (slow)
        win.content = Some(alloc::vec![0; content_len].into_boxed_slice());
    }
    
    comp.add_window(win);
    (comp.windows.len() - 1) as u64 // Handle
}

fn sys_gui_get_buffer(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }

    let win = &comp.windows[handle as usize];
    let content_w = win.width.saturating_sub(2);
    let content_h = win.height.saturating_sub(22);

    // Pack width and height into return value (low 32 = width, high 32 = height)
    ((content_w as u64) & 0xFFFF_FFFF) | ((content_h as u64) << 32)
}

fn sys_gui_map_buffer(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }
    
    let win = &comp.windows[handle as usize];
    let phys_addr = match win.phys_addr {
        Some(p) => p,
        None => return 0, // Not a shared memory window
    };

    let content_w = win.width.saturating_sub(2);
    let content_h = win.height.saturating_sub(22);
    let size_bytes = content_w * content_h * 4;
    let pages_needed = (size_bytes + 4095) / 4096;

    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return 0 };

    // Find a virtual address to map to
    static NEXT_GUI_MAP_ADDR: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0x5000_0000_0000);
    let v_addr = NEXT_GUI_MAP_ADDR.fetch_add(pages_needed as u64 * 4096, core::sync::atomic::Ordering::SeqCst);

    use crate::memory::buddy::BuddyFrameAllocator;
    let mut frame_allocator = BuddyFrameAllocator;
    let mut mapper = if let Some(m) = unsafe { process.address_space.mapper() } { m } else { return 0; };

    for i in 0..pages_needed {
        let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(v_addr + i as u64 * 4096));
        let frame = x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(phys_addr + i as u64 * 4096));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;
        
        unsafe {
            if let Ok(t) = mapper.map_to(page, frame, flags, &mut frame_allocator) {
                t.flush();
            }
        }
    }

    process.add_vma(crate::task::process::Vma {
        start: v_addr,
        end: v_addr + pages_needed as u64 * 4096,
        flags: PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE,
        _name: "gui_buffer",
    });

    v_addr
}

fn sys_gui_flush(handle: u64, buf_ptr: *const u32) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EBADF as u64; }
    
    let win = &mut comp.windows[handle as usize];
    if win.phys_addr.is_some() {
        // Zero copy: buffer is already updated by user
        // We just need to trigger a compositor render
    } else if let Some(ref mut content) = win.content {
        let len: usize = (*content).len();
        if !buf_ptr.is_null() {
            unsafe {
                crate::syscalls::user_access::copy_from_user(
                    core::slice::from_raw_parts_mut(content.as_mut_ptr() as *mut u8, len * 4), 
                    buf_ptr as *const u8
                ).unwrap_or(());
            }
        }
    }
    comp.render(0, 0);
    0
}

fn sys_gui_get_key(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }
    let win = &mut comp.windows[handle as usize];
    win.key_events.pop_front().map(|k| k as u64).unwrap_or(0)
}

fn sys_gui_get_mouse(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    use crate::drivers::mouse::MOUSE;
    let comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return 0; }
    let win = &comp.windows[handle as usize];
    let m = MOUSE.lock();
    // Return mouse position relative to window content area
    let rel_x = (m.x as i64 - win.x as i64 - 1).max(0) as u64;
    let rel_y = (m.y as i64 - win.y as i64 - 21).max(0) as u64;
    let buttons = m.buttons as u64;
    let scroll = (m.scroll as i8 as i64) as u64;
    // Pack: low16=x, bits16-31=y, bits32-39=buttons, bits40-47=scroll
    (rel_x & 0xFFFF) | ((rel_y & 0xFFFF) << 16) | ((buttons & 0xFF) << 32) | ((scroll & 0xFF) << 40)
}

fn sys_gui_set_title(handle: u64, title_ptr: *const u8) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    let win = &mut comp.windows[handle as usize];
    if title_ptr.is_null() { return errno::Errno::EINVAL as u64; }
    let mut len = 0;
    unsafe {
        while *title_ptr.add(len) != 0 && len < 64 { len += 1; }
    }
    let title_slice = unsafe { core::slice::from_raw_parts(title_ptr, len) };
    if let Ok(s) = core::str::from_utf8(title_slice) {
        let boxed = alloc::string::String::from(s).into_boxed_str();
        let leaked: &'static str = unsafe { core::mem::transmute(&*boxed) };
        core::mem::forget(boxed);
        win.title = leaked;
    }
    0
}

fn sys_gui_destroy_window(handle: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    comp.windows.remove(handle as usize);
    0
}

fn sys_gui_resize_window(handle: u64, width: u64, height: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    let win = &mut comp.windows[handle as usize];
    win.width = width as usize;
    win.height = height as usize;
    0
}

fn sys_gui_move_window(handle: u64, x: u64, y: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    if handle as usize >= comp.windows.len() { return errno::Errno::EINVAL as u64; }
    let win = &mut comp.windows[handle as usize];
    win.x = x as usize;
    win.y = y as usize;
    0
}

/// SYS_CLIPBOARD: arg1=mode (0=read, 1=write), arg2=buf ptr, arg3=buf len
/// Read: copies clipboard to user buffer, returns bytes copied
/// Write: copies user buffer to clipboard
fn sys_clipboard(mode: u64, buf: *mut u8, len: u64) -> u64 {
    use crate::gui::COMPOSITOR;
    let mut comp = COMPOSITOR.lock();
    match mode {
        0 => {
            // Read clipboard
            let copy_len = (len as usize).min(comp.clipboard.len());
            if copy_len == 0 { return 0; }
            if buf.is_null() { return comp.clipboard.len() as u64; }
            unsafe {
                core::ptr::copy_nonoverlapping(comp.clipboard.as_ptr(), buf, copy_len);
            }
            copy_len as u64
        }
        1 => {
            // Write clipboard
            if buf.is_null() || len == 0 { comp.clipboard.clear(); return 0; }
            let mut new_data = alloc::vec![0u8; len as usize];
            unsafe {
                core::ptr::copy_nonoverlapping(buf, new_data.as_mut_ptr(), len as usize);
            }
            comp.clipboard = new_data;
            len
        }
        2 => {
            // Get clipboard length
            comp.clipboard.len() as u64
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

/// SYS_NOTIFY: arg1=text ptr (null-terminated), arg2=duration_ms, arg3=kind (0=Info,1=Warning,2=Error)
fn sys_notify(text_ptr: *const u8, duration_ms: u64, kind: u64) -> u64 {
    use crate::gui::{COMPOSITOR, NotifKind};
    if text_ptr.is_null() { return errno::Errno::EINVAL as u64; }
    let mut len = 0;
    unsafe {
        while *text_ptr.add(len) != 0 && len < 256 { len += 1; }
    }
    let text_slice = unsafe { core::slice::from_raw_parts(text_ptr, len) };
    let text = match core::str::from_utf8(text_slice) {
        Ok(s) => alloc::string::String::from(s),
        Err(_) => return errno::Errno::EINVAL as u64,
    };
    let notif_kind = match kind {
        1 => NotifKind::Warning,
        2 => NotifKind::Error,
        _ => NotifKind::Info,
    };
    let ticks = (duration_ms / 10).max(10);
    let mut comp = COMPOSITOR.lock();
    comp.notifications.push(crate::gui::Notification {
        text,
        kind: notif_kind,
        ticks_remaining: ticks,
        x: 0,
        y: 0,
    });
    0
}

fn sys_getcwd(buf: *mut u8, size: usize) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    if let Some(ref process) = *process_lock {
        let cwd = process.cwd.lock();
        if cwd.len() + 1 > size {
            return errno::Errno::ERANGE as u64;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf, cwd.len());
            *buf.add(cwd.len()) = 0;
        }
        return buf as u64;
    }
    errno::Errno::ESRCH as u64
}

fn sys_chdir(path_ptr: *const u8) -> u64 {
    let mut len = 0;
    unsafe {
        while *path_ptr.add(len) != 0 { len += 1; }
    }
    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, len) };
    let path_str = core::str::from_utf8(path_slice).unwrap_or("");

    if let Some(node) = VFS.lock().resolve_path(path_str) {
        if !node.is_dir() {
            return errno::Errno::ENOTDIR as u64;
        }
        if !check_node_permission(&node, 1) {
            return errno::Errno::EACCES as u64;
        }
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let mut new_cwd = String::from(path_str);
            if !new_cwd.starts_with('/') {
                let cur_cwd = process.cwd.lock();
                if *cur_cwd == "/" {
                    new_cwd = alloc::format!("/{}", new_cwd);
                } else {
                    new_cwd = alloc::format!("{}/{}", cur_cwd, new_cwd);
                }
            }
            if new_cwd.len() > 1 && new_cwd.ends_with('/') {
                new_cwd.pop();
            }
            *process.cwd.lock() = new_cwd;
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}

fn sys_mkdir(path_ptr: *const u8, _mode: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    let last_slash = path_str.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 && !path_str.starts_with('/') {
        (".", path_str.as_str())
    } else if last_slash == 0 {
        ("/", &path_str[1..])
    } else {
        (&path_str[..last_slash], &path_str[last_slash+1..])
    };

    let vfs = VFS.lock();
    if let Some(parent_node) = vfs.resolve_path(parent_path) {
        // Need write+execute on parent directory to create entries
        if !check_node_permission(&parent_node, 3) {
            return errno::Errno::EACCES as u64;
        }
        if parent_node.mkdir(name).is_ok() {
            return 0;
        }
    }
    errno::Errno::EIO as u64
}

fn sys_unlink(path_ptr: *const u8) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    let last_slash = path_str.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 && !path_str.starts_with('/') {
        (".", path_str.as_str())
    } else if last_slash == 0 {
        ("/", &path_str[1..])
    } else {
        (&path_str[..last_slash], &path_str[last_slash+1..])
    };

    let vfs = VFS.lock();
    if let Some(parent_node) = vfs.resolve_path(parent_path) {
        // Need write+execute on parent directory to remove entries
        if !check_node_permission(&parent_node, 3) {
            return errno::Errno::EACCES as u64;
        }
        if parent_node.unlink(name).is_ok() {
            return 0;
        }
    }
    errno::Errno::EIO as u64
}

fn sys_rename(old_path_ptr: *const u8, new_path_ptr: *const u8) -> u64 {
    let old_path = match unsafe { user_access::read_user_string(old_path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };
    let new_path = match unsafe { user_access::read_user_string(new_path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    let vfs = VFS.lock();

    // Read source
    let source_node = match vfs.resolve_path(&old_path) {
        Some(n) => n,
        None => return errno::Errno::ENOENT as u64,
    };

    // Need w+x on source parent directory to unlink the original
    let src_last_slash = old_path.rfind('/').unwrap_or(0);
    let src_parent_path = if src_last_slash == 0 && !old_path.starts_with('/') { "." }
        else if src_last_slash == 0 { "/" }
        else { &old_path[..src_last_slash] };
    if let Some(src_parent) = vfs.resolve_path(src_parent_path) {
        if !check_node_permission(&src_parent, 3) {
            return errno::Errno::EACCES as u64;
        }
    }

    let data = match source_node.read(usize::MAX) {
        Ok(d) => d,
        Err(_) => return errno::Errno::EIO as u64,
    };

    // Resolve destination parent
    let last_slash = new_path.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 && !new_path.starts_with('/') {
        (".", new_path.as_str())
    } else if last_slash == 0 {
        ("/", &new_path[1..])
    } else {
        (&new_path[..last_slash], &new_path[last_slash+1..])
    };

    let parent_node = match vfs.resolve_path(parent_path) {
        Some(n) => n,
        None => return errno::Errno::ENOENT as u64,
    };

    // Need w+x on destination parent to create the new entry
    if !check_node_permission(&parent_node, 3) {
        return errno::Errno::EACCES as u64;
    }

    // Create new file
    if parent_node.create(name).is_err() {
        return errno::Errno::EIO as u64;
    }

    // Write data to new file
    let new_node = match parent_node.find_child(name) {
        Some(n) => n,
        None => return errno::Errno::EIO as u64,
    };
    if new_node.write(&data).is_err() {
        return errno::Errno::EIO as u64;
    }

    // Remove old source
    let old_last_slash = old_path.rfind('/').unwrap_or(0);
    let (old_parent_path, old_name) = if old_last_slash == 0 && !old_path.starts_with('/') {
        (".", old_path.as_str())
    } else if old_last_slash == 0 {
        ("/", &old_path[1..])
    } else {
        (&old_path[..old_last_slash], &old_path[old_last_slash+1..])
    };

    if let Some(old_parent) = vfs.resolve_path(old_parent_path) {
        let _ = old_parent.unlink(old_name);
    }

    0
}


#[cfg(feature = "ai_rule")]
fn sys_vahiai(intent_ptr: *const u8, args_ptr: *const *const u8, arg_count: u64, out_ptr: *mut u8, out_len: u64) -> u64 {
    let intent_name = match unsafe { user_access::read_user_string(intent_ptr, 128) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    let mut args = Vec::new();
    if !args_ptr.is_null() && arg_count > 0 {
        for i in 0..core::cmp::min(arg_count as usize, 10) {
            let mut ptr: *const u8 = core::ptr::null();
            unsafe {
                if user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut ptr as *mut _ as *mut u8, 8), args_ptr.add(i) as *const u8).is_err() {
                    break;
                }
            }
            if !ptr.is_null() {
                if let Ok(s) = unsafe { user_access::read_user_string(ptr, 256) } {
                    args.push(s);
                }
            }
        }
    }

    let args_slices: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let engine = crate::vahiai::ENGINE.lock();
    match engine.execute(&intent_name, &args_slices) {
        crate::vahiai::IntentResult::Success(msg) => {
            if !out_ptr.is_null() && out_len > 0 {
                let copy_len = core::cmp::min(msg.len(), out_len as usize);
                if unsafe { user_access::copy_to_user(out_ptr, &msg.as_bytes()[..copy_len]) }.is_err() {
                    return errno::Errno::EFAULT as u64;
                }
                return copy_len as u64;
            }
            0
        },
        crate::vahiai::IntentResult::Error(_) => errno::Errno::EINVAL as u64,
        crate::vahiai::IntentResult::ExecuteSyscall(n, _s_args) => {
            // Placeholder: currently we don't trigger the nested syscall here for return simplicity
            n
        }
    }
}

core::arch::global_asm!(
    r#"
    .global syscall_entry
    syscall_entry:
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
        add rsp, 8              # Skip scratch r11 — real RFLAGS is loaded later
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
        # Skip saved rax (syscall number) — return value from handler is already in RAX
        add rsp, 8
        # Drop saved user_rip, rflags, rsp (they are restored via sysret and mov rsp)
        add rsp, 24

        mov rsp, gs:[0x18]     # Restore user RSP
        swapgs              # Switch back to user GS base
        sysretq
    "#
);
#[cfg(not(feature = "net"))]
fn sys_resolve(_name_ptr: *const u8, _ip_ptr: *mut u8) -> u64 {
    errno::Errno::ENOSYS as u64
}

#[cfg(feature = "net")]
fn sys_resolve(name_ptr: *const u8, ip_ptr: *mut u8) -> u64 {
    let name_str = match unsafe { user_access::read_user_string(name_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    if let Some(ip) = crate::net::dns::resolve_hostname(&name_str) {
        let smoltcp::wire::IpAddress::Ipv4(ipv4) = ip;
        let bytes = ipv4.as_bytes();
        if unsafe { user_access::copy_to_user(ip_ptr, bytes) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
        return 0;
    }

    errno::Errno::ENOENT as u64
}

fn sys_select(nfds: u64, readfds: *mut u64, writefds: *mut u64, exceptfds: *mut u64, timeout: *const u64) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };
    let fd_table = process.fd_table.lock();
    let mut ready_count;
    let deadline = if !timeout.is_null() {
        let mut tv_sec = 0u64;
        let mut tv_nsec = 0u64;
        unsafe {
            let _ = user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut tv_sec as *mut _ as *mut u8, 8), timeout as *const u8);
            let _ = user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut tv_nsec as *mut _ as *mut u8, 8), timeout.add(8) as *const u8);
        }
        let timeout_ms = tv_sec * 1000 + tv_nsec / 1_000_000;
        let now = crate::interrupts::get_ticks() * 10;
        if timeout_ms > 0 { now + timeout_ms / 10 } else { 0 }
    } else { 0 };

    let mut poll_count = 0;
    loop {
        poll_count += 1;
        if poll_count > 1000 { break; }

        let mut read_set: u64 = 0;
        let mut write_set: u64 = 0;
        #[allow(unused_mut)]
        let mut except_set: u64 = 0;

        for fd in 0..core::cmp::min(nfds, 64) {
            if fd as usize >= fd_table.len() { continue; }
            let readable = match fd_table[fd as usize] {
                Some(ref desc) => match desc {
                    FileDescriptor::File { node, .. } => node.stat().map(|s| s.st_size > 0).unwrap_or(false),
                    FileDescriptor::Socket(_, _) => true,
                    FileDescriptor::PtyMaster { pair, .. } => !pair.lock().master.buf.is_empty(),
                    FileDescriptor::PtySlave { pair, .. } => !pair.lock().slave.buf.is_empty(),
                },
                None => false,
            };
            let writable = fd_table[fd as usize].is_some();

            if readable { read_set |= 1 << fd; }
            if writable { write_set |= 1 << fd; }
        }

        let mut read_set_masked = read_set;
        let mut write_set_masked = write_set;
        let mut except_set_masked = except_set;

        if !readfds.is_null() {
            let mut user_set = 0u64;
            unsafe { let _ = user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut user_set as *mut _ as *mut u8, 8), readfds as *const u8); }
            read_set_masked &= user_set;
        }
        if !writefds.is_null() {
            let mut user_set = 0u64;
            unsafe { let _ = user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut user_set as *mut _ as *mut u8, 8), writefds as *const u8); }
            write_set_masked &= user_set;
        }
        if !exceptfds.is_null() {
            let mut user_set = 0u64;
            unsafe { let _ = user_access::copy_from_user(core::slice::from_raw_parts_mut(&mut user_set as *mut _ as *mut u8, 8), exceptfds as *const u8); }
            except_set_masked &= user_set;
        }

        ready_count = read_set_masked.count_ones() as u64 + write_set_masked.count_ones() as u64 + except_set_masked.count_ones() as u64;

        if ready_count > 0 {
            if !readfds.is_null() { unsafe { let _ = user_access::copy_to_user(readfds as *mut u8, core::slice::from_raw_parts(&read_set_masked as *const _ as *const u8, 8)); } }
            if !writefds.is_null() { unsafe { let _ = user_access::copy_to_user(writefds as *mut u8, core::slice::from_raw_parts(&write_set_masked as *const _ as *const u8, 8)); } }
            if !exceptfds.is_null() { unsafe { let _ = user_access::copy_to_user(exceptfds as *mut u8, core::slice::from_raw_parts(&except_set_masked as *const _ as *const u8, 8)); } }
            return ready_count;
        }

        if !timeout.is_null() {
            let ticks = crate::interrupts::get_ticks() * 10;
            if deadline > 0 && ticks >= deadline { break; }
        }
        crate::task::scheduler::try_schedule();
    }

    if !readfds.is_null() { unsafe { let _ = user_access::copy_to_user(readfds as *mut u8, &0u64.to_ne_bytes()); } }
    if !writefds.is_null() { unsafe { let _ = user_access::copy_to_user(writefds as *mut u8, &0u64.to_ne_bytes()); } }
    if !exceptfds.is_null() { unsafe { let _ = user_access::copy_to_user(exceptfds as *mut u8, &0u64.to_ne_bytes()); } }
    0
}

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLNVAL: i16 = 0x020;

fn sys_poll(fds: *const u8, nfds: usize, timeout_ms: i32) -> u64 {
    if nfds > 256 { return errno::Errno::ENOMEM as u64; }
    if fds.is_null() { return errno::Errno::EFAULT as u64; }

    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return errno::Errno::ESRCH as u64,
    };

    // Copy pollfd array from userspace
    let mut poll_fds: alloc::vec::Vec<(i32, i16, i16)> = alloc::vec::Vec::with_capacity(nfds);
    for i in 0..nfds {
        let mut buf = [0u8; 8];
        unsafe {
            if user_access::copy_from_user(&mut buf, fds.add(i * 8)).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        let fd = i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let events = i16::from_ne_bytes([buf[4], buf[5]]);
        poll_fds.push((fd, events, 0i16));
    }

    let deadline = if timeout_ms > 0 {
        let now = crate::interrupts::get_ticks() * 10;
        Some(now + timeout_ms as u64 / 10)
    } else {
        None
    };

    let mut poll_count = 0;
    loop {
        poll_count += 1;
        if poll_count > 1000 { break; }

        let fd_table = process.fd_table.lock();
        let mut ready = 0usize;
        for (_i, (fd, events, revents)) in poll_fds.iter_mut().enumerate() {
            if *fd < 0 { continue; }
            *revents = 0;
            let desc = if (*fd as usize) < fd_table.len() {
                fd_table[*fd as usize].as_ref()
            } else {
                None
            };
            match desc {
                Some(FileDescriptor::File { node, .. }) => {
                    if *events & POLLIN != 0 && node.stat().map(|s| s.st_size > 0).unwrap_or(false) {
                        *revents |= POLLIN;
                    }
                    if *events & POLLOUT != 0 { *revents |= POLLOUT; }
                }
                Some(FileDescriptor::Socket(_, _)) => {
                    if *events & POLLIN != 0 { *revents |= POLLIN; }
                    if *events & POLLOUT != 0 { *revents |= POLLOUT; }
                }
                Some(FileDescriptor::PtyMaster { pair, .. }) => {
                    let buf = &pair.lock().master.buf;
                    if *events & POLLIN != 0 && !buf.is_empty() { *revents |= POLLIN; }
                    if *events & POLLOUT != 0 { *revents |= POLLOUT; }
                }
                Some(FileDescriptor::PtySlave { pair, .. }) => {
                    let buf = &pair.lock().slave.buf;
                    if *events & POLLIN != 0 && !buf.is_empty() { *revents |= POLLIN; }
                    if *events & POLLOUT != 0 { *revents |= POLLOUT; }
                }
                None => { *revents |= POLLNVAL; }
            }
            if *revents != 0 { ready += 1; }
        }
        drop(fd_table);

        if ready > 0 {
            for (i, (_fd, _events, revents)) in poll_fds.iter().enumerate() {
                let r = revents.to_ne_bytes();
                unsafe { let _ = user_access::copy_to_user(fds.add(i * 8 + 4) as *mut u8, &r); }
            }
            return ready as u64;
        }

        if let Some(dl) = deadline {
            let ticks = crate::interrupts::get_ticks() * 10;
            if ticks >= dl { break; }
        } else if timeout_ms == 0 {
            break;
        }
        crate::task::scheduler::try_schedule();
    }

    // Timeout or nothing ready: write zero revents back
    for i in 0..nfds {
        let zero: [u8; 2] = [0, 0];
        unsafe { let _ = user_access::copy_to_user(fds.add(i * 8 + 4) as *mut u8, &zero); }
    }
    0
}

fn sys_sync() -> u64 {
    let devices = crate::drivers::block::BLOCK_DEVICES.lock();
    for dev in devices.iter() {
        dev.lock().sync();
    }
    0
}

fn sys_reboot(magic: u64, cmd: u64) -> u64 {
    if magic != 0xDEAD_BEEF {
        return errno::Errno::EINVAL as u64;
    }
    // Only root or CAP_SYS_BOOT can reboot
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_BOOT) {
        audit_log("CAP_SYS_BOOT", "DENIED");
        return errno::Errno::EPERM as u64;
    }
    audit_log("REBOOT", if cmd == 0 { "poweroff" } else { "reboot" });
    match cmd {
        0 => { // Power off
            crate::println!("[SYSCALL] system poweroff");
            // Try ACPI S5 first, then fall back to QEMU-specific
            if *crate::acpi::PM1A_CNT_PORT.get().unwrap_or(&0) != 0 {
                crate::acpi::acpi_shutdown();
            }
            // QEMU-specific: isa-debug-exit at port 0xf4, exit code 0x10
            let mut port = x86_64::instructions::port::Port::<u32>::new(0xf4);
            unsafe { port.write(0x10); }
            let mut port2 = x86_64::instructions::port::Port::<u16>::new(0x604);
            unsafe { port2.write(0x2000); }
            x86_64::instructions::interrupts::disable();
            loop { x86_64::instructions::hlt(); }
        }
        1 => { // Reboot
            crate::println!("[SYSCALL] system reboot");
            // Try ACPI reset first, fall back to legacy
            crate::acpi::acpi_reboot();
            x86_64::instructions::interrupts::disable();
            loop { x86_64::instructions::hlt(); }
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

fn sys_getuid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { *p.uid.lock() as u64 } else { 0 }
}

fn sys_getgid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { *p.gid.lock() as u64 } else { 0 }
}

fn sys_setuid(uid: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let euid = *p.euid.lock();
        if euid == 0 || has_capability(CAP_SETUID) {
            *p.uid.lock() = uid as u32;
            *p.euid.lock() = uid as u32;
            0
        } else if euid == uid as u32 {
            *p.uid.lock() = uid as u32;
            0
        } else {
            audit_log("CAP_SETUID", &alloc::format!("setuid({}) DENIED", uid));
            errno::Errno::EPERM as u64
        }
    } else { errno::Errno::ESRCH as u64 }
}

fn sys_setgid(gid: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let egid = *p.egid.lock();
        if egid == 0 || has_capability(CAP_SETGID) {
            *p.gid.lock() = gid as u32;
            *p.egid.lock() = gid as u32;
            0
        } else if egid == gid as u32 {
            *p.gid.lock() = gid as u32;
            0
        } else {
            audit_log("CAP_SETGID", &alloc::format!("setgid({}) DENIED", gid));
            errno::Errno::EPERM as u64
        }
    } else { errno::Errno::ESRCH as u64 }
}

fn sys_geteuid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { *p.euid.lock() as u64 } else { 0 }
}

fn sys_getegid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { *p.egid.lock() as u64 } else { 0 }
}

fn sys_korlang(id: u64, arg1: u64, arg2: u64, arg3: u64, _arg4: u64) -> u64 {
    use crate::korlang::runtime;
    match id {
        1 => runtime::korlang_alloc(arg1 as usize, arg2 as usize) as u64,
        2 => {
            runtime::korlang_free(arg1 as *mut u8, arg2 as usize, arg3 as usize);
            0
        },
        10 => {
            let mut buf = vec![0u8; arg2 as usize];
            if unsafe { user_access::copy_from_user(&mut buf, arg1 as *const u8) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            runtime::_kor_stdout_write(buf.as_ptr(), buf.len());
            0
        },
        11 => {
             let mut buf = vec![0u8; arg2 as usize];
             if unsafe { user_access::copy_from_user(&mut buf, arg1 as *const u8) }.is_err() {
                 return errno::Errno::EFAULT as u64;
             }
             runtime::_kor_stdout_write(buf.as_ptr(), buf.len());
             crate::print!("\n");
             0
        },
        20 => {
            let path = match unsafe { user_access::read_user_string(arg1 as *const u8, 256) } {
                Ok(s) => s,
                Err(_) => return errno::Errno::EFAULT as u64,
            };
            runtime::_kor_file_open(path.as_ptr(), path.len()) as u64
        },
        99 => {
             let msg = match unsafe { user_access::read_user_string(arg1 as *const u8, 256) } {
                 Ok(s) => s,
                 Err(_) => "Korlang panic (failed to read msg)".into(),
             };
             runtime::_kor_panic(msg.as_ptr(), msg.len());
        },
        _ => 0,
    }
}

fn sys_drmctl(_fd: u64, request: u64, arg: *mut u8) -> u64 {
    const DRM_IOCTL_GET_DISPLAY_INFO: u64 = 0x0100;
    const DRM_IOCTL_CREATE_DUMB: u64 = 0x0101;
    const DRM_IOCTL_DESTROY_DUMB: u64 = 0x0103;
    const DRM_IOCTL_FLIP: u64 = 0x0104;
    const DRM_IOCTL_SET_MODE: u64 = 0x0105;
    const DRM_IOCTL_MAP_DUMB: u64 = 0x0106;
    const DRM_IOCTL_PAGE_FLIP: u64 = 0x0107;
    const DRM_IOCTL_GEM_CREATE: u64 = 0x0108;
    const DRM_IOCTL_GEM_MMAP: u64 = 0x0109;

    match request {
        DRM_IOCTL_GET_DISPLAY_INFO => {
            #[repr(C)]
            struct DisplayInfo { width: u32, height: u32 }
            let info = DisplayInfo {
                width: crate::drivers::gpu::width(),
                height: crate::drivers::gpu::height(),
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(&info as *const DisplayInfo as *const u8, core::mem::size_of::<DisplayInfo>())
            };
            if unsafe { user_access::copy_to_user(arg, bytes).is_err() } {
                return errno::Errno::EFAULT as u64;
            }
            0
        }
        DRM_IOCTL_CREATE_DUMB => {
            use alloc::boxed::Box;
            use alloc::vec;
            let w = crate::drivers::gpu::width();
            let h = crate::drivers::gpu::height();
            let fb: &'static mut [u32] = Box::leak(vec![0u32; (w * h) as usize].into_boxed_slice());
            let paddr = crate::memory::virt_to_phys(VirtAddr::from_ptr(fb.as_ptr())).unwrap().as_u64();
            #[repr(C)]
            struct DumbInfo { id: u64, size: u64, addr: u64 }
            let di = DumbInfo { id: 1, size: (w * h * 4) as u64, addr: paddr };
            let bytes = unsafe {
                core::slice::from_raw_parts(&di as *const DumbInfo as *const u8, core::mem::size_of::<DumbInfo>())
            };
            if unsafe { user_access::copy_to_user(arg, bytes).is_err() } {
                return errno::Errno::EFAULT as u64;
            }
            0
        }
        DRM_IOCTL_DESTROY_DUMB => {
            0 // Memory will be freed on process exit; for now, no-op
        }
        DRM_IOCTL_FLIP => {
            crate::drivers::gpu::virtio_gpu::flip();
            0
        }
        DRM_IOCTL_SET_MODE => {
            // arg1=width, arg2=height (passed as direct args from userspace)
            let new_w = _fd as usize;
            let new_h = request as usize;
            if new_w < 640 || new_w > 3840 || new_h < 480 || new_h > 2160 {
                return errno::Errno::EINVAL as u64;
            }
            crate::drivers::gpu::set_mode(new_w as u32, new_h as u32);
            crate::drivers::graphics::WIDTH.store(new_w, core::sync::atomic::Ordering::SeqCst);
            crate::drivers::graphics::HEIGHT.store(new_h, core::sync::atomic::Ordering::SeqCst);
            crate::drivers::graphics::STRIDE.store(new_w, core::sync::atomic::Ordering::SeqCst);
            crate::gui::COMPOSITOR.lock().set_resolution(new_w, new_h);
            crate::println!("DRM: set_mode {}x{}", new_w, new_h);
            0
        }
        DRM_IOCTL_MAP_DUMB => {
            // Return the virtual address of the framebuffer
            let fb_ptr = crate::drivers::graphics::FRAMEBUFFER.load(core::sync::atomic::Ordering::Relaxed);
            fb_ptr as u64
        }
        DRM_IOCTL_PAGE_FLIP => {
            // Flip to a specific buffer (id in arg1)
            crate::drivers::gpu::virtio_gpu::flip();
            0
        }
        DRM_IOCTL_GEM_CREATE => {
            // Allocate a GEM object of `_fd` bytes size
            use alloc::boxed::Box;
            use alloc::vec;
            let size = _fd as usize;
            if size == 0 || size > 64 * 1024 * 1024 {
                return errno::Errno::EINVAL as u64;
            }
            let buf: &'static mut [u8] = Box::leak(vec![0u8; size].into_boxed_slice());
            buf.as_ptr() as u64
        }
        DRM_IOCTL_GEM_MMAP => {
            // id is the address returned by GEM_CREATE (the kernel buffer address)
            // Return it as the mmap address
            _fd
        }
        // 0x010A = SET_ACCENT_COLOR: arg = packed ARGB u32
        0x010A => {
            let color = arg as u32 | 0xFF000000;
            unsafe { crate::gui::ACCENT_COLOR = color; }
            crate::println!("DRM: accent color -> 0x{:08X}", color);
            0
        }
        // 0x010B = SET_WALLPAPER: arg = path string pointer
        0x010B => {
            let path = match unsafe { user_access::read_user_string(arg, 256) } {
                Ok(s) => s,
                Err(_) => return errno::Errno::EFAULT as u64,
            };
            let mut comp = crate::gui::COMPOSITOR.lock();
            comp.set_wallpaper(path);
            crate::println!("DRM: wallpaper path set");
            0
        }
        _ => errno::Errno::ENOSYS as u64,
    }
}

fn sys_hash(hash_type: u64, password_ptr: *const u8, password_len: u64, salt_out_ptr: *mut u8, _iterations: u64) -> u64 {
    const HASH_SHA256_PBKDF2: u64 = 0;

    match hash_type {
        HASH_SHA256_PBKDF2 => {
            let pw_len = password_len as usize;
            if pw_len > 256 { return errno::Errno::EINVAL as u64; }
            let mut password = alloc::vec![0u8; pw_len];
            if pw_len > 0 {
                if unsafe { user_access::copy_from_user(&mut password, password_ptr).is_err() } {
                    return errno::Errno::EFAULT as u64;
                }
            }

            // salt_out_ptr points to a 48-byte buffer: [salt 16 | dk 32]
            let mut buf = [0u8; 48];
            if unsafe { user_access::copy_from_user(&mut buf[..16], salt_out_ptr).is_err() } {
                return errno::Errno::EFAULT as u64;
            }

            let iterations = if _iterations > 0 { _iterations as u32 } else { 10000 };

            let mut dk = [0u8; 32];
            crate::crypto::pbkdf2(&password, &buf[..16], iterations, 32, &mut dk);

            // Write back: salt (16) + dk (32) = 48 bytes
            buf[16..48].copy_from_slice(&dk);
            if unsafe { user_access::copy_to_user(salt_out_ptr, &buf).is_err() } {
                return errno::Errno::EFAULT as u64;
            }
            iterations as u64
        }
        _ => errno::Errno::ENOSYS as u64,
    }
}

/// SYS_OPENPTY — create master/slave PTY pair, returns packed (master_fd | slave_fd << 16)
fn sys_openpty() -> u64 {
    let (idx, pair) = match crate::pty::alloc_pty() {
        Some(p) => p,
        None => return errno::Errno::ENFILE as u64,
    };
    let proc_lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *proc_lock {
        let mut ft = proc.fd_table.lock();
        let m = ft.iter().position(|f| f.is_none()).unwrap_or(ft.len());
        if m >= 256 { return errno::Errno::ENFILE as u64; }
        if m == ft.len() { ft.push(None); }
        ft[m] = Some(FileDescriptor::PtyMaster { _idx: idx, pair: pair.clone() });
        let s = ft.iter().position(|f| f.is_none()).unwrap_or(ft.len());
        if s >= 256 { ft[m] = None; return errno::Errno::ENFILE as u64; }
        if s == ft.len() { ft.push(None); }
        ft[s] = Some(FileDescriptor::PtySlave { _idx: idx, pair });
        (m as u64) | ((s as u64) << 16)
    } else {
        errno::Errno::ENOTTY as u64
    }
}
