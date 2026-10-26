//! POSIX message queue syscalls.
//!
//! Implements mq_open (240), mq_unlink (241), mq_timedsend (242),
//! mq_timedreceive (243) and mq_getsetattr (245). mq_notify (244) is not
//! implemented and falls through to `ENOSYS` in dispatch.
//!
//! A queue descriptor is an ordinary fd, so `close(2)`, `dup(2)`, `poll(2)`,
//! `select(2)` and epoll work on it with no special casing. The name lives in
//! the namespace map; open descriptors hold the queue directly and keep
//! working after `mq_unlink`.
//!
//! Names are the Linux syscall ABI form: no leading slash, because glibc and
//! musl strip the `/` of the POSIX name before entering the kernel. A slash
//! inside the name is rejected with EACCES, matching `lookup_one_len`.
//!
//! Syscall entry points take raw pointers but validate them internally.
//! The `not_unsafe_ptr_arg_deref` lint is suppressed because these are
//! kernel entry points that perform proper validation.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod object;
pub mod queue;

use crate::ipc::mqueue::object::MqObject;
use crate::ipc::mqueue::queue::{MqAttr, MqQueueHandle};
use crate::syscalls::errno::Errno;
use crate::syscalls::helpers::{
    check_file_permission, get_current_egid, get_current_euid, has_capability, CAP_SYS_RESOURCE,
};
use crate::syscalls::user_access;
use crate::task::process::{FileDescriptor, Process, CURRENT_PROCESS};
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;

const O_RDONLY: i32 = 0;
const O_WRONLY: i32 = 1;
const O_CREAT: i32 = 0x40;
const O_EXCL: i32 = 0x80;
const O_NONBLOCK: i32 = 0x800;
const MQ_PRIO_MAX: u64 = 32768;

/// Longest accepted queue name after the leading slash (NAME_MAX).
const NAME_MAX: usize = 255;

// Linux defaults (ipc/mqueue.c, include/linux/ipc_namespace.h).
const DFLT_MSGMAX: i64 = 10;
const DFLT_MSGSIZEMAX: i64 = 8192;
const DFLT_QUEUESMAX: usize = 256;
// include/uapi/linux/mqueue.h: caps for CAP_SYS_RESOURCE holders.
const HARD_MSGMAX: i64 = 65536;
const HARD_MSGSIZEMAX: i64 = 16 * 1024 * 1024;

/// Internal FD_CLOEXEC bit used by the fd table, as in `F_SETFD`.
const FD_CLOEXEC: u64 = 0x80000;

/// A name in the queue namespace: the queue plus the inode-like metadata
/// that every open of that name shares.
#[derive(Clone)]
struct MqEntry {
    queue: Arc<MqQueueHandle>,
    mode: u32,
    uid: u32,
    gid: u32,
}

lazy_static::lazy_static! {
    /// Global message queue namespace: name -> queue entry.
    static ref MQ_NAMESPACE: crate::sync::IrqSafeMutex<BTreeMap<String, MqEntry>> =
        crate::sync::IrqSafeMutex::new(BTreeMap::new());
}

/// Read and validate a queue name.
///
/// The kernel ABI receives the name **without** a leading slash (glibc and
/// musl strip it); Linux rejects an empty name, `.`/`..`, and any slash with
/// EACCES, as `lookup_one_len` does.
///
/// # Safety
/// `ptr` must be null or a userspace pointer; the range is validated by
/// `read_user_string`.
unsafe fn read_name(ptr: *const u8) -> Result<String, Errno> {
    let name = user_access::read_user_string(ptr, NAME_MAX + 1).map_err(|_| Errno::EFAULT)?;
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(Errno::EACCES);
    }
    if name.len() > NAME_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(name)
}

/// Copy mq_attr from userspace. Validation is per-caller: creation checks the
/// queue limits, `mq_setattr` checks `mq_flags`.
///
/// # Safety
/// `attr_ptr` must be null or a userspace pointer; the range is validated
/// by `copy_from_user`.
unsafe fn copy_mq_attr_from_user(attr_ptr: *const u8) -> Result<Option<MqAttr>, Errno> {
    if attr_ptr.is_null() {
        return Ok(None);
    }
    let mut attr = MqAttr::default_attrs();
    // SAFETY: `attr` is a live stack object and the slice covers exactly its
    // size; `copy_from_user` copies at most that many bytes into it.
    let slice = core::slice::from_raw_parts_mut(
        &mut attr as *mut MqAttr as *mut u8,
        core::mem::size_of::<MqAttr>(),
    );
    if user_access::copy_from_user(slice, attr_ptr).is_err() {
        return Err(Errno::EFAULT);
    }
    Ok(Some(attr))
}

