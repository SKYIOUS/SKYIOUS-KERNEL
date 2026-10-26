//! Executable contract tests for POSIX message queues.
use crate::memory::buddy::BuddyFrameAllocator;
use crate::memory::paging::AddressSpace;
use crate::selftest;
use crate::syscalls::fs_open::sys_close;
use crate::task::process::{Process, CURRENT_PROCESS};
use alloc::sync::Arc;

pub const USER_PAGE_VA: u64 = 0x0000_2000_0000;

/// SAFETY: `va..va + bytes.len()` must be inside a mapped userspace page.
pub fn user_write(va: u64, bytes: &[u8]) -> Result<(), &'static str> {
    unsafe { crate::syscalls::user_access::copy_to_user(va as *mut u8, bytes) }
        .map_err(|_| "copy_to_user failed")
}

/// SAFETY: `va..va + out.len()` must be inside a mapped userspace page.
pub fn user_read(va: u64, out: &mut [u8]) -> Result<(), &'static str> {
    unsafe { crate::syscalls::user_access::copy_from_user(out, va as *const u8) }
        .map_err(|_| "copy_from_user failed")
}

/// SAFETY: `va..va + size_of::<T>()` must be inside a mapped page.
pub fn user_write_struct<T>(va: u64, value: &T) -> Result<(), &'static str> {
    let bytes = unsafe {
        core::slice::from_raw_parts(value as *const T as *const u8, core::mem::size_of::<T>())
    };
    user_write(va, bytes)
}

/// SAFETY: `va..va + size_of::<T>()` must be inside a mapped page.
pub fn user_read_struct<T>(va: u64, out: &mut T) -> Result<(), &'static str> {
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(out as *mut T as *mut u8, core::mem::size_of::<T>())
    };
    user_read(va, bytes)
}

/// Runs `f` with a process whose address space has one mapped user page.
pub fn with_user_page<T>(f: impl FnOnce(u64) -> T) -> Result<T, &'static str> {
    use x86_64::registers::control::Cr3;
    use x86_64::structures::paging::{Page, Size4KiB};
    use x86_64::VirtAddr;

    let previous_process = CURRENT_PROCESS.lock().clone();
    let (previous_pml4, previous_flags) = Cr3::read();

    let mut frame_allocator = BuddyFrameAllocator;
    let mut address_space =
        AddressSpace::new(&mut frame_allocator).ok_or("AddressSpace::new failed")?;
    let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(USER_PAGE_VA));
    if address_space
        .map_user_page(page, &mut frame_allocator)
        .is_none()
    {
        address_space.destroy();
        return Err("map_user_page failed");
    }

    let mut process = Arc::new(Process::new(Process::next_id(), None, address_space));
    // SAFETY: `address_space` is live, uniquely owned here, and its PML4 is
    // fully built; activating it for the closure is the point of this helper.
    unsafe { process.address_space.activate() };
    *CURRENT_PROCESS.lock() = Some(process.clone());

    let result = f(USER_PAGE_VA);

    // SAFETY: restores the CR3 value read at entry, which was valid then and
    // is still the kernel address space this test must return to.
    unsafe { Cr3::write(previous_pml4, previous_flags) };
    *CURRENT_PROCESS.lock() = previous_process;
    if let Some(proc_mut) = Arc::get_mut(&mut process) {
        let leaf = proc_mut.address_space.unmap_user_page(page);
        proc_mut.address_space.destroy();
        if let Some(frame) = leaf {
            crate::memory::buddy::BUDDY_ALLOCATOR
                .lock()
                .deallocate_frame(frame);
        }
    }
    Ok(result)
}

