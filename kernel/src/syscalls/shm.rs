use spin::Mutex;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use crate::syscalls::errno;
use crate::vfs::FileSystem;
use crate::syscalls::user_access;
use crate::syscalls::get_current_process;
use crate::syscalls::get_current_euid;
use crate::syscalls::has_capability;
use crate::syscalls::CAP_SYS_ADMIN;
use crate::task::process::{CURRENT_PROCESS, Vma, FileDescriptor};
use crate::memory::buddy::BuddyFrameAllocator;
use x86_64::structures::paging::{Page, Size4KiB, Mapper, FrameAllocator, PageTableFlags, PhysFrame};
use x86_64::PhysAddr;
use x86_64::VirtAddr;

// ─── Data structures ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ShmSegment {
    pub id: u32,
    pub key: i32,
    pub size: usize,
    pub pages: Vec<u64>,     // physical page addresses
    pub perms: u16,          // permission bits (mode & 0x1FF)
    pub uid: u32,
    pub gid: u32,
    pub cpid: u64,
    pub lpid: u64,
    pub nattch: u32,
    pub atime: u64,
    pub dtime: u64,
    pub ctime: u64,
    pub deleted: bool,
}

#[repr(C)]
pub struct shmid_ds {
    pub shm_perm: IpcPerm,
    pub shm_segsz: usize,
    pub shm_lpid: u64,
    pub shm_cpid: u64,
    pub shm_nattch: u32,
    pub shm_atime: u64,
    pub shm_dtime: u64,
    pub shm_ctime: u64,
    pub shm_internal: [u8; 16],
}

#[repr(C)]
pub struct IpcPerm {
    pub key: i32,
    pub uid: u32,
    pub gid: u32,
    pub cuid: u32,
    pub cgid: u32,
    pub mode: u16,
    pub seq: u16,
}

lazy_static::lazy_static! {
    pub static ref SHM_SEGMENTS: Mutex<BTreeMap<u32, ShmSegment>> = Mutex::new(BTreeMap::new());
    pub static ref SHM_KEY_MAP: Mutex<BTreeMap<i32, u32>> = Mutex::new(BTreeMap::new());
}

pub static NEXT_SHM_ID: AtomicU32 = AtomicU32::new(1);

const SHMMAX: usize = 32 * 1024 * 1024; // 32 MB
const SHMMIN: usize = 1;
const SHMMNI: usize = 4096;

const IPC_PRIVATE: i32 = 0;
const IPC_CREAT: i32 = 0x200;
const IPC_EXCL: i32 = 0x400;
const SHM_RDONLY: i32 = 0x1000;
const SHM_REMAP: i32 = 0x2000;

const IPC_RMID: i32 = 0;
const IPC_SET: i32 = 1;
const IPC_STAT: i32 = 2;
const IPC_INFO: i32 = 3;
const SHM_STAT: i32 = 13;
const SHM_STAT_ANY: i32 = 14;

// ─── Helpers ────────────────────────────────────────────────────────

fn current_pid() -> u64 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map(|p| p.id).unwrap_or(0)
}

fn current_gid() -> u32 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(0, |p| p.creds.lock().gid)
}

fn current_egid() -> u32 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(0, |p| p.creds.lock().egid)
}

fn now_ticks() -> u64 {
    crate::interrupts::get_ticks()
}

/// Allocate physical pages for a segment.
/// Returns the list of physical addresses or frees on failure.
fn alloc_segment_pages(num_pages: usize) -> Result<Vec<u64>, errno::Errno> {
    let mut frame_allocator = BuddyFrameAllocator;
    let mut pages = Vec::with_capacity(num_pages);
    for _ in 0..num_pages {
        match frame_allocator.allocate_frame() {
            Some(frame) => {
                pages.push(frame.start_address().as_u64());
            }
            None => {
                // SAFETY: freeing frames we just allocated; no aliasing
                for &paddr in &pages {
                    let f = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(paddr));
                    crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(f);
                }
                return Err(errno::Errno::ENOMEM);
            }
        }
    }
    Ok(pages)
}

/// Free physical pages for a segment.
fn free_segment_pages(pages: &[u64]) {
    for &paddr in pages {
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(paddr));
        crate::memory::buddy::BUDDY_ALLOCATOR.lock().deallocate_frame(frame);
        crate::memory::frame_info::decrement(frame.start_address());
    }
}

// ─── sys_shmget(29) ────────────────────────────────────────────────

