//! File metadata, permissions, and link syscalls.
//! Extracted from fs.rs to keep each module under 1k lines.

use super::errno;
use super::*;
use super::fs_open::{split_parent, check_chown_permission};
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::vfs::{VFS, VfsNode, Stat};
use alloc::string::String;

// ─── stat / fstat / lstat / fstatat / statfs ──────────────────────

pub fn sys_stat(path_ptr: *const u8, stat_buf: *mut Stat) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    if let Some(node) = VFS.lock().resolve_path(&path_str) {
        if let Ok(stat) = node.stat() {
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}

pub fn sys_statfs(path_ptr: *const u8, statfs_buf: *mut u8) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let mut vfs = VFS.lock();
    if let Some(node) = vfs.resolve_path(&path_str) {
        if let Ok(statfs) = node.statfs() {
            let slice = unsafe { core::slice::from_raw_parts(&statfs as *const _ as *const u8, core::mem::size_of::<crate::vfs::StatFs>()) };
            if unsafe { user_access::copy_to_user(statfs_buf, slice) }.is_err() { return errno::Errno::EFAULT as u64; }
            return 0;
        }
    }
    if let Some(root) = vfs.statfs_mount(&path_str) {
        if let Ok(statfs) = root.statfs() {
            let slice = unsafe { core::slice::from_raw_parts(&statfs as *const _ as *const u8, core::mem::size_of::<crate::vfs::StatFs>()) };
            if unsafe { user_access::copy_to_user(statfs_buf, slice) }.is_err() { return errno::Errno::EFAULT as u64; }
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}

pub fn sys_fstat(fd: u64, stat_buf: *mut Stat) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            if let Ok(stat) = node.stat() {
                if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                return 0;
            }
            errno::Errno::EIO as u64
        },
        Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => {
            let stat = Stat { st_mode: crate::vfs::_S_IFCHR | 0o620, ..Stat::default() };
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())).is_err() } { return errno::Errno::EFAULT as u64; }
            0
        },
        Some(FileDescriptor::Socket(_, _)) | Some(FileDescriptor::UnixSocket(_, _)) => {
            let stat = Stat { st_mode: crate::vfs::_S_IFSOCK | 0o666, ..Stat::default() };
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
            0
        },
        Some(FileDescriptor::SignalFd(_)) | Some(FileDescriptor::EventFd(_)) => {
            let stat = Stat { st_mode: crate::vfs::_S_IFCHR | 0o600, ..Stat::default() };
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
            0
        },
        None => errno::Errno::EBADF as u64,
    }
}

pub fn sys_lstat(path_ptr: *const u8, stat_buf: *mut Stat) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    if let Some(node) = VFS.lock().resolve_path(&path_str) {
        if let Ok(stat) = node.stat() {
            if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
            return 0;
        }
    }
    errno::Errno::ENOENT as u64
}