/// Copy mq_attr to userspace.
///
/// # Safety
/// `attr_ptr` must be a writable userspace pointer; the range is validated
/// by `copy_to_user`.
unsafe fn copy_mq_attr_to_user(attr_ptr: *mut u8, attr: &MqAttr) -> Result<(), Errno> {
    if attr_ptr.is_null() {
        return Err(Errno::EFAULT);
    }
    // SAFETY: `attr` is a live reference and the slice covers exactly its
    // size; `copy_to_user` only reads from it.
    let slice = core::slice::from_raw_parts(
        attr as *const MqAttr as *const u8,
        core::mem::size_of::<MqAttr>(),
    );
    if user_access::copy_to_user(attr_ptr, slice).is_err() {
        return Err(Errno::EFAULT);
    }
    Ok(())
}

/// Install `obj` in the lowest free fd slot. Cannot fail: the caller already
/// holds the process. mq descriptors are always close-on-exec (mq_overview(7)).
fn install_fd(process: &Process, obj: Arc<MqObject>) -> i32 {
    process.with_fd_table(|fd_table, fd_flags| {
        let desc = Some(FileDescriptor::Mqueue(obj));
        let idx = match fd_table.iter().position(Option::is_none) {
            Some(i) => {
                fd_table[i] = desc;
                i
            }
            None => {
                fd_table.push(desc);
                fd_table.len() - 1
            }
        };
        if fd_flags.len() <= idx {
            fd_flags.resize(idx + 1, 0);
        }
        fd_flags[idx] |= FD_CLOEXEC;
        idx as i32
    })
}

/// Resolve a message queue descriptor, enforcing the open direction.
fn lookup(mqd: i32, need_read: bool, need_write: bool) -> Result<Arc<MqObject>, Errno> {
    if mqd < 0 {
        return Err(Errno::EBADF);
    }
    let process = CURRENT_PROCESS.lock().clone().ok_or(Errno::ESRCH)?;
    let obj = process
        .with_fd_table(|fd_table, _| match fd_table.get(mqd as usize) {
            Some(Some(FileDescriptor::Mqueue(obj))) => Some(obj.clone()),
            _ => None,
        })
        .ok_or(Errno::EBADF)?;
    if (need_read && !obj.can_read()) || (need_write && !obj.can_write()) {
        return Err(Errno::EBADF);
    }
    Ok(obj)
}

/// Attribute limits for the creating process: `CAP_SYS_RESOURCE` lifts the
/// default caps to the hard caps, as on Linux.
fn attr_limits() -> (i64, i64) {
    if has_capability(CAP_SYS_RESOURCE) {
        (HARD_MSGMAX, HARD_MSGSIZEMAX)
    } else {
        (DFLT_MSGMAX, DFLT_MSGSIZEMAX)
    }
}

// ─── mq_open ────────────────────────────────────────────────────────