pub fn test_mq_open_installs_fd_and_round_trips() -> Result<(), &'static str> {
    use crate::ipc::mqueue::queue::MqAttr;
    use crate::ipc::mqueue::{mq_getsetattr, mq_open, mq_receive, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;
    use crate::syscalls::fs_open::{sys_dup, sys_fcntl};

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 64;
        const OUT: u64 = 128;
        const ATTR: u64 = 256;
        const O_RDONLY: i32 = 0;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;
        const F_GETFD: i32 = 1;

        user_write(base + NAME, b"selftest_mq\0")?;
        user_write(base + MSG, b"hello")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }
        // mq_overview(7): a queue descriptor is always close-on-exec.
        if sys_fcntl(fd, F_GETFD, 0) != 1 {
            return Err("mq_open did not set FD_CLOEXEC");
        }

        if mq_send(fd as i32, (base + MSG) as *const u8, 5, 5) != 0 {
            return Err("mq_send failed");
        }
        let received = mq_receive(
            fd as i32,
            (base + OUT) as *mut u8,
            8192,
            core::ptr::null_mut(),
        );
        if received != 5 {
            return Err("mq_receive returned the wrong length");
        }

        let mut got = [0u8; 5];
        // SAFETY: `OUT..OUT+5` is inside the mapped page at `base`.
        user_read(base + OUT, &mut got)?;
        if &got != b"hello" {
            return Err("mq_receive payload mismatch");
        }

        // A queue descriptor is an fd: dup(2) works and shares the queue.
        let dup_fd = sys_dup(fd);
        if (dup_fd as i64) < 0 {
            return Err("dup of a queue descriptor failed");
        }
        if mq_send(dup_fd as i32, (base + MSG) as *const u8, 5, 5) != 0 {
            return Err("mq_send on the dup'd descriptor failed");
        }
        if mq_receive(
            fd as i32,
            (base + OUT) as *mut u8,
            8192,
            core::ptr::null_mut(),
        ) != 5
        {
            return Err("descripors do not share the queue");
        }
        if sys_close(dup_fd) != 0 {
            return Err("close of the dup'd descriptor failed");
        }

        // Unlinking drops the name, not the descriptor.
        if mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("mq_unlink failed");
        }
        if mq_send(fd as i32, (base + MSG) as *const u8, 5, 5) != 0 {
            return Err("mq_send after mq_unlink failed");
        }
        if mq_receive(
            fd as i32,
            (base + OUT) as *mut u8,
            8192,
            core::ptr::null_mut(),
        ) != 5
        {
            return Err("mq_receive after mq_unlink failed");
        }

        // The name is gone, so O_CREAT builds a new, empty queue.
        let fresh = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fresh as i64) < 0 {
            return Err("mq_open with O_CREAT after unlink failed");
        }
        if mq_getsetattr(fresh as i32, core::ptr::null(), (base + ATTR) as *mut u8) != 0 {
            return Err("mq_getsetattr failed");
        }
        let mut attr = MqAttr::default();
        user_read_struct(base + ATTR, &mut attr)?;
        if attr.mq_curmsgs != 0 {
            return Err("the recreated queue is not empty");
        }
        if sys_close(fresh) != 0 {
            return Err("close of the fresh descriptor failed");
        }
        if mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("second mq_unlink failed");
        }

        // With the name gone and no O_CREAT, the open must fail.
        if mq_open(
            (base + NAME) as *const u8,
            O_RDONLY,
            0,
            core::ptr::null_mut(),
        ) != Errno::ENOENT as u64
        {
            return Err("mq_open of an unlinked queue did not return ENOENT");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        if mq_send(fd as i32, (base + MSG) as *const u8, 5, 5) != Errno::EBADF as u64 {
            return Err("mq_send after close did not return EBADF");
        }
        Ok(())
    })?
}

