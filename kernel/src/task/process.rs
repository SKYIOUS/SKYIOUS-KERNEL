use xmas_elf::ElfFile;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use hashbrown::HashMap;
use crate::sync::IrqSafeMutex as Mutex;
use crate::memory::paging::AddressSpace;
use x86_64::structures::paging::PageTableFlags;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::objects::handle::{HandleTable, HandleValue};
use crate::objects::ObjectTypeId;

pub static CURRENT_PROCESS: Mutex<Option<Arc<Process>>> = Mutex::new(None);

lazy_static::lazy_static! {
    pub static ref PROCESS_TABLE: Mutex<alloc::collections::BTreeMap<u64, Arc<Process>>> = Mutex::new(alloc::collections::BTreeMap::new());
}

impl Process {
    pub fn next_id() -> u64 {
        static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(100); // Start user PIDs at 100
        NEXT_PROCESS_ID.fetch_add(1, Ordering::Relaxed)
    }
}

/// Represents a region of virtual memory.
#[derive(Debug, Clone)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub flags: PageTableFlags,
        pub _name: &'static str,
    pub file_handle: Option<u64>,
    pub file_offset: u64,
    pub is_shared: bool,
    pub shm_id: Option<u32>,  // None for normal mappings
}

use smoltcp::iface::SocketHandle;

#[derive(Clone, Copy, PartialEq)]
pub enum SocketType { Tcp, Udp, Raw, Unix }

#[allow(dead_code)]
pub enum FileDescriptor {
    File { node: Arc<dyn VfsNode>, offset: crate::sync::IrqSafeMutex<usize> },
    Socket(SocketHandle, SocketType),
    UnixSocket(u64, SocketType),
    PtyMaster { _idx: usize, pair: alloc::sync::Arc<crate::sync::IrqSafeMutex<crate::pty::PtyPair>> },
    PtySlave { _idx: usize, pair: alloc::sync::Arc<crate::sync::IrqSafeMutex<crate::pty::PtyPair>> },
    SignalFd(u64),
    EventFd(alloc::sync::Arc<crate::sync::IrqSafeMutex<EventFdData>>),
}

