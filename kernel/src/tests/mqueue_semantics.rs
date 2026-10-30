//! Contract tests for POSIX message queue behaviour past the fd round trip:
//! notification, absolute deadlines, the `read(2)` status line,
//! `RLIMIT_MSGQUEUE` accounting and IPC-namespace scoping.
use crate::selftest;
use crate::syscalls::fs_open::sys_close;
use crate::task::process::{Process, CURRENT_PROCESS};
use crate::tests::mqueue_contract::{user_read, user_write, user_write_struct, with_user_page};

/// Test mq_notify: event validation, EBUSY, one-shot delivery on the
/// empty → non-empty edge, and deregistration when the owner closes.
pub fn test_mq_notify() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_notify, mq_open, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;
    use crate::syscalls::posix_timers::{SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD};

    /// Kernel ABI `struct sigevent` (64 bytes) as `mq_notify` reads it.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SigEvent {
        value: i64,
        signo: i32,
        notify: i32,
        pad: [u8; 48],
    }

    with_user_page(|base| {
        const NAME: u64 = 0;
        const EV: u64 = 64;
        const MSG: u64 = 192;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        let me = CURRENT_PROCESS.lock().clone().ok_or("no current process")?;
        // Delivery resolves the owner through PROCESS_TABLE; this helper's
        // process is not registered by default.
        Process::register(me.clone());
        let pid = me.id;

        let result = (|| -> Result<(), &'static str> {
            user_write(base + NAME, b"test_notify\0")?;
            user_write(base + MSG, b"x")?;

            let fd = mq_open(
                (base + NAME) as *const u8,
                O_CREAT | O_RDWR,
                0o600,
                core::ptr::null_mut(),
            );
            if (fd as i64) < 0 {
                return Err("mq_open failed");
            }
            let other = mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut());
            if (other as i64) < 0 {
                return Err("second mq_open failed");
            }

            let signal = SigEvent {
                value: 0,
                signo: 10,
                notify: SIGEV_SIGNAL,
                pad: [0; 48],
            };
            let none = SigEvent {
                value: 0,
                signo: 0,
                notify: SIGEV_NONE,
                pad: [0; 48],
            };
            let thread = SigEvent {
                value: 0,
                signo: 0,
                notify: SIGEV_THREAD,
                pad: [0; 48],
            };

            // SIGEV_THREAD needs the netlink transport this kernel lacks.
            user_write_struct(base + EV, &thread)?;
            if mq_notify(fd as i32, (base + EV) as *const u8) != Errno::ENOSYS as u64 {
                return Err("SIGEV_THREAD did not return ENOSYS");
            }
            // Signal numbers outside 1..=64 and unknown kinds are EINVAL.
            for bad in [
                SigEvent { signo: 0, ..signal },
                SigEvent {
                    signo: 65,
                    ..signal
                },
                SigEvent {
                    notify: 99,
                    ..signal
                },
            ] {
                user_write_struct(base + EV, &bad)?;
                if mq_notify(fd as i32, (base + EV) as *const u8) != Errno::EINVAL as u64 {
                    return Err("mq_notify accepted an invalid sigevent");
                }
            }

            // No registration yet: a NULL request is a successful no-op.
            if mq_notify(fd as i32, core::ptr::null()) != 0 {
                return Err("NULL mq_notify failed");
            }

            user_write_struct(base + EV, &signal)?;
            if mq_notify(fd as i32, (base + EV) as *const u8) != 0 {
                return Err("mq_notify failed");
            }
            if mq_notify(other as i32, (base + EV) as *const u8) != Errno::EBUSY as u64 {
                return Err("second registration did not return EBUSY");
            }

            // Empty → non-empty raises the signal once and self-clears.
            if mq_send(other as i32, (base + MSG) as *const u8, 1, 0) != 0 {
                return Err("mq_send failed");
            }
            if me.signals.lock().pending & (1 << 9) == 0 {
                return Err("mq_notify did not raise the signal on empty->non-empty");
            }
            me.signals.lock().pending &= !(1 << 9);
            if mq_send(other as i32, (base + MSG) as *const u8, 1, 0) != 0 {
                return Err("mq_send failed");
            }
            if me.signals.lock().pending != 0 {
                return Err("mq_notify fired more than once");
            }

            // Registration succeeds again only because delivery cleared it.
            if mq_notify(fd as i32, (base + EV) as *const u8) != 0 {
                return Err("re-registration after delivery failed");
            }

            // Closing the owner's descriptor deregisters it.
            if sys_close(fd) != 0 {
                return Err("close failed");
            }
            if mq_notify(other as i32, (base + EV) as *const u8) != 0 {
                return Err("close did not clear the notification");
            }
            if mq_notify(other as i32, core::ptr::null()) != 0 {
                return Err("NULL mq_notify failed");
            }

            // SIGEV_NONE still occupies the single slot until it fires.
            user_write_struct(base + EV, &none)?;
            if mq_notify(other as i32, (base + EV) as *const u8) != 0 {
                return Err("SIGEV_NONE was rejected");
            }
            if mq_notify(other as i32, (base + EV) as *const u8) != Errno::EBUSY as u64 {
                return Err("SIGEV_NONE did not reserve the registration slot");
            }
            if mq_notify(other as i32, core::ptr::null()) != 0 {
                return Err("NULL mq_notify failed");
            }

            if sys_close(other) != 0 {
                return Err("close failed");
            }
            if mq_unlink((base + NAME) as *const u8) != 0 {
                return Err("mq_unlink failed");
            }
            Ok(())
        })();

        // Always restore the shared state: a leftover pending signal breaks
        // later blocking tests, and a table entry leaks the address space.
        me.signals.lock().pending = 0;
        crate::task::process::PROCESS_TABLE.lock().remove(&pid);
        result
    })?
}

