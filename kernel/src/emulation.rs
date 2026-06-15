use crate::task::process::{Process, EmulationMode};
use crate::syscalls::numbers;
use crate::syscalls::user_access;
use crate::syscalls::errno;

/// Dispatch a Linux syscall from a Linux ELF binary.
/// Returns the result directly (in Linux ABI: 0 = success, negative = -errno).
pub fn dispatch_linux_syscall(
    n: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    arg4: u64,
    arg5: u64,
    regs: *mut u64,
) -> u64 {
    match n {
        13 => linux_rt_sigaction(arg1 as u64, arg2 as *const u8, arg3 as *mut u8, arg4 as u64),
        15 => linux_rt_sigreturn(regs),
        57 => linux_fork(),
        63 => linux_uname(arg1 as *mut u8),
        158 => linux_arch_prctl(arg1 as u32, arg2 as u64),
        _ => {
    let vahi_n = map_linux_to_vahi(n);
    crate::syscalls::do_syscall(vahi_n, arg1, arg2, arg3, arg4, arg5, regs)
        }
    }
}

/// Linux `fork()` — implemented via clone with SIGCHLD.
fn linux_fork() -> u64 {
    let sigchld = 17u64; // SIGCHLD
    crate::syscalls::syscall_handler(numbers::SYS_CLONE, sigchld, 0, 0, 0, 0, core::ptr::null_mut())
}

/// Linux `uname()` — returns Linux-compatible utsname.
fn linux_uname(buf: *mut u8) -> u64 {
    #[repr(C, packed)]
    struct LinuxUtsName {
        sysname: [u8; 65],
        nodename: [u8; 65],
        release: [u8; 65],
        version: [u8; 65],
        machine: [u8; 65],
        domainname: [u8; 65],
    }

    let mut uts = LinuxUtsName {
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

    fill(&mut uts.sysname, "Linux");
    fill(&mut uts.nodename, "sarga-os");
    fill(&mut uts.release, "5.15.0-sarga");
    fill(&mut uts.version, "#1 SARGA OS Compatibility Layer");
    fill(&mut uts.machine, "x86_64");

    if unsafe { user_access::copy_to_user(buf, core::slice::from_raw_parts(&uts as *const _ as *const u8, core::mem::size_of::<LinuxUtsName>())) }.is_err() {
        return -(errno::Errno::EFAULT as i64) as u64;
    }
    0
}

/// Linux `arch_prctl()` — handles `ARCH_SET_FS` for TLS base.
fn linux_arch_prctl(code: u32, addr: u64) -> u64 {
    match code {
        0x1002 => { // ARCH_SET_FS
            if let Some(mut thread) = crate::task::scheduler::current_thread() {
                thread.fs_base = addr;
                crate::task::scheduler::set_current_thread(thread);
                // Write the FS base immediately so it's active
                crate::task::thread::write_fs_base(addr);
                0
            } else {
                -(errno::Errno::ESRCH as i64) as u64
            }
        }
        0x1003 => { // ARCH_GET_FS
            let fs_base = crate::task::scheduler::current_thread()
                .map(|t| t.fs_base)
                .unwrap_or(0);
            let out_ptr = addr as *mut u64;
            if unsafe { user_access::copy_to_user(out_ptr as *mut u8, core::slice::from_raw_parts(&fs_base as *const _ as *const u8, 8)) }.is_err() {
                return -(errno::Errno::EFAULT as i64) as u64;
            }
            0
        }
        0x1004 => { // ARCH_SET_GS (unused on x86_64 userspace, but accept)
            0
        }
        0x1005 => { // ARCH_GET_GS
            let out_ptr = addr as *mut u64;
            let gs: u64 = 0;
            if unsafe { user_access::copy_to_user(out_ptr as *mut u8, core::slice::from_raw_parts(&gs as *const _ as *const u8, 8)) }.is_err() {
                return -(errno::Errno::EFAULT as i64) as u64;
            }
            0
        }
        0x1001 => { // ARCH_GET_CPUID (deprecated, just succeed)
            0
        }
        _ => -(errno::Errno::EINVAL as i64) as u64,
    }
}

/// Linux `rt_sigaction()` — translates Linux `sigaction` struct to Vahi format.
fn linux_rt_sigaction(
    _signum: u64,
    _act: *const u8,
    _oldact: *mut u8,
    _sigsetsize: u64,
) -> u64 {
    if _act.is_null() && _oldact.is_null() {
        return 0;
    }

    if !_act.is_null() {
        let mut handler: u64 = 0;
        let mut flags: u64 = 0;
        let mut restorer: u64 = 0;
        unsafe {
            if user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut handler as *mut _ as *mut u8, 8),
                _act,
            ).is_err() {
                return -(errno::Errno::EFAULT as i64) as u64;
            }
            if user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut flags as *mut _ as *mut u8, 8),
                _act.add(8),
            ).is_err() {
                return -(errno::Errno::EFAULT as i64) as u64;
            }
            // sa_restorer at offset 16 (x86_64 Linux sigaction)
            if (flags & 0x04000000) != 0 { // SA_RESTORER
                if user_access::copy_from_user(
                    core::slice::from_raw_parts_mut(&mut restorer as *mut _ as *mut u8, 8),
                    _act.add(16),
                ).is_err() {
                    return -(errno::Errno::EFAULT as i64) as u64;
                }
            }
        }

        let lock = crate::task::process::CURRENT_PROCESS.lock();
        if let Some(ref proc) = *lock {
            if _signum < 32 {
                let idx = _signum as usize;
                proc.signal_handlers.lock()[idx] = handler;
                if restorer != 0 {
                    proc.signal_restorers.lock()[idx] = restorer;
                }
            }
        }
    }

    if !_oldact.is_null() {
        let handler = {
            let lock = crate::task::process::CURRENT_PROCESS.lock();
            lock.as_ref().and_then(|proc| {
                if _signum < 32 {
                    Some(proc.signal_handlers.lock()[_signum as usize])
                } else {
                    None
                }
            }).unwrap_or(0)
        };
        if unsafe { user_access::copy_to_user(
            _oldact,
            core::slice::from_raw_parts(&handler as *const _ as *const u8, 8),
        ) }.is_err() {
            return -(errno::Errno::EFAULT as i64) as u64;
        }
    }

    0
}