/// Open or create a message queue.
/// name_ptr: userspace pointer to queue name (no leading slash; glibc strips it)
/// oflag: O_RDONLY/O_WRONLY/O_RDWR plus O_CREAT/O_EXCL/O_NONBLOCK/O_CLOEXEC
/// mode: permission bits used only when the queue is created
/// attr_ptr: userspace pointer to mq_attr (optional; only used on creation)
pub fn mq_open(name_ptr: *const u8, oflag: i32, mode: i32, attr_ptr: *mut u8) -> u64 {
    // Linux copies the attribute whenever it is supplied, before looking the
    // name up; it only influences creation. The copy is also the first thing
    // the syscall does, so a bad attr pointer wins over a bad name.
    // SAFETY: `attr_ptr` is null or a userspace pointer; the callee validates
    // the range through `copy_from_user`.
    let attr = match unsafe { copy_mq_attr_from_user(attr_ptr as *const u8) } {
        Ok(a) => a,
        Err(e) => return e as u64,
    };

    // SAFETY: `name_ptr` is a userspace pointer; `read_name` validates the
    // range before copying.
    let name = match unsafe { read_name(name_ptr) } {
        Ok(n) => n,
        Err(e) => return e as u64,
    };

    if (oflag & 3) == 3 {
        return Errno::EINVAL as u64;
    }

    let process = match CURRENT_PROCESS.lock().clone() {
        Some(p) => p,
        None => return Errno::ESRCH as u64,
    };

    // Lock order: MQ_NAMESPACE → fd table (via install_fd, after unlocking).
    let entry = {
        let mut ns = MQ_NAMESPACE.lock();
        match ns.get(&name).cloned() {
            Some(existing) => {
                if (oflag & O_CREAT) != 0 && (oflag & O_EXCL) != 0 {
                    return Errno::EEXIST as u64;
                }
                let need = match oflag & 3 {
                    O_RDONLY => 4,
                    O_WRONLY => 2,
                    _ => 6,
                };
                if !check_file_permission(existing.mode, existing.uid, existing.gid, need) {
                    return Errno::EACCES as u64;
                }
                existing
            }
            None => {
                if (oflag & O_CREAT) == 0 {
                    return Errno::ENOENT as u64;
                }
                if ns.len() >= DFLT_QUEUESMAX && !has_capability(CAP_SYS_RESOURCE) {
                    return Errno::ENOSPC as u64;
                }
                let attr = attr.unwrap_or_else(MqAttr::default_attrs);
                let (max_msgs, max_size) = attr_limits();
                if attr.mq_maxmsg <= 0
                    || attr.mq_maxmsg > max_msgs
                    || attr.mq_msgsize <= 0
                    || attr.mq_msgsize > max_size
                {
                    return Errno::EINVAL as u64;
                }
                let entry = MqEntry {
                    queue: Arc::new(MqQueueHandle::new(attr)),
                    mode: (mode as u32) & !*process.umask.lock() & 0o777,
                    uid: get_current_euid(),
                    gid: get_current_egid(),
                };
                ns.insert(name, entry.clone());
                entry
            }
        }
    };

    let obj = Arc::new(MqObject::new(
        entry.queue,
        oflag,
        entry.mode,
        entry.uid,
        entry.gid,
    ));
    install_fd(&process, obj) as u64
}

// ─── mq_send ────────────────────────────────────────────────────────

/// Send a message to a queue.
/// mqd: message queue descriptor
/// msg_ptr: userspace pointer to message data
/// msg_len: length of message
/// prio: message priority (0 to MQ_PRIO_MAX-1)
pub fn mq_send(mqd: i32, msg_ptr: *const u8, msg_len: usize, prio: u64) -> u64 {
    if prio >= MQ_PRIO_MAX {
        return Errno::EINVAL as u64;
    }

    let mq = match lookup(mqd, false, true) {
        Ok(m) => m,
        Err(e) => return e as u64,
    };

    if msg_len > mq.queue().max_msg_size() {
        return Errno::EMSGSIZE as u64;
    }

    let mut buf = alloc::vec![0u8; msg_len];
    // SAFETY: `msg_ptr` is a userspace pointer; `copy_from_user` validates
    // the range and copies at most `msg_len` bytes into `buf`.
    if unsafe { user_access::copy_from_user(&mut buf, msg_ptr) }.is_err() {
        return Errno::EFAULT as u64;
    }

    loop {
        match mq.queue().send(&buf, prio as u32) {
            Ok(()) => return 0,
            Err(Errno::EAGAIN) if mq.flags() & O_NONBLOCK == 0 => {
                if crate::syscalls::signal::has_pending_signal() {
                    return Errno::EINTR as u64;
                }
                crate::task::scheduler::sleep_until_tick(crate::interrupts::get_ticks() + 1);
            }
            Err(e) => return e as u64,
        }
    }
}

// ─── mq_receive ─────────────────────────────────────────────────────