/// Test mq_open with O_CREAT|O_EXCL returns EEXIST when queue exists.
pub fn test_mq_open_excl_existing() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;
        const O_EXCL: i32 = 0x80;

        user_write(base + NAME, b"test_excl\0")?;

        // First open succeeds
        let fd1 = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd1 as i64) < 0 {
            return Err("first mq_open failed");
        }

        // Second open with O_EXCL should fail with EEXIST
        let fd2 = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_EXCL | O_RDWR,
            0,
            core::ptr::null_mut(),
        );
        if fd2 != Errno::EEXIST as u64 {
            return Err("mq_open with O_EXCL on existing queue did not return EEXIST");
        }

        if sys_close(fd1) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }

        // The name is free again: O_EXCL must now create.
        let fd3 = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_EXCL | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd3 as i64) < 0 {
            return Err("mq_open with O_EXCL after unlink did not create");
        }
        if sys_close(fd3) != 0 {
            return Err("close of the recreated queue failed");
        }
        if mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("second mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_open without O_CREAT on missing queue returns ENOENT, and that
/// malformed names are rejected as Linux documents.
pub fn test_mq_open_missing_no_creat() -> Result<(), &'static str> {
    use crate::ipc::mqueue::mq_open;
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"nonexistent_mq\0")?;

        let fd = mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut());
        if fd != Errno::ENOENT as u64 {
            return Err("mq_open of missing queue without O_CREAT did not return ENOENT");
        }

        // The syscall ABI takes the name without the leading slash, because
        // glibc/musl strip the POSIX `/` before entering the kernel. Linux's
        // lookup_one_len rejects an empty name, `.`, `..` and any slash with
        // EACCES.
        user_write(base + NAME, b"\0")?;
        if mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut())
            != Errno::EACCES as u64
        {
            return Err("mq_open of an empty name did not return EACCES");
        }
        for bad in [
            &b".\0"[..],
            &b"..\0"[..],
            &b"/leading\0"[..],
            &b"two/slashes\0"[..],
        ] {
            user_write(base + NAME, bad)?;
            if mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut())
                != Errno::EACCES as u64
            {
                return Err("mq_open accepted a name Linux rejects with EACCES");
            }
        }

        // 256 name bytes exceed NAME_MAX and must not alias a shorter name.
        let long = [b'a'; 257];
        user_write(base + NAME, &long)?;
        if mq_open((base + NAME) as *const u8, O_RDWR, 0, core::ptr::null_mut())
            != Errno::ENAMETOOLONG as u64
        {
            return Err("mq_open with an overlong name did not return ENAMETOOLONG");
        }

        // 255 bytes is the largest accepted name, and it is usable.
        let mut max = [b'b'; 256];
        max[255] = 0;
        user_write(base + NAME, &max)?;
        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open rejected a NAME_MAX-length name");
        }
        if crate::ipc::mqueue::mq_unlink((base + NAME) as *const u8) != 0 {
            return Err("mq_unlink of a NAME_MAX-length name failed");
        }
        if sys_close(fd) != 0 {
            return Err("close of the NAME_MAX-length queue failed");
        }

        // (oflag & O_ACCMODE) == 3 is not a valid access mode.
        user_write(base + NAME, b"bad_accmode\0")?;
        if mq_open(
            (base + NAME) as *const u8,
            O_CREAT | 3,
            0o600,
            core::ptr::null_mut(),
        ) != Errno::EINVAL as u64
        {
            return Err("mq_open with access mode 3 did not return EINVAL");
        }
        Ok(())
    })?
}

/// Test mq_send with bad fd returns EBADF.
pub fn test_mq_send_bad_fd() -> Result<(), &'static str> {
    use crate::ipc::mqueue::mq_send;
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const MSG: u64 = 0;
        user_write(base + MSG, b"hello")?;

        let rc = mq_send(999, (base + MSG) as *const u8, 5, 5);
        if rc != Errno::EBADF as u64 {
            return Err("mq_send with bad fd did not return EBADF");
        }
        Ok(())
    })?
}

/// Test mq_receive with bad fd returns EBADF.
pub fn test_mq_receive_bad_fd() -> Result<(), &'static str> {
    use crate::ipc::mqueue::mq_receive;
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const OUT: u64 = 0;
        let rc = mq_receive(999, (base + OUT) as *mut u8, 64, core::ptr::null_mut());
        if rc != Errno::EBADF as u64 {
            return Err("mq_receive with bad fd did not return EBADF");
        }
        Ok(())
    })?
}

