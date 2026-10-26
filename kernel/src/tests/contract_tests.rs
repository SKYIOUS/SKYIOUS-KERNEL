//! Executable contract for the lock-family campaign behavior (I4 + exit-code).
//!
//! Pins what the SMP-wedge fixes added, so a regression fails the TAP gate
//! instead of freezing a boot:
//!   - `exit_code` encoding: i32::MIN = not exited (wait4's reap predicate),
//!     raw u64→i32 store (kill_from_fault writes 139 = 128 + SIGSEGV).
//!   - `route_signal_to_signalfd_for`: mask-gated delivery with full ssi_*
//!     population, FIFO drain, and try-lock-only acquisition — holding any of
//!     its three locks bails/skips instead of blocking. With the re-entrancy
//!     detector armed, any regression to `lock()` panics right here.

use crate::memory::buddy::BuddyFrameAllocator;
use crate::memory::paging::AddressSpace;
use crate::selftest;
use crate::sync::IrqSafeMutex;
use crate::task::process::{
    route_signal_to_signalfd_for, FileDescriptor, Process, SignalFdData, CURRENT_PROCESS,
    SIGNAL_FDS, SI_CHILD,
};
use alloc::collections::VecDeque;
use alloc::sync::Arc;

// Shared userspace-page helpers live with the mqueue contracts; reuse them here.
use crate::tests::mqueue_contract::{
    user_read, user_read_struct, user_write, user_write_struct, with_user_page,
};

fn make_proc() -> Result<Arc<Process>, &'static str> {
    let mut fa = BuddyFrameAllocator;
    let aspace = AddressSpace::new(&mut fa).ok_or("AddressSpace::new failed")?;
    Ok(Arc::new(Process::new(Process::next_id(), None, aspace)))
}

fn install_signalfd(proc: &Arc<Process>, handle: u64, mask: u64) {
    SIGNAL_FDS.lock().insert(
        handle,
        Arc::new(IrqSafeMutex::new(SignalFdData {
            mask,
            pending: VecDeque::new(),
        })),
    );
    proc.files
        .lock()
        .fd_table
        .push(Some(FileDescriptor::SignalFd(handle)));
}

fn pending_len(handle: u64) -> usize {
    let fds = SIGNAL_FDS.lock();
    let data = fds.get(&handle).unwrap().lock();
    data.pending.len()
}

// ─── exit_code contract ───────────────────────────────────────────

fn test_exit_code_encoding() -> Result<(), &'static str> {
    use core::sync::atomic::Ordering;

    let p = make_proc()?;
    if p.exit_code.load(Ordering::Relaxed) != i32::MIN {
        return Err("fresh exit_code must be i32::MIN (wait4 skips it)");
    }

    // (writer, raw status, expected i32) — store is `raw as i32`; 139 is the
    // fault-kill encoding (128+SIGSEGV); high bits wrap.
    let cases: [(&str, u64, i32); 3] = [
        ("sys_exit(7)", 7, 7),
        ("kill_from_fault", 139, 139),
        ("exit(0x1_0000_0001) wraps", 0x1_0000_0001, 1),
    ];
    for (name, raw, expect) in cases {
        let p = make_proc()?;
        p.exit_code.store(raw as i32, Ordering::Relaxed);
        let v = p.exit_code.load(Ordering::Relaxed);
        let reapable = v != i32::MIN; // wait4's predicate
        if v != expect || !reapable {
            return Err(name);
        }
    }
    Ok(())
}

// ─── signalfd routing contract ────────────────────────────────────

fn test_signalfd_mask_matrix() -> Result<(), &'static str> {
    // (mask, routed signo, expected deliveries) — bit n selects signal n+1.
    let cases: [(&str, u64, u32, usize); 4] = [
        ("mask includes signo -> delivered", 1 << 16, 17, 1),
        ("mask excludes signo -> skipped", 1 << 16, 19, 0),
        ("mask 0 -> never delivers", 0, 17, 0),
        ("full mask -> delivers", u64::MAX, 19, 1),
    ];
    for (i, (name, mask, signo, expect)) in cases.iter().enumerate() {
        let handle = 0xF00D_0000 + i as u64;
        let proc = make_proc()?;
        install_signalfd(&proc, handle, *mask);
        route_signal_to_signalfd_for(&proc, *signo, SI_CHILD, 42, 7, 0);
        let n = pending_len(handle);
        SIGNAL_FDS.lock().remove(&handle);
        if n != *expect {
            return Err(*name);
        }
    }
    Ok(())
}

