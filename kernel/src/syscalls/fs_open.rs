//! File open/close/dup/fcntl/pipe/access syscalls.
//! Extracted from fs.rs to keep each module under 1k lines.

use super::errno;
use super::numbers;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::string::String;

// ─── Shared helpers (used by stat.rs too) ────────────────────────

pub fn check_chown_permission(current_uid: u32, current_gid: u32, new_uid: u32, new_gid: u32) -> bool {
    if has_capability(CAP_CHOWN) { return true; }
    if new_uid != current_uid { return false; }
    if new_gid != current_gid && new_gid != get_current_egid() { return false; }
    true
}

pub fn split_parent(path: &str) -> (String, String) {
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

// ─── open / openat / open_path ────────────────────────────────────

pub fn sys_open_path(path: &str) -> Result<u64, errno::Errno> {
    let path_c = alloc::format!("{}\0", path);
    let fd = syscall_handler(numbers::SYS_OPEN, path_c.as_ptr() as u64, 0x0, 0, 0, 0, core::ptr::null_mut());
    if (fd as i64) < 0 {
        let errval = fd as i64;
        let e = match errval {
            -1  => errno::Errno::EPERM, -2  => errno::Errno::ENOENT, -3  => errno::Errno::ESRCH,
            -4  => errno::Errno::EINTR, -5  => errno::Errno::EIO,   -6  => errno::Errno::ENXIO,
            -7  => errno::Errno::E2BIG, -8  => errno::Errno::ENOEXEC, -9  => errno::Errno::EBADF,
            -10 => errno::Errno::ECHILD, -11 => errno::Errno::EAGAIN, -12 => errno::Errno::ENOMEM,
            -13 => errno::Errno::EACCES, -14 => errno::Errno::EFAULT, -15 => errno::Errno::ENOTBLK,
            -16 => errno::Errno::EBUSY, -17 => errno::Errno::EEXIST, -18 => errno::Errno::EXDEV,
            -19 => errno::Errno::ENODEV, -20 => errno::Errno::ENOTDIR, -21 => errno::Errno::EISDIR,
            -22 => errno::Errno::EINVAL, -23 => errno::Errno::ENFILE, -25 => errno::Errno::ENOTTY,
            -26 => errno::Errno::ETXTBSY, -27 => errno::Errno::EFBIG, -28 => errno::Errno::ENOSPC,
            -29 => errno::Errno::ESPIPE, -30 => errno::Errno::EROFS, -31 => errno::Errno::EMLINK,
            -32 => errno::Errno::EPIPE, -33 => errno::Errno::EDOM, -34 => errno::Errno::ERANGE,
            -38 => errno::Errno::ENOSYS, -40 => errno::Errno::ELOOP,
            _ => errno::Errno::EINVAL,
        };
        Err(e)
    } else {
        Ok(fd)
    }
}

pub fn open_file(path: &str, flags: i32, mode: u32) -> u64 {
    const O_CREAT: i32 = 0x40;
    const O_DIRECTORY: i32 = 0x10000;
    const O_CLOEXEC: i32 = 0x80000;
    let vfs = VFS.lock();
    if let Some(node) = vfs.resolve_path(path) {
        let subj = crate::security::current_subject();
        let perm = match flags & 3 { 1 => "write", 2 => "read+write", _ => "read" };
        if !crate::security::hook_file_perm(&subj, path, perm) {
            return errno::Errno::EACCES as u64;
        }
        let landlock_access = match flags & 3 {
            1 => crate::syscalls::landlock::LANDLOCK_ACCESS_FS_WRITE_FILE,
            2 => crate::syscalls::landlock::LANDLOCK_ACCESS_FS_READ_FILE | crate::syscalls::landlock::LANDLOCK_ACCESS_FS_WRITE_FILE,
            _ => crate::syscalls::landlock::LANDLOCK_ACCESS_FS_READ_FILE,
        };
        if !crate::syscalls::landlock::check_fs_access(path, landlock_access) {
            return errno::Errno::EACCES as u64;
        }
        if (flags & O_DIRECTORY) != 0 && !node.is_dir() {
            return errno::Errno::ENOTDIR as u64;
        }
        if let Some(p) = get_current_process() {
            let fd = add_fd(&p, node.clone() as Arc<dyn VfsNode>, flags);
            if (fd as i64) >= 0 {
                if node.is_dir() { store_dir_path(&p, fd, path); }
                if (flags & O_CLOEXEC) != 0 {
                    let mut fd_flags = p.files.lock().fd_flags.clone();
                    if (fd as usize) >= fd_flags.len() { fd_flags.resize(fd as usize + 1, 0); }
                    fd_flags[fd as usize] |= 0x80000;
                }
            }
            return fd;
        }
    } else if (flags & O_CREAT) != 0 {
        let last_slash = path.rfind('/').unwrap_or(0);
        let (parent_path, name) = if last_slash == 0 { ("/", &path[1..]) } else { (&path[..last_slash], &path[last_slash+1..]) };
        drop(vfs);
        let vfs2 = VFS.lock();
        if let Some(parent) = vfs2.resolve_path(parent_path) {
            let subj = crate::security::current_subject();
            if !crate::security::hook_file_create(&subj, path) {
                return errno::Errno::EACCES as u64;
            }
            if !check_node_permission(&parent, 3) {
                return errno::Errno::EACCES as u64;
            }
            let new_node = match parent.create(name) {
                Ok(n) => n,
                Err(()) => return errno::Errno::EIO as u64,
            };
            let (euid, egid, umask_val) = {
                let lock = CURRENT_PROCESS.lock();
                lock.as_ref().map(|p| {
                    let c = p.creds.lock();
                    (c.euid, c.egid, *p.umask.lock())
                }).unwrap_or((0, 0, 0o022))
            };
            let raw_mode = if mode == 0 { 0o666 } else { mode };
            let _ = new_node.chmod(raw_mode & !umask_val);
            let _ = new_node.chown(euid, egid);
            if let Some(p) = get_current_process() {
                let fd = add_fd(&p, new_node as Arc<dyn VfsNode>, flags);
                if (fd as i64) >= 0 && (flags & O_CLOEXEC) != 0 {
                    let mut fd_flags = p.files.lock().fd_flags.clone();
                    if (fd as usize) >= fd_flags.len() { fd_flags.resize(fd as usize + 1, 0); }
                    fd_flags[fd as usize] |= 0x80000;
                }
                return fd;
            }
        }
    }
    errno::Errno::ENOENT as u64
}

pub fn sys_open(path_ptr: *const u8, flags: i32, mode: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64,
    };
    open_file(&path_str, flags, mode)
}