impl Clone for FileDescriptor {
    fn clone(&self) -> Self {
        match self {
            FileDescriptor::File { node, offset } => FileDescriptor::File { node: node.clone(), offset: crate::sync::IrqSafeMutex::new(*offset.lock()) },
            FileDescriptor::Socket(h, t) => FileDescriptor::Socket(*h, *t),
            FileDescriptor::UnixSocket(h, t) => FileDescriptor::UnixSocket(*h, *t),
            FileDescriptor::PtyMaster { _idx, pair } => FileDescriptor::PtyMaster { _idx: *_idx, pair: pair.clone() },
            FileDescriptor::PtySlave { _idx, pair } => FileDescriptor::PtySlave { _idx: *_idx, pair: pair.clone() },
            FileDescriptor::SignalFd(h) => FileDescriptor::SignalFd(*h),
            FileDescriptor::EventFd(d) => FileDescriptor::EventFd(d.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationMode {
    Native,
    Linux,
    Windows,
}

pub struct Process {
    pub id: u64,
    pub parent_id: Option<u64>,
    #[allow(dead_code)]
    pub tgid: u64,
    pub address_space: AddressSpace,
    pub vmas: Mutex<Vec<Vma>>,
    pub entry_point: u64,
    pub fd_table: Mutex<Vec<Option<FileDescriptor>>>,
    pub fd_flags: Mutex<Vec<u64>>,
    pub handle_table: Mutex<HandleTable>,
    pub handle_audit_id_counter: AtomicU64,
    pub exit_code: Mutex<Option<i32>>,
    pub children: Mutex<Vec<u64>>,
    pub brk: Mutex<u64>,
    pub cwd: Mutex<String>,
    /// Map from directory fd to its normalized absolute path (for *at syscalls)
    pub dir_fds: Mutex<HashMap<usize, String>>,
    pub signals: Mutex<crate::syscalls::signal::SignalState>,
    pub signal_handlers: Mutex<[u64; 32]>,
    pub signal_restorers: Mutex<[u64; 32]>,
    /// All POSIX credentials in one struct — single-lock atomic read.
    pub creds: crate::sync::IrqSafeMutex<Credentials>,
    pub io_rings: Mutex<Vec<(u64, usize)>>,
    pub clear_child_tid: Mutex<u64>,
    pub emulation: Mutex<EmulationMode>,
    pub umask: Mutex<u32>,
    pub pgid: crate::sync::IrqSafeMutex<u64>,
    pub session: crate::sync::IrqSafeMutex<u64>,
    pub is_group_leader: crate::sync::IrqSafeMutex<bool>,
    pub rlim_cur: crate::sync::IrqSafeMutex<[i64; 16]>,
    pub rlim_max: crate::sync::IrqSafeMutex<[i64; 16]>,
    pub altstack: crate::sync::IrqSafeMutex<stack_t>,
    pub itimer_real: crate::sync::IrqSafeMutex<itimerval>,
    pub utime: core::sync::atomic::AtomicU64,
    pub stime: core::sync::atomic::AtomicU64,
    pub cutime: core::sync::atomic::AtomicU64,
    pub cstime: core::sync::atomic::AtomicU64,
    pub boot_ticks: u64,
    pub groups: crate::sync::IrqSafeMutex<alloc::vec::Vec<u32>>,
    /// virt_page_addr → (device_idx, slot_idx) for swapped-out pages
    pub swap_map: crate::sync::IrqSafeMutex<hashbrown::HashMap<u64, (usize, usize)>>,
}

// ─── sigaltstack / itimerval / tms types ─────────────────────────

pub const SS_DISABLE: i32 = 2;
pub const SS_ONSTACK: i32 = 1;
pub const SIGSTKSZ: usize = 8192;
pub const MINSIGSTKSZ: usize = 2048;

#[repr(C)]
pub struct stack_t {
    pub ss_sp: *mut u8,
    pub ss_flags: i32,
    pub ss_size: usize,
}

// ponytail: stack_t holds a raw pointer used only for signal altstack storage,
// never dereferenced from another thread. Marking Send is safe here.
unsafe impl Send for stack_t {}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct timeval {
    pub tv_sec: i64,
    pub tv_usec: i64,
}

#[repr(C)]
pub struct itimerval {
    pub it_interval: timeval,
    pub it_value: timeval,
}

#[repr(C)]
pub struct tms {
    pub tms_utime: i64,
    pub tms_stime: i64,
    pub tms_cutime: i64,
    pub tms_cstime: i64,
}

// ─── signalfd types ──────────────────────────────────────────────

pub const SFD_NONBLOCK: i32 = 0x800;
pub const SFD_CLOEXEC: i32 = 0x80000;

pub struct SignalFdData {
    pub mask: u64,
    pub pending: alloc::collections::VecDeque<SignalFdInfo>,
    pub nonblock: bool,
    pub cloexec: bool,
}

pub struct SignalFdInfo {
    pub signo: u32,
    pub pid: u32,
    pub uid: u32,
}

// ─── eventfd types ──────────────────────────────────────────────

pub const EFD_SEMAPHORE: i32 = 1;
pub const EFD_NONBLOCK: i32 = 0x800;
pub const EFD_CLOEXEC: i32 = 0x40000;
pub const EFD_MAX: u64 = 0xFFFF_FFFF_FFFF_FFFE;

pub struct EventFdData {
    pub counter: u64,
    pub semaphore: bool,
    pub nonblock: bool,
}

/// All POSIX credentials in one struct — single-lock snapshot.
#[derive(Clone, Copy, Debug)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub suid: u32,
    pub sgid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
    pub cap_effective: u64,
    #[allow(dead_code)]
    pub cap_permitted: u64,
    #[allow(dead_code)]
    pub cap_inheritable: u64,
    pub umask: u32,
}

impl Default for Credentials {
    fn default() -> Self {
        Credentials {
            uid: 0, gid: 0, euid: 0, egid: 0,
            suid: 0, sgid: 0, fsuid: 0, fsgid: 0,
            cap_effective: 0, // No capabilities by default
            cap_permitted: 0, // No permitted capabilities by default
            cap_inheritable: 0,
            umask: 0o022,
        }
    }
}

impl Process {
    /// Take a snapshot of the process's credentials (single Mutex lock).
    pub fn credentials(&self) -> Credentials {
        self.creds.lock().clone()
    }

    /// Apply a credential change (e.g., from setuid exec).
    pub fn set_credentials(&self, cred: &Credentials) {
        let mut c = self.creds.lock();
        c.euid = cred.euid;
        c.egid = cred.egid;
        c.suid = cred.suid;
        c.cap_effective = cred.cap_effective;
    }

    /// Inherit credentials from a parent process.
    pub fn clone_credentials_from(&self, parent: &Process) {
        let pc = parent.creds.lock();
        let mut c = self.creds.lock();
        c.uid = pc.uid;
        c.gid = pc.gid;
        c.euid = pc.euid;
        c.egid = pc.egid;
        c.suid = pc.suid;
        c.sgid = pc.sgid;
        c.fsuid = pc.fsuid;
        c.fsgid = pc.fsgid;
        c.cap_effective = pc.cap_effective;
        c.cap_permitted = pc.cap_permitted;
        c.cap_inheritable = pc.cap_inheritable;
        *self.umask.lock() = *parent.umask.lock();
        *self.groups.lock() = parent.groups.lock().clone();
    }
}

use crate::vfs::VfsNode;
use crate::objects::KernelObject;

lazy_static::lazy_static! {
    /// Global signalfd registry: fd_handle → SignalFdData.
    pub static ref SIGNAL_FDS: crate::sync::IrqSafeMutex<hashbrown::HashMap<u64, alloc::sync::Arc<crate::sync::IrqSafeMutex<SignalFdData>>>> =
        crate::sync::IrqSafeMutex::new(hashbrown::HashMap::new());
}

impl Process {
    /// Execute a closure with mutable access to a handle entry.
    pub fn with_handle<F, R>(&self, fd: HandleValue, f: F) -> Result<R, u64>
    where F: FnOnce(&mut crate::objects::handle::HandleEntry) -> Result<R, u64> {
        let mut ht = self.handle_table.lock();
        let entry = ht.get_mut(fd).ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        f(entry)
    }

    /// Read-only access to a handle entry.
    pub fn with_handle_readonly<F, R>(&self, fd: HandleValue, f: F) -> Result<R, u64>
    where F: FnOnce(&crate::objects::handle::HandleEntry) -> Result<R, u64> {
        let ht = self.handle_table.lock();
        let entry = ht.get(fd).ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        f(entry)
    }

    /// Create a new handle with bind-time security check.
    pub fn new_handle(&self, object: Arc<dyn KernelObject>, access: u32, flags: u64) -> Result<HandleValue, ()> {
        self.handle_table.lock().insert(object, access, flags)
    }

    /// Set flags on a handle (e.g., O_NONBLOCK).
    pub fn set_handle_flags(&self, fd: HandleValue, flags: u64) -> Result<(), u64> {
        let mut ht = self.handle_table.lock();
        let entry = ht.get_mut(fd).ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        entry.flags = flags;
        Ok(())
    }

    /// Get flags from a handle.
    pub fn get_handle_flags(&self, fd: HandleValue) -> Result<u64, u64> {
        let ht = self.handle_table.lock();
        let entry = ht.get(fd).ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        Ok(entry.flags)
    }

    /// Close a handle.
    pub fn close_handle(&self, fd: HandleValue) -> Option<Arc<dyn KernelObject>> {
        self.handle_table.lock().close(fd)
    }

    pub fn enum_handles(&self) -> Vec<(HandleValue, ObjectTypeId)> {
        self.handle_table.lock().audit_trail().into_iter().map(|(hv, _)| {
            (hv, ObjectTypeId(0))
        }).collect()
    }

    pub fn new(id: u64, parent_id: Option<u64>, address_space: AddressSpace) -> Self {
        Process {
            id,
            parent_id,
            tgid: id,
            address_space,
            vmas: Mutex::new(Vec::new()),
            entry_point: 0,
            fd_table: Mutex::new(Vec::new()),
            fd_flags: Mutex::new(Vec::new()),
            handle_table: Mutex::new(HandleTable::new()),
            handle_audit_id_counter: AtomicU64::new(1),
            exit_code: Mutex::new(None),
            children: Mutex::new(Vec::new()),
            brk: Mutex::new(0),
            cwd: Mutex::new(String::from("/")),
            dir_fds: Mutex::new(hashbrown::HashMap::new()),
            signals: Mutex::new(crate::syscalls::signal::SignalState::new()),
            signal_handlers: Mutex::new([0; 32]),
            signal_restorers: Mutex::new([0; 32]),
            creds: crate::sync::IrqSafeMutex::new(Credentials::default()),
            io_rings: Mutex::new(Vec::new()),
            clear_child_tid: Mutex::new(0),
            emulation: Mutex::new(EmulationMode::Native),
            umask: Mutex::new(0o022),
            pgid: crate::sync::IrqSafeMutex::new(id),
            session: crate::sync::IrqSafeMutex::new(id),
            is_group_leader: crate::sync::IrqSafeMutex::new(true),
            rlim_cur: crate::sync::IrqSafeMutex::new([i64::MAX; 16]),
            rlim_max: crate::sync::IrqSafeMutex::new([i64::MAX; 16]),
            altstack: crate::sync::IrqSafeMutex::new(stack_t {
                ss_sp: core::ptr::null_mut(),
                ss_flags: SS_DISABLE,
                ss_size: 0,
            }),
            itimer_real: crate::sync::IrqSafeMutex::new(itimerval {
                it_interval: timeval { tv_sec: 0, tv_usec: 0 },
                it_value: timeval { tv_sec: 0, tv_usec: 0 },
            }),
            utime: core::sync::atomic::AtomicU64::new(0),
            stime: core::sync::atomic::AtomicU64::new(0),
            cutime: core::sync::atomic::AtomicU64::new(0),
            cstime: core::sync::atomic::AtomicU64::new(0),
            boot_ticks: crate::interrupts::get_ticks(),
            groups: crate::sync::IrqSafeMutex::new(alloc::vec::Vec::new()),
            swap_map: crate::sync::IrqSafeMutex::new(hashbrown::HashMap::new()),
        }
    }

    pub fn add_vma(&self, new_vma: Vma) {
        let mut vmas = self.vmas.lock();
        vmas.push(new_vma);
        vmas.sort_by(|a, b| a.start.cmp(&b.start));
        self.merge_vmas_inner(&mut vmas);
    }

    /// Merge overlapping and adjacent VMAs with compatible flags and file backing.
    fn merge_vmas_inner(&self, vmas: &mut Vec<Vma>) {
        let mut i = 0;
        while i + 1 < vmas.len() {
            let same_backing = vmas[i].file_handle == vmas[i + 1].file_handle
                && vmas[i].is_shared == vmas[i + 1].is_shared
                && vmas[i].shm_id == vmas[i + 1].shm_id;
            let can_merge = vmas[i].flags == vmas[i + 1].flags && same_backing;
            let overlaps_or_adjacent = vmas[i].end >= vmas[i + 1].start;
            if can_merge && overlaps_or_adjacent {
                vmas[i].end = vmas[i].end.max(vmas[i + 1].end);
                vmas.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }

    /// Remove or trim VMAs that intersect [start, end).
    /// Returns the number of pages removed from the page table (caller must handle that).
    pub fn remove_vma_range(&self, start: u64, end: u64) {
        let mut vmas = self.vmas.lock();
        let mut i = 0;
        while i < vmas.len() {
            let v = &vmas[i];
            if v.end <= start || v.start >= end {
                i += 1;
                continue;
            }
            // v overlaps [start, end)
            if v.start < start && v.end > end {
                // Middle section removed — split into two
                let right = Vma { start: end, end: v.end, flags: v.flags, _name: v._name, file_handle: v.file_handle, file_offset: v.file_offset, is_shared: v.is_shared, shm_id: v.shm_id };
                vmas[i].end = start;
                vmas.insert(i + 1, right);
                return; // no further overlap possible with this VMA after split
            }
            if v.start >= start && v.end <= end {
                // Completely covered — remove
                vmas.remove(i);
                continue;
            }
            if v.start < start && v.end <= end {
                // Trim right
                vmas[i].end = start;
                i += 1;
            } else if v.start >= start && v.end > end {
                // Trim left
                vmas[i].start = end;
                i += 1;
            }
        }
    }

    /// Coalesce the entire VMA list (merges any adjacent/overlapping VMAs with matching flags).
    pub fn merge_all_vmas(&self) {
        let mut vmas = self.vmas.lock();
        if vmas.is_empty() { return; }
        vmas.sort_by(|a, b| a.start.cmp(&b.start));
        self.merge_vmas_inner(&mut vmas);
    }

    pub fn find_vma(&self, addr: u64) -> Option<Vma> {
        let vmas = self.vmas.lock();
        vmas.iter().find(|vma| addr >= vma.start && addr < vma.end).cloned()
    }

    pub fn load_elf(elf_data: &[u8], mut address_space: AddressSpace) -> Result<Self, &'static str> {
        crate::serial_write("[load_elf] Entering load_elf\n");
        let (mut entry, mut vmas) = Self::load_elf_static(elf_data, &mut address_space)?;
        crate::serial_write("[load_elf] load_elf_static completed successfully\n");

        let elf = ElfFile::new(elf_data).map_err(|_| "Failed to re-parse ELF")?;
        crate::serial_write("[load_elf] ELF re-parsed successfully\n");
        let has_dynamic = elf.program_iter().any(|ph| matches!(ph.get_type(), Ok(xmas_elf::program::Type::Dynamic)));

        if has_dynamic {
            crate::serial_write("[load_elf] Processing dynamic binary\n");
            crate::elf_dyn::load_dynamic_binary(elf_data, &mut address_space, &mut entry, &mut vmas)?;
            crate::serial_write("[load_elf] load_dynamic_binary completed\n");
        }
        
        let mut process = Process::new(Process::next_id(), None, address_space);
        process.entry_point = entry;
        crate::serial_write("[load_elf] Process instance created\n");
        
        // Add VMAs via add_vma to merge adjacent/overlapping segments
        for vma in vmas {
            process.add_vma(vma);
        }
        crate::serial_write("[load_elf] VMAs added\n");

        // Merge remaining after all segments added
        process.merge_all_vmas();
        crate::serial_write("[load_elf] merge_all_vmas completed\n");

        let vmas = process.vmas.lock();
        let mut initial_brk = 0;
        for vma in vmas.iter() {
            if vma.end > initial_brk {
                initial_brk = vma.end;
            }
        }
        drop(vmas);
        // Page align the initial break
        let initial_brk = (initial_brk + 4095) & !4095;
        *process.brk.lock() = initial_brk;
        crate::serial_write("[load_elf] initial_brk configured, returning process\n");
        Ok(process)
    }

    /// Loads an ELF into an existing AddressSpace without creating a Process yet.
    /// Returns (entry_point, vmas).
    pub fn load_elf_static(elf_data: &[u8], address_space: &mut AddressSpace) -> Result<(u64, Vec<Vma>), &'static str> {
        let elf = ElfFile::new(elf_data).map_err(|_| "Failed to parse ELF")?;
        
                        use x86_64::structures::paging::{Mapper, Page, Size4KiB, FrameAllocator, Translate};
                        use crate::memory::buddy::BuddyFrameAllocator;
                        let mut frame_allocator = BuddyFrameAllocator;
        let mut mapper = unsafe { address_space.mapper().ok_or("Failed to get mapper")? };

        let entry_point = elf.header.pt2.entry_point();
        let mut vmas = Vec::new();
        
        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
                let virt_start = ph.virtual_addr();
                let file_size = ph.file_size();
                let mem_size = ph.mem_size();
                let offset = ph.offset() as usize;

                let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
                if ph.flags().is_write() { flags |= PageTableFlags::WRITABLE; }
                if !ph.flags().is_execute() { flags |= PageTableFlags::NO_EXECUTE; }

                // Define VMA
                vmas.push(Vma {
                    start: virt_start,
                    end: virt_start + mem_size,
                    flags,
                    _name: "elf_phdr",
                    file_handle: None,
                    file_offset: 0,
                    is_shared: false,
                    shm_id: None,
                });

                // Map and Copy
                let start_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(virt_start));
                let end_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(virt_start + mem_size - 1));
                
                for page in Page::range_inclusive(start_page, end_page) {
                    let map_flags = flags | PageTableFlags::WRITABLE;
                    let mut was_mapped = true;
                    let frame = match mapper.translate_page(page) {
                        Ok(f) => {
                            // Page already mapped from a previous overlapping segment.
                            // Get current flags and add WRITABLE for the copy.
                            let addr = page.start_address();
                            let old_flags = match mapper.translate(addr) {
                                x86_64::structures::paging::mapper::TranslateResult::Mapped { flags, .. } => flags,
                                _ => map_flags,
                            };
                            unsafe {
                                let _ = mapper.update_flags(page, old_flags | PageTableFlags::WRITABLE);
                            }
                            f
                        }
                        Err(_) => {
                            was_mapped = false;
                            let f = frame_allocator.allocate_frame().ok_or("Out of memory during ELF load")?;
                            unsafe {
                                mapper.map_to(page, f, map_flags, &mut frame_allocator)
                                    .map_err(|_| "Failed to map ELF page")?.flush();
                            }
                            crate::memory::frame_info::increment(f.start_address());
                            f
                        }
                    };

                    let page_start = page.start_address().as_u64();
                    let offset_in_segment = page_start.saturating_sub(virt_start);
                    let copy_start = virt_start + offset_in_segment;
                    let copy_end = core::cmp::min(virt_start + file_size, page_start + 4096);
                    
                    if copy_start < copy_end {
                        let len = copy_end - copy_start;
                        let src_off = offset + (copy_start - virt_start) as usize;
                        unsafe {
                            let dst_ptr = (x86_64::VirtAddr::new(crate::memory::physical_memory_offset()) + frame.start_address().as_u64()).as_mut_ptr::<u8>();
                            let page_offset = virt_start.saturating_sub(page_start);
                            core::ptr::copy_nonoverlapping(
                                elf_data[src_off..src_off + len as usize].as_ptr(),
                                dst_ptr.add(page_offset as usize),
                                len as usize
                            );
                        }
                    }

                    // Set final flags only for freshly mapped pages.
                    // Overlapping pages keep RWX to satisfy all segments.
                    if !was_mapped {
                        unsafe {
                            mapper.update_flags(page, flags).map_err(|_| "Failed to update flags")?.flush();
                        }
                    }
                }
            }
        }
        
