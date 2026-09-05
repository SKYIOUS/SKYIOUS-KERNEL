// ponytail: 1150 lines — exceeds 1000-line ceiling. Extract process_types
// (Vma, FileDescriptor, Credentials, etc.) to a submodule when the type
// dependencies are untangled from the Process impl blocks.
use crate::memory::paging::AddressSpace;
use crate::objects::handle::{HandleTable, HandleValue};
use crate::objects::ObjectTypeId;
use crate::sync::IrqSafeMutex as Mutex;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use hashbrown::HashMap;
use x86_64::structures::paging::PageTableFlags;
use xmas_elf::ElfFile;

// Self-contained types (signal, eventfd, timerfd, credentials) extracted to submodule.
pub mod types;
pub use types::*;

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
    pub shm_id: Option<u32>, // None for normal mappings
}

/// Find the lowest `len`-byte region at or above `hint` that overlaps no VMA.
///
/// Anonymous `mmap(NULL, …)` must return a distinct region per call. Returning a
/// fixed address (as this once did) makes every mapping alias one page, so
/// userspace heap allocations silently overwrite each other. Returns the first
/// free gap, or `None` once the scan passes `max_addr`.
pub fn find_free_vma_region(vmas: &[Vma], hint: u64, len: u64, max_addr: u64) -> Option<u64> {
    let page_size: u64 = 4096;
    let mut candidate = hint;
    // The whole [candidate, candidate+len) region must fit below max_addr:
    // a scan that only checked `candidate < max_addr` could hand back a
    // mapping that crosses the ceiling (regression caught by selftest
    // mmap::regions_distinct).
    while candidate.saturating_add(len) <= max_addr {
        let conflict = vmas.iter().find_map(|v| {
            if candidate < v.end && candidate + len > v.start {
                Some(v.end.max(candidate))
            } else {
                None
            }
        });
        match conflict {
            None => return Some(candidate),
            Some(end) => candidate = (end + page_size - 1) & !(page_size - 1),
        }
    }
    None
}

use smoltcp::iface::SocketHandle;

#[derive(Clone, Copy, PartialEq)]
pub enum SocketType {
    Tcp,
    Udp,
    Raw,
    Unix,
}

pub enum FileDescriptor {
    File {
        node: Arc<dyn crate::vfs::VfsNode>,
        offset: crate::sync::IrqSafeMutex<usize>,
    },
    Socket(SocketHandle, SocketType),
    UnixSocket(u64, SocketType),
    PtyMaster {
        _idx: usize,
        pair: alloc::sync::Arc<crate::sync::IrqSafeMutex<crate::pty::PtyPair>>,
    },
    PtySlave {
        _idx: usize,
        pair: alloc::sync::Arc<crate::sync::IrqSafeMutex<crate::pty::PtyPair>>,
    },
    SignalFd(u64),
    EventFd(alloc::sync::Arc<crate::sync::IrqSafeMutex<EventFdData>>),
    TimerFd(alloc::sync::Arc<crate::sync::IrqSafeMutex<TimerFdData>>),
    InotifyFd {
        instance_key: u64,
    },
    IoUringFd(alloc::sync::Arc<crate::sync::IrqSafeMutex<dyn core::any::Any + Send + Sync>>),
}