pub fn sys_openat(dirfd: i64, path_ptr: *const u8, flags: i32, mode: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64,
    };
    let process = match get_current_process() {
        Some(p) => p, None => return errno::Errno::ESRCH as u64,
    };
    let abs_path = match resolve_path_at(dirfd, &path_str, &process) {
        Ok(p) => p, Err(e) => return e as u64,
    };
    open_file(&abs_path, flags, mode)
}

// ─── close ────────────────────────────────────────────────────────

pub fn sys_close(fd: u64) -> u64 {
    let process = {
        let process_lock = CURRENT_PROCESS.lock();
        match *process_lock { Some(ref p) => p.clone(), None => return errno::Errno::ESRCH as u64, }
    };
    let mut found = false;
    let mut fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) < fd_table.len() {
        if let Some(ref desc) = fd_table[fd as usize] {
            if let FileDescriptor::Socket(handle, _stype) = desc {
                #[cfg(feature = "net")]
                { crate::net::SOCKETS.lock().remove(*handle); }
            }
            if let FileDescriptor::UnixSocket(handle, _) = desc {
                crate::net::unix::cleanup_unix_socket(*handle);
            }
            if let FileDescriptor::SignalFd(handle) = desc {
                super::SIGNAL_FDS.lock().remove(handle);
            }
            found = true;
        }
        fd_table[fd as usize] = None;
    }
    drop(fd_table);
    let mut flags = process.files.lock().fd_flags.clone();
    if (fd as usize) < flags.len() { flags[fd as usize] = 0; }
    drop(flags);
    if found { 0 } else { errno::Errno::EBADF as u64 }
}