pub fn sys_shmget(key: i32, size: usize, shmflg: i32) -> u64 {
    if size > SHMMAX {
        return errno::Errno::EINVAL as u64;
    }
    let size_aligned = (size + 4095) & !4095;
    if size_aligned == 0 {
        return errno::Errno::EINVAL as u64;
    }

    // Look up existing key
    if key != IPC_PRIVATE {
        let key_map = SHM_KEY_MAP.lock();
        if let Some(&shmid) = key_map.get(&key) {
            let segments = SHM_SEGMENTS.lock();
            if let Some(_seg) = segments.get(&shmid) {
                if (shmflg & IPC_EXCL) != 0 && (shmflg & IPC_CREAT) != 0 {
                    return errno::Errno::EEXIST as u64;
                }
                return shmid as u64;
            }
        }
    }

    // Must have IPC_CREAT to create
    if (shmflg & IPC_CREAT) == 0 {
        return errno::Errno::ENOENT as u64;
    }

    // Check segment count limit
    if SHM_SEGMENTS.lock().len() >= SHMMNI {
        return errno::Errno::ENOSPC as u64;
    }

    let num_pages = size_aligned / 4096;
    let pages = match alloc_segment_pages(num_pages) {
        Ok(p) => p,
        Err(e) => return e as u64,
    };

    let shmid = NEXT_SHM_ID.fetch_add(1, Ordering::Relaxed);
    let uid = get_current_euid();
    let gid = current_gid();
    let pid = current_pid();

    let segment = ShmSegment {
        id: shmid,
        key,
        size: size_aligned,
        pages,
        perms: (shmflg as u16) & 0x1FF,
        uid,
        gid,
        cpid: pid,
        lpid: 0,
        nattch: 0,
        atime: 0,
        dtime: 0,
        ctime: now_ticks(),
        deleted: false,
    };

    SHM_SEGMENTS.lock().insert(shmid, segment);
    if key != IPC_PRIVATE {
        SHM_KEY_MAP.lock().insert(key, shmid);
    }

    shmid as u64
}

// ─── sys_shmat(30) ─────────────────────────────────────────────────

pub fn sys_shmat(shmid: i32, shmaddr: *const u8, shmflg: i32) -> u64 {
    // Clone segment info while holding lock
    let (size, perms, seg_uid, seg_gid, pages) = {
        let segments = SHM_SEGMENTS.lock();
        let seg = match segments.get(&(shmid as u32)) {
            Some(s) => s,
            None => return -(errno::Errno::EINVAL as i64) as u64,
        };
        if seg.deleted {
            return -(errno::Errno::EIDRM as i64) as u64;
        }
        (seg.size, seg.perms, seg.uid, seg.gid, seg.pages.clone())
    };

    // Permission check
    let euid = get_current_euid();
    let egid = current_egid();
    let read_only = (shmflg & SHM_RDONLY) != 0;
    let perm_bits = if euid == seg_uid { (perms >> 6) & 7 }
                    else if egid == seg_gid { (perms >> 3) & 7 }
                    else { perms & 7 };
    // Read permission required for any attach
    if (perm_bits & 4) == 0 {
        return -(errno::Errno::EACCES as i64) as u64;
    }
    // Write permission required unless SHM_RDONLY
    if !read_only && (perm_bits & 2) == 0 {
        // ponytail: also check owner write bit; this matches Linux behaviour
        if euid != seg_uid || (perms & 0o200) == 0 {
            return -(errno::Errno::EACCES as i64) as u64;
        }
    }

    let process = match get_current_process() {
        Some(p) => p,
        None => return -(errno::Errno::ESRCH as i64) as u64,
    };

    // Choose attach address
    let len_aligned = size;
    let mmap_addr = if shmaddr.is_null() {
        const SHM_MIN: u64 = 0x4000_0000_0000;
        const SHM_MAX: u64 = 0x7F00_0000_0000;
        static SHM_NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(SHM_MIN);
        let addr = SHM_NEXT.fetch_add(len_aligned as u64, Ordering::Relaxed);
        let addr_aligned = addr & !0xFFF;
        if addr_aligned + len_aligned as u64 > SHM_MAX {
            SHM_NEXT.store(SHM_MIN, Ordering::Relaxed);
            SHM_MIN
        } else {
            addr_aligned
        }
    } else if (shmflg & SHM_REMAP) != 0 {
        shmaddr as u64
    } else {
        (shmaddr as u64) & !0xFFF
    };

    let mut page_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if !read_only {
        page_flags |= PageTableFlags::WRITABLE;
    }

    // Map pages into process address space
    let mut mapper = match unsafe { process.address_space.mapper() } {
        Some(m) => m,
        None => return -(errno::Errno::ENOMEM as i64) as u64,
    };
    let mut frame_allocator = BuddyFrameAllocator;

    for (i, &paddr) in pages.iter().enumerate() {
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(mmap_addr + (i as u64) * 4096));
        let frame = PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(paddr));
        unsafe {
            match mapper.map_to(page, frame, page_flags, &mut frame_allocator) {
                Ok(t) => t.flush(),
                Err(_) => {
                    // Unmap already-mapped pages on failure
                    for j in 0..i {
                        let p = Page::<Size4KiB>::containing_address(VirtAddr::new(mmap_addr + (j as u64) * 4096));
                        if let Ok((f, t)) = mapper.unmap(p) {
                            t.flush();
                            crate::memory::frame_info::decrement(f.start_address());
                        }
                    }
                    return -(errno::Errno::ENOMEM as i64) as u64;
                }
            }
        }
        crate::memory::frame_info::increment(frame.start_address());
    }

    // Add VMA
    process.add_vma(Vma {
        start: mmap_addr,
        end: mmap_addr + len_aligned as u64,
        flags: page_flags,
        _name: "shmat",
        file_handle: None,
        file_offset: 0,
        is_shared: true,
        shm_id: Some(shmid as u32),
    });

    // Update segment metadata
    {
        let mut segments = SHM_SEGMENTS.lock();
        if let Some(seg) = segments.get_mut(&(shmid as u32)) {
            seg.nattch += 1;
            seg.lpid = current_pid();
            seg.atime = now_ticks();
        }
    }

    mmap_addr
}

