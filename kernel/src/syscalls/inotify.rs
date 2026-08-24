//! inotify — filesystem event monitoring.
//!
//! Implements inotify_init, inotify_add_watch, and inotify_rm_watch syscalls.
//! The inotify file descriptor is readable via epoll/poll/select and returns
//! variable-length inotify_event structs.
//!
//! Events are fired by calling `inotify_emit()` from VFS operations.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;
use crate::task::process::CURRENT_PROCESS;
use super::errno;

// ─── Public inotify event mask constants (Linux-compatible) ──────

pub const IN_ACCESS: u32        = 0x0000_0001;
pub const IN_MODIFY: u32        = 0x0000_0002;
pub const IN_ATTRIB: u32        = 0x0000_0004;
pub const IN_CLOSE_WRITE: u32   = 0x0000_0008;
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
pub const IN_OPEN: u32          = 0x0000_0020;
pub const IN_MOVED_FROM: u32    = 0x0000_0040;
pub const IN_MOVED_TO: u32      = 0x0000_0080;
pub const IN_CREATE: u32        = 0x0000_0100;
pub const IN_DELETE: u32        = 0x0000_0200;
pub const IN_DELETE_SELF: u32   = 0x0000_0400;
pub const IN_MOVE_SELF: u32     = 0x0000_0800;

pub const IN_UNMOUNT: u32       = 0x0000_2000;
pub const IN_Q_OVERFLOW: u32    = 0x0000_4000;
pub const IN_IGNORED: u32       = 0x0000_8000;
pub const IN_ONLYDIR: u32       = 0x0100_0000;
pub const IN_DONT_FOLLOW: u32   = 0x0200_0000;
pub const IN_EXCL_UNLINK: u32   = 0x0400_0000;
pub const IN_MASK_CREATE: u32   = 0x1000_0000;
pub const IN_MASK_ADD: u32      = 0x2000_0000;
pub const IN_ISDIR: u32         = 0x4000_0000;
pub const IN_ONESHOT: u32       = 0x8000_0000;

/// All event types that can be monitored
pub const IN_ALL_EVENTS: u32 = IN_ACCESS | IN_MODIFY | IN_ATTRIB | IN_CLOSE_WRITE
    | IN_CLOSE_NOWRITE | IN_OPEN | IN_MOVED_FROM | IN_MOVED_TO
    | IN_CREATE | IN_DELETE | IN_DELETE_SELF | IN_MOVE_SELF;

/// The `inotify_event` struct layout (variable-length due to optional name).
/// Written as raw bytes to userspace via read().
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InotifyEvent {
    /// Watch descriptor
    pub wd: i32,
    /// Event mask
    pub mask: u32,
    /// Cookie for rename pairing (0 if not a rename)
    pub cookie: u32,
    /// Length of the optional name field (0 if no name)
    pub name_len: u32,
}

/// A single pending inotify event stored in the ring buffer.
#[derive(Debug, Clone)]
pub struct PendingEvent {
    pub wd: i32,
    pub mask: u32,
    pub cookie: u32,
    pub name: String,
}

/// An inotify watch.
#[derive(Debug, Clone)]
pub struct InotifyWatch {
    pub wd: i32,
    /// The path being watched (resolved absolute path).
    pub path: String,
    /// Event mask for this watch.
    pub mask: u32,
    /// Inode number of the watched node, for matching events.
    pub ino: u64,
}

/// An inotify instance (one per file descriptor).
pub struct InotifyInstance {
    /// Watch descriptor → watch
    pub watches: BTreeMap<i32, InotifyWatch>,
    /// Path → watch descriptor (for fast lookup by path)
    pub path_wd: BTreeMap<String, i32>,
    /// Pending events queue (read by userspace via read()).
    pub events: Vec<PendingEvent>,
    /// Next watch descriptor to allocate.
    pub next_wd: i32,
    /// Maximum queued events (overflow triggers IN_Q_OVERFLOW).
    pub max_events: usize,
    /// Whether non-blocking mode is set.
    pub nonblock: bool,
}

impl InotifyInstance {
    pub fn new(nonblock: bool) -> Self {
        Self {
            watches: BTreeMap::new(),
            path_wd: BTreeMap::new(),
            events: Vec::new(),
            next_wd: 1,
            max_events: 16384,
            nonblock,
        }
    }