// ─── dup / dup2 ───────────────────────────────────────────────────

pub fn sys_dup(old_fd: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.files.lock().fd_table.clone();
        if old_fd as usize >= fd_table.len() || fd_table[old_fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
        let old_desc = fd_table[old_fd as usize].clone().unwrap();
        let mut flags = p.files.lock().fd_flags.clone();
        for (i, slot) in fd_table.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(old_desc);
                if i >= flags.len() { flags.resize(i + 1, 0); }
                flags[i] = 0;
                return i as u64;
            }
        }
        fd_table.push(Some(old_desc));
        flags.push(0);
        return (fd_table.len() - 1) as u64;
    }
    errno::Errno::ESRCH as u64
}

pub fn sys_dup2(old_fd: u64, new_fd: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.files.lock().fd_table.clone();
        if old_fd as usize >= fd_table.len() || fd_table[old_fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
        let old_desc = fd_table[old_fd as usize].clone();
        let old_flags = { let flags = p.files.lock().fd_flags.clone(); if (old_fd as usize) < flags.len() { flags[old_fd as usize] } else { 0 } };
        if new_fd as usize >= fd_table.len() { fd_table.resize(new_fd as usize + 1, None); }
        let mut flags = p.files.lock().fd_flags.clone();
        if flags.len() < fd_table.len() { flags.resize(fd_table.len(), 0); }
        fd_table[new_fd as usize] = old_desc;
        flags[new_fd as usize] = old_flags & !0x80000;
        return new_fd;
    }
    errno::Errno::ESRCH as u64
}

// ─── fcntl ────────────────────────────────────────────────────────

pub fn sys_fcntl(fd: u64, cmd: i32, arg: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let mut fd_table = p.files.lock().fd_table.clone();
        if fd as usize >= fd_table.len() || fd_table[fd as usize].is_none() {
            return errno::Errno::EBADF as u64;
        }
        match cmd {
            F_DUPFD => {
                let desc = fd_table[fd as usize].clone().unwrap();
                let mut flags = p.files.lock().fd_flags.clone();
                for (i, slot) in fd_table.iter_mut().enumerate() {
                    if slot.is_none() && i as u64 > arg {
                        *slot = Some(desc);
                        if i >= flags.len() { flags.resize(i + 1, 0); }
                        flags[i] = 0;
                        return i as u64;
                    }
                }
                fd_table.push(Some(desc));
                flags.push(0);
                (fd_table.len() - 1) as u64
            }
            F_GETFD => { let flags = p.files.lock().fd_flags.clone(); if (fd as usize) < flags.len() && flags[fd as usize] & 0x80000 != 0 { 1 } else { 0 } }
            F_SETFD => {
                let mut flags = p.files.lock().fd_flags.clone();
                if (fd as usize) >= flags.len() { flags.resize(fd as usize + 1, 0); }
                if arg & 1 != 0 { flags[fd as usize] |= 0x80000; } else { flags[fd as usize] &= !0x80000; }
                0
            }
            F_GETFL => { let flags = p.files.lock().fd_flags.clone(); if (fd as usize) < flags.len() { flags[fd as usize] } else { 0 } }
            F_SETFL => {
                let mut flags = p.files.lock().fd_flags.clone();
                if fd as usize >= flags.len() { flags.resize(fd as usize + 1, 0); }
                flags[fd as usize] = arg & 0xFFFF;
                0
            }
            _ => errno::Errno::EINVAL as u64,
        }
    } else { errno::Errno::ESRCH as u64 }
}

// ─── pipe ─────────────────────────────────────────────────────────

