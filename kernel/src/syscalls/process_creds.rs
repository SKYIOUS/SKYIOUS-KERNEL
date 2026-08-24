#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! Process credentials and resource limit syscalls: uid, gid, capabilities,
//! process groups, resource limits.
//! Extracted from process.rs to keep each module under 1k lines.

use super::errno;
use super::numbers;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;

#[repr(C)]
struct RLimit {
    rlim_cur: i64,
    rlim_max: i64,
}

pub fn sys_setpgid(pid: u64, pgid: u64) -> u64 {
    let current = {
        let lock = CURRENT_PROCESS.lock();
        match lock.as_ref() {
            Some(p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };
    let target_pid = if pid == 0 { current.id } else { pid };

    if pgid > i32::MAX as u64 {
        return errno::Errno::EINVAL as u64;
    }

    let target = {
        let table = crate::task::process::PROCESS_TABLE.lock();
        match table.get(&target_pid) {
            Some(p) => p.clone(),
            None => return errno::Errno::ESRCH as u64,
        }
    };

    // Session leader cannot change its pgid
    if target.identity.lock().session == target_pid {
        return errno::Errno::EPERM as u64;
    }

    // Only self, child, or process with CAP_SYS_ADMIN
    if target_pid != current.id {
        let is_child = current.children.lock().contains(&target_pid);
        if !is_child && !has_capability(CAP_SYS_ADMIN) {
            return errno::Errno::EPERM as u64;
        }
    }

    let new_pgid = if pgid == 0 { target_pid } else { pgid };

    // If joining an existing group, verify it exists and is in the same session
    if new_pgid != target_pid {
        let table = crate::task::process::PROCESS_TABLE.lock();
        match table.get(&new_pgid) {
            Some(group_leader) => {
                if group_leader.identity.lock().session != current.identity.lock().session {
                    return errno::Errno::EPERM as u64;
                }
            }
            None => return errno::Errno::ESRCH as u64,
        }
    }

    { let mut id = target.identity.lock(); id.pgid = new_pgid; };
    0
}

pub fn sys_getpgid(pid: u64) -> u64 {
    let target_pid = if pid == 0 {
        let lock = CURRENT_PROCESS.lock();
        match lock.as_ref() {
            Some(p) => p.id,
            None => return errno::Errno::ESRCH as u64,
        }
    } else {
        pid
    };
    let table = crate::task::process::PROCESS_TABLE.lock();
    match table.get(&target_pid) {
        Some(p) => p.identity.lock().pgid,
        None => errno::Errno::ESRCH as u64,
    }
}

pub fn sys_getpgrp() -> u64 {
    sys_getpgid(0)
}

pub fn sys_setsid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    match lock.as_ref() {
        Some(p) => {
            let id = p.id;
            {
                let mut ident = p.identity.lock();
                if ident.session == id || ident.is_group_leader {
                    return errno::Errno::EPERM as u64;
                }
                ident.session = id;
                ident.pgid = id;
                ident.is_group_leader = true;
            }
            p.id
        }
        None => errno::Errno::ESRCH as u64,
    }
}

pub fn sys_getsid(pid: u64) -> u64 {
    let (target_pid, current_session) = {
        let lock = CURRENT_PROCESS.lock();
        match lock.as_ref() {
            Some(p) => (if pid == 0 { p.id } else { pid }, p.identity.lock().session),
            None => return errno::Errno::ESRCH as u64,
        }
    };
    let table = crate::task::process::PROCESS_TABLE.lock();
    match table.get(&target_pid) {
        Some(p) => {
            if p.identity.lock().session != current_session {
                return errno::Errno::EPERM as u64;
            }
            p.identity.lock().session
        }
        None => errno::Errno::ESRCH as u64,
    }
}

pub fn sys_getrlimit(resource: u64, rlim_ptr: *mut u8) -> u64 {
    if resource >= 16 || rlim_ptr.is_null() {
        return errno::Errno::EINVAL as u64;
    }
    let lock = CURRENT_PROCESS.lock();
    match lock.as_ref() {
        Some(p) => {
            let lim = p.limits.lock();
            let rlim = RLimit {
                rlim_cur: lim.rlim_cur[resource as usize],
                rlim_max: lim.rlim_max[resource as usize],
            };
            unsafe {
                if user_access::copy_to_user(rlim_ptr,
                    core::slice::from_raw_parts(&rlim as *const _ as *const u8, core::mem::size_of::<RLimit>())).is_err()
                {
                    return errno::Errno::EFAULT as u64;
                }
            }
            0
        }
        None => errno::Errno::ESRCH as u64,
    }
}

pub fn sys_setrlimit(resource: u64, rlim_ptr: *const u8) -> u64 {
    if resource >= 16 || rlim_ptr.is_null() {
        return errno::Errno::EINVAL as u64;
    }
    let mut new_rlim = RLimit { rlim_cur: 0, rlim_max: 0 };
    unsafe {
        if user_access::copy_from_user(
            core::slice::from_raw_parts_mut(&mut new_rlim as *mut _ as *mut u8, core::mem::size_of::<RLimit>()),
            rlim_ptr,
        ).is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    if new_rlim.rlim_cur > new_rlim.rlim_max {
        return errno::Errno::EINVAL as u64;
    }
    let lock = CURRENT_PROCESS.lock();
    match lock.as_ref() {
        Some(p) => {
            let euid = p.creds.lock().euid;
            if euid != 0 {
                if new_rlim.rlim_max > p.limits.lock().rlim_max[resource as usize] {
                    return errno::Errno::EPERM as u64;
                }
            }
            {
                let mut lim = p.limits.lock();
                lim.rlim_cur[resource as usize] = new_rlim.rlim_cur;
                lim.rlim_max[resource as usize] = new_rlim.rlim_max;
            }
            0
        }
        None => errno::Errno::ESRCH as u64,
    }
}

pub fn sys_prlimit64(pid: u64, resource: u64, new_rlim_ptr: *const u8, old_rlim_ptr: *mut u8) -> u64 {
    if resource >= 16 {
        return errno::Errno::EINVAL as u64;
    }
    let target = {
        let current = {
            let lock = CURRENT_PROCESS.lock();
            match lock.as_ref() {
                Some(p) => p.clone(),
                None => return errno::Errno::ESRCH as u64,
            }
        };
        if pid == 0 || pid == current.id {
            current
        } else {
            if !has_capability(CAP_SYS_ADMIN) {
                return errno::Errno::EPERM as u64;
            }
            let table = crate::task::process::PROCESS_TABLE.lock();
            match table.get(&pid) {
                Some(p) => p.clone(),
                None => return errno::Errno::ESRCH as u64,
            }
        }
    };

    if !old_rlim_ptr.is_null() {
        let old_rlim = {
            let lim = target.limits.lock();
            RLimit {
                rlim_cur: lim.rlim_cur[resource as usize],
                rlim_max: lim.rlim_max[resource as usize],
            }
        };
        unsafe {
            if user_access::copy_to_user(old_rlim_ptr,
                core::slice::from_raw_parts(&old_rlim as *const _ as *const u8, core::mem::size_of::<RLimit>())).is_err()
            {
                return errno::Errno::EFAULT as u64;
            }
        }
    }

    if !new_rlim_ptr.is_null() {
        let mut new_rlim = RLimit { rlim_cur: 0, rlim_max: 0 };
        unsafe {
            if user_access::copy_from_user(
                core::slice::from_raw_parts_mut(&mut new_rlim as *mut _ as *mut u8, core::mem::size_of::<RLimit>()),
                new_rlim_ptr,
            ).is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
        if new_rlim.rlim_cur > new_rlim.rlim_max {
            return errno::Errno::EINVAL as u64;
        }
        let euid = target.creds.lock().euid;
        {
            let mut lim = target.limits.lock();
            if euid != 0 {
                if new_rlim.rlim_max > lim.rlim_max[resource as usize] {
                    return errno::Errno::EPERM as u64;
                }
            }
            lim.rlim_cur[resource as usize] = new_rlim.rlim_cur;
            lim.rlim_max[resource as usize] = new_rlim.rlim_max;
        }
    }
    0
}

pub fn sys_getuid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { p.creds.lock().uid as u64 } else { 0 }
}

pub fn sys_getgid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { p.creds.lock().gid as u64 } else { 0 }
}

pub fn sys_setuid(uid: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let euid = p.creds.lock().euid;
        if euid == 0 || has_capability(CAP_SETUID) {
            let mut c = p.creds.lock();
            c.uid = uid as u32;
            c.euid = uid as u32;
            0
        } else if euid == uid as u32 {
            p.creds.lock().uid = uid as u32;
            0
        } else {
            audit_log("CAP_SETUID", &alloc::format!("setuid({}) DENIED", uid));
            errno::Errno::EPERM as u64
        }
    } else { errno::Errno::ESRCH as u64 }
}