impl Clone for FileDescriptor {
    fn clone(&self) -> Self {
        match self {
            FileDescriptor::File { node, offset } => FileDescriptor::File {
                node: node.clone(),
                offset: crate::sync::IrqSafeMutex::new(*offset.lock()),
            },
            FileDescriptor::Socket(h, t) => FileDescriptor::Socket(*h, *t),
            FileDescriptor::UnixSocket(h, t) => FileDescriptor::UnixSocket(*h, *t),
            FileDescriptor::PtyMaster { _idx, pair } => FileDescriptor::PtyMaster {
                _idx: *_idx,
                pair: pair.clone(),
            },
            FileDescriptor::PtySlave { _idx, pair } => FileDescriptor::PtySlave {
                _idx: *_idx,
                pair: pair.clone(),
            },
            FileDescriptor::SignalFd(h) => FileDescriptor::SignalFd(*h),
            FileDescriptor::EventFd(d) => FileDescriptor::EventFd(d.clone()),
            FileDescriptor::TimerFd(d) => FileDescriptor::TimerFd(d.clone()),
            FileDescriptor::InotifyFd { instance_key } => FileDescriptor::InotifyFd {
                instance_key: *instance_key,
            },
            FileDescriptor::IoUringFd(d) => FileDescriptor::IoUringFd(d.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationMode {
    Native,
    Linux,
    Windows,
}

/// Process identity fields — always accessed together in fork/setsid/setpgid.
#[derive(Clone, Copy)]
pub struct ProcessIdentity {
    pub pgid: u64,
    pub session: u64,
    pub is_group_leader: bool,
}

/// Resource limits — always accessed together in getrlimit/setrlimit/prlimit64.
#[derive(Clone, Copy)]
pub struct ResourceLimits {
    pub rlim_cur: [i64; 16],
    pub rlim_max: [i64; 16],
}
// ─── Process sub-structs ───────────────────────────────────────
// Group related fields behind a single Mutex on Process.
// Access pattern: process.memory.lock().brk, process.files.lock().fd_table, etc.

/// Memory-related process state: VMAs, brk, swap map.
pub struct ProcessMemory {
    pub brk: u64,
    /// Next address for MAP_ANONYMOUS mmap with addr=0.
    pub mmap_base: u64,
    pub vmas: Vec<Vma>,
    /// virt_page_addr → (device_idx, slot_idx) for swapped-out pages
    pub swap_map: hashbrown::HashMap<u64, (usize, usize)>,
}

/// File-descriptor-related process state: fd table, flags, cwd, dir fds.
pub struct ProcessFiles {
    pub fd_table: Vec<Option<FileDescriptor>>,
    pub fd_flags: Vec<u64>,
    pub cwd: String,
    /// Map from directory fd to its normalized absolute path (for *at syscalls)
    pub dir_fds: HashMap<usize, String>,
}

/// Security-related process state: seccomp, landlock, namespaces.
pub struct ProcessSecurity {
    pub seccomp: crate::syscalls::seccomp::SeccompState,
    pub landlock: crate::syscalls::landlock::LandlockState,
    pub namespaces: crate::syscalls::namespaces::NamespaceSet,
    /// Landlock rulesets indexed by fd number
    pub landlock_fds: hashbrown::HashMap<usize, crate::syscalls::landlock::LandlockRuleset>,
    /// Namespace references indexed by fd number (for setns)
    pub namespace_fds: hashbrown::HashMap<usize, crate::syscalls::namespaces::NamespaceType>,
    /// ptrace state: tracer/tracee relationship, stop reasons, flags.
    pub ptrace: crate::syscalls::ptrace::PtraceState,
}

pub struct Process {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub tgid: u64,
    pub address_space: AddressSpace,
    pub entry_point: u64,
    pub handle_table: Mutex<HandleTable>,
    pub handle_audit_id_counter: AtomicU64,
    /// Exit status; `i32::MIN` = not exited yet. Atomic so the fault-kill
    /// path (IF=0, invariant I4) can record it without blocking on a mutex.
    pub exit_code: core::sync::atomic::AtomicI32,
    pub children: Mutex<Vec<u64>>,
    pub signals: Mutex<crate::syscalls::signal::SignalState>,
    pub signal_handlers: Mutex<[u64; 32]>,
    pub signal_restorers: Mutex<[u64; 32]>,
    /// All POSIX credentials in one struct — single-lock atomic read.
    pub creds: crate::sync::IrqSafeMutex<Credentials>,
    pub io_rings: Mutex<Vec<(u64, usize)>>,
    pub clear_child_tid: Mutex<u64>,
    pub emulation: Mutex<EmulationMode>,
    pub umask: Mutex<u32>,
    pub identity: crate::sync::IrqSafeMutex<ProcessIdentity>,
    pub limits: crate::sync::IrqSafeMutex<ResourceLimits>,
    pub altstack: crate::sync::IrqSafeMutex<stack_t>,
    pub itimer_real: crate::sync::IrqSafeMutex<itimerval>,
    pub utime: core::sync::atomic::AtomicU64,
    pub stime: core::sync::atomic::AtomicU64,
    pub cutime: core::sync::atomic::AtomicU64,
    pub cstime: core::sync::atomic::AtomicU64,
    pub boot_ticks: u64,
    pub groups: crate::sync::IrqSafeMutex<alloc::vec::Vec<u32>>,
    /// Process/thread name (up to 15 chars + null)
    pub name: crate::sync::IrqSafeMutex<String>,
    /// Whether exec can gain new privileges
    pub no_new_privs: core::sync::atomic::AtomicBool,
    /// Whether core dumps are enabled
    pub dumpable: core::sync::atomic::AtomicBool,
    /// Whether this process is a child subreaper
    pub child_subreaper: core::sync::atomic::AtomicBool,
    /// Timer slack value in nanoseconds
    pub timerslack: core::sync::atomic::AtomicU64,
    /// Cgroup path (e.g. "/", "/system.slice/docker.service")
    pub cgroup_path: crate::sync::IrqSafeMutex<String>,
    // ─── Sub-structs (single Mutex each) ─────────────────────────
    pub memory: Mutex<ProcessMemory>,
    pub files: Mutex<ProcessFiles>,
    pub security: Mutex<ProcessSecurity>,
    /// Isolate-based virtual memory: wraps AddressSpace with region tracking.
    pub isolate: Option<crate::memory::isolate::Isolate>,
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
    pub cap_permitted: u64,
    pub cap_inheritable: u64,
    pub umask: u32,
}

impl Default for Credentials {
    fn default() -> Self {
        Credentials {
            uid: 0,
            gid: 0,
            euid: 0,
            egid: 0,
            suid: 0,
            sgid: 0,
            fsuid: 0,
            fsgid: 0,
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
        *self.creds.lock()
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

use crate::objects::KernelObject;

lazy_static::lazy_static! {
    /// Global signalfd registry: fd_handle → SignalFdData.
    pub static ref SIGNAL_FDS: crate::sync::IrqSafeMutex<hashbrown::HashMap<u64, alloc::sync::Arc<crate::sync::IrqSafeMutex<SignalFdData>>>> =
        crate::sync::IrqSafeMutex::new(hashbrown::HashMap::new());
}

/// Route a signal to the signalfd instances of an ALREADY-RESOLVED process.
/// IRQ/fault-safe (invariant I4): try_lock only, bails silently on any
/// contention. The signal itself was already raised to the process's queue;
/// signalfd delivery is a secondary notification — a blocking acquisition
/// here with IF=0 would freeze the CPU until the holder releases.
pub fn route_signal_to_signalfd_for(
    proc: &Process,
    signo: u32,
    code: i32,
    sender_pid: u64,
    sender_uid: u32,
    sigval: u64,
) {
    let sig_bit = 1u64 << (signo - 1);
    let fd_table = match proc.files.try_lock() {
        Some(g) => g.fd_table.clone(),
        None => return,
    };

    for entry in fd_table.iter() {
        if let Some(FileDescriptor::SignalFd(handle)) = entry {
            let fds = match SIGNAL_FDS.try_lock() {
                Some(g) => g,
                None => return,
            };
            if let Some(data_arc) = fds.get(handle) {
                let mut data = match data_arc.try_lock() {
                    Some(g) => g,
                    None => continue,
                };
                if (data.mask & sig_bit) != 0 {
                    let mut info = SignalFdInfo::new();
                    info.ssi_signo = signo;
                    info.ssi_code = code;
                    info.ssi_pid = sender_pid as u32;
                    info.ssi_uid = sender_uid;
                    info.ssi_sigval = sigval;
                    data.pending.push_back(info);
                }
            }
        }
    }
}

impl Process {
    /// Execute a closure with mutable access to a handle entry.
    pub fn with_handle<F, R>(&self, fd: HandleValue, f: F) -> Result<R, u64>
    where
        F: FnOnce(&mut crate::objects::handle::HandleEntry) -> Result<R, u64>,
    {
        let mut ht = self.handle_table.lock();
        let entry = ht
            .get_mut(fd)
            .ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        f(entry)
    }

    /// Read-only access to a handle entry.
    pub fn with_handle_readonly<F, R>(&self, fd: HandleValue, f: F) -> Result<R, u64>
    where
        F: FnOnce(&crate::objects::handle::HandleEntry) -> Result<R, u64>,
    {
        let ht = self.handle_table.lock();
        let entry = ht
            .get(fd)
            .ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        f(entry)
    }

    /// Create a new handle with bind-time security check.
    #[allow(clippy::result_unit_err)]
    pub fn new_handle(
        &self,
        object: Arc<dyn KernelObject>,
        access: u32,
        flags: u64,
    ) -> Result<HandleValue, ()> {
        self.handle_table.lock().insert(object, access, flags)
    }

    /// Set flags on a handle (e.g., O_NONBLOCK).
    pub fn set_handle_flags(&self, fd: HandleValue, flags: u64) -> Result<(), u64> {
        let mut ht = self.handle_table.lock();
        let entry = ht
            .get_mut(fd)
            .ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        entry.flags = flags;
        Ok(())
    }

    /// Get flags from a handle.
    pub fn get_handle_flags(&self, fd: HandleValue) -> Result<u64, u64> {
        let ht = self.handle_table.lock();
        let entry = ht
            .get(fd)
            .ok_or(crate::syscalls::errno::Errno::EBADF as u64)?;
        Ok(entry.flags)
    }

    /// Close a handle.
    pub fn close_handle(&self, fd: HandleValue) -> Option<Arc<dyn KernelObject>> {
        self.handle_table.lock().close(fd)
    }

    pub fn enum_handles(&self) -> Vec<(HandleValue, ObjectTypeId)> {
        self.handle_table
            .lock()
            .audit_trail()
            .into_iter()
            .map(|(hv, _)| (hv, ObjectTypeId(0)))
            .collect()
    }

    pub fn new(id: u64, parent_id: Option<u64>, address_space: AddressSpace) -> Self {
        Process {
            id,
            parent_id,
            tgid: id,
            address_space,
            entry_point: 0,
            handle_table: Mutex::new(HandleTable::new()),
            handle_audit_id_counter: AtomicU64::new(1),
            exit_code: core::sync::atomic::AtomicI32::new(i32::MIN),
            children: Mutex::new(Vec::new()),
            signals: Mutex::new(crate::syscalls::signal::SignalState::new()),
            signal_handlers: Mutex::new([0; 32]),
            signal_restorers: Mutex::new([0; 32]),
            creds: crate::sync::IrqSafeMutex::new(Credentials::default()),
            io_rings: Mutex::new(Vec::new()),
            clear_child_tid: Mutex::new(0),
            emulation: Mutex::new(EmulationMode::Native),
            umask: Mutex::new(0o022),
            identity: crate::sync::IrqSafeMutex::new(ProcessIdentity {
                pgid: id,
                session: id,
                is_group_leader: true,
            }),
            limits: crate::sync::IrqSafeMutex::new(ResourceLimits {
                rlim_cur: [i64::MAX; 16],
                rlim_max: [i64::MAX; 16],
            }),
            altstack: crate::sync::IrqSafeMutex::new(stack_t {
                ss_sp: core::ptr::null_mut(),
                ss_flags: SS_DISABLE,
                ss_size: 0,
            }),
            itimer_real: crate::sync::IrqSafeMutex::new(itimerval {
                it_interval: timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                it_value: timeval {
                    tv_sec: 0,
                    tv_usec: 0,
                },
            }),
            utime: core::sync::atomic::AtomicU64::new(0),
            stime: core::sync::atomic::AtomicU64::new(0),
            cutime: core::sync::atomic::AtomicU64::new(0),
            cstime: core::sync::atomic::AtomicU64::new(0),
            boot_ticks: crate::interrupts::get_ticks(),
            groups: crate::sync::IrqSafeMutex::new(alloc::vec::Vec::new()),
            name: crate::sync::IrqSafeMutex::new(String::from("init")),
            no_new_privs: core::sync::atomic::AtomicBool::new(false),
            dumpable: core::sync::atomic::AtomicBool::new(true),
            child_subreaper: core::sync::atomic::AtomicBool::new(false),
            timerslack: core::sync::atomic::AtomicU64::new(0),
            cgroup_path: crate::sync::IrqSafeMutex::new(String::from("/")),
            // Sub-structs
            memory: Mutex::new(ProcessMemory {
                brk: 0,
                mmap_base: 0,
                vmas: Vec::new(),
                swap_map: hashbrown::HashMap::new(),
            }),
            files: Mutex::new(ProcessFiles {
                fd_table: Vec::new(),
                fd_flags: Vec::new(),
                cwd: String::from("/"),
                dir_fds: hashbrown::HashMap::new(),
            }),
            security: Mutex::new(ProcessSecurity {
                seccomp: crate::syscalls::seccomp::SeccompState::default(),
                landlock: crate::syscalls::landlock::LandlockState::default(),
                namespaces: crate::syscalls::namespaces::NamespaceSet::default(),
                landlock_fds: hashbrown::HashMap::new(),
                namespace_fds: hashbrown::HashMap::new(),
                ptrace: crate::syscalls::ptrace::PtraceState::default(),
            }),
            isolate: None, // Initialized later when address space is ready
        }
    }

    pub fn add_vma(&self, new_vma: Vma) {
        let mut mem = self.memory.lock();
        Self::insert_vma_locked(&mut mem.vmas, new_vma);
    }

    /// Push + sort + merge a VMA into a list whose lock the caller ALREADY
    /// holds. mmap uses this to keep find-free-region and insert atomic under
    /// one guard — a separate add_vma() re-lock between scan and insert lets
    /// two concurrent mmaps pick the same free region and alias each other.
    pub(crate) fn insert_vma_locked(vmas: &mut Vec<Vma>, new_vma: Vma) {
        vmas.push(new_vma);
        vmas.sort_by_key(|a| a.start);
        Self::merge_vmas_inner(vmas);
    }

    /// Merge overlapping and adjacent VMAs with compatible flags and file backing.
    fn merge_vmas_inner(vmas: &mut Vec<Vma>) {
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
        let mut mem = self.memory.lock();
        let vmas = &mut mem.vmas;
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
                let right = Vma {
                    start: end,
                    end: v.end,
                    flags: v.flags,
                    _name: v._name,
                    file_handle: v.file_handle,
                    file_offset: v.file_offset,
                    is_shared: v.is_shared,
                    shm_id: v.shm_id,
                };
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
        let mut mem = self.memory.lock();
        if mem.vmas.is_empty() {
            return;
        }
        mem.vmas.sort_by_key(|a| a.start);
        Self::merge_vmas_inner(&mut mem.vmas);
    }

    pub fn find_vma(&self, addr: u64) -> Option<Vma> {
        let mem = self.memory.lock();
        mem.vmas
            .iter()
            .find(|vma| addr >= vma.start && addr < vma.end)
            .cloned()
    }

    pub fn load_elf(
        elf_data: &[u8],
        mut address_space: AddressSpace,
    ) -> Result<Self, &'static str> {
        let (mut entry, mut vmas) = Self::load_elf_static(elf_data, &mut address_space)?;

        let elf = ElfFile::new(elf_data).map_err(|_| "Failed to re-parse ELF")?;
        let has_dynamic = elf
            .program_iter()
            .any(|ph| matches!(ph.get_type(), Ok(xmas_elf::program::Type::Dynamic)));

        if has_dynamic {
            crate::elf_dyn::load_dynamic_binary(
                elf_data,
                &mut address_space,
                &mut entry,
                &mut vmas,
            )?;
        }

        let mut process = Process::new(Process::next_id(), None, address_space);
        process.entry_point = entry;

        // Add VMAs via add_vma to merge adjacent/overlapping segments
        for vma in vmas {
            process.add_vma(vma);
        }

        // Merge remaining after all segments added
        process.merge_all_vmas();

        let vmas = process.memory.lock().vmas.clone();
        let mut initial_brk = 0;
        for vma in vmas.iter() {
            if vma.end > initial_brk {
                initial_brk = vma.end;
            }
        }
        drop(vmas);
        // Page align the initial break + ASLR randomization.
        // 30-bit entropy: up to 1GB random offset from ELF end.
        let brk_base = (initial_brk + 4095) & !4095;
        let brk_random = (Self::aslr_entropy() & 0x3FFF_FFFF) & !0xFFFu64; // 4KB-aligned
        process.memory.lock().brk = brk_base + brk_random;
        Ok(process)
    }

    /// Loads an ELF into an existing AddressSpace without creating a Process yet.
    /// Returns (entry_point, vmas).
    pub fn load_elf_static(
        elf_data: &[u8],
        address_space: &mut AddressSpace,
    ) -> Result<(u64, Vec<Vma>), &'static str> {
        let elf = ElfFile::new(elf_data).map_err(|_| "Failed to parse ELF")?;

        use crate::memory::buddy::BuddyFrameAllocator;
        use x86_64::structures::paging::{FrameAllocator, Mapper, Page, Size4KiB, Translate};
        let mut frame_allocator = BuddyFrameAllocator;
        // SAFETY: mapper() returns a page table mapper via HHDM. Valid because
        // address_space was initialized during process creation.
        let mut mapper = unsafe { address_space.mapper().ok_or("Failed to get mapper")? };

        let entry_point = elf.header.pt2.entry_point();
        let mut vmas = Vec::new();

        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
                let virt_start = ph.virtual_addr();
                let file_size = ph.file_size();
                let mem_size = ph.mem_size();
                let offset = ph.offset() as usize;

                // Reject header-told-but-file-lacking ranges before slicing.
                // Malicious/truncated ELFs must fail to load, not panic.
                if offset
                    .checked_add(file_size as usize)
                    .map(|e| e > elf_data.len())
                    .unwrap_or(true)
                {
                    return Err("Program header ranges past end of file");
                }
                if mem_size == 0 {
                    continue;
                }
                let virt_end = match virt_start.checked_add(mem_size) {
                    Some(e) => e,
                    None => return Err("Program header virtual range overflow"),
                };

                let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
                if ph.flags().is_write() {
                    flags |= PageTableFlags::WRITABLE;
                }
                if !ph.flags().is_execute() {
                    flags |= PageTableFlags::NO_EXECUTE;
                }

                // Define VMA
                vmas.push(Vma {
                    start: virt_start,
                    end: virt_end,
                    flags,
                    _name: "elf_phdr",
                    file_handle: None,
                    file_offset: 0,
                    is_shared: false,
                    shm_id: None,
                });

                // Map and Copy
                let start_page =
                    Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(virt_start));
                let end_page =
                    Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(virt_end - 1));

                for page in Page::range_inclusive(start_page, end_page) {
                    let map_flags = flags | PageTableFlags::WRITABLE;
                    let mut was_mapped = true;
                    let frame = match mapper.translate_page(page) {
                        Ok(f) => {
                            // Page already mapped from a previous overlapping segment.
                            // Get current flags and add WRITABLE for the copy.
                            let addr = page.start_address();
                            let old_flags = match mapper.translate(addr) {
                                x86_64::structures::paging::mapper::TranslateResult::Mapped {
                                    flags,
                                    ..
                                } => flags,
                                _ => map_flags,
                            };
                            // SAFETY: update_flags modifies the existing page table entry
                            // to add WRITABLE for COW/demand-zero handling.
                            unsafe {
                                let _ =
                                    mapper.update_flags(page, old_flags | PageTableFlags::WRITABLE);
                            }
                            f
                        }
                        Err(_) => {
                            was_mapped = false;
                            let f = frame_allocator
                                .allocate_frame()
                                .ok_or("Out of memory during ELF load")?;
                            // SAFETY: map_to creates a new page table entry mapping the ELF
                            // segment page to the freshly allocated physical frame.
                            unsafe {
                                mapper
                                    .map_to(page, f, map_flags, &mut frame_allocator)
                                    .map_err(|_| "Failed to map ELF page")?
                                    .flush();
                            }
                            crate::memory::frame_info::increment(f.start_address());
                            f
                        }
                    };

                    let page_start = page.start_address().as_u64();
                    let offset_in_segment = page_start.saturating_sub(virt_start);
                    let copy_start = virt_start + offset_in_segment;
                    let copy_end =
                        core::cmp::min(virt_start.saturating_add(file_size), page_start + 4096);

                    if copy_start < copy_end {
                        let len = copy_end - copy_start;
                        let src_off = offset + (copy_start - virt_start) as usize;
                        // SAFETY: dst_ptr is the HHDM-mapped physical frame we just
                        // mapped above. src_off..src_off+len is within elf_data bounds
                        // (validated by the ELF loader). The copy writes ELF segment
                        // data into the frame.
                        unsafe {
                            let dst_ptr =
                                (x86_64::VirtAddr::new(crate::memory::physical_memory_offset())
                                    + frame.start_address().as_u64())
                                .as_mut_ptr::<u8>();
                            let page_offset = virt_start.saturating_sub(page_start);
                            core::ptr::copy_nonoverlapping(
                                elf_data[src_off..src_off + len as usize].as_ptr(),
                                dst_ptr.add(page_offset as usize),
                                len as usize,
                            );
                        }
                    }

                    // Set final flags only for freshly mapped pages.
                    // Overlapping pages keep RWX to satisfy all segments.
                    if !was_mapped {
                        // SAFETY: update_flags sets the final page permissions (RX for code,
                        // RW for data) after copying ELF content.
                        unsafe {
                            mapper
                                .update_flags(page, flags)
                                .map_err(|_| "Failed to update flags")?
                                .flush();
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
                if dyn_off
                    .checked_add(dyn_filesz)
                    .map(|e| e > elf_data.len())
                    .unwrap_or(true)
                {
                    return Err("Dynamic header past end of file");
                }
                let dyn_data = &elf_data[dyn_off..dyn_off + dyn_filesz];

                let mut rela_vaddr = 0u64;
                let mut rela_size = 0u64;
                let num_dyn = dyn_data.len() / 16;
                for i in 0..num_dyn {
                    // SAFETY: dyn_data is a slice of the ELF file's PT_DYNAMIC segment.
                    // Each entry is 16 bytes (tag:u64, val:u64). num_dyn = len/16 ensures
                    // all reads are within bounds.
                    unsafe {
                        let entry = dyn_data.as_ptr().add(i * 16) as *const u64;
                        let tag = *entry as i64;
                        let val = *entry.add(1);
                        if tag == 7 {
                            rela_vaddr = val;
                        } else if tag == 8 {
                            rela_size = val;
                        }
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
                        let rela_end =
                            (rela_file_off as usize + rela_size as usize).min(elf_data.len());
                        let rela_data = &elf_data[rela_file_off as usize..rela_end];
                        let num_rela = rela_data.len() / 24;
                        for i in 0..num_rela {
                            // SAFETY: rela_data contains ELF relocation entries (24 bytes each).
                            // num_rela = len/24 ensures all pointer arithmetic stays within bounds.
                            // Each read accesses r_offset (0), r_info (+8), r_addend (+16).
                            unsafe {
                                let entry = rela_data.as_ptr().add(i * 24) as *const u64;
                                let r_offset = *entry;
                                let r_info = *entry.add(1);
                                let r_addend = *entry.add(2) as i64;
                                let r_type = (r_info & 0xffffffff) as u32;

                                if r_type == 8 {
                                    let target_va = x86_64::VirtAddr::new(r_offset);
                                    use x86_64::structures::paging::mapper::TranslateResult;
                                    if let TranslateResult::Mapped { frame, offset, .. } =
                                        mapper.translate(target_va)
                                    {
                                        let phys_addr = frame.start_address() + offset;
                                        let kaddr = x86_64::VirtAddr::new(
                                            crate::memory::physical_memory_offset()
                                                + phys_addr.as_u64(),
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
    pub(crate) fn aslr_entropy() -> u64 {
        let lo: u32;
        let hi: u32;
        // SAFETY: RDTSC reads the time stamp counter into EDX:EAX.
        // Non-privileged instruction, safe from any privilege level.
        unsafe {
            core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags));
        }
        ((hi as u64) << 32) | (lo as u64)
    }

    /// User stack setup for execve.
    /// Lays out the Linux ABI: argc | argv | NULL | envp | NULL | auxv | NULL.
    /// Returns Err on OOM (partial frames are freed).
    #[allow(clippy::result_unit_err)]
    pub fn setup_user_stack(
        &self,
        argv: &[String],
        envp: &[String],
        elf_entry: u64,
        elf_data: &[u8],
    ) -> Result<u64, ()> {
        use crate::memory::buddy::BuddyFrameAllocator;
        use x86_64::structures::paging::{FrameAllocator, Mapper, Page, Size4KiB};
        let mut frame_allocator = BuddyFrameAllocator;
        // SAFETY: mapper() returns a page table mapper via HHDM. The address
        // space was initialized during process creation.
        let mut mapper = unsafe {
            self.address_space
                .mapper()
                .expect("Failed to get mapper for stack setup")
        };

        // ASLR: 28-bit entropy for stack base (up to256 MB randomization).
        // Must stay below 0x8000_0000_0000 to avoid colliding with kernel
        // PML4 entries (256..512) which contain HHDM huge pages.
        let stack_random = (Self::aslr_entropy() & 0x0FFF_FFFF) & !0xFFFu64; // page-aligned
        let stack_top_addr = 0x7FFF_F000_0000u64 + stack_random;
        let stack_pages = 2048; // 8 MiB

        let flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        // Pre-allocate all frames before mapping so OOM is handled atomically.
        let mut frames = Vec::with_capacity(stack_pages);
        for _ in 0..stack_pages {
            match frame_allocator.allocate_frame() {
                Some(frame) => frames.push(frame),
                None => {
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
            // SAFETY: map_to maps a user stack page to a freshly allocated frame.
            // The frame is owned by this process (pre-allocated above).
            unsafe {
                mapper
                    .map_to(page, frame, flags, &mut frame_allocator)
                    .expect("map_to failed")
                    .flush();
            }
            crate::memory::frame_info::increment(frame.start_address());
        }

        // Add VMA for user stack.
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

        // Guard page: 1 page below stack, no permissions.
        // Stack overflow hits this → SIGSEGV instead of silent corruption.
        let guard_addr = stack_top_addr - (stack_pages as u64 + 1) * 4096;
        let guard_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(guard_addr));
        let guard_frame = frame_allocator.allocate_frame().ok_or(())?;
        // Map guard as present but non-writable/non-executable — fault on access.
        let guard_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
        // SAFETY: map_to creates the guard page mapping. Access to this page
        // triggers a page fault (no WRITABLE flag), providing stack overflow detection.
        unsafe {
            mapper
                .map_to(guard_page, guard_frame, guard_flags, &mut frame_allocator)
                .expect("guard page map failed")
                .flush();
        }
        crate::memory::frame_info::increment(guard_frame.start_address());
        self.add_vma(Vma {
            start: guard_addr,
            end: guard_addr + 4096,
            flags: guard_flags,
            _name: "stack_guard",
            file_handle: None,
            file_offset: 0,
            is_shared: false,
            shm_id: None,
        });

        // Parse ELF to get PHDR address for AT_PHDR auxv.
        // For a static ELF, program headers live within the first PT_LOAD segment.
        // AT_PHDR = entry_point + file_offset_of_phdrs (standard ELF layout).
        let mut phdr_addr = 0u64;
        let mut phdr_count = 0u64;
        let mut phdr_entry_size = 0u64;
        if let Ok(elf) = ElfFile::new(elf_data) {
            phdr_count = elf.header.pt2.ph_count() as u64;
            phdr_entry_size = elf.header.pt2.ph_entry_size() as u64;
            phdr_addr = elf_entry.wrapping_add(elf.header.pt2.ph_offset());
        }

        // Helper: write a u64 to the user stack and advance RSP.
        let mut rsp = stack_top_addr;
        let push_u64 = |val: u64, rsp: &mut u64| -> Result<(), ()> {
            *rsp -= 8;
            let v = x86_64::VirtAddr::new(*rsp);
            let p = crate::memory::virt_to_phys(v).ok_or(())?;
            let k = (crate::memory::physical_memory_offset() + p.as_u64()) as *mut u64;
            // SAFETY: k points to the HHDM-mapped stack page. The page was just
            // mapped and the virtual address was translated to physical via virt_to_phys.
            unsafe {
                *k = val;
            }
            Ok(())
        };
        // Helper: write a byte slice (null-terminated) to stack, return user pointer.
        let push_str = |s: &[u8], rsp: &mut u64| -> Result<u64, ()> {
            let len = s.len() as u64;
            *rsp -= len + 1;
            let v = x86_64::VirtAddr::new(*rsp);
            let p = crate::memory::virt_to_phys(v).ok_or(())?;
            let k = (crate::memory::physical_memory_offset() + p.as_u64()) as *mut u8;
            // SAFETY: k points to the HHDM-mapped stack page. We write s.len() bytes
            // plus a null terminator. The page is mapped RW by the stack mapping above.
            unsafe {
                core::ptr::copy_nonoverlapping(s.as_ptr(), k, s.len());
                *k.add(s.len()) = 0;
            }
            Ok(*rsp)
        };

        // Layout (grows downward from stack_top_addr):
        //   string data: argv[i], envp[i]
        //   [alignment padding]
        //   argv pointers + NULL terminator
        //   envp pointers + NULL terminator
        //   auxv entries + AT_NULL
        //   argc

        // ── Push argv strings (reversed, then reversed pointers below) ──
        let mut argv_ptrs = Vec::with_capacity(argv.len());
        for arg in argv.iter().rev() {
            argv_ptrs.push(push_str(arg.as_bytes(), &mut rsp)?);
        }
        argv_ptrs.reverse();

        // ── Push envp strings ──
        let mut envp_ptrs = Vec::with_capacity(envp.len());
        for e in envp.iter().rev() {
            envp_ptrs.push(push_str(e.as_bytes(), &mut rsp)?);
        }
        envp_ptrs.reverse();

        // Align RSP to 16 bytes.
        rsp &= !0xF;

        // ── argv pointer array + NULL ──
        push_u64(0, &mut rsp)?; // NULL terminator
        for &ptr in argv_ptrs.iter().rev() {
            push_u64(ptr, &mut rsp)?;
        }
        let _argv_start = rsp;

        // ── envp pointer array + NULL ──
        push_u64(0, &mut rsp)?; // NULL terminator
        for &ptr in envp_ptrs.iter().rev() {
            push_u64(ptr, &mut rsp)?;
        }

        // ── auxv ──
        // First write AT_RANDOM data (16 random bytes) below the pointer array.
        rsp -= 16;
        {
            let random = Self::aslr_entropy();
            let mut rand_buf = [0u8; 16];
            rand_buf[..8].copy_from_slice(&random.to_le_bytes());
            rand_buf[8..16].copy_from_slice(&random.wrapping_mul(0x9E3779B9).to_le_bytes());
            let v = x86_64::VirtAddr::new(rsp);
            let p = crate::memory::virt_to_phys(v).ok_or(())?;
            let k = (crate::memory::physical_memory_offset() + p.as_u64()) as *mut u8;
            // SAFETY: k points to the HHDM-mapped stack page. We write 16 bytes
            // of random data for AT_RANDOM auxv. The page is mapped RW.
            unsafe {
                core::ptr::copy_nonoverlapping(rand_buf.as_ptr(), k, 16);
            }
        }
        let random_bytes_addr = rsp; // pointer to the 16 random bytes

        // AT_NULL (sentinel, pushed first = lowest address)
        push_u64(0, &mut rsp)?; // type = AT_NULL
        push_u64(0, &mut rsp)?; // value = 0
                                // AT_RANDOM
        push_u64(25, &mut rsp)?; // type = AT_RANDOM
        push_u64(random_bytes_addr, &mut rsp)?; // value = pointer to random bytes
                                                // AT_PLATFORM
        push_u64(15, &mut rsp)?; // type = AT_PLATFORM
        push_u64(0, &mut rsp)?; // value = NULL (no platform string)
                                // AT_CLKTCK
        push_u64(17, &mut rsp)?; // type
        push_u64(100, &mut rsp)?; // value = 100 Hz
                                  // AT_PAGESZ
        push_u64(6, &mut rsp)?; // type
        push_u64(4096, &mut rsp)?; // value
                                   // AT_HWCAP
        push_u64(16, &mut rsp)?; // type
        push_u64(0, &mut rsp)?; // value
                                // AT_FLAGS
        push_u64(3, &mut rsp)?; // type
        push_u64(0, &mut rsp)?; // value
                                // AT_ENTRY
        push_u64(9, &mut rsp)?; // type
        push_u64(elf_entry, &mut rsp)?; // value
                                        // AT_BASE (interpreter base, 0 for static)
        push_u64(7, &mut rsp)?; // type
        push_u64(0, &mut rsp)?; // value = no interpreter loaded yet
                                // AT_PHNUM / AT_PHENT / AT_PHDR
        push_u64(5, &mut rsp)?; // type = AT_PHNUM
        push_u64(phdr_count, &mut rsp)?;
        push_u64(4, &mut rsp)?; // type = AT_PHENT
        push_u64(phdr_entry_size, &mut rsp)?;
        push_u64(33, &mut rsp)?; // type = AT_PHDR
        push_u64(phdr_addr, &mut rsp)?;

        // ── argc ──
        push_u64(argv.len() as u64, &mut rsp)?;

        Ok(rsp)
    }

    /// Perform the exit bookkeeping for a fault-killed process.
    /// Records exit code 139 (SIGSEGV), raises SIGCHLD on the parent,
    /// and marks the current thread as Exited. Called from exception
    /// handlers in IRQ context — no allocation, only VMA/sched locks.
    #[cfg(not(target_arch = "aarch64"))]
    pub fn kill_from_fault(&self) -> ! {
        // Without init there is nothing to supervise: the "PID 1 exited"
        // halt guard exists on the sys_exit path only, so a fault-killed
        // init left the machine alive-but-dead (no login, no halt, 0-core
        // hang). Same contract, reached from the fault path. handle_panic
        // is allocation-free, so panicking from IF=0 context prints.
        if self.id == 1 {
            panic!("PID 1 (init) killed by fault — no init process = system dead");
        }
        self.exit_code
            .store(139, core::sync::atomic::Ordering::Relaxed); // SIGSEGV (128+11)
        if let Some(ppid) = self.parent_id {
            // try_lock only (I4): never block on the table in IF=0 context.
            // On contention the SIGCHLD raise is skipped — wait4 reaps via
            // the table scan every tick, so it does not depend on the raise.
            let parent = match crate::task::process::PROCESS_TABLE.try_lock() {
                Some(table) => table.get(&ppid).cloned(),
                None => None,
            };
            if let Some(parent) = parent {
                // try_lock + skip: IF=0 context (I4) — a blocking acquisition
                // could freeze the CPU. wait4 re-scans the table every tick,
                // so reaping does not depend on this raise.
                if let Some(mut sig) = parent.signals.try_lock() {
                    sig.raise(crate::syscalls::signal::Signal::SIGCHLD);
                }
                route_signal_to_signalfd_for(&parent, 17, SI_CHILD, self.id, 0, 0);
            }
        }
        crate::task::scheduler::with_current_thread(|thread| {
            thread.status = crate::task::thread::ThreadStatus::Exited;
        });
        crate::task::scheduler::schedule();
        loop {
            x86_64::instructions::interrupts::enable_and_hlt();
        }
    }
}

// ─── Job Objects (Windows NT equivalent) ──────────────────────────

/// Job object for process grouping and resource management.
/// Similar to Windows NT Job Objects.
pub struct JobObject {
    /// Job name
    pub name: alloc::string::String,
    /// Job ID
    pub id: u64,
    /// Processes in this job
    pub processes: alloc::vec::Vec<u64>,
    /// Maximum processes allowed
    pub max_processes: usize,
    /// Maximum memory per process (bytes)
    pub max_memory_per_process: usize,
    /// Total memory limit for job
    pub total_memory_limit: usize,
    /// CPU rate limit (0 = unlimited)
    pub cpu_rate_limit: u32,
    /// Kill on last close flag
    pub kill_on_last_close: bool,
}

impl JobObject {
    pub fn new(name: alloc::string::String, id: u64) -> Self {
        Self {
            name,
            id,
            processes: alloc::vec::Vec::new(),
            max_processes: 1024,
            max_memory_per_process: 256 * 1024 * 1024, // 256MB
            total_memory_limit: 4 * 1024 * 1024 * 1024, // 4GB
            cpu_rate_limit: 0,
            kill_on_last_close: true,
        }
    }

    /// Add a process to the job
    pub fn add_process(&mut self, pid: u64) -> Result<(), crate::syscalls::errno::Errno> {
        if self.processes.len() >= self.max_processes {
            return Err(crate::syscalls::errno::Errno::EAGAIN);
        }
        self.processes.push(pid);
        Ok(())
    }

    /// Remove a process from the job
    pub fn remove_process(&mut self, pid: u64) {
        self.processes.retain(|&p| p != pid);
    }

    /// Check if job has capacity for more processes
    pub fn has_capacity(&self) -> bool {
        self.processes.len() < self.max_processes
    }
}

// ─── vahi-types ProcessProvider implementation ──────────────────────
// Single owner of process identity: crates that need current-PID/creds
// (net, drivers) read through this provider instead of holding references
// to a second, never-populated process lineage. Registered in scheduler::init.

use crate::syscalls::helpers::get_current_process;

struct KernelProcessProvider;

impl vahi_types::ProcessProvider for KernelProcessProvider {
    fn current_pid(&self) -> vahi_types::Pid {
        get_current_process().map(|p| p.id).unwrap_or(0)
    }

    fn process_exists(&self, pid: vahi_types::Pid) -> bool {
        PROCESS_TABLE.lock().contains_key(&pid)
    }

    fn is_init(&self) -> bool {
        self.current_pid() == 1
    }

    fn current_credentials(&self) -> vahi_types::Credentials {
        get_current_process()
            .map(|p| {
                let c = p.creds.lock();
                vahi_types::Credentials {
                    uid: c.uid,
                    gid: c.gid,
                    euid: c.euid,
                    egid: c.egid,
                }
            })
            .unwrap_or(vahi_types::Credentials::ROOT)
    }

    fn vmas(&self, _pid: vahi_types::Pid) -> Option<&[vahi_types::Vma]> {
        None
    }

    fn is_user_address(&self, addr: vahi_types::VirtAddr) -> bool {
        addr < vahi_types::USER_ADDR_MAX
    }

    fn peer_creds(&self, pid: vahi_types::Pid) -> Option<(u32, u32, u32)> {
        let p = PROCESS_TABLE.lock().get(&pid)?.clone();
        let c = p.creds.lock();
        Some((p.id as u32, c.uid, c.gid))
    }
}

/// Register the kernel as the single provider of process identity.
/// Called from `scheduler::init()`.
pub fn init_process_provider() {
    static PROVIDER: KernelProcessProvider = KernelProcessProvider;
    vahi_types::register_process_provider(&PROVIDER);
    vahi_types::register_tick_fn(|| crate::interrupts::get_ticks());
    // Redirect crate-side pipe blocking (vahi-net unix sockets) to the one
    // live scheduler. Plain fn pointers: no state crosses the boundary.
    vahi_types::register_sched_facade(vahi_types::SchedFacade {
        block_on_pipe: crate::task::scheduler::block_on_pipe,
        wake_pipe: crate::task::scheduler::wake_pipe,
    });
    vahi_types::register_sleep_facade(crate::task::scheduler::sleep_until_tick);
}
