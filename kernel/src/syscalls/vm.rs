#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! VM management syscalls: create, destroy, start, stop, resume,
//! load_kernel, get_info, set_memory, inject_irq.
//! Extracted from misc.rs to keep each module focused.

use super::errno;
use super::*;

#[cfg(feature = "hypervisor")]
pub fn sys_vm_create(name_ptr: *const u8, mem_mb: u64) -> u64 {
    use crate::syscalls::user_access;
    let mut name_buf = [0u8; 64];
    if unsafe { user_access::copy_from_user(&mut name_buf, name_ptr).is_err() } {
        return errno::Errno::EFAULT as u64;
    }
    let name_end = name_buf.iter().position(|&b| b == 0).unwrap_or(64);
    let name = core::str::from_utf8(&name_buf[..name_end]).unwrap_or("guest");
    let mem_size = (mem_mb as usize) * 1024 * 1024;
    match crate::hypervisor::create_guest(name, crate::hypervisor::OsType::BareMetal { entry: 0 }, mem_size) {
        Some(id) => id,
        None => errno::Errno::ENOMEM as u64,
    }
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_destroy(guest_id: u64) -> u64 {
    if !crate::hypervisor::HYPERVISOR_ENABLED.load(core::sync::atomic::Ordering::Relaxed) {
        return errno::Errno::ENODEV as u64;
    }
    if crate::hypervisor::destroy_guest(guest_id) { 0 } else { errno::Errno::ENOENT as u64 }
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_start(guest_id: u64) -> u64 {
    let mut hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_mut() {
        Some(hv) => hv,
        None => return errno::Errno::ENODEV as u64,
    };
    match hv.guests.get_mut(&guest_id) {
        Some(guest) => { guest.state = crate::hypervisor::VmState::Running; 0 }
        None => errno::Errno::ENOENT as u64,
    }
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_stop(guest_id: u64) -> u64 {
    let mut hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_mut() {
        Some(hv) => hv,
        None => return errno::Errno::ENODEV as u64,
    };
    match hv.guests.get_mut(&guest_id) {
        Some(guest) => {
            guest.state = crate::hypervisor::VmState::Stopped;
            for vcpu in &mut guest.vcpus {
                vcpu.state = crate::hypervisor::vcpu::VcpuState::Stopped;
            }
            0
        }
        None => errno::Errno::ENOENT as u64,
    }
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_resume(guest_id: u64) -> u64 {
    let mut hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_mut() {
        Some(hv) => hv,
        None => return errno::Errno::ENODEV as u64,
    };
    match hv.guests.get_mut(&guest_id) {
        Some(guest) => {
            if guest.state == crate::hypervisor::VmState::Paused {
                guest.state = crate::hypervisor::VmState::Running;
                0
            } else {
                errno::Errno::EINVAL as u64
            }
        }
        None => errno::Errno::ENOENT as u64,
    }
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_load_kernel(_guest_id: u64, path_ptr: *const u8) -> u64 {
    use crate::syscalls::user_access;
    let mut path_buf = [0u8; 256];
    if unsafe { user_access::copy_from_user(&mut path_buf, path_ptr).is_err() } {
        return errno::Errno::EFAULT as u64;
    }
    let _path = core::str::from_utf8(&path_buf[..]).unwrap_or("").trim_end_matches(char::from(0));
    errno::Errno::ENOSYS as u64
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_get_info(guest_id: u64, buf: *mut u8, len: usize) -> u64 {
    use crate::syscalls::user_access;
    let hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_ref() {
        Some(hv) => hv,
        None => return errno::Errno::ENODEV as u64,
    };
    let guest = match hv.guests.get(&guest_id) {
        Some(g) => g,
        None => return errno::Errno::ENOENT as u64,
    };
    let info = alloc::format!("{} {} {}", guest.name, guest.vcpus.len(),
        match guest.state {
            crate::hypervisor::VmState::Created => "created",
            crate::hypervisor::VmState::Running => "running",
            crate::hypervisor::VmState::Paused => "paused",
            crate::hypervisor::VmState::Stopped => "stopped",
            crate::hypervisor::VmState::Crashed(_) => "crashed",
        },
    );
    let bytes = info.as_bytes();
    let copy_len = bytes.len().min(len);
    if copy_len > 0 {
        if unsafe { user_access::copy_to_user(buf, &bytes[..copy_len]) }.is_err() {
            return errno::Errno::EFAULT as u64;
        }
    }
    copy_len as u64
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_set_memory(_guest_id: u64, _addr: u64, _size: u64) -> u64 {
    errno::Errno::ENOSYS as u64
}

#[cfg(feature = "hypervisor")]
pub fn sys_vm_inject_irq(guest_id: u64, vector: u8) -> u64 {
    let mut hv_lock = crate::hypervisor::HYPERVISOR.lock();
    let hv = match hv_lock.as_mut() {
        Some(hv) => hv,
        None => return errno::Errno::ENODEV as u64,
    };
    match hv.guests.get_mut(&guest_id) {
        Some(guest) => {
            if let Some(vcpu) = guest.vcpus.first_mut() {
                let vcpu_ref: &mut crate::hypervisor::vcpu::Vcpu = vcpu;
                if vcpu_ref.inject_interrupt(vector) { 0 } else { errno::Errno::EIO as u64 }
            } else {
                errno::Errno::ENOENT as u64
            }
        }
        None => errno::Errno::ENOENT as u64,
    }
}