    pub fn has_pending(&self) -> bool {
        !self.events.is_empty()
    }
}

// ─── Global inotify state ──────────────────────────────────────

lazy_static::lazy_static! {
    /// Map from inotify fd number to the instance.
    /// Keyed by the raw fd value the process sees.
    pub static ref INOTIFY_INSTANCES: Mutex<BTreeMap<u64, Arc<Mutex<InotifyInstance>>>> =
        Mutex::new(BTreeMap::new());
}

/// Next global inotify handle (for generating unique fd keys).
static NEXT_INOTIFY_KEY: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0x2000);

/// Fire an inotify event to all watching instances.
///
/// Called from VFS operations (create, unlink, rename, write, chmod, etc.).
/// `path` is the absolute path of the affected file.
/// `mask` is the event mask (IN_CREATE, IN_DELETE, etc.).
/// `cookie` is a non-zero value for rename pairs (must match moved_from/moved_to).
/// `name` is the filename (last component), empty for watches on the file itself.
pub fn inotify_emit(path: &str, mask: u32, cookie: u32, name: &str) {
    let instances = INOTIFY_INSTANCES.lock();
    for (_key, inst_arc) in instances.iter() {
        let mut inst = inst_arc.lock();
        if inst.events.len() >= inst.max_events {
            // Queue overflow — emit IN_Q_OVERFLOW once, then stop.
            if !inst.events.iter().any(|e| e.mask & IN_Q_OVERFLOW != 0) {
                inst.events.push(PendingEvent {
                    wd: -1,
                    mask: IN_Q_OVERFLOW,
                    cookie: 0,
                    name: String::new(),
                });
            }
            continue;
        }

        // Find matching watches.
        // A watch matches if the event path starts with the watch path,
        // or if the event path IS the watch path.
        let mut matched_wds: Vec<(i32, u32)> = Vec::new();
        for (wd, watch) in &inst.watches {
            let watch_matches = if path == watch.path {
                // Exact match — this event is for the watched file itself
                true
            } else if watch.path.ends_with('/') {
                // Watch is a directory — check if the event is inside it
                path.starts_with(&watch.path)
            } else {
                // Watch is a file — only exact match (already handled above)
                false
            };

            if watch_matches {
                // Only emit if the event is in the watch's mask
                if (mask & watch.mask) != 0 {
                    matched_wds.push((*wd, watch.mask));
                }
            }
        }

        for (wd, watch_mask) in matched_wds {
            let effective_mask = mask & watch_mask;
            // Determine the name to include
            let event_name = if mask & (IN_CREATE | IN_DELETE | IN_MOVED_FROM | IN_MOVED_TO) != 0 {
                if !name.is_empty() {
                    alloc::string::String::from(name)
                } else {
                    String::new()
                }
            } else {
                String::new()
            };
            let name_len = event_name.len() as u32;

            inst.events.push(PendingEvent {
                wd,
                mask: effective_mask,
                cookie,
                name: event_name,
            });

            // If IN_ONESHOT, remove the watch after the first event
            if watch_mask & IN_ONESHOT != 0 {
                inst.watches.remove(&wd);
            }
        }
    }
}

// ─── Syscall implementations ───────────────────────────────────

/// inotify_init(flags) → fd
///
/// Creates a new inotify instance. flags may include O_NONBLOCK, O_CLOEXEC.
pub fn sys_inotify_init(flags: u64) -> u64 {
    let nonblock = (flags & 0x800 /* O_NONBLOCK */) != 0;
    let _cloexec = (flags & 0x80000 /* O_CLOEXEC */) != 0;

    let key = NEXT_INOTIFY_KEY.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let instance = Arc::new(Mutex::new(InotifyInstance::new(nonblock)));

    INOTIFY_INSTANCES.lock().insert(key, instance.clone());

    let lock = CURRENT_PROCESS.lock();
    if let Some(ref proc) = *lock {
        let mut files = proc.files.lock();
        let fd_num = find_free_fd(&files.fd_table);
        if fd_num >= files.fd_table.len() {
            files.fd_table.resize(fd_num + 1, None);
        }
        files.fd_table[fd_num] = Some(crate::task::process::FileDescriptor::InotifyFd {
            instance_key: key,
            _instance: instance,
        });
        fd_num as u64
    } else {
        INOTIFY_INSTANCES.lock().remove(&key);
        errno::Errno::ESRCH as u64
    }
}