// ─── sys_shmdt(67) ─────────────────────────────────────────────────

pub fn sys_shmdt(shmaddr: *const u8) -> u64 {
    let addr = shmaddr as u64;
    if addr == 0 || (addr & 0xFFF) != 0 {
        return errno::Errno::EINVAL as u64;
    }

    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Find the VMA
    let vma = {
        let vmas = process.vmas.lock();
        match vmas.iter().find(|v| v.start == addr) {
            Some(v) => v.clone(),
            None => return errno::Errno::EINVAL as u64,
        }
    };

    let shm_id = match vma.shm_id {
        Some(id) => id,
        None => return errno::Errno::EINVAL as u64,
    };

    // Unmap pages
    let len = vma.end - vma.start;
    let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(addr));
    let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(addr + len - 1));

    let mut mapper = match unsafe { process.address_space.mapper() } {
        Some(m) => m,
        None => return errno::Errno::EINVAL as u64,
    };

    for page in Page::range_inclusive(start_page, end_page) {
        if let Ok((frame, t)) = mapper.unmap(page) {
            t.flush();
            crate::memory::frame_info::decrement(frame.start_address());
        }
    }

    // Remove VMA
    process.remove_vma_range(addr, addr + len);

    // Update segment and possibly free
    let mut segments = SHM_SEGMENTS.lock();
    if let Some(seg) = segments.get_mut(&shm_id) {
        seg.nattch = seg.nattch.saturating_sub(1);
        seg.dtime = now_ticks();
        seg.lpid = current_pid();
        if seg.nattch == 0 && seg.deleted {
            // ponytail: free segment pages eagerly
            let key = seg.key;
            let pages = core::mem::take(&mut seg.pages);
            let _ = seg;
            SHM_KEY_MAP.lock().remove(&key);
            free_segment_pages(&pages);
            segments.remove(&shm_id);
        }
    }

    0
}

// ─── sys_shmctl(31) ─────────────────────────────────────────────────