/// Linux `rt_sigreturn()` — returns from signal handler to interrupted context.
/// Delegates to the Vahi native rt_sigreturn which restores saved context.
fn linux_rt_sigreturn(regs_ptr: *mut u64) -> u64 {
    crate::syscalls::do_syscall(numbers::SYS_RT_SIGRETURN, 0, 0, 0, 0, 0, regs_ptr)
}

/// Map Linux x86_64 syscall number to Vahi syscall number.
fn map_linux_to_vahi(linux_n: u64) -> u64 {
    match linux_n {
        0 => numbers::SYS_READ,
        1 => numbers::SYS_WRITE,
        2 => numbers::SYS_OPEN,
        3 => numbers::SYS_CLOSE,
        4 => numbers::SYS_STAT,
        5 => numbers::SYS_FSTAT,
        8 => numbers::SYS_LSEEK,
        9 => numbers::SYS_MMAP,
        10 => numbers::_SYS_MPROTECT,
        11 => numbers::SYS_MUNMAP,
        12 => numbers::SYS_BRK,
        16 => numbers::SYS_IOCTL,
        21 => numbers::SYS_ACCESS,
        22 => numbers::SYS_PIPE,
        23 => numbers::SYS_SELECT,
        24 => numbers::SYS_SCHED_YIELD,
        32 => numbers::SYS_DUP,
        33 => numbers::SYS_DUP2,
        35 => numbers::SYS_NANOSLEEP,
        36 => numbers::SYS_SYNC,
        39 => numbers::SYS_GETPID,
        41 => numbers::SYS_SOCKET,
        42 => numbers::SYS_CONNECT,
        43 => numbers::SYS_ACCEPT,
        44 => numbers::SYS_SENDTO,
        45 => numbers::SYS_RECVFROM,
        49 => numbers::SYS_BIND,
        50 => numbers::SYS_LISTEN,
        56 => numbers::SYS_CLONE,
        59 => numbers::SYS_EXECVE,
        60 => numbers::SYS_EXIT,
        61 => numbers::SYS_WAIT4,
        62 => numbers::SYS_KILL,
        72 => numbers::SYS_FCNTL,
        79 => numbers::SYS_GETCWD,
        80 => numbers::SYS_CHDIR,
        82 => numbers::SYS_RENAME,
        83 => numbers::SYS_MKDIR,
        87 => numbers::SYS_UNLINK,
        88 => numbers::SYS_SYMLINK,
        89 => numbers::SYS_READLINK,
        91 => numbers::SYS_FCHMOD,
        93 => numbers::SYS_FCHOWN,
        110 => numbers::SYS_GETPPID,
        137 => numbers::SYS_STATFS,
        144 => numbers::SYS_SCHED_SETATTR,
        145 => numbers::SYS_SCHED_GETATTR,
        165 => numbers::SYS_MOUNT,
        167 => numbers::SYS_UMOUNT2,
        169 => numbers::SYS_REBOOT,
        200 => numbers::SYS_RESOLVE,
        202 => numbers::SYS_FUTEX,
        203 => numbers::SYS_SYSINFO,
        210 => numbers::SYS_OPENPTY,
        217 => numbers::SYS_GETDENTS64,
        218 => numbers::SYS_SET_TID_ADDRESS,
        228 => numbers::SYS_CLOCK_GETTIME,
        231 => numbers::SYS_EXIT_GROUP,
        321 => numbers::SYS_BPF,
        _ => 36, // SYS_SYNC as fallback (returns ENOSYS)
    }
}

/// Detect if the given ELF binary is a Linux binary (machine type, interpreter).
pub fn detect_linux_binary(elf_data: &[u8]) -> bool {
    use xmas_elf::ElfFile;
    if let Ok(elf) = ElfFile::new(elf_data) {
        let machine = elf.header.pt2.machine().as_machine();
        if machine != xmas_elf::header::Machine::X86_64 {
            return false;
        }
        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Interp) = ph.get_type() {
                let off = ph.offset() as usize;
                let size = ph.file_size() as usize;
                if off + size <= elf_data.len() {
                    let interp = &elf_data[off..off + size];
                    let interp_str = interp.split(|&b| b == 0).next().unwrap_or(b"");
                    if let Ok(s) = core::str::from_utf8(interp_str) {
                        if s.contains("ld-linux") || s.contains("ld.so") {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Set emulation mode on a process based on ELF header.
/// Native SARGA ELF binaries are detected by machine type (Vahi-specific EM).
pub fn set_emulation(process: &Process, elf_data: &[u8]) {
    use xmas_elf::ElfFile;
    let mode = if let Ok(elf) = ElfFile::new(elf_data) {
        let machine = elf.header.pt2.machine().as_machine();
        if machine == xmas_elf::header::Machine::X86_64 {
            if detect_linux_binary(elf_data) {
                EmulationMode::Linux
            } else {
                EmulationMode::Native
            }
        } else {
            EmulationMode::Native
        }
    } else {
        EmulationMode::Native
    };
    *process.emulation.lock() = mode;
}