/// Test mq_timedsend/mq_timedreceive deadline validation and expiry. All
/// deadlines here are already past: the suite runs with interrupts disabled,
/// so a wait would never observe the next timer tick.
pub fn test_mq_timeouts() -> Result<(), &'static str> {
    use crate::ipc::mqueue::queue::MqAttr;
    use crate::ipc::mqueue::{mq_open, mq_timedreceive, mq_timedsend, mq_unlink};
    use crate::syscalls::errno::Errno;
    use crate::syscalls::posix_timers::timespec;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 64;
        const OUT: u64 = 128;
        const ATTR: u64 = 192;
        const TS: u64 = 256;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_timeout\0")?;
        user_write(base + MSG, b"m")?;

        // One message of room: the second send is the blocking case.
        let attr = MqAttr {
            mq_maxmsg: 1,
            mq_msgsize: 64,
            ..MqAttr::default_attrs()
        };
        user_write_struct(base + ATTR, &attr)?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            (base + ATTR) as *mut u8,
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        // Absolute CLOCK_REALTIME epoch. Deadline 0 is expired against any
        // clock value, so the check cannot depend on RTC state or ticks.
        let past = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        user_write_struct(base + TS, &past)?;

        if mq_timedreceive(
            fd as i32,
            (base + OUT) as *mut u8,
            64,
            core::ptr::null_mut(),
            (base + TS) as *const u8,
        ) != Errno::ETIMEDOUT as u64
        {
            return Err("expired receive deadline did not return ETIMEDOUT");
        }

        // A deadline only matters once the operation would block.
        if mq_timedsend(
            fd as i32,
            (base + MSG) as *const u8,
            1,
            0,
            (base + TS) as *const u8,
        ) != 0
        {
            return Err("timedsend on a non-full queue failed");
        }
        if mq_timedsend(
            fd as i32,
            (base + MSG) as *const u8,
            1,
            0,
            (base + TS) as *const u8,
        ) != Errno::ETIMEDOUT as u64
        {
            return Err("expired send deadline on a full queue did not return ETIMEDOUT");
        }

        // Malformed timespecs are rejected before the descriptor is used.
        for bad in [
            timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
            timespec {
                tv_sec: 0,
                tv_nsec: -1,
            },
            timespec {
                tv_sec: -1,
                tv_nsec: 0,
            },
        ] {
            user_write_struct(base + TS, &bad)?;
            if mq_timedreceive(
                fd as i32,
                (base + OUT) as *mut u8,
                64,
                core::ptr::null_mut(),
                (base + TS) as *const u8,
            ) != Errno::EINVAL as u64
            {
                return Err("mq_timedreceive accepted a malformed timespec");
            }
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        if mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test read(2) on a queue descriptor: the /dev/mqueue status line, EOF, a
/// per-description file position, and EBADF for a write-only descriptor.
pub fn test_mq_read_status_line() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_unlink};
    use crate::syscalls::errno::Errno;
    use crate::syscalls::fs_io::sys_read;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const OUT: u64 = 64;
        const O_RDONLY: i32 = 0;
        const O_WRONLY: i32 = 1;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_read\0")?;

        let wfd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_WRONLY,
            0o600,
            core::ptr::null_mut(),
        );
        if (wfd as i64) < 0 {
            return Err("mq_open failed");
        }
        let fd = mq_open(
            (base + NAME) as *const u8,
            O_RDONLY,
            0,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("second mq_open failed");
        }

        let n = sys_read(fd, (base + OUT) as *mut u8, 4096);
        if n == 0 || (n as i64) < 0 {
            return Err("read on a queue descriptor returned nothing");
        }
        let mut line = alloc::vec![0u8; n as usize];
        user_read(base + OUT, &mut line)?;
        let line = core::str::from_utf8(&line).map_err(|_| "status line is not UTF-8")?;
        if !line.starts_with("QSIZE:") {
            return Err("status line does not start with QSIZE:");
        }
        for field in ["NOTIFY:", "SIGNO:", "NOTIFY_PID:"] {
            if !line.contains(field) {
                return Err("status line is missing a notification field");
            }
        }
        if !line.ends_with('\n') {
            return Err("status line is not newline-terminated");
        }

        // One line per description: the next read is at EOF.
        if sys_read(fd, (base + OUT) as *mut u8, 4096) != 0 {
            return Err("second read did not return EOF");
        }

        // A fresh description starts at offset 0 and honours `count`.
        let short = mq_open(
            (base + NAME) as *const u8,
            O_RDONLY,
            0,
            core::ptr::null_mut(),
        );
        if (short as i64) < 0 {
            return Err("third mq_open failed");
        }
        if sys_read(short, (base + OUT) as *mut u8, 6) != 6 {
            return Err("short read was not clamped to count");
        }
        let mut head = [0u8; 6];
        user_read(base + OUT, &mut head)?;
        if &head != b"QSIZE:" {
            return Err("short read did not return the line prefix");
        }
        if sys_read(short, (base + OUT) as *mut u8, 0) != 0 {
            return Err("zero-length read was not a no-op");
        }

        if sys_read(wfd, (base + OUT) as *mut u8, 16) != Errno::EBADF as u64 {
            return Err("write-only descriptor was readable");
        }

        if sys_close(wfd) != 0 || sys_close(fd) != 0 || sys_close(short) != 0 {
            return Err("close failed");
        }
        if mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test RLIMIT_MSGQUEUE accounting: EMFILE when the worst-case charge would
