use crate::sync::IrqSafeMutex as Mutex;
use crate::syscalls::errno;
use crate::syscalls::user_access;
use crate::task::process::CURRENT_PROCESS;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use hashbrown::HashMap;
use core::sync::atomic::{AtomicU32, Ordering};

const O_RDONLY: i32 = 0; const O_WRONLY: i32 = 1; const O_RDWR: i32 = 2;
const O_CREAT: i32 = 0x40; const O_EXCL: i32 = 0x80; const O_NONBLOCK: i32 = 0x800;
const MQ_PRIO_MAX: u32 = 32768;

#[derive(Clone)]
struct MqMsg { data: Vec<u8>, prio: u32 }

struct MqQueue {
    msgs: Vec<MqMsg>,
    max_msgs: usize,
    max_msg_size: usize,
    deleted: bool,
}

struct MqFd { qname: String, flags: i32 }

lazy_static::lazy_static! {
    static ref QUEUES: Mutex<BTreeMap<String, MqQueue>> = Mutex::new(BTreeMap::new());
    static ref FDS: Mutex<HashMap<(u64, i32), MqFd>> = Mutex::new(HashMap::new());
}
static NEXT_ID: AtomicU32 = AtomicU32::new(100);

fn cur_pid() -> u64 { CURRENT_PROCESS.lock().as_ref().map_or(0, |p| p.id) }

/// Consistent lock order everywhere: QUEUES → FDS.
/// mq_open: QUEUES → FDS
/// mq_close: QUEUES → FDS
/// mq_unlink: QUEUES → FDS
/// mq_send/mq_receive: lookup FDS (released), then QUEUES (no nesting)

// ─── mq_open ────────────────────────────────────────────────────────

pub fn mq_open(name_ptr: *const u8, oflag: i32, _mode: i32, _attr: *mut u8) -> u64 {
    let name = match unsafe { user_access::read_user_string(name_ptr, 256) } {
        Ok(s) if s.starts_with('/') => s,
        _ => return -(errno::Errno::EINVAL as i64) as u64,
    };
    let pid = cur_pid();
    let fd = NEXT_ID.fetch_add(1, Ordering::Relaxed) as i32;
    // Lock order: QUEUES → FDS
    let mut q = QUEUES.lock();
    let mut fds = FDS.lock();
    if q.contains_key(&name) {
        if (oflag & O_CREAT) != 0 && (oflag & O_EXCL) != 0 {
            return -(errno::Errno::EEXIST as i64) as u64;
        }
        // Don't open a deleted queue unless it has live descriptors
        if q.get(&name).map_or(false, |q| q.deleted) && !fds.values().any(|f| f.qname == name) {
            return -(errno::Errno::ENOENT as i64) as u64;
        }
    } else if (oflag & O_CREAT) == 0 {
        return -(errno::Errno::ENOENT as i64) as u64;
    } else {
        q.insert(name.clone(), MqQueue { msgs: Vec::new(), max_msgs: 10, max_msg_size: 8192, deleted: false });
    }
    fds.insert((pid, fd), MqFd { qname: name, flags: oflag & 3 });
    fd as u64
}

// ─── mq_send ────────────────────────────────────────────────────────

pub fn mq_send(mqd: i32, msg_ptr: *const u8, msg_len: usize, prio: u32) -> u64 {
    if prio >= MQ_PRIO_MAX { return -(errno::Errno::EINVAL as i64) as u64; }
    let pid = cur_pid();
    // Lookup FD (locks then releases FDS — no nesting with QUEUES)
    let (qn, qf) = {
        let fds = FDS.lock();
        match fds.get(&(pid, mqd)) {
            Some(d) if d.flags != O_RDONLY => (d.qname.clone(), d.flags),
            Some(_) => return -(errno::Errno::EBADF as i64) as u64,
            None => return -(errno::Errno::EBADF as i64) as u64,
        }
    };

    let mut buf = vec![0u8; msg_len];
    if unsafe { user_access::copy_from_user(&mut buf, msg_ptr) }.is_err() {
        return -(errno::Errno::EFAULT as i64) as u64;
    }

    let mut q = QUEUES.lock();
    let queue = match q.get_mut(&qn) { Some(q) => q, None => return -(errno::Errno::ENOENT as i64) as u64 };
    if queue.deleted { return -(errno::Errno::EIDRM as i64) as u64; }
    if msg_len > queue.max_msg_size { return -(errno::Errno::EMSGSIZE as i64) as u64; }
    if queue.msgs.len() >= queue.max_msgs {
        if (qf & O_NONBLOCK) != 0 { return -(errno::Errno::EAGAIN as i64) as u64; }
        drop(q);
        crate::task::scheduler::yield_now();
        let mut q = QUEUES.lock();
        let queue = match q.get_mut(&qn) { Some(q) => q, None => return -(errno::Errno::ENOENT as i64) as u64 };
        if queue.msgs.len() >= queue.max_msgs { return -(errno::Errno::EAGAIN as i64) as u64; }
        queue.msgs.push(MqMsg { data: buf, prio });
        queue.msgs.sort_by(|a, b| b.prio.cmp(&a.prio));
    } else {
        queue.msgs.push(MqMsg { data: buf, prio });
        queue.msgs.sort_by(|a, b| b.prio.cmp(&a.prio));
    }
    0
}