/// inotify_add_watch(fd, pathname, mask) → watch descriptor (≥ 1)
///
/// Adds a watch on the file at `pathname` with the given `mask`.
pub fn sys_inotify_add_watch(fd: u64, pathname: *const u8, mask: u32) -> u64 {
    if pathname.is_null() {
        return errno::Errno::EINVAL as u64;
    }

    // Read the path from userspace
    let path = match unsafe { super::user_access::read_user_string(pathname, 4096) } {
        Ok(p) => p,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    if path.is_empty() {
        return errno::Errno::EINVAL as u64;
    }

    // Resolve the path to a canonical absolute path
    let resolved = resolve_path(&path);

    // Look up the inotify instance for this fd
    let instance_key = {
        let lock = CURRENT_PROCESS.lock();
        match lock.as_ref() {
            Some(proc) => {
                let files = proc.files.lock();
                if fd as usize >= files.fd_table.len() {
                    return errno::Errno::EBADF as u64;
                }
                match &files.fd_table[fd as usize] {
                    Some(crate::task::process::FileDescriptor::InotifyFd { instance_key, .. }) => *instance_key,
                    _ => return errno::Errno::EINVAL as u64,
                }
            }
            None => return errno::Errno::ESRCH as u64,
        }
    };

    // Get a stat to read the inode number
    let ino = {
        let vfs = crate::vfs::VFS.lock();
        match vfs.resolve_path(&resolved) {
            Some(node) => node.stat().map(|s| s.st_ino).unwrap_or(0),
            None => return errno::Errno::ENOENT as u64,
        }
    };

    let instances = INOTIFY_INSTANCES.lock();
    let inst_arc = match instances.get(&instance_key) {
        Some(inst) => inst.clone(),
        None => {
            return errno::Errno::EBADF as u64;
        }
    };
    drop(instances);

    let mut inst = inst_arc.lock();

    // If the path already has a watch, update the mask (IN_MASK_ADD semantics)
    if let Some(&existing_wd) = inst.path_wd.get(&resolved) {
        if let Some(watch) = inst.watches.get_mut(&existing_wd) {
            if (mask & IN_MASK_ADD) != 0 {
                watch.mask |= mask & IN_ALL_EVENTS;
            } else {
                watch.mask = mask & IN_ALL_EVENTS;
            }
            return existing_wd as u64;
        }
    }

    // Create a new watch
    let wd = inst.next_wd;
    inst.next_wd += 1;

    let watch = InotifyWatch {
        wd,
        path: resolved.clone(),
        mask: mask & IN_ALL_EVENTS,
        ino,
    };

    inst.watches.insert(wd, watch);
    inst.path_wd.insert(resolved, wd);

    wd as u64
}

/// inotify_rm_watch(fd, wd) → 0
///
/// Removes the watch with descriptor `wd` from the inotify instance.
pub fn sys_inotify_rm_watch(fd: u64, wd: u32) -> u64 {
    let instance_key = {
        let lock = CURRENT_PROCESS.lock();
        match lock.as_ref() {
            Some(proc) => {
                let files = proc.files.lock();
                if fd as usize >= files.fd_table.len() {
                    return errno::Errno::EBADF as u64;
                }
                match &files.fd_table[fd as usize] {
                    Some(crate::task::process::FileDescriptor::InotifyFd { instance_key, .. }) => *instance_key,
                    _ => return errno::Errno::EINVAL as u64,
                }
            }
            None => return errno::Errno::ESRCH as u64,
        }
    };

    let instances = INOTIFY_INSTANCES.lock();
    let inst_arc = match instances.get(&instance_key) {
        Some(inst) => inst.clone(),
        None => return errno::Errno::EBADF as u64,
    };
    drop(instances);

    let mut inst = inst_arc.lock();
    let wd = wd as i32;

    if let Some(watch) = inst.watches.remove(&wd) {
        inst.path_wd.remove(&watch.path);
        // Emit IN_IGNORED for the removed watch
        inst.events.push(PendingEvent {
            wd,
            mask: IN_IGNORED,
            cookie: 0,
            name: String::new(),
        });
        0
    } else {
        errno::Errno::EINVAL as u64
    }
}

/// Read inotify events from the fd. Called from sys_read for inotify fds.
/// Returns the number of bytes written to `buf`.
pub fn inotify_read(key: u64, buf: &mut [u8]) -> Result<usize, errno::Errno> {
    let instances = INOTIFY_INSTANCES.lock();
    let inst_arc = match instances.get(&key) {
        Some(inst) => inst.clone(),
        None => return Err(errno::Errno::EBADF),
    };
    drop(instances);

    let mut inst = inst_arc.lock();

    if inst.events.is_empty() {
        if inst.nonblock {
            return Err(errno::Errno::EAGAIN);
        }
        // Blocking — but we can't block in this context easily.
        // Return EAGAIN for now; a real implementation would sleep.
        return Err(errno::Errno::EAGAIN);
    }

    let mut offset = 0usize;

    // Serialize events into the buffer
    while let Some(event) = inst.events.first() {
        // Each event: 16-byte header + padded name
        let name_bytes = event.name.as_bytes();
        let name_padded_len = (name_bytes.len() + 1 + 3) & !3; // null-terminated, 4-byte aligned
        let event_size = 16 + name_padded_len;

        if offset + event_size > buf.len() {
            break; // Not enough space
        }

        let header = InotifyEvent {
            wd: event.wd,
            mask: event.mask,
            cookie: event.cookie,
            name_len: if !event.name.is_empty() {
                (name_bytes.len() + 1) as u32 // include null terminator
            } else {
                0
            },
        };

        // Write the 16-byte header
        let header_bytes = unsafe {
            core::slice::from_raw_parts(&header as *const _ as *const u8, 16)
        };
        buf[offset..offset + 16].copy_from_slice(header_bytes);
        offset += 16;

        // Write the name (if present)
        if !event.name.is_empty() {
            buf[offset..offset + name_bytes.len()].copy_from_slice(name_bytes);
            buf[offset + name_bytes.len()] = 0; // null terminator
            offset += name_padded_len;
        }

        inst.events.remove(0);
    }

    Ok(offset)
}

/// Returns whether an inotify fd has pending events (for poll/epoll).
pub fn inotify_has_events(key: u64) -> bool {
    let instances = INOTIFY_INSTANCES.lock();
    if let Some(inst_arc) = instances.get(&key) {
        let inst = inst_arc.lock();
        inst.has_pending()
    } else {
        false
    }
}

/// Cleanup inotify instance when the fd is closed.
pub fn inotify_close(key: u64) {
    let mut instances = INOTIFY_INSTANCES.lock();
    if let Some(inst_arc) = instances.remove(&key) {
        let inst = inst_arc.lock();
        // Emit IN_IGNORED for all watches
        for (&_wd, watch) in &inst.watches {
            // Can't emit here since we're removing the instance,
            // but the watches are being cleaned up
            let _ = watch;
        }
    }
}

// ─── Helpers ───────────────────────────────────────────────────

fn find_free_fd(fd_table: &[Option<crate::task::process::FileDescriptor>]) -> usize {
    for (i, slot) in fd_table.iter().enumerate() {
        if slot.is_none() {
            return i;
        }
    }
    fd_table.len()
}

/// Resolve a path to its canonical absolute form.
/// Handles ".", "..", and ensures it starts with "/".
fn resolve_path(path: &str) -> String {
    let path = if path.starts_with('/') {
        alloc::string::String::from(path)
    } else {
        // Prepend cwd
        let lock = CURRENT_PROCESS.lock();
        let cwd = lock.as_ref()
            .map(|p| p.files.lock().cwd.clone())
            .unwrap_or_else(|| String::from("/"));
        if cwd == "/" {
            alloc::format!("/{}", path)
        } else {
            alloc::format!("{}/{}", cwd, path)
        }
    };

    // Normalize: resolve "." and ".."
    let mut components: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => { components.pop(); }
            other => components.push(other),
        }
    }

    let mut result = String::from("/");
    for (i, comp) in components.iter().enumerate() {
        if i > 0 { result.push('/'); }
        result.push_str(comp);
    }

    if result.is_empty() {
        String::from("/")
    } else {
        result
    }
}
