use core::arch::asm;

#[inline(always)]
pub fn syscall0(n: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, out("rcx") _, out("r11") _, lateout("rax") ret);
    }
    ret
}

#[inline(always)]
pub fn syscall1(n: u64, a1: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, out("rcx") _, out("r11") _, lateout("rax") ret);
    }
    ret
}

#[inline(always)]
pub fn syscall2(n: u64, a1: u64, a2: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, out("rcx") _, out("r11") _, lateout("rax") ret);
    }
    ret
}

#[inline(always)]
pub fn syscall3(n: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, out("rcx") _, out("r11") _, lateout("rax") ret);
    }
    ret
}

#[inline(always)]
pub fn syscall4(n: u64, a1: u64, a2: u64, a3: u64, a4: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, out("rcx") _, out("r11") _, lateout("rax") ret);
    }
    ret
}

#[inline(always)]
pub fn syscall5(n: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!("syscall", in("rax") n, in("rdi") a1, in("rsi") a2, in("rdx") a3, in("r10") a4, in("r8") a5, out("rcx") _, out("r11") _, lateout("rax") ret);
    }
    ret
}

pub fn exit(code: i32) -> ! {
    syscall1(crate::SYS_EXIT, code as u64);
    loop { unsafe { asm!("hlt"); } }
}

pub fn write(fd: u64, buf: &[u8]) -> u64 {
    let ret = syscall3(crate::SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64);
    crate::errno::check(ret)
}

pub fn read(fd: u64, buf: &mut [u8]) -> u64 {
    let ret = syscall3(crate::SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64);
    crate::errno::check(ret)
}

pub fn open(path: *const u8, flags: i32) -> u64 {
    let ret = syscall2(crate::SYS_OPEN, path as u64, flags as u64);
    crate::errno::check(ret)
}

pub fn close(fd: u64) -> u64 {
    let ret = syscall1(crate::SYS_CLOSE, fd);
    crate::errno::check(ret)
}

pub fn getpid() -> u64 {
    syscall0(crate::SYS_GETPID)
}

pub fn fork() -> u64 {
    let ret = syscall0(crate::SYS_FORK);
    crate::errno::check(ret)
}

pub fn execve(path: *const u8, argv: *const *const u8, envp: *const *const u8) -> u64 {
    let ret = syscall3(crate::SYS_EXECVE, path as u64, argv as u64, envp as u64);
    crate::errno::check(ret)
}

pub fn wait4(pid: i64, status: *mut i32, options: i32, rusage: *mut u8) -> u64 {
    let ret = syscall4(crate::SYS_WAIT4, pid as u64, status as u64, options as u64, rusage as u64);
    crate::errno::check(ret)
}

pub fn brk(addr: u64) -> u64 {
    syscall1(crate::SYS_BRK, addr)
}

pub fn sbrk(increment: i64) -> u64 {
    let current = syscall1(crate::SYS_BRK, 0);
    if increment == 0 { return current; }
    let new = (current as i64 + increment) as u64;
    syscall1(crate::SYS_BRK, new)
}

pub fn stat(path: *const u8, buf: *mut u8) -> u64 {
    let ret = syscall2(crate::SYS_STAT, path as u64, buf as u64);
    crate::errno::check(ret)
}

pub fn fstat(fd: u64, buf: *mut u8) -> u64 {
    let ret = syscall2(crate::SYS_FSTAT, fd, buf as u64);
    crate::errno::check(ret)
}

pub fn lseek(fd: u64, offset: i64, whence: i32) -> u64 {
    syscall3(crate::SYS_LSEEK, fd, offset as u64, whence as u64)
}

pub fn ioctl(fd: u64, request: u64, argp: *mut u8) -> u64 {
    let ret = syscall3(crate::SYS_IOCTL, fd, request, argp as u64);
    crate::errno::check(ret)
}

pub fn pipe(fds: *mut u32) -> u64 {
    let ret = syscall1(crate::SYS_PIPE, fds as u64);
    crate::errno::check(ret)
}

pub fn dup2(oldfd: u64, newfd: u64) -> u64 {
    let ret = syscall2(crate::SYS_DUP2, oldfd, newfd);
    crate::errno::check(ret)
}

pub fn nanosleep(req: *const u8, rem: *mut u8) -> u64 {
    let ret = syscall2(crate::SYS_NANOSLEEP, req as u64, rem as u64);
    crate::errno::check(ret)
}

pub fn socket(domain: u64, type_: u64, protocol: u64) -> u64 {
    let ret = syscall3(crate::SYS_SOCKET, domain, type_, protocol);
    crate::errno::check(ret)
}

pub fn bind(sockfd: u64, addr: *const u8, addrlen: u64) -> u64 {
    let ret = syscall3(crate::SYS_BIND, sockfd, addr as u64, addrlen);
    crate::errno::check(ret)
}