fn test_signalfd_info_fields() -> Result<(), &'static str> {
    let handle = 0xF00D_0010;
    let proc = make_proc()?;
    install_signalfd(&proc, handle, 1 << 16);

    // Two deliveries accumulate in FIFO order with full ssi_* context.
    route_signal_to_signalfd_for(&proc, 17, SI_CHILD, 1234, 99, 0xDEAD_BEEF);
    route_signal_to_signalfd_for(&proc, 17, SI_CHILD, 5678, 11, 0);

    {
        let fds = SIGNAL_FDS.lock();
        let data = fds.get(&handle).unwrap().lock();
        if data.pending.len() != 2 {
            return Err("expected 2 pending deliveries");
        }
        let first = data.pending.front().unwrap();
        if first.ssi_signo != 17
            || first.ssi_code != SI_CHILD
            || first.ssi_pid != 1234
            || first.ssi_uid != 99
            || first.ssi_sigval != 0xDEAD_BEEF
        {
            return Err("delivered info fields wrong");
        }
    }

    // Read path contract: pop_front drains in order.
    {
        let fds = SIGNAL_FDS.lock();
        let mut data = fds.get(&handle).unwrap().lock();
        let _ = data.pending.pop_front();
        if data.pending.len() != 1 || data.pending.front().unwrap().ssi_pid != 5678 {
            return Err("read did not drain FIFO");
        }
    }
    SIGNAL_FDS.lock().remove(&handle);
    Ok(())
}

fn test_signalfd_contention_bails() -> Result<(), &'static str> {
    let handle = 0xF00D_0020;
    let proc = make_proc()?;
    install_signalfd(&proc, handle, 1 << 16);

    // I4: every acquisition in the route is try_lock. Holding any one of its
    // three locks must bail (or skip that fd) without blocking — the exact
    // same-CPU re-acquisition that wedged SMP-4, caught at first use now.
    {
        let _files = proc.files.lock();
        route_signal_to_signalfd_for(&proc, 17, SI_CHILD, 1, 0, 0);
    }
    {
        let _fds = SIGNAL_FDS.lock();
        route_signal_to_signalfd_for(&proc, 17, SI_CHILD, 1, 0, 0);
    }
    {
        let fds = SIGNAL_FDS.lock();
        let data = fds.get(&handle).unwrap().clone();
        let _data = data.lock();
        route_signal_to_signalfd_for(&proc, 17, SI_CHILD, 1, 0, 0);
    }

    let n = pending_len(handle);
    SIGNAL_FDS.lock().remove(&handle);
    if n != 0 {
        return Err("contended route delivered a signal");
    }
    Ok(())
}

// ─── fd-table write-through contract ──────────────────────────────

