use alloc::vec::Vec;
use alloc::vec;
use crate::objects::handle::HandleValue;
use crate::syscalls::errno::Errno;
use crate::syscalls::user_access;
use crate::task::process::CURRENT_PROCESS;

/// SYS_OBJECT_OPEN (384) — open an object by namespace path, returns a handle.
pub fn sys_object_open(path_ptr: *const u8, access_mask: u32) -> u64 {
    let path = match unsafe { user_access::read_user_string(path_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return Errno::EFAULT as u64,
    };
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return Errno::ESRCH as u64,
    };
    let obj = match crate::objects::namespace::resolve_object(&path) {
        Some(o) => o,
        None => return Errno::ENOENT as u64,
    };
    match process.new_handle(obj, access_mask, 0) {
        Ok(hv) => hv,
        Err(_) => Errno::EACCES as u64,
    }
}

/// SYS_OBJECT_CLOSE (385) — close a handle.
pub fn sys_object_close(handle: HandleValue) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return Errno::ESRCH as u64,
    };
    if process.close_handle(handle).is_some() {
        0
    } else {
        Errno::EBADF as u64
    }
}

/// SYS_OBJECT_DUPLICATE (386) — duplicate a handle, optionally to another process.
pub fn sys_object_duplicate(handle: HandleValue, target_pid: u64, access_mask: u32) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return Errno::ESRCH as u64,
    };
    let entry = {
        let ht = process.handle_table.lock();
        match ht.get(handle) {
            Some(e) => {
                let obj = e.object.clone();
                let mask = if access_mask != 0 { access_mask } else { e.access_mask };
                (obj, mask)
            }
            None => return Errno::EBADF as u64,
        }
    };
    if target_pid == 0 || target_pid == process.id {
        match process.new_handle(entry.0, entry.1, 0) {
            Ok(hv) => hv,
            Err(_) => Errno::ENFILE as u64,
        }
    } else {
        let table = crate::task::process::PROCESS_TABLE.lock();
        let target = match table.get(&target_pid) {
            Some(p) => p.clone(),
            None => return Errno::ESRCH as u64,
        };
        drop(table);
        match target.new_handle(entry.0, entry.1, 0) {
            Ok(hv) => hv,
            Err(_) => Errno::ENFILE as u64,
        }
    }
}

/// SYS_OBJECT_QUERY (387) — query object type and name by handle.
pub fn sys_object_query(handle: HandleValue, buf: *mut u8, len: usize) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return Errno::ESRCH as u64,
    };
    let entry = {
        let ht = process.handle_table.lock();
        match ht.get(handle) {
            Some(e) => (e.object.clone(), e.access_mask),
            None => return Errno::EBADF as u64,
        }
    };
    let type_id = entry.0.type_id().0;
    let name = entry.0.query_name().unwrap_or_default();
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len() as u64;
    let hdr_size = 2 + 4 + 8;
    if len < hdr_size { return Errno::EINVAL as u64; }
    let mut out = Vec::new();
    out.extend_from_slice(&type_id.to_le_bytes());
    out.extend_from_slice(&entry.1.to_le_bytes());
    out.extend_from_slice(&name_len.to_le_bytes());
    let copy_len = core::cmp::min(name_bytes.len(), len.saturating_sub(hdr_size));
    out.extend_from_slice(&name_bytes[..copy_len]);
    if unsafe { user_access::copy_to_user(buf, &out) }.is_err() {
        return Errno::EFAULT as u64;
    }
    out.len() as u64
}

/// SYS_OBJECT_WAIT (388) — wait on one or more synchronization objects.
/// ponytail: simple spin-wait with timeout, no fancy wait-set yet.
pub fn sys_object_wait(handles_ptr: *const u32, num_handles: u32, timeout_ms: u64) -> u64 {
    let process = match *CURRENT_PROCESS.lock() {
        Some(ref p) => p.clone(),
        None => return Errno::ESRCH as u64,
    };
    if num_handles == 0 || num_handles > 64 { return Errno::EINVAL as u64; }
    let size = (num_handles as usize) * 4;
    let mut handle_bytes = vec![0u8; size];
    if unsafe { user_access::copy_from_user(&mut handle_bytes, handles_ptr as *const u8) }.is_err() {
        return Errno::EFAULT as u64;
    }
    let handles: Vec<u32> = handle_bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let deadline = if timeout_ms > 0 {
        let ticks = crate::interrupts::get_ticks();
        Some(ticks + (timeout_ms / 10).max(1))
    } else {
        None
    };
    loop {
        for &hv in &handles {
            let ht = process.handle_table.lock();
            if let Some(entry) = ht.get(hv as HandleValue) {
                if entry.object.poll_readable() {
                    drop(ht);
                    return hv as u64;
                }
            } else {
                drop(ht);
                return Errno::EBADF as u64;
            }
        }
        if let Some(dead) = deadline {
            if crate::interrupts::get_ticks() >= dead {
                return Errno::EAGAIN as u64;
            }
        }
        if timeout_ms == 0 { return Errno::EAGAIN as u64; }
        crate::task::scheduler::try_schedule();
    }
}