/// exceed the limit, released when the queue is destroyed, and never charged
/// for opening an existing name.
pub fn test_mq_rlimit_msgqueue() -> Result<(), &'static str> {
    use crate::ipc::mqueue::queue::MqAttr;
    use crate::ipc::mqueue::{mq_open, mq_unlink};
    use crate::syscalls::errno::Errno;

    const RLIMIT_MSGQUEUE: usize = 12;
    // sizeof(msg_msg) + one posix_msg_tree_node, as mqueue charges it.
    const OVERHEAD: i64 = 96;

    with_user_page(|base| {
        const NAME_A: u64 = 0;
        const NAME_B: u64 = 64;
        const ATTR: u64 = 128;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME_A, b"test_rlimit_a\0")?;
        user_write(base + NAME_B, b"test_rlimit_b\0")?;

        let attr = MqAttr {
            mq_maxmsg: 1,
            mq_msgsize: 64,
            ..MqAttr::default_attrs()
        };
        user_write_struct(base + ATTR, &attr)?;

        let me = CURRENT_PROCESS.lock().clone().ok_or("no current process")?;
        let saved = {
            let mut lim = me.limits.lock();
            let saved = lim.rlim_cur[RLIMIT_MSGQUEUE];
            lim.rlim_cur[RLIMIT_MSGQUEUE] = 0;
            saved
        };

        let result = (|| -> Result<(), &'static str> {
            if mq_open(
                (base + NAME_A) as *const u8,
                O_CREAT | O_RDWR,
                0o600,
                (base + ATTR) as *mut u8,
            ) != Errno::EMFILE as u64
            {
                return Err("RLIMIT_MSGQUEUE=0 did not reject creation with EMFILE");
            }
            Ok(())
        })();

        if result.is_ok() {
            let budget = 64 + OVERHEAD;
            me.limits.lock().rlim_cur[RLIMIT_MSGQUEUE] = budget;
            let result = (|| -> Result<(), &'static str> {
                let first = mq_open(
                    (base + NAME_A) as *const u8,
                    O_CREAT | O_RDWR,
                    0o600,
                    (base + ATTR) as *mut u8,
                );
                if (first as i64) < 0 {
                    return Err("mq_open within the limit failed");
                }
                if mq_open(
                    (base + NAME_B) as *const u8,
                    O_CREAT | O_RDWR,
                    0o600,
                    (base + ATTR) as *mut u8,
                ) != Errno::EMFILE as u64
                {
                    return Err("second queue did not return EMFILE");
                }
                // Opening an existing name does not charge again.
                let alias = mq_open(
                    (base + NAME_A) as *const u8,
                    O_RDWR,
                    0,
                    core::ptr::null_mut(),
                );
                if (alias as i64) < 0 {
                    return Err("opening an existing queue was charged");
                }
                if sys_close(alias) != 0 {
                    return Err("close failed");
                }
                // Unlink plus close destroys the queue and releases the charge.
                if mq_unlink((base + NAME_A) as *const u8) != 0 {
                    return Err("mq_unlink failed");
                }
                if sys_close(first) != 0 {
                    return Err("close failed");
                }
                let second = mq_open(
                    (base + NAME_B) as *const u8,
                    O_CREAT | O_RDWR,
                    0o600,
                    (base + ATTR) as *mut u8,
                );
                if (second as i64) < 0 {
                    return Err("charge was not released with the queue");
                }
                if mq_unlink((base + NAME_B) as *const u8) != 0 {
                    return Err("mq_unlink failed");
                }
                if sys_close(second) != 0 {
                    return Err("close failed");
                }
                Ok(())
            })();
            me.limits.lock().rlim_cur[RLIMIT_MSGQUEUE] = saved;
            result
        } else {
            me.limits.lock().rlim_cur[RLIMIT_MSGQUEUE] = saved;
            result
        }
    })?
}