pub fn sys_fstatat(dirfd: i64, pathname_ptr: *const u8, stat_buf: *mut crate::vfs::Stat, flags: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(pathname_ptr, 256) } {
        Ok(s) => s,
        Err(_) => {
            if (flags & AT_EMPTY_PATH) != 0 && pathname_ptr.is_null() { alloc::string::String::new() }
            else { return errno::Errno::EFAULT as u64; }
        }
    };
    if (flags & AT_EMPTY_PATH) != 0 && path_str.is_empty() {
        let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
        let fd_table = process.files.lock().fd_table.clone();
        if (dirfd as usize) >= fd_table.len() {
            drop(fd_table);
            if let Some(entry) = process.handle_table.lock().get(dirfd as u64) {
                if let Ok(stat) = entry.object.stat() {
                    if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                    return 0;
                }
            }
            return errno::Errno::EBADF as u64;
        }
        match &fd_table[dirfd as usize] {
            Some(FileDescriptor::File { node, .. }) => {
                if let Ok(stat) = node.stat() {
                    if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                    return 0;
                }
                return errno::Errno::EIO as u64;
            }
            Some(FileDescriptor::PtyMaster { .. }) | Some(FileDescriptor::PtySlave { .. }) => {
                let stat = crate::vfs::Stat { st_mode: crate::vfs::_S_IFCHR | 0o620, ..crate::vfs::Stat::default() };
                if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                return 0;
            }
            Some(FileDescriptor::Socket(_, _)) | Some(FileDescriptor::UnixSocket(_, _)) => {
                let stat = crate::vfs::Stat { st_mode: crate::vfs::_S_IFSOCK | 0o666, ..crate::vfs::Stat::default() };
                if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                return 0;
            }
            Some(FileDescriptor::SignalFd(_)) | Some(FileDescriptor::EventFd(_)) => {
                let stat = crate::vfs::Stat { st_mode: crate::vfs::_S_IFCHR | 0o600, ..crate::vfs::Stat::default() };
                if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                return 0;
            }
            None => return errno::Errno::EBADF as u64,
        }
    }
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_path = match resolve_path_at(dirfd, &path_str, &process) { Ok(p) => p, Err(e) => return e as u64 };
    if (flags & AT_SYMLINK_NOFOLLOW) != 0 {
        let (parent_path, name) = split_parent(&abs_path);
        let vfs = VFS.lock();
        if let Some(parent) = vfs.resolve_path(&parent_path) {
            if let Some(node) = parent.find_child(&name) {
                if let Ok(stat) = node.stat() {
                    if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                    return 0;
                }
            }
        }
        errno::Errno::ENOENT as u64
    } else {
        if let Some(node) = VFS.lock().resolve_path(&abs_path) {
            if let Ok(stat) = node.stat() {
                if unsafe { user_access::copy_to_user(stat_buf as *mut u8, core::slice::from_raw_parts(&stat as *const _ as *const u8, core::mem::size_of::<crate::vfs::Stat>())) }.is_err() { return errno::Errno::EFAULT as u64; }
                return 0;
            }
        }
        errno::Errno::ENOENT as u64
    }
}

// ─── chmod / fchmod / chown / fchown / umask ──────────────────────

pub fn sys_chmod(path_ptr: *const u8, mode: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let node = match VFS.lock().resolve_path(&path_str) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if !check_file_owner(&node) { audit_log("CAP_FOWNER", &alloc::format!("chmod({}) DENIED", path_str)); return errno::Errno::EACCES as u64; }
    if node.chmod(mode).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
}

pub fn sys_fchmod(fd: u64, mode: u32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            if !check_file_owner(node) { audit_log("CAP_FOWNER", "fchmod DENIED"); return errno::Errno::EACCES as u64; }
            if node.chmod(mode).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
        },
        _ => errno::Errno::ENOSYS as u64,
    }
}

pub fn sys_chown(path_ptr: *const u8, uid: u32, gid: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let node = match VFS.lock().resolve_path(&path_str) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let cur = match node.stat() { Ok(s) => s, Err(_) => return errno::Errno::EIO as u64 };
    if !check_file_owner(&node) { audit_log("CAP_FOWNER", &alloc::format!("chown({}) DENIED", path_str)); return errno::Errno::EACCES as u64; }
    if !check_chown_permission(cur.st_uid, cur.st_gid, uid, gid) { audit_log("CAP_CHOWN", &alloc::format!("chown({}) DENIED", path_str)); return errno::Errno::EPERM as u64; }
    let new_uid = if uid as i32 == -1 { cur.st_uid } else { uid };
    let new_gid = if gid as i32 == -1 { cur.st_gid } else { gid };
    if node.chown(new_uid, new_gid).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
}