        // Apply R_X86_64_RELATIVE relocations from PT_DYNAMIC
        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Dynamic) = ph.get_type() {
                let dyn_off = ph.offset() as usize;
                let dyn_filesz = ph.file_size() as usize;
                let dyn_data = &elf_data[dyn_off..dyn_off + dyn_filesz];

                let mut rela_vaddr = 0u64;
                let mut rela_size = 0u64;
                let num_dyn = dyn_data.len() / 16;
                for i in 0..num_dyn {
                    unsafe {
                        let entry = dyn_data.as_ptr().add(i * 16) as *const u64;
                        let tag = *entry as i64;
                        let val = *entry.add(1);
                        if tag == 7 { rela_vaddr = val; }
                        else if tag == 8 { rela_size = val; }
                    }
                }

                if rela_vaddr != 0 && rela_size != 0 {
                    let mut rela_file_off = 0u64;
                    for ph2 in elf.program_iter() {
                        if let Ok(xmas_elf::program::Type::Load) = ph2.get_type() {
                            let seg_start = ph2.virtual_addr();
                            let seg_end = seg_start + ph2.file_size();
                            if rela_vaddr >= seg_start && rela_vaddr < seg_end {
                                rela_file_off = ph2.offset() + (rela_vaddr - seg_start);
                                break;
                            }
                        }
                    }

                    if rela_file_off != 0 || rela_vaddr == 0 {
                        let rela_end = (rela_file_off as usize + rela_size as usize).min(elf_data.len());
                        let rela_data = &elf_data[rela_file_off as usize..rela_end];
                        let num_rela = rela_data.len() / 24;
                        for i in 0..num_rela {
                            unsafe {
                                let entry = rela_data.as_ptr().add(i * 24) as *const u64;
                                let r_offset = *entry;
                                let r_info = *entry.add(1);
                                let r_addend = *entry.add(2) as i64;
                                let r_type = (r_info & 0xffffffff) as u32;

                                if r_type == 8 {
                                    let target_va = x86_64::VirtAddr::new(r_offset);
                                    use x86_64::structures::paging::mapper::TranslateResult;
                                    if let TranslateResult::Mapped { frame, offset, .. } = mapper.translate(target_va) {
                                        let phys_addr = frame.start_address() + offset;
                                        let kaddr = x86_64::VirtAddr::new(
                                            crate::memory::physical_memory_offset() + phys_addr.as_u64()
                                        );
                                        *(kaddr.as_mut_ptr::<u64>()) = r_addend as u64;
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }

        Ok((entry_point, vmas))
    }

    pub fn register(process: Arc<Process>) {
        PROCESS_TABLE.lock().insert(process.id, process.clone());
    }

    /// Cheap per-process ASLR entropy (RDTSC-based).
    fn aslr_entropy() -> u64 {
        let lo: u32;
        let hi: u32;
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags)); }
        ((hi as u64) << 32) | (lo as u64)
    }

    /// PHASE D2: User stack setup in execve
    /// Maps 64KB stack at a randomized location and populates argc/argv.
    /// Returns Err on OOM (partial frames are freed).
    pub fn setup_user_stack(&self, argv: &[alloc::string::String]) -> Result<u64, ()> {
                        use x86_64::structures::paging::{Mapper, Page, Size4KiB, PageTableFlags, FrameAllocator};
                        use crate::memory::buddy::BuddyFrameAllocator;
        let mut frame_allocator = BuddyFrameAllocator;
        let mut mapper = unsafe { self.address_space.mapper().expect("Failed to get mapper for stack setup") };

        // ASLR: randomize stack base in a 64MB range just below the old hardcoded address.
        // Old: 0x7FFF_FFFF_E000. New: 0x7FFF_F000_0000 + random * 4096 (up to 0xFFF pages)
        let stack_random = (Self::aslr_entropy() & 0xFFF) * 4096;
        let stack_top_addr = 0x7FFF_F000_0000u64 + stack_random;
        let stack_pages = 16; // 64 KB
        
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        // Pre-allocate all frames before mapping so OOM is handled atomically
        let mut frames = alloc::vec::Vec::with_capacity(stack_pages);
        for _ in 0..stack_pages {
            match frame_allocator.allocate_frame() {
                Some(frame) => frames.push(frame),
                None => {
                    // Free any frames already allocated
                    for f in &frames {
                        crate::memory::frame_info::decrement(f.start_address());
                    }
                    return Err(());
                }
            }
        }

        for (i, frame) in frames.into_iter().enumerate() {
             let page_addr = stack_top_addr - (i as u64 + 1) * 4096;
             let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(page_addr));
             unsafe {
                 mapper.map_to(page, frame, flags, &mut frame_allocator).expect("map_to failed").flush();
             }
             crate::memory::frame_info::increment(frame.start_address());
        }

        // Add VMA for user stack
        self.add_vma(Vma {
            start: stack_top_addr - (stack_pages as u64) * 4096,
            end: stack_top_addr,
            flags,
            _name: "user_stack",
            file_handle: None,
            file_offset: 0,
            is_shared: false,
            shm_id: None,
        });

        // Copy strings to the top of the stack
        let mut current_rsp = stack_top_addr;
        let mut arg_ptrs = Vec::new();

        for arg in argv.iter().rev() {
            let bytes = arg.as_bytes();
            current_rsp -= (bytes.len() + 1) as u64; // +1 for null terminator
            let virt = x86_64::VirtAddr::new(current_rsp);
            
            // Map virtual to physical for direct writing
            let phys = crate::memory::virt_to_phys(virt).ok_or(())?;
            let offset = crate::memory::physical_memory_offset();
            let k_ptr = (offset + phys.as_u64()) as *mut u8;
            
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), k_ptr, bytes.len());
                *k_ptr.add(bytes.len()) = 0;
            }
            arg_ptrs.push(current_rsp);
        }

        // Align RSP
        current_rsp &= !0xF;
        
        // Push argv pointers (null terminated)
        current_rsp -= 8; // NULL
        
        for ptr in arg_ptrs {
            current_rsp -= 8;
            let virt = x86_64::VirtAddr::new(current_rsp);
            let phys = crate::memory::virt_to_phys(virt).ok_or(())?;
            let k_ptr = (crate::memory::physical_memory_offset() + phys.as_u64()) as *mut u64;
            unsafe { *k_ptr = ptr; }
        }
        
        let _argv_start = current_rsp;

        // Push argc
        current_rsp -= 8;
        let virt = x86_64::VirtAddr::new(current_rsp);
        let phys = crate::memory::virt_to_phys(virt).ok_or(())?;
        let k_ptr = (crate::memory::physical_memory_offset() + phys.as_u64()) as *mut u64;
        unsafe { *k_ptr = argv.len() as u64; }

        Ok(current_rsp)
    }
}

/// Kill a process by PID — marks all its threads as exited and sends SIGCHLD to parent.
#[allow(dead_code)]
pub fn kill_process(pid: u64) {
    let mut table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        *proc.exit_code.lock() = Some(-1);
        crate::println!("[OOM] Killed process pid={}", pid);
        if let Some(parent) = proc.parent_id.and_then(|ppid| table.get(&ppid)) {
            parent.signals.lock().raise(crate::syscalls::signal::Signal::SIGCHLD);
        }
        table.remove(&pid);
    }
}