pub fn connect(sockfd: u64, addr: *const u8, addrlen: u64) -> u64 {
    let ret = syscall3(crate::SYS_CONNECT, sockfd, addr as u64, addrlen);
    crate::errno::check(ret)
}

pub fn sendto(sockfd: u64, buf: *const u8, len: u64, dest_addr: *const u8, addrlen: u64) -> u64 {
    let ret = syscall5(crate::SYS_SENDTO, sockfd, buf as u64, len, dest_addr as u64, addrlen);
    crate::errno::check(ret)
}

pub fn recvfrom(sockfd: u64, buf: *mut u8, len: u64, src_addr: *mut u8, addrlen: *mut u32) -> u64 {
    let ret = syscall5(crate::SYS_RECVFROM, sockfd, buf as u64, len, src_addr as u64, addrlen as u64);
    crate::errno::check(ret)
}

pub fn mkdir(path: *const u8, mode: u32) -> u64 {
    let ret = syscall2(crate::SYS_MKDIR, path as u64, mode as u64);
    crate::errno::check(ret)
}

pub fn unlink(path: *const u8) -> u64 {
    let ret = syscall1(crate::SYS_UNLINK, path as u64);
    crate::errno::check(ret)
}

pub fn getcwd(buf: *mut u8, size: usize) -> u64 {
    let ret = syscall2(crate::SYS_GETCWD, buf as u64, size as u64);
    crate::errno::check(ret)
}

pub fn chdir(path: *const u8) -> u64 {
    let ret = syscall1(crate::SYS_CHDIR, path as u64);
    crate::errno::check(ret)
}

pub fn futex(uaddr: *mut u32, op: u32, val: u32) -> u64 {
    syscall3(crate::SYS_FUTEX, uaddr as u64, op as u64, val as u64)
}

pub fn arch_prctl(code: u64, addr: u64) -> u64 {
    syscall2(crate::SYS_ARCH_PRCTL, code, addr)
}

pub fn clock_gettime(clk_id: u64, ts: *mut u8) -> u64 {
    let ret = syscall2(crate::SYS_CLOCK_GETTIME, clk_id, ts as u64);
    crate::errno::check(ret)
}

pub fn getdents64(fd: u64, buf: *mut u8, len: usize) -> u64 {
    let ret = syscall3(crate::SYS_GETDENTS64, fd, buf as u64, len as u64);
    crate::errno::check(ret)
}

pub fn kill(pid: i64, sig: u32) -> u64 {
    let ret = syscall2(crate::SYS_KILL, pid as u64, sig as u64);
    crate::errno::check(ret)
}

pub fn symlink(target: *const u8, linkpath: *const u8) -> u64 {
    let ret = syscall2(crate::SYS_SYMLINK, target as u64, linkpath as u64);
    crate::errno::check(ret)
}

pub fn readlink(path: *const u8, buf: *mut u8, bufsize: u64) -> u64 {
    let ret = syscall3(crate::SYS_READLINK, path as u64, buf as u64, bufsize);
    crate::errno::check(ret)
}

pub fn mount(source: *const u8, target: *const u8, fstype: *const u8, flags: u64) -> u64 {
    let ret = syscall5(crate::SYS_MOUNT, source as u64, target as u64, fstype as u64, flags, 0);
    crate::errno::check(ret)
}

pub fn umount2(target: *const u8, flags: u64) -> u64 {
    let ret = syscall2(crate::SYS_UMOUNT2, target as u64, flags);
    crate::errno::check(ret)
}

pub fn beep(freq_hz: u32, duration_ms: u32) -> u64 {
    syscall2(crate::SYS_BEEP, freq_hz as u64, duration_ms as u64)
}

pub fn sched_yield() {
    syscall0(crate::SYS_SCHED_YIELD);
}

pub fn select(nfds: u64, readfds: *mut u64, writefds: *mut u64, exceptfds: *mut u64, timeout: *const u64) -> u64 {
    let ret = syscall5(crate::SYS_SELECT, nfds, readfds as u64, writefds as u64, exceptfds as u64, timeout as u64);
    crate::errno::check(ret)
}

pub fn poll(fds: *mut u8, nfds: u64, timeout: u64) -> u64 {
    let ret = syscall3(crate::SYS_POLL, fds as u64, nfds, timeout);
    crate::errno::check(ret)
}

pub fn getuid() -> u64 {
    syscall0(crate::SYS_GETUID)
}

pub fn getgid() -> u64 {
    syscall0(crate::SYS_GETGID)
}

pub fn setuid(uid: u32) -> u64 {
    syscall1(crate::SYS_SETUID, uid as u64)
}

pub fn setgid(gid: u32) -> u64 {
    syscall1(crate::SYS_SETGID, gid as u64)
}

pub fn geteuid() -> u64 {
    syscall0(crate::SYS_GETEUID)
}

pub fn getegid() -> u64 {
    syscall0(crate::SYS_GETEGID)
}