pub fn sys_fchown(fd: u64, uid: u32, gid: u32) -> u64 {
    let process_lock = CURRENT_PROCESS.lock();
    let process = match *process_lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let fd_table = process.files.lock().fd_table.clone();
    if (fd as usize) >= fd_table.len() { return errno::Errno::EBADF as u64; }
    match fd_table[fd as usize] {
        Some(FileDescriptor::File { ref node, .. }) => {
            let cur = match node.stat() { Ok(s) => s, Err(_) => return errno::Errno::EIO as u64 };
            if !check_file_owner(node) { audit_log("CAP_FOWNER", "fchown DENIED"); return errno::Errno::EACCES as u64; }
            if !check_chown_permission(cur.st_uid, cur.st_gid, uid, gid) { audit_log("CAP_CHOWN", "fchown DENIED"); return errno::Errno::EPERM as u64; }
            let new_uid = if uid as i32 == -1 { cur.st_uid } else { uid };
            let new_gid = if gid as i32 == -1 { cur.st_gid } else { gid };
            if node.chown(new_uid, new_gid).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
        },
        _ => errno::Errno::ENOSYS as u64,
    }
}

pub fn sys_umask(mask: u32) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { let old = *p.umask.lock(); *p.umask.lock() = mask & 0o777; old as u64 } else { 0 }
}

// ─── symlink / symlinkat / readlink / readlinkat ──────────────────