pub fn sys_shmctl(shmid: i32, cmd: i32, buf: *mut u8) -> u64 {
    match cmd {
        IPC_INFO => {
            // Return system limits as an array of u64s
            let info = [
                SHMMAX as u64,
                SHMMIN as u64,
                SHMMNI as u64,
                0, // shmmin (default 1)
                0, // shmmni (default 4096)
                0, // shmall (default)
            ];
            let bytes = unsafe {
                core::slice::from_raw_parts(info.as_ptr() as *const u8, info.len() * 8)
            };
            if unsafe { user_access::copy_to_user(buf, bytes) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            0
        }

        IPC_STAT | SHM_STAT | SHM_STAT_ANY => {
            let segments = SHM_SEGMENTS.lock();
            let seg_id = if cmd == IPC_STAT { shmid as u32 } else { shmid as u32 };
            let seg = match segments.get(&seg_id) {
                Some(s) => s,
                None => return errno::Errno::EINVAL as u64,
            };
            if cmd == IPC_STAT && seg.deleted {
                return errno::Errno::EIDRM as u64;
            }
            let ds = shmid_ds {
                shm_perm: IpcPerm {
                    key: seg.key,
                    uid: seg.uid,
                    gid: seg.gid,
                    cuid: seg.uid,
                    cgid: seg.gid,
                    mode: seg.perms,
                    seq: 0,
                },
                shm_segsz: seg.size,
                shm_lpid: seg.lpid,
                shm_cpid: seg.cpid,
                shm_nattch: seg.nattch,
                shm_atime: seg.atime,
                shm_dtime: seg.dtime,
                shm_ctime: seg.ctime,
                shm_internal: [0u8; 16],
            };
            let bytes = unsafe {
                core::slice::from_raw_parts(&ds as *const _ as *const u8, core::mem::size_of::<shmid_ds>())
            };
            if unsafe { user_access::copy_to_user(buf, bytes) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            if cmd == SHM_STAT { seg.id as u64 } else { 0 }
        }

        IPC_SET => {
            let mut ds: shmid_ds = unsafe { core::mem::zeroed() };
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(&mut ds as *mut _ as *mut u8, core::mem::size_of::<shmid_ds>())
            };
            if unsafe { user_access::copy_from_user(bytes, buf) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            let euid = get_current_euid();
            let mut segments = SHM_SEGMENTS.lock();
            let seg = match segments.get_mut(&(shmid as u32)) {
                Some(s) => s,
                None => return errno::Errno::EINVAL as u64,
            };
            if seg.deleted {
                return errno::Errno::EIDRM as u64;
            }
            // Must be root, creator, or have CAP_SYS_ADMIN
            if euid != 0 && euid != seg.uid && !has_capability(CAP_SYS_ADMIN) {
                return errno::Errno::EPERM as u64;
            }
            seg.uid = ds.shm_perm.uid;
            seg.gid = ds.shm_perm.gid;
            seg.perms = ds.shm_perm.mode;
            seg.ctime = now_ticks();
            0
        }

        IPC_RMID => {
            let euid = get_current_euid();
            let mut segments = SHM_SEGMENTS.lock();
            let seg = match segments.get_mut(&(shmid as u32)) {
                Some(s) => s,
                None => return errno::Errno::EINVAL as u64,
            };
            if seg.deleted {
                return errno::Errno::EIDRM as u64;
            }
            // Must be root, creator, or have CAP_SYS_ADMIN
            if euid != 0 && euid != seg.uid && !has_capability(CAP_SYS_ADMIN) {
                return errno::Errno::EPERM as u64;
            }
            seg.deleted = true;
            if seg.nattch == 0 {
                let key = seg.key;
                let pages = core::mem::take(&mut seg.pages);
                let _ = seg;
                SHM_KEY_MAP.lock().remove(&key);
                free_segment_pages(&pages);
                segments.remove(&(shmid as u32));
            }
            0
        }

        _ => errno::Errno::EINVAL as u64,
    }
}

// ─── sys_memfd_create(319) ─────────────────────────────────────────

pub fn sys_memfd_create(name_ptr: *const u8, flags: u32) -> u64 {
    // ponytail: MFD_HUGETLB not supported
    if (flags & 0x0004) != 0 {
        return errno::Errno::ENOSYS as u64;
    }

    let name = match unsafe { user_access::read_user_string(name_ptr, 256) } {
        Ok(s) => s,
        Err(_) => return errno::Errno::EFAULT as u64,
    };

    // Create a temporary ramfs file (not mounted in VFS — private to this fd)
    let tmpfs = crate::vfs::ramfs::Tmpfs::new();
    let root = match tmpfs.root() {
        Ok(r) => r,
        Err(_) => return errno::Errno::ENOMEM as u64,
    };
    let node = match root.create(&name) {
        Ok(n) => n,
        Err(_) => return errno::Errno::ENOMEM as u64,
    };
    let _ = node.chmod(0o600);

    let process = match get_current_process() {
        Some(p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    let fd_obj = FileDescriptor::File { node, offset: spin::Mutex::new(0) };
    let mut fd_table = process.fd_table.lock();
    let fd = {
        let mut slot = None;
        for (i, s) in fd_table.iter_mut().enumerate() {
            if s.is_none() {
                *s = Some(fd_obj.clone());
                slot = Some(i);
                break;
            }
        }
        match slot {
            Some(i) => i,
            None => {
                fd_table.push(Some(fd_obj));
                fd_table.len() - 1
            }
        }
    };

    // MFD_CLOEXEC
    if (flags & 0x0001) != 0 {
        let mut fd_flags = process.fd_flags.lock();
        if fd >= fd_flags.len() { fd_flags.resize(fd + 1, 0); }
        fd_flags[fd] |= 0x80000; // FD_CLOEXEC
    }

    fd as u64
}