pub fn sys_setgid(gid: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let egid = p.creds.lock().egid;
        if egid == 0 || has_capability(CAP_SETGID) {
            let mut c = p.creds.lock();
            c.gid = gid as u32;
            c.egid = gid as u32;
            0
        } else if egid == gid as u32 {
            p.creds.lock().gid = gid as u32;
            0
        } else {
            audit_log("CAP_SETGID", &alloc::format!("setgid({}) DENIED", gid));
            errno::Errno::EPERM as u64
        }
    } else { errno::Errno::ESRCH as u64 }
}

pub fn sys_geteuid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { p.creds.lock().euid as u64 } else { 0 }
}

pub fn sys_getegid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock { p.creds.lock().egid as u64 } else { 0 }
}

pub fn sys_capget(hdrp: *mut u8, datap: *mut u8) -> u64 {
    if hdrp.is_null() { return errno::Errno::EFAULT as u64; }
    let mut header = [0u8; 8];
    if unsafe { user_access::copy_from_user(&mut header, hdrp) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    let version = u32::from_ne_bytes([header[0], header[1], header[2], header[3]]);
    let _pid = i32::from_ne_bytes([header[4], header[5], header[6], header[7]]);

    if version != 0x19980330 { return errno::Errno::EINVAL as u64; }
    if datap.is_null() { return errno::Errno::EFAULT as u64; }

    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };
    let c = proc.creds.lock();
    let data = [
        (c.cap_effective as u32).to_ne_bytes(),
        (c.cap_permitted as u32).to_ne_bytes(),
        (c.cap_inheritable as u32).to_ne_bytes(),
    ]
    .concat();
    drop(c);
    if unsafe { user_access::copy_to_user(datap, &data) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    0
}

pub fn sys_capset(hdrp: *const u8, datap: *const u8) -> u64 {
    if hdrp.is_null() || datap.is_null() { return errno::Errno::EFAULT as u64; }
    let mut header = [0u8; 8];
    if unsafe { user_access::copy_from_user(&mut header, hdrp) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    let version = u32::from_ne_bytes([header[0], header[1], header[2], header[3]]);
    let _pid = i32::from_ne_bytes([header[4], header[5], header[6], header[7]]);

    if version != 0x19980330 { return errno::Errno::EINVAL as u64; }

    let mut caps = [0u8; 12];
    if unsafe { user_access::copy_from_user(&mut caps, datap) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    let eff = u32::from_ne_bytes([caps[0], caps[1], caps[2], caps[3]]) as u64;
    let perm = u32::from_ne_bytes([caps[4], caps[5], caps[6], caps[7]]) as u64;
    let inh = u32::from_ne_bytes([caps[8], caps[9], caps[10], caps[11]]) as u64;

    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock { Some(ref p) => p, None => return errno::Errno::ESRCH as u64 };

    let euid = proc.creds.lock().euid;
    if euid != 0 && !has_capability(CAP_SETPCAP) {
        return errno::Errno::EPERM as u64;
    }

    let mut c = proc.creds.lock();
    c.cap_effective = eff;
    c.cap_permitted = perm;
    c.cap_inheritable = inh;
    0
}

pub fn sys_getgroups(size: i32, list: *mut u32) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let groups = process.groups.lock();
    if size < 0 {
        return errno::Errno::EINVAL as u64;
    }
    if size == 0 {
        return groups.len() as u64;
    }
    let size = size as usize;
    if size < groups.len() {
        return errno::Errno::EINVAL as u64;
    }
    let slice = unsafe { core::slice::from_raw_parts_mut(list, groups.len()) };
    for (i, g) in groups.iter().enumerate() {
        slice[i] = *g;
    }
    groups.len() as u64
}

pub fn sys_setgroups(size: i64, list: *const u32) -> u64 {
    if !has_capability(CAP_SETGID) {
        return errno::Errno::EPERM as u64;
    }
    if size < 0 || size > 65536 {
        return errno::Errno::EINVAL as u64;
    }
    let count = size as usize;
    let slice = if count > 0 {
        unsafe { core::slice::from_raw_parts(list, count) }
    } else {
        &[]
    };
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let mut groups = process.groups.lock();
    groups.clear();
    groups.extend_from_slice(slice);
    0
}

pub fn sys_getresuid(ruid_ptr: *mut u32, euid_ptr: *mut u32, suid_ptr: *mut u32) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let creds = process.creds.lock();
    let vals = [creds.uid, creds.euid, creds.suid];
    let ptrs = [ruid_ptr, euid_ptr, suid_ptr];
    for (val, ptr) in vals.iter().zip(ptrs.iter()) {
        if !ptr.is_null() {
            if unsafe { user_access::copy_to_user(*ptr as *mut u8, core::slice::from_raw_parts(val as *const u32 as *const u8, 4)) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
    }
    0
}

pub fn sys_setresuid(ruid: u32, euid: u32, suid: u32) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let mut creds = process.creds.lock();
    // POSIX: unprivileged may only set each to current real/effective/saved
    if !has_capability(CAP_SETUID) {
        if (ruid != !0u32 && ruid != creds.uid && ruid != creds.euid && ruid != creds.suid) ||
           (euid != !0u32 && euid != creds.uid && euid != creds.euid && euid != creds.suid) ||
           (suid != !0u32 && suid != creds.uid && suid != creds.euid && suid != creds.suid) {
            return errno::Errno::EPERM as u64;
        }
    }
    if ruid != !0u32 { creds.uid = ruid; }
    if euid != !0u32 { creds.euid = euid; }
    if suid != !0u32 { creds.suid = suid; }
    // Always keep fsuid in sync with euid
    creds.fsuid = creds.euid;
    0
}

pub fn sys_getresgid(rgid_ptr: *mut u32, egid_ptr: *mut u32, sgid_ptr: *mut u32) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let creds = process.creds.lock();
    let vals = [creds.gid, creds.egid, creds.sgid];
    let ptrs = [rgid_ptr, egid_ptr, sgid_ptr];
    for (val, ptr) in vals.iter().zip(ptrs.iter()) {
        if !ptr.is_null() {
            if unsafe { user_access::copy_to_user(*ptr as *mut u8, core::slice::from_raw_parts(val as *const u32 as *const u8, 4)) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
        }
    }
    0
}

pub fn sys_setresgid(rgid: u32, egid: u32, sgid: u32) -> u64 {
    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let mut creds = process.creds.lock();
    if !has_capability(CAP_SETGID) {
        if (rgid != !0u32 && rgid != creds.gid && rgid != creds.egid && rgid != creds.sgid) ||
           (egid != !0u32 && egid != creds.gid && egid != creds.egid && egid != creds.sgid) ||
           (sgid != !0u32 && sgid != creds.gid && sgid != creds.egid && sgid != creds.sgid) {
            return errno::Errno::EPERM as u64;
        }
    }
    if rgid != !0u32 { creds.gid = rgid; }
    if egid != !0u32 { creds.egid = egid; }
    if sgid != !0u32 { creds.sgid = sgid; }
    creds.fsgid = creds.egid;
    0
}

// ─── getrusage ────────────────────────────────────────────────────

/// Linux struct timeval { long tv_sec; long tv_usec; } — 8 bytes each on x86_64.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Timeval {
    tv_sec: i64,
    tv_usec: i64,
}

/// Linux struct rusage — 136 bytes on x86_64.
#[repr(C)]
#[derive(Clone, Copy)]
struct Rusage {
    ru_utime: Timeval,       //  0
    ru_stime: Timeval,       // 16
    ru_maxrss: i64,          // 32
    ru_ixrss: i64,           // 40
    ru_idrss: i64,           // 48
    ru_isrss: i64,           // 56
    ru_minflt: i64,          // 64
    ru_majflt: i64,          // 72
    ru_nswap: i64,           // 80
    ru_inblock: i64,         // 88
    ru_oublock: i64,         // 96
    ru_msgsnd: i64,          //104
    ru_msgrcv: i64,          //112
    ru_nsignals: i64,        //120
    ru_nvcsw: i64,           //128
    ru_nivcsw: i64,          //136
}  // total: 144 bytes

impl Default for Rusage {
    fn default() -> Self {
        Self {
            ru_utime: Timeval::default(),
            ru_stime: Timeval::default(),
            ru_maxrss: 0, ru_ixrss: 0, ru_idrss: 0, ru_isrss: 0,
            ru_minflt: 0, ru_majflt: 0, ru_nswap: 0,
            ru_inblock: 0, ru_oublock: 0,
            ru_msgsnd: 0, ru_msgrcv: 0, ru_nsignals: 0,
            ru_nvcsw: 0, ru_nivcsw: 0,
        }
    }
}

/// getrusage(who, rusage) → 0 on success.
///
/// who: 0 = RUSAGE_SELF, 1 = RUSAGE_CHILDREN.
pub fn sys_getrusage(who: u64, rusage_ptr: *mut u8) -> u64 {
    if rusage_ptr.is_null() {
        return errno::Errno::EINVAL as u64;
    }

    const RUSAGE_SELF: u64 = 0;
    const RUSAGE_CHILDREN: u64 = 1;

    let proc_lock = CURRENT_PROCESS.lock();
    let proc = match proc_lock.as_ref() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let mut ru = Rusage::default();

    match who {
        RUSAGE_SELF => {
            let uticks = proc.utime.load(core::sync::atomic::Ordering::Relaxed);
            let sticks = proc.stime.load(core::sync::atomic::Ordering::Relaxed);
            // Convert clock ticks (100 Hz) to timeval
            ru.ru_utime.tv_sec = (uticks / 100) as i64;
            ru.ru_utime.tv_usec = ((uticks % 100) * 10_000) as i64;
            ru.ru_stime.tv_sec = (sticks / 100) as i64;
            ru.ru_stime.tv_usec = ((sticks % 100) * 10_000) as i64;
            // Max RSS in kilobytes
            let mem = proc.memory.lock();
            let vsize_kb = mem.vmas.iter().map(|v| (v.end - v.start) / 1024).sum::<u64>();
            ru.ru_maxrss = vsize_kb as i64;
        }
        RUSAGE_CHILDREN => {
            let cuticks = proc.cutime.load(core::sync::atomic::Ordering::Relaxed);
            let csticks = proc.cstime.load(core::sync::atomic::Ordering::Relaxed);
            ru.ru_utime.tv_sec = (cuticks / 100) as i64;
            ru.ru_utime.tv_usec = ((cuticks % 100) * 10_000) as i64;
            ru.ru_stime.tv_sec = (csticks / 100) as i64;
            ru.ru_stime.tv_usec = ((csticks % 100) * 10_000) as i64;
        }
        _ => {
            return errno::Errno::EINVAL as u64;
        }
    }

    drop(proc_lock);

    unsafe {
        if user_access::copy_to_user(rusage_ptr,
            core::slice::from_raw_parts(&ru as *const _ as *const u8, core::mem::size_of::<Rusage>())
        ).is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    0
}