pub fn sys_pipe(fds_ptr: *mut u32) -> u64 {
    let (reader, writer) = crate::vfs::pipe::Pipe::new();
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let r_fd = {
        let obj = crate::vfs::VfsObject::new(reader, crate::objects::TYPE_FILE) as Arc<dyn crate::objects::KernelObject>;
        match process.new_handle(obj, crate::objects::security::ACCESS_READ, 0) { Ok(fd) => fd, Err(_) => return errno::Errno::EACCES as u64 }
    };
    let w_fd = {
        let obj = crate::vfs::VfsObject::new(writer, crate::objects::TYPE_FILE) as Arc<dyn crate::objects::KernelObject>;
        match process.new_handle(obj, crate::objects::security::ACCESS_WRITE, 1) {
            Ok(fd) => fd, Err(_) => { process.close_handle(r_fd); return errno::Errno::EACCES as u64; }
        }
    };
    let fds = [r_fd as u32, w_fd as u32];
    let bytes = unsafe { core::slice::from_raw_parts(&fds as *const _ as *const u8, core::mem::size_of_val(&fds)) };
    if unsafe { user_access::copy_to_user(fds_ptr as *mut u8, bytes) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

// ─── access / faccessat ───────────────────────────────────────────

pub fn sys_access(path_ptr: *const u8, mode: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let node = match VFS.lock().resolve_path(&path_str) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let need = (mode & 7) as u32;
    if need == 0 { return 0; }
    if check_node_permission(&node, need) { 0 } else { errno::Errno::EACCES as u64 }
}

pub fn sys_faccessat(dirfd: i64, pathname_ptr: *const u8, mode: i32, flags: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(pathname_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_path = match resolve_path_at(dirfd, &path_str, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let node = match VFS.lock().resolve_path(&abs_path) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let need = (mode & 7) as u32;
    if need == 0 { return 0; }
    if (flags & AT_EACCESS) != 0 {
        if let Ok(stat) = node.stat() {
            let euid = get_current_euid(); let egid = get_current_egid();
            if has_capability(CAP_DAC_OVERRIDE) { return 0; }
            if has_capability(CAP_DAC_READ_SEARCH) && (need & 2) == 0 { return 0; }
            let bits = if euid == stat.st_uid { (stat.st_mode >> 6) & 7 } else if egid == stat.st_gid { (stat.st_mode >> 3) & 7 } else { stat.st_mode & 7 };
            if (bits & need) == need { 0 } else { errno::Errno::EACCES as u64 }
        } else { 0 }
    } else {
        if check_node_permission(&node, need) { 0 } else { errno::Errno::EACCES as u64 }
    }
}

// ─── getcwd / chdir ───────────────────────────────────────────────

pub fn sys_getcwd(buf: *mut u8, size: usize) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    if let Some(ref process) = *process_lock {
        let cwd = process.files.lock().cwd.clone();
        if cwd.len() + 1 > size { return errno::Errno::ERANGE as u64; }
        unsafe { core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf, cwd.len()); *buf.add(cwd.len()) = 0; }
        return buf as u64;
    }
    errno::Errno::ESRCH as u64
}

pub fn sys_chdir(path_ptr: *const u8) -> u64 {
    let mut len = 0;
    unsafe { while *path_ptr.add(len) != 0 { len += 1; } }
    let path_slice = unsafe { core::slice::from_raw_parts(path_ptr, len) };
    let path_str = core::str::from_utf8(path_slice).unwrap_or("");
    if let Some(node) = VFS.lock().resolve_path(path_str) {
        if !node.is_dir() { return errno::Errno::ENOTDIR as u64; }
        if !check_node_permission(&node, 1) { return errno::Errno::EACCES as u64; }
        let process_lock = CURRENT_PROCESS.lock();
        if let Some(ref process) = *process_lock {
            let mut new_cwd = String::from(path_str);
            if !new_cwd.starts_with('/') {
                let cur_cwd = process.files.lock().cwd.clone();
                if cur_cwd == "/" { new_cwd = alloc::format!("/{}", new_cwd); } else { new_cwd = alloc::format!("{}/{}", cur_cwd, new_cwd); }
            }
            if new_cwd.len() > 1 && new_cwd.ends_with('/') { new_cwd.pop(); }
            process.files.lock().cwd = new_cwd;
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}