// ─── mq_receive ─────────────────────────────────────────────────────

pub fn mq_receive(mqd: i32, msg_ptr: *mut u8, msg_len: usize, prio_ptr: *mut u32) -> u64 {
    let pid = cur_pid();
    let (qn, qf) = {
        let fds = FDS.lock();
        match fds.get(&(pid, mqd)) {
            Some(d) if d.flags != O_WRONLY => (d.qname.clone(), d.flags),
            Some(_) => return -(errno::Errno::EBADF as i64) as u64,
            None => return -(errno::Errno::EBADF as i64) as u64,
        }
    };

    let msg = {
        let mut q = QUEUES.lock();
        let queue = match q.get_mut(&qn) { Some(q) => q, None => return -(errno::Errno::ENOENT as i64) as u64 };
        if queue.deleted { return -(errno::Errno::EIDRM as i64) as u64; }
        if queue.msgs.is_empty() {
            if (qf & O_NONBLOCK) != 0 { return -(errno::Errno::EAGAIN as i64) as u64; }
            drop(q);
            crate::task::scheduler::yield_now();
            let mut q = QUEUES.lock();
            let queue = match q.get_mut(&qn) { Some(q) => q, None => return -(errno::Errno::ENOENT as i64) as u64 };
            if queue.msgs.is_empty() { return -(errno::Errno::EAGAIN as i64) as u64; }
            queue.msgs.remove(0)
        } else { queue.msgs.remove(0) }
    };
    if msg_len < msg.data.len() { return -(errno::Errno::EMSGSIZE as i64) as u64; }
    if unsafe { user_access::copy_to_user(msg_ptr, &msg.data) }.is_err() {
        return -(errno::Errno::EFAULT as i64) as u64;
    }
    if !prio_ptr.is_null() { unsafe { *prio_ptr = msg.prio; } }
    msg.data.len() as u64
}

// ─── mq_close ───────────────────────────────────────────────────────

pub fn mq_close(mqd: i32) -> u64 {
    let pid = cur_pid();
    // Lock order: QUEUES → FDS (matches mq_unlink and mq_open)
    let mut q = QUEUES.lock();
    let mut fds = FDS.lock();
    let qname = match fds.remove(&(pid, mqd)) {
        Some(d) => d.qname,
        None => return -(errno::Errno::EBADF as i64) as u64,
    };
    if !fds.values().any(|f| f.qname == qname) {
        if q.get(&qname).map_or(false, |q| q.deleted) { q.remove(&qname); }
    }
    0
}

// ─── mq_unlink ──────────────────────────────────────────────────────

pub fn mq_unlink(name_ptr: *const u8) -> u64 {
    let name = match unsafe { user_access::read_user_string(name_ptr, 256) } {
        Ok(s) => s, Err(_) => return -(errno::Errno::EINVAL as i64) as u64,
    };
    // Lock order: QUEUES → FDS (matches mq_close and mq_open)
    let mut q = QUEUES.lock();
    let fds = FDS.lock();
    match q.get_mut(&name) {
        Some(queue) => {
            queue.deleted = true;
            if !fds.values().any(|f| f.qname == name) { q.remove(&name); }
            0
        }
        None => -(errno::Errno::ENOENT as i64) as u64,
    }
}

// ─── mq_close_all (called on process exit) ──────────────────────────

pub fn mq_close_all(pid: u64) {
    let mut q = QUEUES.lock();
    let mut fds = FDS.lock();
    // Collect queue names owned by this process
    let names: Vec<String> = fds.iter()
        .filter(|(&(p, _), _)| p == pid)
        .map(|(_, d)| d.qname.clone())
        .collect();
    // Remove all fds for this process
    fds.retain(|(&(p, _), _)| p != pid);
    // For each affected queue, check if it should be destroyed
    for name in names {
        if !fds.values().any(|f| f.qname == name) {
            if q.get(&name).map_or(false, |q| q.deleted) { q.remove(&name); }
        }
    }
}