/// `dup`/`dup2`/`fcntl` must mutate the live fd table and flag vector: every
/// returned fd resolves, and flag writes read back. An earlier version wrote
/// into a clone, so the returned numbers were absent from the real table.
fn test_fd_dup_fcntl_write_through() -> Result<(), &'static str> {
    use crate::syscalls::epoll::sys_epoll_create1;
    use crate::syscalls::errno::Errno;
    use crate::syscalls::fs_open::{sys_close, sys_dup, sys_dup2, sys_fcntl};

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc);
    let out = (|| {
        const F_DUPFD: i32 = 0;
        const F_GETFD: i32 = 1;
        const F_SETFD: i32 = 2;
        const F_GETFL: i32 = 3;
        const F_SETFL: i32 = 4;
        const O_NONBLOCK: u64 = 0x800;

        let fd = crate::syscalls::net_socket::sys_socket(1, 1, 0);
        if fd > 0xFFFF {
            return Err("socket(AF_UNIX) did not yield an fd");
        }
        let dup_fd = sys_dup(fd);
        if dup_fd > 0xFFFF || dup_fd == fd {
            return Err("dup did not install a new fd");
        }
        if sys_close(dup_fd) != 0 {
            return Err("close(dup fd) failed");
        }
        if sys_dup(dup_fd) != Errno::EBADF as u64 {
            return Err("dup of closed fd did not return EBADF");
        }
        let fcntl_fd = sys_fcntl(fd, F_DUPFD, 0);
        if fcntl_fd > 0xFFFF || sys_close(fcntl_fd) != 0 {
            return Err("fcntl(F_DUPFD) did not install a closable fd");
        }
        let target = fd + 10;
        if sys_dup2(fd, target) != target {
            return Err("dup2 did not install the target fd");
        }
        if sys_fcntl(target, F_SETFD, 1) != 0 || sys_fcntl(target, F_GETFD, 0) != 1 {
            return Err("FD_CLOEXEC did not write through");
        }
        if sys_fcntl(target, F_SETFL, O_NONBLOCK) != 0
            || sys_fcntl(target, F_GETFL, 0) & O_NONBLOCK == 0
        {
            return Err("O_NONBLOCK did not write through");
        }
        if sys_fcntl(target, F_GETFD, 0) != 1 {
            return Err("F_SETFL erased O_CLOEXEC");
        }
        if sys_close(target) != 0 {
            return Err("close(dup2 fd) failed");
        }
        let d3 = crate::syscalls::compat::sys_dup3(fd, fd + 20, 0x80000);
        if d3 != fd + 20 || sys_fcntl(d3, F_GETFD, 0) != 1 {
            return Err("dup3 O_CLOEXEC did not land on the live fd");
        }
        if crate::syscalls::compat::sys_dup3(fd, fd + 21, 0x1) != Errno::EINVAL as u64 {
            return Err("dup3 accepted a flag other than O_CLOEXEC");
        }
        if sys_close(d3) != 0 {
            return Err("close of dup3 fd failed");
        }
        let ep = sys_epoll_create1(0);
        if ep > 0xFFFF {
            return Err("epoll_create1 did not install an fd");
        }
        if sys_fcntl(ep, F_SETFD, 1) != 0 || sys_fcntl(ep, F_GETFD, 0) != 1 {
            return Err("epoll fd flags did not write through");
        }
        if sys_close(ep) != 0 || sys_close(fd) != 0 {
            return Err("close of socket/epoll fd failed");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

// ─── fd-install write-through contract ────────────────────────────

fn fd_is_file(proc: &Arc<Process>, fd: u64) -> bool {
    proc.with_fd_table(|table, _| {
        matches!(
            table.get(fd as usize),
            Some(Some(FileDescriptor::File { .. }))
        )
    })
}

fn fd_count(proc: &Arc<Process>) -> usize {
    proc.with_fd_table(|table, _| table.iter().filter(|slot| slot.is_some()).count())
}

/// `install_pipe` must write both ends into the live table (the previous
/// handle-only install left the returned numbers absent, so read/write hit
/// EBADF) and store the pipe2 flags in the same lock.
fn test_pipe_install_write_through() -> Result<(), &'static str> {
    const O_CLOEXEC: u64 = 0x80000;
    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let (reader, writer) = crate::vfs::pipe::Pipe::new();
        let (r, w) = crate::syscalls::fs_open::install_pipe(&proc, reader, writer, O_CLOEXEC);
        if !fd_is_file(&proc, r) || !fd_is_file(&proc, w) {
            return Err("pipe fds missing from the live fd table");
        }
        let flags_ok = proc.with_fd_table(|_, flags| {
            flags.get(r as usize) == Some(&O_CLOEXEC) && flags.get(w as usize) == Some(&O_CLOEXEC)
        });
        if !flags_ok {
            return Err("pipe O_CLOEXEC did not reach fd_flags");
        }
        let io_ok = proc.with_fd_table(|table, _| {
            let reader = match table.get(r as usize) {
                Some(Some(FileDescriptor::File { node, .. })) => node.clone(),
                _ => return false,
            };
            let writer = match table.get(w as usize) {
                Some(Some(FileDescriptor::File { node, .. })) => node.clone(),
                _ => return false,
            };
            writer.write(b"ping").is_ok()
                && matches!(reader.read(4), Ok(data) if data.as_slice() == b"ping")
        });
        if !io_ok {
            return Err("pipe ends are not readable/writable");
        }
        if crate::syscalls::fs_open::sys_close(r) != 0
            || crate::syscalls::fs_open::sys_close(w) != 0
        {
            return Err("close of a pipe fd failed");
        }
        let cleared = proc.with_fd_table(|table, flags| {
            table.get(r as usize).map_or(true, |slot| slot.is_none())
                && table.get(w as usize).map_or(true, |slot| slot.is_none())
                && flags.get(r as usize).copied().unwrap_or(0) == 0
                && flags.get(w as usize).copied().unwrap_or(0) == 0
        });
        if !cleared {
            return Err("close left pipe fd entries behind");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `open` must return a number that indexes the live fd table and honour
/// O_CLOEXEC there; the resolver below is the always-mounted ctlfs node.
fn test_open_install_write_through() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;

    const O_CLOEXEC: i32 = 0x80000;
    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let fd = crate::syscalls::fs_open::open_file("/ctl/kernel/hostname", O_CLOEXEC, 0);
        if (fd as i64) < 0 {
            return Err("open(/ctl/kernel/hostname) failed");
        }
        if !fd_is_file(&proc, fd) {
            return Err("open fd missing from the live fd table");
        }
        let cloexec = proc.with_fd_table(|_, flags| {
            flags
                .get(fd as usize)
                .map_or(false, |f| f & O_CLOEXEC as u64 != 0)
        });
        if !cloexec {
            return Err("open O_CLOEXEC did not reach fd_flags");
        }
        let readable = proc.with_fd_table(|table, _| match table.get(fd as usize) {
            Some(Some(FileDescriptor::File { node, .. })) => {
                node.read(16).map_or(false, |data| !data.is_empty())
            }
            _ => false,
        });
        if !readable {
            return Err("open fd does not resolve to a readable node");
        }
        if crate::syscalls::fs_open::sys_close(fd) != 0 {
            return Err("close of open fd failed");
        }
        if crate::syscalls::fs_open::sys_close(fd) != Errno::EBADF as u64 {
            return Err("second close of open fd did not return EBADF");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `openpty` must install master and slave in the live table and close both;
/// the previous clone-only install returned numbers that resolved to EBADF.
fn test_openpty_install_write_through() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let packed = crate::syscalls::gui::sys_openpty();
        if (packed as i64) < 0 {
            return Err("openpty failed");
        }
        let m = packed & 0xFFFF;
        let s = (packed >> 16) & 0xFFFF;
        let installed = proc.with_fd_table(|table, _| {
            matches!(
                table.get(m as usize),
                Some(Some(FileDescriptor::PtyMaster { .. }))
            ) && matches!(
                table.get(s as usize),
                Some(Some(FileDescriptor::PtySlave { .. }))
            )
        });
        if !installed {
            return Err("pty fds missing from the live fd table");
        }
        if crate::syscalls::fs_open::sys_close(m) != 0
            || crate::syscalls::fs_open::sys_close(s) != 0
        {
            return Err("close of a pty fd failed");
        }
        let cleared = proc.with_fd_table(|table, _| {
            table.get(m as usize).map_or(true, |slot| slot.is_none())
                && table.get(s as usize).map_or(true, |slot| slot.is_none())
        });
        if !cleared {
            return Err("close left pty fd entries behind");
        }
        if crate::syscalls::fs_open::sys_close(m) != Errno::EBADF as u64 {
            return Err("second close of pty fd did not return EBADF");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `sys_close` must drop the handle-table entry too: otherwise `add_fd` keeps
/// skipping the freed index and every open/close pair burns a number.
fn test_open_close_frees_fd_and_handle() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;
    use crate::syscalls::fs_open::{open_file, sys_close};

    const O_CLOEXEC: i32 = 0x80000;
    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let fd = open_file("/ctl/kernel/hostname", O_CLOEXEC, 0);
        if (fd as i64) < 0 {
            return Err("open(/ctl/kernel/hostname) failed");
        }
        if !proc.handle_table.lock().is_valid(fd) {
            return Err("open fd has no handle-table entry");
        }
        if sys_close(fd) != 0 {
            return Err("close of open fd failed");
        }
        if proc.handle_table.lock().is_valid(fd) {
            return Err("close left the handle-table entry behind");
        }
        let again = open_file("/ctl/kernel/hostname", O_CLOEXEC, 0);
        if again != fd {
            return Err("open after close did not reuse the freed fd number");
        }
        if sys_close(again) != 0 {
            return Err("close of reopened fd failed");
        }
        if sys_close(again) != Errno::EBADF as u64 {
            return Err("double close did not return EBADF");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// AF_UNIX accept must install the returned fd into the live table (not a
/// clone) and accept4 must put its flags on that same live slot. The backlog
/// is staged directly because bind/connect need user-memory sockaddrs.
fn test_unix_accept_installs_live_fd() -> Result<(), &'static str> {
    use crate::syscalls::fs_open::sys_close;
    use crate::syscalls::net_socket::{sys_accept, sys_accept4};

    const SOCK_CLOEXEC: u64 = 0x80000;
    const SOCK_NONBLOCK: u64 = 0x800;

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let listener = crate::net::unix::create_unix_socket(1)
            .map_err(|_| "create_unix_socket(listener) failed")?;
        let listen_fd = proc.with_fd_table(|fd_table, _| {
            fd_table.push(Some(FileDescriptor::UnixSocket(
                listener,
                crate::task::process::SocketType::Unix,
            )));
            (fd_table.len() - 1) as u64
        });
        {
            let socks = crate::net::unix::UNIX_SOCKETS.lock();
            let sock = socks.get(&listener).ok_or("listener missing")?;
            let mut inner = sock.inner.lock();
            inner.is_listening = true;
            inner.bind_path = Some(alloc::string::String::from("/contract.sock"));
        }

        let client = crate::net::unix::create_unix_socket(1)
            .map_err(|_| "create_unix_socket(client) failed")?;
        crate::net::unix::UNIX_SOCKETS
            .lock()
            .get(&listener)
            .ok_or("listener missing")?
            .inner
            .lock()
            .backlog
            .push(client);

        let fd = sys_accept(listen_fd, core::ptr::null_mut(), core::ptr::null_mut());
        if (fd as i64) < 0 {
            return Err("accept on a queued AF_UNIX connection failed");
        }
        let live = proc.with_fd_table(|table, _| {
            matches!(
                table.get(fd as usize),
                Some(Some(FileDescriptor::UnixSocket(h, _))) if *h != listener
            )
        });
        if !live {
            return Err("accepted fd is not live in the real fd table");
        }

        let client2 = crate::net::unix::create_unix_socket(1)
            .map_err(|_| "create_unix_socket(client2) failed")?;
        crate::net::unix::UNIX_SOCKETS
            .lock()
            .get(&listener)
            .ok_or("listener missing")?
            .inner
            .lock()
            .backlog
            .push(client2);

        let fd4 = sys_accept4(
            listen_fd,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            SOCK_CLOEXEC | SOCK_NONBLOCK,
        );
        if (fd4 as i64) < 0 {
            return Err("accept4 on a queued AF_UNIX connection failed");
        }
        let flags_ok = proc.with_fd_table(|table, flags| {
            matches!(
                table.get(fd4 as usize),
                Some(Some(FileDescriptor::UnixSocket(..)))
            ) && flags.get(fd4 as usize).map_or(false, |f| {
                f & (SOCK_CLOEXEC | SOCK_NONBLOCK) == SOCK_CLOEXEC | SOCK_NONBLOCK
            })
        });
        if !flags_ok {
            return Err("accept4 flags did not land on the live fd");
        }
        if sys_close(fd) != 0 || sys_close(fd4) != 0 || sys_close(listen_fd) != 0 {
            return Err("close of accepted AF_UNIX fds failed");
        }
        crate::net::unix::cleanup_unix_socket(client);
        crate::net::unix::cleanup_unix_socket(client2);
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// pipe2 accepts only O_CLOEXEC|O_NONBLOCK; anything else is EINVAL and must
/// not install (or leak) a pipe pair.
fn test_pipe2_rejects_unknown_flags() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let before = fd_count(&proc);
        for flags in [0x1, 0x4000, i32::MIN] {
            let rc = crate::syscalls::compat::sys_pipe2(core::ptr::null_mut(), flags);
            if rc != Errno::EINVAL as u64 {
                return Err("pipe2 accepted an unsupported flag");
            }
        }
        if fd_count(&proc) != before {
            return Err("rejected pipe2 installed fds");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// A bad `fds` buffer must roll back both entries, not leak them.
fn test_pipe_rolls_back_on_bad_user_buffer() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let before = fd_count(&proc);
        let rc = crate::syscalls::fs_open::sys_pipe(core::ptr::null_mut());
        if rc != Errno::EFAULT as u64 {
            return Err("pipe with a null fds pointer did not return EFAULT");
        }
        if fd_count(&proc) != before {
            return Err("pipe leaked fds on EFAULT");
        }
        let rc = crate::syscalls::compat::sys_pipe2(core::ptr::null_mut(), 0x80000);
        if rc != Errno::EFAULT as u64 {
            return Err("pipe2 with a null fds pointer did not return EFAULT");
        }
        if fd_count(&proc) != before {
            return Err("pipe2 leaked fds on EFAULT");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `memfd_create` must route its name through `read_user_string` and reject
/// any pointer outside the userspace range instead of dereferencing it.
///
/// The selftest environment maps no userspace pages (see `abi_args.rs`), so the
/// success path — read a name, create the tmpfs node, install the fd — cannot be
/// driven here. Asserting an install would pass regardless of the handler and is
/// the vacuous coverage this repo rejects. What *is* observable, and what the
/// syscall-hygiene fix is actually about, is that a kernel-space name is
/// EFAULT-rejected and leaks no fd.
fn test_memfd_create_rejects_non_user_name() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;
    use crate::syscalls::shm::sys_memfd_create;

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let before = fd_count(&proc);
        // .rodata string: a kernel address, must not be dereferenced.
        let kernel_name = b"test_memfd\0";
        let rc = sys_memfd_create(kernel_name.as_ptr(), 0);
        if rc != Errno::EFAULT as u64 {
            return Err("memfd_create did not reject a kernel-space name");
        }
        let rc = sys_memfd_create(core::ptr::null(), 0);
        if rc != Errno::EFAULT as u64 {
            return Err("memfd_create with null name did not return EFAULT");
        }
        if fd_count(&proc) != before {
            return Err("rejected memfd_create leaked an fd");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

/// `socketpair` must reject a non-AF_UNIX domain (EAFNOSUPPORT) and an unknown
/// socket type (EINVAL) before installing anything.
///
/// The install/rollback contract is owned by
/// `userptr:socketpair_rolls_back_on_bad_user_buffer`; the success path needs
/// mapped userspace pages the selftest environment does not provide (see
/// `abi_args.rs`), so this test pins only the argument-validation half.
fn test_socketpair_validates_domain_and_type() -> Result<(), &'static str> {
    use crate::syscalls::errno::Errno;
    use crate::syscalls::net_socket::sys_socketpair;

    let previous = CURRENT_PROCESS.lock().clone();
    let proc = make_proc()?;
    *CURRENT_PROCESS.lock() = Some(proc.clone());
    let out = (|| {
        let before = fd_count(&proc);
        let rc = sys_socketpair(99, 1, 0, core::ptr::null_mut());
        if rc != Errno::EAFNOSUPPORT as u64 {
            return Err("socketpair accepted a non-AF_UNIX domain");
        }
        let rc = sys_socketpair(1, 99, 0, core::ptr::null_mut());
        if rc != Errno::EINVAL as u64 {
            return Err("socketpair accepted an unknown socket type");
        }
        if fd_count(&proc) != before {
            return Err("rejected socketpair installed an fd");
        }
        Ok(())
    })();
    *CURRENT_PROCESS.lock() = previous;
    out
}

fn test_sendmsg_recvmsg_split_into_iovecs() -> Result<(), &'static str> {
    use crate::syscalls::net_helpers::{iovec, msghdr};
    use crate::syscalls::net_options::{sys_recvmsg, sys_sendmsg};
    use crate::syscalls::net_socket::sys_socketpair;

    with_user_page(|base| {
        const SV: u64 = 0;
        const HDR_SEND: u64 = 32;
        const IOV_SEND: u64 = 128;
        const HDR_RECV: u64 = 192;
        const IOV_RECV: u64 = 288;
        const DATA: u64 = 352;
        const OUT_A: u64 = 384;
        const OUT_B: u64 = 400;

        let rc = sys_socketpair(1, 1, 0, (base + SV) as *mut i32);
        if rc != 0 {
            return Err("socketpair failed");
        }
        let mut sv = [0i32; 2];
        // SAFETY: `SV..SV+8` is inside the mapped page at `base`.
        unsafe {
            user_read(
                base + SV,
                core::slice::from_raw_parts_mut(sv.as_mut_ptr() as *mut u8, 8),
            )?
        };
        let (send_fd, recv_fd) = (sv[0] as i64, sv[1] as i64);

        let send_iov = iovec {
            iov_base: (base + DATA) as *mut u8,
            iov_len: 5,
        };
        let send_hdr = msghdr {
            msg_iov: (base + IOV_SEND) as *const iovec,
            msg_iovlen: 1,
            ..msghdr::default()
        };
        user_write(base + DATA, b"hello")?;
        user_write_struct(base + IOV_SEND, &send_iov)?;
        user_write_struct(base + HDR_SEND, &send_hdr)?;

        let sent = sys_sendmsg(send_fd, (base + HDR_SEND) as *const msghdr, 0);
        if sent != 5 {
            return Err("sendmsg returned the wrong length");
        }

        let recv_iovs = [
            iovec {
                iov_base: (base + OUT_A) as *mut u8,
                iov_len: 2,
            },
            iovec {
                iov_base: (base + OUT_B) as *mut u8,
                iov_len: 16,
            },
        ];
        let mut recv_hdr = msghdr {
            msg_iov: (base + IOV_RECV) as *const iovec,
            msg_iovlen: 2,
            ..msghdr::default()
        };
        for (i, iov) in recv_iovs.iter().enumerate() {
            user_write_struct(base + IOV_RECV + (i as u64) * 16, iov)?;
        }
        user_write_struct(base + HDR_RECV, &recv_hdr)?;

        let received = sys_recvmsg(recv_fd, (base + HDR_RECV) as *mut msghdr, 0);
        if received != 5 {
            return Err("recvmsg returned the wrong length");
        }

        let mut out_a = [0u8; 2];
        let mut out_b = [0u8; 3];
        user_read(base + OUT_A, &mut out_a)?;
        user_read(base + OUT_B, &mut out_b)?;
        if &out_a != b"he" || &out_b != b"llo" {
            return Err("recvmsg did not split the payload across iovecs");
        }

        user_read_struct(base + HDR_RECV, &mut recv_hdr)?;
        if recv_hdr.msg_iovlen != 2 {
            return Err("recvmsg overwrote msg_iovlen instead of msg_flags");
        }
        Ok(())
    })?
}

pub fn register() {
    selftest::register("contract:exit_code_encoding", test_exit_code_encoding);
    selftest::register("contract:signalfd_mask_matrix", test_signalfd_mask_matrix);
    selftest::register("contract:signalfd_info_fields", test_signalfd_info_fields);
    selftest::register(
        "contract:signalfd_contention_bails",
        test_signalfd_contention_bails,
    );
    selftest::register(
        "contract:dup_fcntl_epoll_write_through",
        test_fd_dup_fcntl_write_through,
    );
    selftest::register(
        "contract:pipe_install_write_through",
        test_pipe_install_write_through,
    );
    selftest::register(
        "contract:open_install_write_through",
        test_open_install_write_through,
    );
    selftest::register(
        "contract:openpty_install_write_through",
        test_openpty_install_write_through,
    );
    selftest::register(
        "contract:pipe_rolls_back_on_bad_user_buffer",
        test_pipe_rolls_back_on_bad_user_buffer,
    );
    selftest::register(
        "contract:open_close_frees_fd_and_handle",
        test_open_close_frees_fd_and_handle,
    );
    selftest::register(
        "contract:unix_accept_installs_live_fd",
        test_unix_accept_installs_live_fd,
    );
    selftest::register(
        "contract:pipe2_rejects_unknown_flags",
        test_pipe2_rejects_unknown_flags,
    );
    selftest::register(
        "contract:memfd_create_rejects_non_user_name",
        test_memfd_create_rejects_non_user_name,
    );
    selftest::register(
        "contract:socketpair_validates_domain_and_type",
        test_socketpair_validates_domain_and_type,
    );
    selftest::register(
        "contract:sendmsg_recvmsg_split_into_iovecs",
        test_sendmsg_recvmsg_split_into_iovecs,
    );
}