/// Test mq_send with message too large returns EMSGSIZE, and that the open
/// direction is enforced.
pub fn test_mq_send_msgsize() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 64;
        const OUT: u64 = 128;
        const O_RDONLY: i32 = 0;
        const O_WRONLY: i32 = 1;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_msgsize\0")?;
        user_write(base + MSG, b"hello")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        // Declared length is rejected before any userspace copy is attempted.
        let rc = mq_send(fd as i32, (base + MSG) as *const u8, 10000, 5);
        if rc != Errno::EMSGSIZE as u64 {
            return Err("mq_send with oversized message did not return EMSGSIZE");
        }

        // A descriptor opened O_RDONLY cannot send: EBADF, not EACCES.
        let ro = mq_open(
            (base + NAME) as *const u8,
            O_RDONLY,
            0,
            core::ptr::null_mut(),
        );
        if (ro as i64) < 0 {
            return Err("read-only open failed");
        }
        if mq_send(ro as i32, (base + MSG) as *const u8, 5, 5) != Errno::EBADF as u64 {
            return Err("mq_send on a read-only descriptor did not return EBADF");
        }

        // ...and a write-only descriptor cannot receive.
        let wo = mq_open(
            (base + NAME) as *const u8,
            O_WRONLY,
            0,
            core::ptr::null_mut(),
        );
        if (wo as i64) < 0 {
            return Err("write-only open failed");
        }
        if crate::ipc::mqueue::mq_receive(
            wo as i32,
            (base + OUT) as *mut u8,
            8192,
            core::ptr::null_mut(),
        ) != Errno::EBADF as u64
        {
            return Err("mq_receive on a write-only descriptor did not return EBADF");
        }
        if sys_close(ro) != 0 || sys_close(wo) != 0 {
            return Err("closing the extra descriptors failed");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_receive with buffer too small returns EMSGSIZE.
pub fn test_mq_receive_msgsize() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_receive, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 64;
        const OUT: u64 = 128;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_rx_msgsize\0")?;
        user_write(base + MSG, b"hello world")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        let rc = mq_send(fd as i32, (base + MSG) as *const u8, 11, 5);
        if rc != 0 {
            return Err("mq_send failed");
        }

        // Receive with buffer smaller than message
        let rc = mq_receive(fd as i32, (base + OUT) as *mut u8, 5, core::ptr::null_mut());
        if rc != Errno::EMSGSIZE as u64 {
            return Err("mq_receive with small buffer did not return EMSGSIZE");
        }

        // The rejected receive must not consume the message.
        if mq_receive(
            fd as i32,
            (base + OUT) as *mut u8,
            8192,
            core::ptr::null_mut(),
        ) != 11
        {
            return Err("mq_receive after EMSGSIZE lost the message");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_open with invalid name pointer returns EFAULT.
pub fn test_mq_open_bad_name_ptr() -> Result<(), &'static str> {
    use crate::ipc::mqueue::mq_open;
    use crate::syscalls::errno::Errno;

    let rc = mq_open(core::ptr::null(), 0, 0, core::ptr::null_mut());
    if rc != Errno::EFAULT as u64 {
        return Err("mq_open with null name did not return EFAULT");
    }

    // Kernel address should also return EFAULT
    let kernel_ptr = 0xFFFFFFFF80000000u64 as *const u8;
    let rc = mq_open(kernel_ptr, 0, 0, core::ptr::null_mut());
    if rc != Errno::EFAULT as u64 {
        return Err("mq_open with kernel address did not return EFAULT");
    }
    Ok(())
}

/// Test mq_send with invalid msg_ptr returns EFAULT.
pub fn test_mq_send_bad_msg_ptr() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_send_fault\0")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        // Null msg_ptr
        let rc = mq_send(fd as i32, core::ptr::null(), 5, 5);
        if rc != Errno::EFAULT as u64 {
            return Err("mq_send with null msg_ptr did not return EFAULT");
        }

        // Kernel address
        let kernel_ptr = 0xFFFFFFFF80000000u64 as *const u8;
        let rc = mq_send(fd as i32, kernel_ptr, 5, 5);
        if rc != Errno::EFAULT as u64 {
            return Err("mq_send with kernel address did not return EFAULT");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_receive with invalid msg_ptr returns EFAULT.
pub fn test_mq_receive_bad_msg_ptr() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_receive, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 64;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_rx_fault\0")?;
        user_write(base + MSG, b"hello")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        let rc = mq_send(fd as i32, (base + MSG) as *const u8, 5, 5);
        if rc != 0 {
            return Err("mq_send failed");
        }

        // Null msg_ptr
        let rc = mq_receive(
            fd as i32,
            core::ptr::null_mut(),
            8192,
            core::ptr::null_mut(),
        );
        if rc != Errno::EFAULT as u64 {
            return Err("mq_receive with null msg_ptr did not return EFAULT");
        }

        // Kernel address
        // Re-send: the null-pointer attempt above already consumed the message.
        let rc = mq_send(fd as i32, (base + MSG) as *const u8, 5, 5);
        if rc != 0 {
            return Err("mq_send before kernel-address check failed");
        }
        let kernel_ptr = 0xFFFFFFFF80000000u64 as *mut u8;
        let rc = mq_receive(fd as i32, kernel_ptr, 8192, core::ptr::null_mut());
        if rc != Errno::EFAULT as u64 {
            return Err("mq_receive with kernel address did not return EFAULT");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_receive with invalid prio_ptr returns EFAULT.
pub fn test_mq_receive_bad_prio_ptr() -> Result<(), &'static str> {
    use crate::ipc::mqueue::{mq_open, mq_receive, mq_send, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const MSG: u64 = 64;
        const OUT: u64 = 128;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_prio_fault\0")?;
        user_write(base + MSG, b"hello")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }

        let rc = mq_send(fd as i32, (base + MSG) as *const u8, 5, 5);
        if rc != 0 {
            return Err("mq_send failed");
        }

        // Kernel address for prio_ptr
        let kernel_ptr = 0xFFFFFFFF80000000u64 as *mut u32;
        let rc = mq_receive(fd as i32, (base + OUT) as *mut u8, 8192, kernel_ptr);
        if rc != Errno::EFAULT as u64 {
            return Err("mq_receive with kernel prio_ptr did not return EFAULT");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_open with mq_attr on O_CREAT respects attributes and rejects
/// attributes outside the limits.
pub fn test_mq_open_with_attr() -> Result<(), &'static str> {
    use crate::ipc::mqueue::queue::MqAttr;
    use crate::ipc::mqueue::{mq_getsetattr, mq_open, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const ATTR: u64 = 64;
        const OUT_ATTR: u64 = 128;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_attr\0")?;

        // Zero limits, and limits above the hard caps, are EINVAL.
        for bad in [
            MqAttr {
                mq_maxmsg: 0,
                mq_msgsize: 4096,
                ..MqAttr::default_attrs()
            },
            MqAttr {
                mq_maxmsg: 10,
                mq_msgsize: 0,
                ..MqAttr::default_attrs()
            },
            MqAttr {
                mq_maxmsg: 65537,
                mq_msgsize: 4096,
                ..MqAttr::default_attrs()
            },
            MqAttr {
                mq_maxmsg: 10,
                mq_msgsize: 16 * 1024 * 1024 + 1,
                ..MqAttr::default_attrs()
            },
        ] {
            user_write_struct(base + ATTR, &bad)?;
            if mq_open(
                (base + NAME) as *const u8,
                O_CREAT | O_RDWR,
                0o600,
                (base + ATTR) as *mut u8,
            ) != Errno::EINVAL as u64
            {
                return Err("mq_open accepted an out-of-range mq_attr");
            }
        }

        // Write custom mq_attr
        let attr = MqAttr {
            mq_flags: 0,
            mq_maxmsg: 20,
            mq_msgsize: 4096,
            mq_curmsgs: 0,
            __reserved: [0; 4],
        };
        user_write_struct(base + ATTR, &attr)?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            (base + ATTR) as *mut u8,
        );
        if (fd as i64) < 0 {
            return Err("mq_open with attr failed");
        }

        // Get attributes back
        let rc = mq_getsetattr(fd as i32, core::ptr::null(), (base + OUT_ATTR) as *mut u8);
        if rc != 0 {
            return Err("mq_getsetattr failed");
        }

        let mut got_attr = MqAttr::default();
        user_read_struct(base + OUT_ATTR, &mut got_attr)?;

        if got_attr.mq_maxmsg != 20 {
            return Err("mq_maxmsg not respected");
        }
        if got_attr.mq_msgsize != 4096 {
            return Err("mq_msgsize not respected");
        }

        if sys_close(fd) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

/// Test mq_getsetattr round-trip, including per-description O_NONBLOCK.
pub fn test_mq_getsetattr() -> Result<(), &'static str> {
    use crate::ipc::mqueue::queue::MqAttr;
    use crate::ipc::mqueue::{mq_getsetattr, mq_open, mq_receive, mq_unlink};
    use crate::syscalls::errno::Errno;

    with_user_page(|base| {
        const NAME: u64 = 0;
        const ATTR: u64 = 64;
        const OUT_ATTR: u64 = 128;
        const OUT: u64 = 256;
        const O_RDONLY: i32 = 0;
        const O_RDWR: i32 = 2;
        const O_CREAT: i32 = 0x40;

        user_write(base + NAME, b"test_setattr\0")?;

        let fd = mq_open(
            (base + NAME) as *const u8,
            O_CREAT | O_RDWR,
            0o600,
            core::ptr::null_mut(),
        );
        if (fd as i64) < 0 {
            return Err("mq_open failed");
        }
        // A second descriptor on the same queue, blocking by default.
        let other = mq_open(
            (base + NAME) as *const u8,
            O_RDONLY,
            0,
            core::ptr::null_mut(),
        );
        if (other as i64) < 0 {
            return Err("second mq_open failed");
        }

        // Get initial attributes
        let rc = mq_getsetattr(fd as i32, core::ptr::null(), (base + OUT_ATTR) as *mut u8);
        if rc != 0 {
            return Err("mq_getsetattr failed");
        }

        // Set new flags (O_NONBLOCK)
        let new_attr = MqAttr {
            mq_flags: 0x800, // O_NONBLOCK
            mq_maxmsg: 10,
            mq_msgsize: 8192,
            mq_curmsgs: 0,
            __reserved: [0; 4],
        };
        user_write_struct(base + ATTR, &new_attr)?;

        let rc = mq_getsetattr(
            fd as i32,
            (base + ATTR) as *const u8,
            (base + OUT_ATTR) as *mut u8,
        );
        if rc != 0 {
            return Err("mq_setattr failed");
        }

        // Third argument is the previous attribute set (flags were 0).
        let mut old_attr = MqAttr::default();
        user_read_struct(base + OUT_ATTR, &mut old_attr)?;
        if old_attr.mq_flags != 0 {
            return Err("mq_setattr did not report the previous flags");
        }

        // The new flags are observable through mq_getattr.
        let rc = mq_getsetattr(fd as i32, core::ptr::null(), (base + OUT_ATTR) as *mut u8);
        if rc != 0 {
            return Err("mq_getattr after mq_setattr failed");
        }
        let mut got_attr = MqAttr::default();
        user_read_struct(base + OUT_ATTR, &mut got_attr)?;
        if got_attr.mq_flags != 0x800 {
            return Err("mq_setattr did not change flags");
        }

        // fcntl(2) observes and changes the same file status flags, as on
        // Linux where mq_setattr is F_SETFL on the descriptor.
        use crate::syscalls::fs_open::sys_fcntl;
        const F_GETFL: i32 = 3;
        const F_SETFL: i32 = 4;
        if sys_fcntl(fd, F_GETFL, 0) & 0x800 == 0 {
            return Err("F_GETFL did not report O_NONBLOCK set by mq_setattr");
        }
        if sys_fcntl(fd, F_SETFL, 0) != 0 || sys_fcntl(fd, F_GETFL, 0) & 0x800 != 0 {
            return Err("F_SETFL could not clear O_NONBLOCK on a queue descriptor");
        }
        if sys_fcntl(fd, F_SETFL, 0x800) != 0 {
            return Err("F_SETFL could not restore O_NONBLOCK");
        }

        // O_NONBLOCK belongs to one description: the other one is unchanged.
        let rc = mq_getsetattr(
            other as i32,
            core::ptr::null(),
            (base + OUT_ATTR) as *mut u8,
        );
        if rc != 0 {
            return Err("mq_getattr on the second descriptor failed");
        }
        user_read_struct(base + OUT_ATTR, &mut got_attr)?;
        if got_attr.mq_flags != 0 {
            return Err("O_NONBLOCK leaked to another descriptor");
        }

        // O_NONBLOCK is per description, so an empty queue returns EAGAIN
        // instead of blocking.
        if mq_receive(
            fd as i32,
            (base + OUT) as *mut u8,
            8192,
            core::ptr::null_mut(),
        ) != Errno::EAGAIN as u64
        {
            return Err("non-blocking receive on an empty queue did not return EAGAIN");
        }

        // Only O_NONBLOCK may be set through mq_setattr.
        let bad_attr = MqAttr {
            mq_flags: 0x1000,
            ..MqAttr::default_attrs()
        };
        user_write_struct(base + ATTR, &bad_attr)?;
        if mq_getsetattr(fd as i32, (base + ATTR) as *const u8, core::ptr::null_mut())
            != Errno::EINVAL as u64
        {
            return Err("mq_setattr accepted an unknown flag");
        }

        if sys_close(fd) != 0 || sys_close(other) != 0 {
            return Err("close failed");
        }
        let rc = mq_unlink((base + NAME) as *const u8);
        if rc != 0 {
            return Err("mq_unlink failed");
        }
        Ok(())
    })?
}

pub fn register() {
    selftest::register(
        "contract:mq_open_installs_fd_and_round_trips",
        test_mq_open_installs_fd_and_round_trips,
    );
    selftest::register("contract:mq_open_excl_existing", test_mq_open_excl_existing);
    selftest::register(
        "contract:mq_open_missing_no_creat",
        test_mq_open_missing_no_creat,
    );
    selftest::register("contract:mq_send_bad_fd", test_mq_send_bad_fd);
    selftest::register("contract:mq_receive_bad_fd", test_mq_receive_bad_fd);
    selftest::register("contract:mq_send_msgsize", test_mq_send_msgsize);
    selftest::register("contract:mq_receive_msgsize", test_mq_receive_msgsize);
    selftest::register("contract:mq_open_bad_name_ptr", test_mq_open_bad_name_ptr);
    selftest::register("contract:mq_send_bad_msg_ptr", test_mq_send_bad_msg_ptr);
    selftest::register(
        "contract:mq_receive_bad_msg_ptr",
        test_mq_receive_bad_msg_ptr,
    );
    selftest::register(
        "contract:mq_receive_bad_prio_ptr",
        test_mq_receive_bad_prio_ptr,
    );
    selftest::register("contract:mq_open_with_attr", test_mq_open_with_attr);
    selftest::register("contract:mq_getsetattr", test_mq_getsetattr);
}
