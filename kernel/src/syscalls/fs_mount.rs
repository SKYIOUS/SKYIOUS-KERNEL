//! Mount, unmount, mkfs, swapon/swapoff, sync syscalls.
//! Extracted from fs.rs to keep each module under 1k lines.

use super::errno;
use super::*;
use crate::vfs::VFS;

pub fn sys_mount(source: *const u8, target: *const u8, fstype: *const u8, _flags: u64, _data: *const u8) -> u64 {
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_ADMIN) { audit_log("CAP_SYS_ADMIN", "mount DENIED"); return errno::Errno::EPERM as u64; }
    let mut src_buf = [0u8; 256]; let mut tgt_buf = [0u8; 256]; let mut fs_buf = [0u8; 32];
    if unsafe { user_access::copy_from_user(&mut src_buf[..255], source).is_err() } { return errno::Errno::EFAULT as u64; }
    if unsafe { user_access::copy_from_user(&mut tgt_buf[..255], target).is_err() } { return errno::Errno::EFAULT as u64; }
    if unsafe { user_access::copy_from_user(&mut fs_buf[..31], fstype).is_err() } { return errno::Errno::EFAULT as u64; }
    let _src_str = match core::ffi::CStr::from_bytes_until_nul(&src_buf) { Ok(c) => match c.to_str() { Ok(s) => s, Err(_) => return errno::Errno::EINVAL as u64 }, Err(_) => return errno::Errno::EINVAL as u64 };
    let tgt_str = match core::ffi::CStr::from_bytes_until_nul(&tgt_buf) { Ok(c) => match c.to_str() { Ok(s) => s, Err(_) => return errno::Errno::EINVAL as u64 }, Err(_) => return errno::Errno::EINVAL as u64 };
    let fs_str = match core::ffi::CStr::from_bytes_until_nul(&fs_buf) { Ok(c) => match c.to_str() { Ok(s) => s, Err(_) => return errno::Errno::EINVAL as u64 }, Err(_) => return errno::Errno::EINVAL as u64 };
    let devices = crate::drivers::block::BLOCK_DEVICES.lock();
    let fs: Option<alloc::sync::Arc<dyn crate::vfs::FileSystem>> = match fs_str {
        "tmpfs" => Some(alloc::sync::Arc::new(crate::vfs::ramfs::Tmpfs::new())),
        "devfs" => Some(alloc::sync::Arc::new(crate::vfs::devfs::DevFs::new())),
        "ctlfs" => Some(alloc::sync::Arc::new(crate::vfs::ctlfs::CtlFs::new())),
        "ext2" => { let mut found = None; for dev in devices.iter() { if let Ok(ext2fs) = crate::vfs::ext2::mount(dev.clone()) { found = Some(ext2fs as alloc::sync::Arc<dyn crate::vfs::FileSystem>); break; } } found }
        "skyfs" => { let mut found = None; for dev in devices.iter() { if let Ok(skyfs) = crate::vfs::skyfs::SkyFSHandle::mount(dev.clone()) { found = Some(skyfs as alloc::sync::Arc<dyn crate::vfs::FileSystem>); break; } } found }
        _ => None,
    };
    drop(devices);
    let fs = match fs { Some(f) => f, None => return errno::Errno::ENODEV as u64 };
    let mut vfs = crate::vfs::VFS.lock();
    vfs.mount(tgt_str, fs);
    0
}

pub fn sys_umount2(target: *const u8, _flags: u64) -> u64 {
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_ADMIN) { audit_log("CAP_SYS_ADMIN", "umount DENIED"); return errno::Errno::EPERM as u64; }
    let path_str = match unsafe { user_access::read_user_string(target, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    match VFS.lock().umount(&path_str) { Ok(_) => 0, Err(_) => errno::Errno::EINVAL as u64 }
}

pub fn sys_mkfs(fstype: *const u8, device: u64) -> u64 {
    if get_current_euid() != 0 && !has_capability(CAP_SYS_ADMIN) { audit_log("CAP_SYS_ADMIN", "mkfs DENIED"); return errno::Errno::EPERM as u64; }
    let fs_type = match unsafe { user_access::read_user_string(fstype, 32) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let blk = crate::drivers::block::BLOCK_DEVICES.lock();
    let dev = match blk.get(device as usize) { Some(d) => d.clone(), None => return errno::Errno::ENODEV as u64 };
    drop(blk);
    match fs_type.as_str() {
        "skyfs" => { if crate::vfs::skyfs::SkyFSHandle::format(dev).is_ok() { 0 } else { errno::Errno::EIO as u64 } }
        _ => errno::Errno::EINVAL as u64,
    }
}

pub fn sys_swapon(path_ptr: *const u8, _swap_flags: i32) -> u64 {
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_ADMIN) { audit_log("CAP_SYS_ADMIN", "swapon DENIED"); return errno::Errno::EPERM as u64; }
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let vfs = crate::vfs::VFS.lock();
    let node = match vfs.resolve_path(&path_str) { Some(n) => n, None => return errno::Errno::ENOENT as u64 };
    drop(vfs);
    let dev_size = match node.stat() { Ok(s) => { if s.st_size <= 0 { return errno::Errno::ENODEV as u64; } s.st_size as usize }, Err(_) => return errno::Errno::ENODEV as u64 };
    let slot_count = dev_size / 4096;
    if slot_count == 0 { return errno::Errno::ENODEV as u64; }
    { let existing = crate::memory::swap::SWAP_DEVICES.lock(); for dev in existing.iter() { if dev.dev_node.name() == node.name() { return errno::Errno::EBUSY as u64; } } }
    let sig_bytes = crate::memory::swap::SWAP_SIGNATURE.to_le_bytes();
    if node.write(&sig_bytes).is_err() { return errno::Errno::EIO as u64; }
    let mut slots = alloc::vec![];
    slots.resize_with(slot_count, || crate::memory::swap::SwapSlot::new());
    if let Some(slot) = slots.get_mut(0) { slot.mark_used(); }
    let swap_dev = crate::memory::swap::SwapDevice { device_path: path_str.clone(), dev_node: node, slot_count, slots, page_count: core::sync::atomic::AtomicU64::new(1) };
    crate::memory::swap::SWAP_DEVICES.lock().push(swap_dev);
    crate::println!("[SWAP] swapon: {} slots={}", path_str, slot_count);
    0
}

pub fn sys_swapoff(path_ptr: *const u8) -> u64 {
    let euid = get_current_euid();
    if euid != 0 && !has_capability(CAP_SYS_ADMIN) { audit_log("CAP_SYS_ADMIN", "swapoff DENIED"); return errno::Errno::EPERM as u64; }
    let path_str = match unsafe { user_access::read_user_string(path_ptr, 256) } { Ok(s) => s, Err(_) => return errno::Errno::EFAULT as u64 };
    let mut devices = crate::memory::swap::SWAP_DEVICES.lock();
    match devices.iter().position(|d| d.device_path == path_str) {
        Some(idx) => { devices.remove(idx); crate::println!("[SWAP] swapoff: {}", path_str); 0 }
        None => errno::Errno::EINVAL as u64,
    }
}

pub fn sys_sync() -> u64 {
    let devices = crate::drivers::block::BLOCK_DEVICES.lock();
    for dev in devices.iter() { dev.lock().sync(); }
    0
}