pub fn sys_symlink(target: *const u8, linkpath: *const u8) -> u64 {
    let target_str = match unsafe { user_access::read_user_string(target, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let linkpath_str = match unsafe { user_access::read_user_string(linkpath, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let vfs = crate::vfs::VFS.lock();
    let (parent_path, name) = split_parent(&linkpath_str);
    if let Some(parent) = vfs.resolve_path(&parent_path) {
        if !check_node_permission(&parent, 2) { return errno::Errno::EACCES as u64; }
        let subj = crate::security::current_subject();
        if !crate::security::hook_file_create(&subj, &linkpath_str) { return errno::Errno::EACCES as u64; }
        if parent.symlink(&name, &target_str).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
    } else { errno::Errno::ENOENT as u64 }
}

pub fn sys_symlinkat(target_ptr: *const u8, newdirfd: i64, linkpath_ptr: *const u8) -> u64 {
    let target_str = match unsafe { user_access::read_user_string(target_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let linkpath_str = match unsafe { user_access::read_user_string(linkpath_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_path = match resolve_path_at(newdirfd, &linkpath_str, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let vfs = crate::vfs::VFS.lock();
    let (parent_path, name) = split_parent(&abs_path);
    if let Some(parent) = vfs.resolve_path(&parent_path) {
        if !check_node_permission(&parent, 2) { return errno::Errno::EACCES as u64; }
        let subj = crate::security::current_subject();
        if !crate::security::hook_file_create(&subj, &abs_path) { return errno::Errno::EACCES as u64; }
        if parent.symlink(&name, &target_str).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
    } else { errno::Errno::ENOENT as u64 }
}

pub fn sys_readlink(pathname: *const u8, buf: *mut u8, bufsize: u64) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(pathname, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let vfs = crate::vfs::VFS.lock();
    if let Some(node) = vfs.resolve_path(&path_str) {
        match node.readlink() {
            Ok(target) => { let len = core::cmp::min(target.len(), bufsize as usize); if unsafe { user_access::copy_to_user(buf, &target.as_bytes()[..len]) }.is_err() { return errno::Errno::EFAULT as u64; } len as u64 }
            Err(_) => errno::Errno::EINVAL as u64,
        }
    } else { errno::Errno::ENOENT as u64 }
}

pub fn sys_readlinkat(dirfd: i64, pathname_ptr: *const u8, buf: *mut u8, bufsize: u64) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(pathname_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_path = match resolve_path_at(dirfd, &path_str, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let vfs = crate::vfs::VFS.lock();
    if let Some(node) = vfs.resolve_path(&abs_path) {
        match node.readlink() {
            Ok(target) => { let len = core::cmp::min(target.len(), bufsize as usize); if unsafe { user_access::copy_to_user(buf, &target.as_bytes()[..len]) }.is_err() { return errno::Errno::EFAULT as u64; } len as u64 }
            Err(_) => errno::Errno::EINVAL as u64,
        }
    } else { errno::Errno::ENOENT as u64 }
}

// ─── link / linkat / rename / renameat ────────────────────────────

pub fn sys_link(old_path_ptr: *const u8, new_path_ptr: *const u8) -> u64 {
    let old_path = match unsafe { user_access::read_user_string(old_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let new_path = match unsafe { user_access::read_user_string(new_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let vfs = VFS.lock();
    let old_node = match vfs.resolve_path(&old_path) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let (parent_dir, new_name) = match new_path.rsplit_once('/') { Some((p, n)) => (p, n), None => ("", new_path.as_str()) };
    let parent_node = match vfs.resolve_path(if parent_dir.is_empty() { "." } else { parent_dir }) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if parent_node.link(old_node, new_name).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
}

pub fn sys_linkat(olddirfd: i64, old_path_ptr: *const u8, newdirfd: i64, new_path_ptr: *const u8, _flags: i32) -> u64 {
    let old_path = match unsafe { user_access::read_user_string(old_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let new_path = match unsafe { user_access::read_user_string(new_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let old_abs = match resolve_path_at(olddirfd, &old_path, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let new_abs = match resolve_path_at(newdirfd, &new_path, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let vfs = VFS.lock();
    let existing_node = match vfs.resolve_path(&old_abs) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let last_slash = new_abs.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 { ("/", &new_abs[1..]) } else { (&new_abs[..last_slash], &new_abs[last_slash+1..]) };
    let parent_node = match vfs.resolve_path(parent_path) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if !check_node_permission(&parent_node, 3) { return errno::Errno::EACCES as u64; }
    match parent_node.link(existing_node, name) { Ok(()) => 0, Err(()) => errno::Errno::EIO as u64 }
}

pub fn sys_rename(old_path_ptr: *const u8, new_path_ptr: *const u8) -> u64 {
    let old_path = match unsafe { user_access::read_user_string(old_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let new_path = match unsafe { user_access::read_user_string(new_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let vfs = VFS.lock();
    let source_node = match vfs.resolve_path(&old_path) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if !check_node_permission(&source_node, 2) { return errno::Errno::EACCES as u64; }
    let src_last = old_path.rfind('/').unwrap_or(0);
    let dst_last = new_path.rfind('/').unwrap_or(0);
    let (src_parent, src_name) = if src_last == 0 { ("/", &old_path[1..]) } else { (&old_path[..src_last], &old_path[src_last+1..]) };
    let (dst_parent, dst_name) = if dst_last == 0 { ("/", &new_path[1..]) } else { (&new_path[..dst_last], &new_path[dst_last+1..]) };
    if src_parent == dst_parent {
        if let Some(parent) = vfs.resolve_path(src_parent) {
            if !check_node_permission(&parent, 3) { return errno::Errno::EACCES as u64; }
            if parent.rename(src_name, dst_name).is_ok() { return 0; }
        }
    }
    let data = match source_node.read(usize::MAX) { Ok(d) => d, Err(_) => return errno::Errno::EIO as u64 };
    let dst_p = match vfs.resolve_path(dst_parent) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if !check_node_permission(&dst_p, 3) { return errno::Errno::EACCES as u64; }
    if dst_p.create(dst_name).is_err() { return errno::Errno::EIO as u64; }
    if let Some(new_node) = dst_p.find_child(dst_name) { let _ = new_node.write(&data); }
    let src_p = match vfs.resolve_path(src_parent) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let _ = src_p.unlink(src_name);
    0
}

pub fn sys_renameat(olddirfd: i64, old_path_ptr: *const u8, newdirfd: i64, new_path_ptr: *const u8) -> u64 {
    let old_path = match unsafe { user_access::read_user_string(old_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let new_path = match unsafe { user_access::read_user_string(new_path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_old = match resolve_path_at(olddirfd, &old_path, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let abs_new = match resolve_path_at(newdirfd, &new_path, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let vfs = VFS.lock();
    let source_node = match vfs.resolve_path(&abs_old) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if !check_node_permission(&source_node, 2) { return errno::Errno::EACCES as u64; }
    let src_last = abs_old.rfind('/').unwrap_or(0);
    let dst_last = abs_new.rfind('/').unwrap_or(0);
    let (src_parent, src_name) = if src_last == 0 { ("/", &abs_old[1..]) } else { (&abs_old[..src_last], &abs_old[src_last+1..]) };
    let (dst_parent, dst_name) = if dst_last == 0 { ("/", &abs_new[1..]) } else { (&abs_new[..dst_last], &abs_new[dst_last+1..]) };
    if src_parent == dst_parent {
        if let Some(parent) = vfs.resolve_path(src_parent) {
            if !check_node_permission(&parent, 3) { return errno::Errno::EACCES as u64; }
            if parent.rename(src_name, dst_name).is_ok() { return 0; }
        }
    }
    let data = match source_node.read(usize::MAX) { Ok(d) => d, Err(_) => return errno::Errno::EIO as u64 };
    let dst_p = match vfs.resolve_path(dst_parent) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    if !check_node_permission(&dst_p, 3) { return errno::Errno::EACCES as u64; }
    if dst_p.create(dst_name).is_err() { return errno::Errno::EIO as u64; }
    if let Some(new_node) = dst_p.find_child(dst_name) { let _ = new_node.write(&data); }
    let src_p = match vfs.resolve_path(src_parent) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let _ = src_p.unlink(src_name);
    0
}

// ─── mkdir / mkdirat / unlink / unlinkat ──────────────────────────

pub fn sys_mkdir(path_ptr: *const u8, mode: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let last_slash = path_str.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 && !path_str.starts_with('/') { (".", path_str.as_str()) } else if last_slash == 0 { ("/", &path_str[1..]) } else { (&path_str[..last_slash], &path_str[last_slash+1..]) };
    let vfs = VFS.lock();
    if let Some(parent_node) = vfs.resolve_path(parent_path) {
        if !check_node_permission(&parent_node, 3) { return errno::Errno::EACCES as u64; }
        let subj = crate::security::current_subject();
        if !crate::security::hook_dir_mkdir(&subj, &path_str) { return errno::Errno::EACCES as u64; }
        if let Ok(new_node) = parent_node.mkdir(name) {
            let (euid, egid, umask_val) = { let lock = CURRENT_PROCESS.lock(); lock.as_ref().map(|p| { let c = p.creds.lock(); (c.euid, c.egid, *p.umask.lock()) }).unwrap_or((0, 0, 0)) };
            let raw_mode = if mode == 0 { 0o777 } else { mode };
            let _ = new_node.chmod(raw_mode & !umask_val & 0o777);
            let _ = new_node.chown(euid, egid);
            return 0;
        }
    }
    errno::Errno::EIO as u64
}

pub fn sys_mkdirat(dirfd: i64, path_ptr: *const u8, mode: u32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_path = match resolve_path_at(dirfd, &path_str, &process) { Ok(p) => p, Err(e) => return e as u64 };
    let last_slash = abs_path.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 { ("/", &abs_path[1..]) } else { (&abs_path[..last_slash], &abs_path[last_slash+1..]) };
    let vfs = VFS.lock();
    if let Some(parent_node) = vfs.resolve_path(parent_path) {
        if !check_node_permission(&parent_node, 3) { return errno::Errno::EACCES as u64; }
        let subj = crate::security::current_subject();
        if !crate::security::hook_dir_mkdir(&subj, &abs_path) { return errno::Errno::EACCES as u64; }
        if let Ok(new_node) = parent_node.mkdir(name) {
            let (euid, egid, umask_val) = { let lock = CURRENT_PROCESS.lock(); lock.as_ref().map(|p| { let c = p.creds.lock(); (c.euid, c.egid, *p.umask.lock()) }).unwrap_or((0, 0, 0)) };
            let raw_mode = if mode == 0 { 0o777 } else { mode };
            let _ = new_node.chmod(raw_mode & !umask_val & 0o777);
            let _ = new_node.chown(euid, egid);
            return 0;
        }
    }
    errno::Errno::EIO as u64
}

pub fn sys_unlink(path_ptr: *const u8) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let last_slash = path_str.rfind('/').unwrap_or(0);
    let (parent_path, name) = if last_slash == 0 && !path_str.starts_with('/') { (".", path_str.as_str()) } else if last_slash == 0 { ("/", &path_str[1..]) } else { (&path_str[..last_slash], &path_str[last_slash+1..]) };
    let vfs = VFS.lock();
    if let Some(parent_node) = vfs.resolve_path(parent_path) {
        if !check_node_permission(&parent_node, 3) { return errno::Errno::EACCES as u64; }
        let subj = crate::security::current_subject();
        if !crate::security::hook_file_unlink(&subj, &path_str) { return errno::Errno::EACCES as u64; }
        if parent_node.unlink(name).is_ok() { return 0; }
    }
    errno::Errno::EIO as u64
}

pub fn sys_unlinkat(dirfd: i64, path_ptr: *const u8, flags: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let process = match get_current_process() { Some(p) => p, None => return errno::Errno::ESRCH as u64 };
    let abs_path = match resolve_path_at(dirfd, &path_str, &process) { Ok(p) => p, Err(e) => return e as u64 };
    if (flags & AT_REMOVEDIR) != 0 {
        let last_slash = abs_path.rfind('/').unwrap_or(0);
        let (parent_path, name) = if last_slash == 0 { ("/", &abs_path[1..]) } else { (&abs_path[..last_slash], &abs_path[last_slash+1..]) };
        let vfs = VFS.lock();
        if let Some(parent_node) = vfs.resolve_path(parent_path) {
            if !check_node_permission(&parent_node, 3) { return errno::Errno::EACCES as u64; }
            if let Some(node) = parent_node.find_child(name) { if !node.is_dir() { return errno::Errno::ENOTDIR as u64; } }
            if parent_node.unlink(name).is_ok() { return 0; }
        }
        errno::Errno::EIO as u64
    } else {
        let last_slash = abs_path.rfind('/').unwrap_or(0);
        let (parent_path, name) = if last_slash == 0 { ("/", &abs_path[1..]) } else { (&abs_path[..last_slash], &abs_path[last_slash+1..]) };
        let vfs = VFS.lock();
        if let Some(parent_node) = vfs.resolve_path(parent_path) {
            if !check_node_permission(&parent_node, 3) { return errno::Errno::EACCES as u64; }
            let subj = crate::security::current_subject();
            if !crate::security::hook_file_unlink(&subj, &abs_path) { return errno::Errno::EACCES as u64; }
            if parent_node.unlink(name).is_ok() { return 0; }
        }
        errno::Errno::EIO as u64
    }
}

// ─── truncate / ftruncate / utimensat ─────────────────────────────

pub fn sys_truncate(path_ptr: *const u8, len: i64) -> u64 {
    let path = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let vfs = VFS.lock();
    match vfs.resolve_path(&path) {
        Some(node) => { if !check_node_permission(&node, 2) { return errno::Errno::EACCES as u64; } if node.truncate(len).is_ok() { 0 } else { errno::Errno::EIO as u64 } }
        None => errno::Errno::ENOENT as u64,
    }
}

pub fn sys_ftruncate(fd: u64, len: i64) -> u64 {
    let proc = CURRENT_PROCESS.lock();
    if let Some(ref p) = *proc {
        let fd_table = p.files.lock().fd_table.clone();
        if let Some(Some(desc)) = fd_table.get(fd as usize) {
            match desc {
                FileDescriptor::File { node, .. } => { let n = node.clone(); drop(fd_table); drop(proc); if n.truncate(len).is_ok() { 0 } else { errno::Errno::EIO as u64 } }
                _ => { drop(fd_table); drop(proc); errno::Errno::EBADF as u64 }
            }
        } else { errno::Errno::EBADF as u64 }
    } else { errno::Errno::EBADF as u64 }
}

pub fn sys_utimensat(_dirfd: i64, pathname_ptr: *const u8, _times_ptr: *const u8, _flags: i32) -> u64 {
    let path_str = match unsafe { user_access::read_user_string(pathname_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let node = match VFS.lock().resolve_path(&path_str) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    let now = crate::drivers::rtc::read_realtime();
    if node.utimens((now.0, now.1), (now.0, now.1)).is_ok() { 0 } else { errno::Errno::EPERM as u64 }
}
