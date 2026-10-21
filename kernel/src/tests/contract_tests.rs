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
        if sys_close(target) != 0 {
            return Err("close(dup2 fd) failed");
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
}