/// Test that queue names belong to the IPC namespace: a new namespace starts
/// empty while descriptors opened before switching keep working.
pub fn test_mq_ipc_namespace_isolation() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;
    use crate::syscalls::namespaces::CLONE_NEWIPC;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 32;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_ipcns\0")?;
        user_write(base + MSG, b"y")?;

        let me = CURRENT_PROCESS.lock().clone().ok_or("no current process")?;
        let saved = me.security.lock().namespaces.ipc_ns.clone();

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        // Same construction unshare(CLONE_NEWIPC) uses.
        let fresh = {
            let sec = me.security.lock();
            sec.namespaces
                .clone_with_flags(CLONE_NEWIPC, 0)
                .ipc_ns
                .clone()
        };
        me.security.lock().namespaces.ipc_ns = fresh;

        let result = (|| -> Result<(), &'static str> {
            if mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut())
                != Errno::ENOENT as u64
            {
                return Err("queue visible across IPC namespaces");
            }
            if mq_send(fd as i32, (base + MSG) as *const u8, 1, 1) != 0 {
                return Err("open descriptor stopped working after unshare");
            }
            // The name is free for a new queue in the fresh namespace.
            let other = mq_open(
                (base + NAME) as *const u8,
                O_CREAT | O_RDWR,
                0o600,
                core::ptr::null_mut(),
            );
            if (other as i64) < 0 {
                return Err("mq_open in the new namespace failed");
            }
            if mq_unlink((base + NAME) as *const u8) != 0 {
                return Err("mq_unlink in the new namespace failed");
            }
            if sys_close(other) != 0 {
                return Err("close failed");
            }
            Ok(())
        })();

        me.security.lock().namespaces.ipc_ns = saved;
        result?;

        // The original namespace still holds the queue.
        let reopened = mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut());
        if (reopened as i64) < 0 {
            return Err("original namespace lost the queue");
        }
        if sys_close(reopened) != 0 || sys_close(fd) != 0 {
            return Err("close failed");
        }
        if mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

pub fn register() {
    selftest::register("contract:mq_notify", test_mq_notify);
    selftest::register("contract:mq_timeouts", test_mq_timeouts);
    selftest::register("contract:mq_read_status_line", test_mq_read_status_line);
    selftest::register("contract:mq_rlimit_msgqueue", test_mq_rlimit_msgqueue);
    selftest::register(
        "contract:mq_ipc_namespace_isolation",
        test_mq_ipc_namespace_isolation,
    );
}