/// Receive a message from a queue.
/// mqd: message queue descriptor
/// msg_ptr: userspace pointer to message buffer
/// msg_len: size of message buffer (at least the queue's mq_msgsize)
/// prio_ptr: userspace pointer to store priority (optional)
pub fn mq_receive(mqd: i32, msg_ptr: *mut u8, msg_len: usize, prio_ptr: *mut u32) -> u64 {
    let mq = match lookup(mqd, true, false) {
        Ok(m) => m,
        Err(e) => return e as u64,
    };

    // Linux rejects a short buffer even when the queue holds a smaller
    // message; the buffer is bounded by mq_msgsize, never by msg_len.
    let queue_size = mq.queue().max_msg_size();
    if msg_len < queue_size {
        return Errno::EMSGSIZE as u64;
    }

    let mut buf = alloc::vec![0u8; queue_size];
    let (received_len, prio) = loop {
        match mq.queue().receive(&mut buf) {
            Ok(r) => break r,
            Err(Errno::EAGAIN) if mq.flags() & O_NONBLOCK == 0 => {
                if crate::syscalls::signal::has_pending_signal() {
                    return Errno::EINTR as u64;
                }
                crate::task::scheduler::sleep_until_tick(crate::interrupts::get_ticks() + 1);
            }
            Err(e) => return e as u64,
        }
    };

    // Priority first, then payload, matching do_mq_timedreceive: a bad
    // prio_ptr returns EFAULT without writing the message buffer.
    if !prio_ptr.is_null() {
        // SAFETY: `prio_ptr` is a userspace pointer; `copy_to_user` validates
        // the range before writing four bytes.
        if unsafe { user_access::copy_to_user(prio_ptr as *mut u8, &prio.to_ne_bytes()) }.is_err() {
            return Errno::EFAULT as u64;
        }
    }

    // SAFETY: `msg_ptr` is a userspace pointer; `copy_to_user` validates the
    // range before writing `received_len` bytes.
    if unsafe { user_access::copy_to_user(msg_ptr, &buf[..received_len]) }.is_err() {
        return Errno::EFAULT as u64;
    }

    received_len as u64
}

/// mq_timedsend. Absolute CLOCK_REALTIME deadlines are not supported yet.
pub fn mq_timedsend(
    mqd: i32,
    msg_ptr: *const u8,
    msg_len: usize,
    prio: u64,
    timeout_ptr: *const u8,
) -> u64 {
    if !timeout_ptr.is_null() {
        // ponytail: needs a CLOCK_REALTIME wait primitive; refuse rather than
        // block past the caller's deadline. Upgrade when one exists.
        return Errno::ENOSYS as u64;
    }
    mq_send(mqd, msg_ptr, msg_len, prio)
}

/// mq_timedreceive. Absolute CLOCK_REALTIME deadlines are not supported yet.
pub fn mq_timedreceive(
    mqd: i32,
    msg_ptr: *mut u8,
    msg_len: usize,
    prio_ptr: *mut u32,
    timeout_ptr: *const u8,
) -> u64 {
    if !timeout_ptr.is_null() {
        return Errno::ENOSYS as u64;
    }
    mq_receive(mqd, msg_ptr, msg_len, prio_ptr)
}

// ─── mq_unlink ──────────────────────────────────────────────────────

/// Remove a message queue name. Descriptors already open keep working; a
/// later `mq_open` of the same name creates a new queue.
pub fn mq_unlink(name_ptr: *const u8) -> u64 {
    // SAFETY: `name_ptr` is a userspace pointer; `read_name` validates the
    // range before copying.
    let name = match unsafe { read_name(name_ptr) } {
        Ok(n) => n,
        Err(e) => return e as u64,
    };

    match MQ_NAMESPACE.lock().remove(&name) {
        Some(_) => 0,
        None => Errno::ENOENT as u64,
    }
}

// ─── mq_getsetattr ──────────────────────────────────────────────────

/// Get and/or set queue attributes. `mq_getattr` and `mq_setattr` in glibc
/// are both this syscall; only `mq_flags` (O_NONBLOCK) is settable and it
/// applies to this descriptor alone.
pub fn mq_getsetattr(mqd: i32, new_attr_ptr: *const u8, old_attr_ptr: *mut u8) -> u64 {
    // SAFETY: `new_attr_ptr` is null or a userspace pointer; the callee
    // validates the range through `copy_from_user`.
    let new_attr = match unsafe { copy_mq_attr_from_user(new_attr_ptr) } {
        Ok(a) => a,
        Err(e) => return e as u64,
    };
    if let Some(attr) = new_attr {
        if attr.mq_flags & !(O_NONBLOCK as i64) != 0 {
            return Errno::EINVAL as u64;
        }
    }

    let mq = match lookup(mqd, false, false) {
        Ok(m) => m,
        Err(e) => return e as u64,
    };

    // The old attribute set is snapshotted before the new flags take effect,
    // and the copy to userspace happens after they are applied, as on Linux.
    let old_attr = if old_attr_ptr.is_null() {
        None
    } else {
        Some(mq.get_attr())
    };

    if let Some(attr) = new_attr {
        mq.set_flags(attr.mq_flags as i32 & O_NONBLOCK);
    }

    if let Some(old_attr) = old_attr {
        // SAFETY: `old_attr_ptr` is a userspace pointer; the callee validates
        // the range through `copy_to_user`.
        if let Err(e) = unsafe { copy_mq_attr_to_user(old_attr_ptr, &old_attr) } {
            return e as u64;
        }
    }

    0
}
