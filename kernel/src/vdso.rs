//! vDSO (Virtual Dynamic Shared Object) — Kernel data page mapped into userspace.
//!
//! Provides lock-free access to time, PID, and CPU ID without syscalls.
//! Updated by kernel on timer ticks and context switches; read by userspace via seqlock.
//!
//! Per-CPU pages: each CPU has its own vDSO page at VDSO_BASE + cpu_id * 4096
//! to eliminate cache-line false sharing on timer tick updates.

#![cfg(target_arch = "x86_64")]

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicPtr, Ordering};
use x86_64::structures::paging::{FrameAllocator, Size4KiB, PhysFrame, Mapper, Page, PageTableFlags};
use x86_64::VirtAddr;

/// Base virtual address for vDSO pages in user space.
/// Each CPU gets a page at VDSO_BASE + cpu_id * 4096.
/// x86_64: 256 KiB below USER_MMAP_TOP (0x0000_7FFF_FFFF_0000)
pub const VDSO_BASE: u64 = 0x0000_7FFF_FFFC_0000;

/// Virtual address of vDSO page for a specific CPU.
pub const fn vdso_virt_addr_for_cpu(cpu_id: usize) -> u64 {
    VDSO_BASE + (cpu_id as u64) * 4096
}

/// Kernel feature flags for CLOCK_SKYIOUS_KERNEL_FLAGS (103).
/// Bitfield exposed via vDSO for userspace feature detection.
pub mod kernel_flags {
    pub const HAS_AVX2: u64 = 1 << 0;          // CPUID leaf 7 EBX bit 5
    pub const HAS_NVME: u64 = 1 << 1;          // NVMe controller present
    pub const HAS_XHCI: u64 = 1 << 2;          // xHCI USB controller present
    pub const HAS_VIRTIO_GPU: u64 = 1 << 3;    // VirtIO GPU present
    // Bits 4-15 reserved for net stack version and FS features
}

/// vDSO data structure — exactly one page (4096 bytes), cache-line aligned.
///
/// Layout (offsets verified by seqlock protocol):
///   0x000: seq (AtomicU64) — even = consistent, odd = updating (time fields)
///   0x008: monotonic_sec (AtomicU64)
///   0x010: monotonic_nsec (AtomicU32) + padding
///   0x018: realtime_sec (AtomicU64)
///   0x020: realtime_nsec (AtomicU32) + padding
///   0x028: tsc_freq_hz (AtomicU64)
///   0x030: tsc_mult (u32) + tsc_shift (u32)
///   0x038: tsc_base (AtomicU64)
///   0x040: pid_cpu_seq (AtomicU64) — even = consistent, odd = updating (PID/CPU fields)
///   0x048: pid (AtomicU64)
///   0x050: cpu_id (AtomicU32) + padding
///   0x058: thread_cputime_ns (AtomicU64) — thread CPU time for CLOCK_SKYIOUS_THREAD_CPUTIME (100)
///   0x060: kernel_flags (AtomicU64) — kernel feature flags for CLOCK_SKYIOUS_KERNEL_FLAGS (103)
///   0x068: process_cgroup_id (AtomicU64) — cgroup ID for CLOCK_SKYIOUS_PROCESS_INFO (104)
///   0x070: process_namespace_flags (AtomicU64) — namespace flags for CLOCK_SKYIOUS_PROCESS_INFO (104)
///   0x078: process_cap_effective (AtomicU64) — effective capabilities for CLOCK_SKYIOUS_PROCESS_INFO (104)
///   0x080: tsc_sync_ack_count (AtomicU32) — TSC sync IPI acknowledgment count (Phase 2.3)
///   0x084: tsc_sync_seq (AtomicU32) — TSC sync sequence number (Phase 2.3)
///   0x088-0xFFF: reserved (zero)
#[repr(C, align(4096))]
pub struct VdsoData {
    pub seq: AtomicU64,
    pub monotonic_sec: AtomicU64,
    pub monotonic_nsec: AtomicU32,
    _pad1: [u32; 1],
    pub realtime_sec: AtomicU64,
    pub realtime_nsec: AtomicU32,
    _pad2: [u32; 1],
    pub tsc_freq_hz: AtomicU64,
    pub tsc_mult: u32,      // Read-only after init (init_vdso)
    pub tsc_shift: u32,     // Read-only after init (init_vdso)
    pub tsc_base: AtomicU64,
    pub pid_cpu_seq: AtomicU64,  // Separate seqlock for PID/CPU fields
    pub pid: AtomicU64,
    pub cpu_id: AtomicU32,
    _pad3: [u32; 1],
    /// Thread CPU time (ns) for CLOCK_SKYIOUS_THREAD_CPUTIME (100)
    /// Updated at context switch, read by userspace without seqlock
    pub thread_cputime_ns: AtomicU64,
    /// Kernel feature flags for CLOCK_SKYIOUS_KERNEL_FLAGS (103)
    /// Read-only after init, bitfield of kernel capabilities
    pub kernel_flags: AtomicU64,
    /// Process cgroup ID for CLOCK_SKYIOUS_PROCESS_INFO (104)
    pub process_cgroup_id: AtomicU64,
    /// Process namespace flags for CLOCK_SKYIOUS_PROCESS_INFO (104)
    pub process_namespace_flags: AtomicU64,
    /// Process effective capabilities for CLOCK_SKYIOUS_PROCESS_INFO (104)
    pub process_cap_effective: AtomicU64,
    /// TSC sync IPI acknowledgment counter for Phase 2.3
    /// Incremented by each CPU after updating its TSC base
    pub tsc_sync_ack_count: AtomicU32,
    /// TSC sync sequence number for Phase 2.3
    /// Incremented by coordinator (CPU 0) before each sync
    pub tsc_sync_seq: AtomicU32,
    _reserved: [u64; 495], // Fill to 4096 bytes exactly
}

// Compile-time size check
const _: () = assert!(core::mem::size_of::<VdsoData>() == 4096);

/// Per-CPU vDSO page info for kernel access via HHDM.
/// Stores physical frame + userspace virtual address + HHDM virtual address for each CPU.
/// HHDM vaddr is cached to avoid PHYSICAL_MEMORY_OFFSET lookup on every access (especially in timer IRQ).
#[derive(Clone, Copy)]
pub struct VdsoPageInfo {
    frame: PhysFrame<Size4KiB>,
    user_vaddr: u64,
    hhdm_vaddr: u64,
}

/// Per-CPU page info array — write-once after init, lock-free reads via AtomicPtr.
/// Each entry is an AtomicPtr<VdsoPageInfo>, null = not initialized.
pub static VDSO_PAGES: [AtomicPtr<VdsoPageInfo>; crate::task::scheduler::MAX_CPUS] =
    [const { AtomicPtr::new(core::ptr::null_mut()) }; crate::task::scheduler::MAX_CPUS];

/// Heap-allocated VdsoPageInfo storage (write-once). Uses MaybeUninit for safe initialization.
static mut VDSO_PAGE_INFOS: [core::mem::MaybeUninit<VdsoPageInfo>; crate::task::scheduler::MAX_CPUS] =
    [core::mem::MaybeUninit::uninit(); crate::task::scheduler::MAX_CPUS];

/// Initialize the vDSO pages (one per CPU).
///
/// Allocates one physical frame per CPU, zeroes them, initializes TSC calibration
/// fields, and stores the references globally. Must be called early in boot
/// (after frame allocator is ready, before first process creation).
pub fn init_vdso(mut frame_allocator: impl FrameAllocator<Size4KiB>) {
    // Initialize TSC calibration fields once (read-only after boot)
    let tsc_freq = crate::hal::timer::get_tsc_frequency();
    crate::serial_write("[VDSO] TSC freq from timer: ");
    crate::serial_write_hex(tsc_freq);
    crate::serial_write("\n");
    let (mult, shift) = if tsc_freq > 0 {
        compute_tsc_conversion(tsc_freq)
    } else {
        (0, 0)
    };

    // Allocate and initialize one page per CPU
    for cpu_id in 0..crate::task::scheduler::MAX_CPUS {
        let frame = frame_allocator
            .allocate_frame()
            .expect("vDSO: failed to allocate frame");

        let user_vaddr = vdso_virt_addr_for_cpu(cpu_id);

        // Initialize the page via HHDM (same mapping used by readers) to ensure cache coherency
        let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().expect("HHDM offset not set");
        let hhdm_vaddr = phys_offset + frame.start_address().as_u64();
        let vdso_ptr = hhdm_vaddr as *mut VdsoData;
        // SAFETY: hhdm_vaddr points to a valid physical frame mapped in HHDM, which is always mapped.
        unsafe { core::ptr::write_bytes(vdso_ptr, 0, 1) };
        // SAFETY: vdso_ptr points to a valid, zeroed VdsoData page in HHDM.
        let vdso = unsafe { &mut *vdso_ptr };

        // Initialize TSC calibration fields (read-only after boot)
        // Read TSC for this CPU (will be same CPU 0 value for all during init,
        // but each CPU can update its own via vdso_update_tsc_base when it comes online)
        let (lo, hi) = read_tsc();
        let tsc_base = ((hi as u64) << 32) | lo as u64;
        vdso.tsc_freq_hz.store(tsc_freq, Ordering::Release);
        vdso.tsc_mult = mult;
        vdso.tsc_shift = shift;
        vdso.tsc_base.store(tsc_base, Ordering::Release);
        vdso.cpu_id.store(cpu_id as u32, Ordering::Release);

        // Initialize time fields from current tick counter and RTC
        let ticks = crate::interrupts::get_ticks();
        let total_ms = ticks * TICK_MS;
        vdso.monotonic_sec.store((total_ms / 1000) as u64, Ordering::Release);
        vdso.monotonic_nsec.store(((total_ms % 1000) * 1_000_000) as u32, Ordering::Release);

        // Initialize kernel feature flags (CPU 0 only, then copy to all CPUs)
        if cpu_id == 0 {
            let mut flags = 0u64;
            // Detect AVX2 (CPUID leaf 7, EBX bit 5)
            let features = core::arch::x86_64::__cpuid(7);
            if (features.ebx >> 5) & 1 != 0 {
                flags |= crate::vdso::kernel_flags::HAS_AVX2;
            }
            // Assume NVMe, XHCI, VirtIO-GPU present (driver availability)
            flags |= crate::vdso::kernel_flags::HAS_NVME;
            flags |= crate::vdso::kernel_flags::HAS_XHCI;
            flags |= crate::vdso::kernel_flags::HAS_VIRTIO_GPU;
            // Net stack version 1 (bits 4-7)
            flags |= 1 << 4;
            // FS features: ext2, ext4, fat32, skyfs, ramfs (bits 8-15)
            flags |= 0x1F << 8;
            
            vdso.kernel_flags.store(flags, Ordering::Release);
        } else {
            // For other CPUs, will be copied after init loop
            vdso.kernel_flags.store(0, Ordering::Release);
        }

        let (rt_sec, rt_nsec) = crate::drivers::rtc::read_realtime();
        if rt_sec == 0 && rt_nsec == 0 {
            crate::serial_write("[VDSO] WARNING: RTC not initialized, realtime will be zero until cached RTC updates\n");
        }
        vdso.realtime_sec.store(rt_sec as u64, Ordering::Release);
        vdso.realtime_nsec.store(rt_nsec as u32, Ordering::Release);

        // Ensure all initialization writes are visible before publishing the page pointer
        core::sync::atomic::fence(Ordering::SeqCst);

        // Store frame info in static array
        let info = VdsoPageInfo { frame, user_vaddr, hhdm_vaddr };
        unsafe {
            VDSO_PAGE_INFOS[cpu_id].write(info);
            let ptr = VDSO_PAGE_INFOS[cpu_id].as_mut_ptr();
            VDSO_PAGES[cpu_id].store(ptr, Ordering::Release);
        }

        // Initialize cached RTC for the first CPU (only need to do this once)
        if cpu_id == 0 {
            vdso_update_cached_rtc();
        }

        crate::serial_write("[VDSO] CPU ");
        crate::serial_write_hex(cpu_id as u64);
        crate::serial_write(" page at 0x");
        crate::serial_write_hex(user_vaddr);
        crate::serial_write("\n");
    }

    // Copy kernel_flags from CPU 0 to all other CPUs
    let cpu0_ptr = VDSO_PAGES[0].load(Ordering::Acquire);
    if !cpu0_ptr.is_null() {
        let info = unsafe { &*cpu0_ptr };
        let cpu0_hhdm = info.hhdm_vaddr;
        let cpu0_vdso = unsafe { &*(cpu0_hhdm as *const VdsoData) };
        let flags = cpu0_vdso.kernel_flags.load(Ordering::Acquire);
        for cpu_id in 1..crate::task::scheduler::MAX_CPUS {
            let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
            if !ptr.is_null() {
                let info = unsafe { &*ptr };
                let hhdm_vaddr = info.hhdm_vaddr;
                let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
                vdso.kernel_flags.store(flags, Ordering::Release);
            }
        }
    }
}

/// Map all vDSO pages into a process's address space.
///
/// Call this when creating a new process/isolate.
pub fn vdso_map_into(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), ()> {
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;

    for cpu_id in 0..crate::task::scheduler::MAX_CPUS {
        let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
        if ptr.is_null() {
            // All pages should be pre-allocated by init_vdso() before any process creation.
            // Return error to indicate incomplete vDSO setup.
            return Err(());
        }
        // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso, which runs single-threaded before any mapping.
        let info = unsafe { &*ptr };
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(info.user_vaddr));

        // SAFETY: info.frame was allocated by init_vdso and is a valid physical frame.
        // The page is a valid user-space virtual address from vdso_virt_addr_for_cpu.
        unsafe {
            mapper
                .map_to(page, info.frame, flags, frame_allocator)
                .map_err(|_| ())?
                .ignore();
        }
    }
    Ok(())
}

/// Map a single CPU's vDSO page into a process's address space.
///
/// Used by the page fault handler when a specific vDSO page is accessed.
pub fn vdso_map_cpu_page_into(
    cpu_id: usize,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), ()> {
    if cpu_id >= crate::task::scheduler::MAX_CPUS {
        return Err(());
    }
    let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
    if ptr.is_null() {
        return Err(());
    }
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::USER_ACCESSIBLE
        | PageTableFlags::NO_EXECUTE;
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    let page = Page::<Size4KiB>::containing_address(VirtAddr::new(info.user_vaddr));
    // SAFETY: info.frame was allocated by init_vdso and is a valid physical frame.
    unsafe {
        mapper
            .map_to(page, info.frame, flags, frame_allocator)
            .map_err(|_| ())?
            .ignore();
    }
    Ok(())
}

/// Get pointer to the current CPU's vDSO page (via HHDM for kernel access).
/// The seqlock in VdsoData protects data consistency for concurrent readers/writers.
/// Returns null if vDSO not initialized for this CPU.
#[inline]
pub fn vdso_page() -> *const VdsoData {
    let cpu_id = crate::smp::get_cpu_id();
    vdso_page_ptr(cpu_id)
}

/// Get pointer to a specific CPU's vDSO page (via HHDM for kernel access).
/// The seqlock in VdsoData protects data consistency for concurrent readers/writers.
/// Returns null if vDSO not initialized for this CPU.
pub fn vdso_page_ptr(cpu_id: usize) -> *const VdsoData {
    let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
    if ptr.is_null() {
        return core::ptr::null();
    }
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    // Use cached HHDM address to avoid PHYSICAL_MEMORY_OFFSET lookup
    info.hhdm_vaddr as *const VdsoData
}

/// Alias for vdso_page_ptr — kept for compatibility with tests.
#[cfg(feature = "self_test")]
#[inline]
pub fn vdso_page_for_cpu(cpu_id: usize) -> *const VdsoData {
    vdso_page_ptr(cpu_id)
}

/// Compute TSC multiplier and shift for nanosecond conversion.
/// Algorithm from Linux: mult = (1e9 << shift) / freq
/// We want mult = (1e9 * 2^shift) / freq to fit in u32.
/// For freq <= 1 GHz, shift=0 works. For freq > 1 GHz, increase shift until mult < 2^32.
fn compute_tsc_conversion(freq_hz: u64) -> (u32, u32) {
    const NSEC_PER_SEC: u64 = 1_000_000_000;
    if freq_hz == 0 {
        return (0, 0);
    }
    let mut shift = 0u32;
    let mut mult = (NSEC_PER_SEC << shift) / freq_hz;

    // Increase shift until mult >= 1 (handles freq > 1 GHz).
    // For real CPU frequencies (MHz–GHz), mult always fits in u32 at shift=0.
    // Extremely low frequencies (< 0.23 Hz) would need shift decrease (impossible at 0),
    // but such hardware doesn't exist. Second loop removed.
    while mult == 0 && shift < 32 {
        shift += 1;
        mult = (NSEC_PER_SEC << shift) / freq_hz;
    }

    debug_assert!(mult <= u32::MAX as u64, "TSC mult overflow");
    (mult as u32, shift)
}

/// Read TSC (x86_64 only).
fn read_tsc() -> (u32, u32) {
    let lo: u32;
    let hi: u32;
    // SAFETY: RDTSC is safe at any privilege level, no side effects.
    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
    (lo, hi)
}

/// Seqlock for cached RTC to prevent torn reads between sec and nsec.
static CACHED_RTC_SEQ: AtomicU64 = AtomicU64::new(0);
/// Cached RTC seconds, updated by vdso_update_cached_rtc().
static CACHED_RTC_SEC: AtomicU64 = AtomicU64::new(0);
/// Cached RTC nanoseconds, updated by vdso_update_cached_rtc().
static CACHED_RTC_NSEC: AtomicU64 = AtomicU64::new(0);

/// Update cached RTC time (called from kernel thread/workqueue, NOT from IRQ).
/// Does CMOS I/O — must NOT be called from interrupt context.
pub fn vdso_update_cached_rtc() {
    let (sec, nsec) = crate::drivers::rtc::read_realtime();
    // Start write: increment seq to odd (writer entering)
    let seq = CACHED_RTC_SEQ.fetch_add(1, Ordering::Release) + 1;
    debug_assert!(seq % 2 == 1, "RTC seqlock seq should be odd during write");

    // Write data with Release ordering
    CACHED_RTC_SEC.store(sec as u64, Ordering::Release);
    CACHED_RTC_NSEC.store(nsec as u32 as u64, Ordering::Release);

    // End write: increment seq to even (writer done)
    CACHED_RTC_SEQ.store(seq + 1, Ordering::Release);
}

/// Get cached RTC time (for vDSO updates). Lock-free, IRQ-safe.
/// Returns (0, 0) if a concurrent write is in progress.
pub fn get_cached_rtc() -> (u64, u32) {
    loop {
        let seq = CACHED_RTC_SEQ.load(Ordering::Acquire);
        if seq & 1 != 0 {
            // Writer in progress, retry
            continue;
        }
        let sec = CACHED_RTC_SEC.load(Ordering::Acquire);
        let nsec = CACHED_RTC_NSEC.load(Ordering::Acquire) as u32;
        let seq2 = CACHED_RTC_SEQ.load(Ordering::Acquire);
        if seq == seq2 {
            return (sec, nsec);
        }
        // Writer was active, retry
    }
}

/// Read current time using TSC (x86_64 only).
/// Returns (seconds, nanoseconds) for the specified clock.
/// Returns (0, 0) if TSC is not calibrated.
#[cfg(target_arch = "x86_64")]
pub fn vdso_read_tsc_time(clock_id: u64) -> (u64, u32) {
    let vdso_ptr = vdso_page();
    if vdso_ptr.is_null() {
        return (0, 0);
    }
    // SAFETY: vdso_page returns valid pointer if not null
    let vdso = unsafe { &*vdso_ptr };
    
    // Check if TSC is calibrated
    let tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
    if tsc_freq == 0 {
        return (0, 0);
    }
    
    // Read TSC calibration parameters
    let tsc_mult = vdso.tsc_mult;
    let tsc_shift = vdso.tsc_shift;
    let tsc_base = vdso.tsc_base.load(Ordering::Acquire);
    
    // Read current TSC
    let (lo, hi) = read_tsc();
    let tsc_now = ((hi as u64) << 32) | lo as u64;
    
    // Calculate elapsed TSC cycles
    let elapsed = tsc_now.wrapping_sub(tsc_base);
    
    if clock_id == 1 { // CLOCK_MONOTONIC
        // Monotonic: TSC-based time directly
        let ns = ((elapsed as u128 * tsc_mult as u128) >> tsc_shift) as u64;
        let sec = ns / 1_000_000_000;
        let nsec = (ns % 1_000_000_000) as u32;
        (sec, nsec)
    } else if clock_id == 0 { // CLOCK_REALTIME
        // CLOCK_REALTIME - use RTC base + TSC delta
        let (rt_sec, rt_nsec) = get_cached_rtc();
        let (lo, hi) = read_tsc();
        let tsc_now = ((hi as u64) << 32) | lo as u64;
        let tsc_base = vdso.tsc_base.load(Ordering::Acquire);
        let tsc_mult = vdso.tsc_mult;
        let tsc_shift = vdso.tsc_shift;
        let elapsed = tsc_now.wrapping_sub(tsc_base);
        let ns = ((elapsed as u128 * tsc_mult as u128) >> tsc_shift) as u64;
        let base_ns = rt_sec as u64 * 1_000_000_000 + rt_nsec as u64;
        let total_ns = base_ns + ns;
        let sec = total_ns / 1_000_000_000;
        let nsec = (total_ns % 1_000_000_000) as u32;
        (sec, nsec)
    } else {
        (0, 0)
    }
}

/// Timer tick interval in milliseconds (100 Hz = 10ms)
const TICK_MS: u64 = 10;

/// Update vDSO time fields for the current CPU (called from timer interrupt, 100 Hz).
///
/// Uses seqlock: odd seq = writing, even seq = consistent.
/// Fields updated: monotonic_sec, monotonic_nsec, realtime_sec, realtime_nsec.
pub fn vdso_update_time() {
    let ticks = crate::interrupts::get_ticks();
    let total_ms = ticks * TICK_MS;
    let monotonic_sec = (total_ms / 1000) as u64;
    let monotonic_nsec = ((total_ms % 1000) * 1_000_000) as u32;

    // Use cached RTC to avoid CMOS I/O on every tick
    let (rt_sec, rt_nsec) = get_cached_rtc();

    // Update ONLY the current CPU's page (timer interrupt runs on one CPU)
    let cpu_id = crate::smp::get_cpu_id();
    let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    // Use cached HHDM address to avoid PHYSICAL_MEMORY_OFFSET lookup in timer IRQ
    let hhdm_vaddr = info.hhdm_vaddr;
    // SAFETY: hhdm_vaddr points to a valid VdsoData page in HHDM.
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };

    // Start write: atomically increment seq to odd (writer entering)
    // Release ensures prior stores don't reorder after the increment.
    let seq = vdso.seq.fetch_add(1, Ordering::Release) + 1;
    debug_assert!(seq % 2 == 1, "vDSO seq should be odd during write");

    // Update time fields with Release ordering to ensure they're visible before the final
    // seq store. On x86 (TSO), Relaxed would suffice due to store-store ordering, but Release
    // makes the intent explicit and is correct on all architectures.
    vdso.monotonic_sec.store(monotonic_sec, Ordering::Release);
    vdso.monotonic_nsec.store(monotonic_nsec, Ordering::Release);
    vdso.realtime_sec.store(rt_sec as u64, Ordering::Release);
    vdso.realtime_nsec.store(rt_nsec as u32, Ordering::Release);

    // End write: increment seq to even (writer done) with Release ordering
    vdso.seq.store(seq + 1, Ordering::Release);
}

/// Update vDSO PID and CPU ID for a specific CPU (called from context switch).
pub fn vdso_update_pid_cpu(pid: u64, cpu_id: u32) {
    let cpu_idx = cpu_id as usize;
    if cpu_idx >= crate::task::scheduler::MAX_CPUS {
        return;
    }
    
    let ptr = VDSO_PAGES[cpu_idx].load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    // Use cached HHDM address
    let hhdm_vaddr = info.hhdm_vaddr;
    // SAFETY: hhdm_vaddr points to a valid VdsoData page in HHDM.
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };

    // Start write: atomically increment pid_cpu_seq to odd (writer entering)
    // Release ensures prior stores don't reorder after the increment.
    let seq = vdso.pid_cpu_seq.fetch_add(1, Ordering::Release) + 1;
    debug_assert!(seq % 2 == 1);

    // Update data fields with Release ordering to ensure they're visible before the final
    // seq store. On x86 (TSO), Relaxed would suffice due to store-store ordering, but Release
    // makes the intent explicit and is correct on all architectures.
    vdso.pid.store(pid, Ordering::Release);
    vdso.cpu_id.store(cpu_id, Ordering::Release);

    // End write: increment seq to even (writer done) with Release ordering
    vdso.pid_cpu_seq.store(seq + 1, Ordering::Release);
}

/// Update thread CPU time in the vDSO page for the current CPU.
/// Called from context switch to expose thread CPU time via CLOCK_SKYIOUS_THREAD_CPUTIME (100).
/// No seqlock needed — single AtomicU64 write is atomic on x86_64.
pub fn vdso_update_thread_cputime(cpu_id: u32, ns: u64) {
    let cpu_idx = cpu_id as usize;
    if cpu_idx >= crate::task::scheduler::MAX_CPUS {
        return;
    }
    
    let ptr = VDSO_PAGES[cpu_idx].load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    // Use cached HHDM address
    let hhdm_vaddr = info.hhdm_vaddr;
    // SAFETY: hhdm_vaddr points to a valid VdsoData page in HHDM.
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
    
    // Atomic store — no seqlock needed for single u64 on x86_64
    vdso.thread_cputime_ns.store(ns, Ordering::Release);
}

/// Update process info in the vDSO page for the current CPU.
/// Called from fork/clone/execve to expose process info via CLOCK_SKYIOUS_PROCESS_INFO (104).
/// No seqlock needed — single AtomicU64 writes are atomic on x86_64.
pub fn vdso_update_process_info(cpu_id: u32, cgroup_id: u64, namespace_flags: u64, cap_effective: u64) {
    let cpu_idx = cpu_id as usize;
    if cpu_idx >= crate::task::scheduler::MAX_CPUS {
        return;
    }
    
    let ptr = VDSO_PAGES[cpu_idx].load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    // Use cached HHDM address
    let hhdm_vaddr = info.hhdm_vaddr;
    // SAFETY: hhdm_vaddr points to a valid VdsoData page in HHDM.
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
    
    // Atomic stores — no seqlock needed for single u64 on x86_64
    vdso.process_cgroup_id.store(cgroup_id, Ordering::Release);
    vdso.process_namespace_flags.store(namespace_flags, Ordering::Release);
    vdso.process_cap_effective.store(cap_effective, Ordering::Release);
}

/// Update TSC base for a specific CPU (called when CPU comes online).
/// This should be called from AP bringup code after the CPU is fully online.
/// Uses pid_cpu_seq seqlock to protect tsc_base for consistency with other fields.
pub fn vdso_update_tsc_base(cpu_id: u32) {
    let cpu_idx = cpu_id as usize;
    if cpu_idx >= crate::task::scheduler::MAX_CPUS {
        return;
    }
    
    let ptr = VDSO_PAGES[cpu_idx].load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
    let info = unsafe { &*ptr };
    // Use cached HHDM address
    let hhdm_vaddr = info.hhdm_vaddr;
    // SAFETY: hhdm_vaddr points to a valid VdsoData page in HHDM (writable kernel mapping).
    // tsc_base is AtomicU64, so we can mutate it through a shared reference.
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };

    // Start write: increment pid_cpu_seq to odd (writer entering)
    // Release ensures prior stores don't reorder after the increment.
    let seq = vdso.pid_cpu_seq.fetch_add(1, Ordering::Release) + 1;
    debug_assert!(seq % 2 == 1);

    let (lo, hi) = read_tsc();
    let tsc_base = ((hi as u64) << 32) | lo as u64;
    vdso.tsc_base.store(tsc_base, Ordering::Release);

    // End write: increment pid_cpu_seq to even (writer done)
    vdso.pid_cpu_seq.store(seq + 1, Ordering::Release);
}

/// Update the current CPU's TSC base with a fresh TSC reading.
/// Called from timer interrupt or IPI handler on each CPU.
pub fn vdso_update_my_tsc_base() {
    let cpu_id = crate::smp::get_cpu_id() as u32;
    vdso_update_tsc_base(cpu_id);
}

/// Extern "C" wrapper for SMP IPI broadcast to update TSC base on each CPU.
/// Called via smp_broadcast_func with kind=3 (function call).
/// Increments the TSC sync acknowledgment counter after updating.
#[no_mangle]
pub extern "C" fn vdso_ipi_update_tsc_base(_arg: u64) {
    vdso_update_my_tsc_base();
    
    // Increment the acknowledgment counter after updating TSC base
    let cpu_id = crate::smp::get_cpu_id() as usize;
    if cpu_id < crate::task::scheduler::MAX_CPUS {
        let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
        if !ptr.is_null() {
            let info = unsafe { &*ptr };
            let hhdm_vaddr = info.hhdm_vaddr;
            let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
            vdso.tsc_sync_ack_count.fetch_add(1, Ordering::Release);
        }
    }
}

/// Synchronize TSC bases across all CPUs via IPI broadcast with acknowledgment.
/// Sends an IPI to all other CPUs to run vdso_ipi_update_tsc_base,
/// waits for all CPUs to acknowledge, then updates the current CPU's TSC base.
pub fn vdso_sync_tsc_across_cpus() {
    let max_cpus = crate::task::scheduler::MAX_CPUS;
    let current_cpu = crate::smp::get_cpu_id() as usize;
    
    // Reset acknowledgment counters on all CPUs
    for cpu_id in 0..max_cpus {
        let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
        if !ptr.is_null() {
            let info = unsafe { &*ptr };
            let hhdm_vaddr = info.hhdm_vaddr;
            let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
            vdso.tsc_sync_ack_count.store(0, Ordering::Release);
        }
    }
    
    // Increment sync sequence number
    let ptr = VDSO_PAGES[current_cpu].load(Ordering::Acquire);
    if !ptr.is_null() {
        let info = unsafe { &*ptr };
        let hhdm_vaddr = info.hhdm_vaddr;
        let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
        vdso.tsc_sync_seq.fetch_add(1, Ordering::Release);
    }
    
    // Broadcast to all other CPUs to update their TSC bases
    // kind=3 is the function-call IPI kind (see smp_broadcast_func)
    crate::smp::smp_broadcast_func(3, vdso_ipi_update_tsc_base as *const () as u64);
    
    // Update our own TSC base
    vdso_update_my_tsc_base();
    
    // Increment our own ack count
    if current_cpu < crate::task::scheduler::MAX_CPUS {
        let ptr = VDSO_PAGES[current_cpu].load(Ordering::Acquire);
        if !ptr.is_null() {
            let info = unsafe { &*ptr };
            let hhdm_vaddr = info.hhdm_vaddr;
            let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
            vdso.tsc_sync_ack_count.fetch_add(1, Ordering::Release);
        }
    }
    
    // Wait for all other CPUs to acknowledge
    // Count online CPUs (those with valid tsc_base)
    let mut online_cpus = 0;
    for cpu_id in 0..max_cpus {
        let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
        if !ptr.is_null() {
            let info = unsafe { &*ptr };
            let hhdm_vaddr = info.hhdm_vaddr;
            let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
            if vdso.tsc_base.load(Ordering::Acquire) != 0 {
                online_cpus += 1;
            }
        }
    }
    
    // Wait for acknowledgments (with timeout)
    const MAX_WAIT_LOOPS: u32 = 1_000_000;
    for _ in 0..MAX_WAIT_LOOPS {
        let mut ack_count = 0;
        for cpu_id in 0..max_cpus {
            let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
            if !ptr.is_null() {
                let info = unsafe { &*ptr };
                let hhdm_vaddr = info.hhdm_vaddr;
                let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };
                if vdso.tsc_sync_ack_count.load(Ordering::Acquire) > 0 {
                    ack_count += 1;
                }
            }
        }
        // We expect all online CPUs to have acknowledged (including ourselves)
        if ack_count >= online_cpus {
            break;
        }
        core::hint::spin_loop();
    }
}

/// Check TSC drift across all online CPUs.
/// Reads each CPU's tsc_base from its vDSO page, converts to nanoseconds
/// using the TSC frequency, and logs a warning if drift exceeds 1ms (1,000,000 ns).
pub fn vdso_check_tsc_drift() {
    // First, update our own TSC base to get a fresh reading
    vdso_update_my_tsc_base();

    const MAX_CPUS: usize = crate::task::scheduler::MAX_CPUS;
    let mut tsc_bases = [0u64; MAX_CPUS];
    let mut valid_cpus = 0;
    let mut tsc_freq = 0u64;

    // Read tsc_base from each CPU's vDSO page
    for cpu_id in 0..MAX_CPUS {
        let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
        if ptr.is_null() {
            continue;
        }
        // SAFETY: VDSO_PAGES only stores valid pointers from init_vdso.
        let info = unsafe { &*ptr };
        let hhdm_vaddr = info.hhdm_vaddr;
        // SAFETY: hhdm_vaddr points to a valid VdsoData page in HHDM.
        let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };

        let base = vdso.tsc_base.load(Ordering::Acquire);
        if base == 0 {
            continue;
        }
        tsc_bases[cpu_id] = base;
        valid_cpus += 1;

        // Read TSC frequency from the first valid CPU (all should be the same)
        if tsc_freq == 0 {
            tsc_freq = vdso.tsc_freq_hz.load(Ordering::Acquire);
        }
    }

    if valid_cpus < 2 || tsc_freq == 0 {
        // Need at least 2 CPUs and valid TSC frequency to check drift
        return;
    }

    // Find min and max tsc_base values
    let mut min_tsc = u64::MAX;
    let mut max_tsc = 0u64;
    let mut min_cpu = 0;
    let mut max_cpu = 0;

    for cpu_id in 0..MAX_CPUS {
        if tsc_bases[cpu_id] == 0 {
            continue;
        }
        if tsc_bases[cpu_id] < min_tsc {
            min_tsc = tsc_bases[cpu_id];
            min_cpu = cpu_id;
        }
        if tsc_bases[cpu_id] > max_tsc {
            max_tsc = tsc_bases[cpu_id];
            max_cpu = cpu_id;
        }
    }

    // Convert TSC difference to nanoseconds: ns = (tsc_diff * 1e9) / freq
    let tsc_diff = max_tsc.wrapping_sub(min_tsc);
    let drift_ns = (tsc_diff as u128 * 1_000_000_000u128 / tsc_freq as u128) as u64;

    const DRIFT_THRESHOLD_NS: u64 = 100; // 100ns target for best-in-class

    if drift_ns > DRIFT_THRESHOLD_NS {
        crate::serial_write("[TSC DRIFT] WARNING: TSC drift detected!\n");
        crate::serial_write("[TSC DRIFT] CPU ");
        crate::serial_write_hex(min_cpu as u64);
        crate::serial_write(" (tsc_base=0x");
        crate::serial_write_hex(min_tsc);
        crate::serial_write(") vs CPU ");
        crate::serial_write_hex(max_cpu as u64);
        crate::serial_write(" (tsc_base=0x");
        crate::serial_write_hex(max_tsc);
        crate::serial_write(") drift=");
        crate::serial_write_hex(drift_ns);
        crate::serial_write(" ns (threshold=1,000,000 ns)\n");
        crate::serial_write("[TSC DRIFT] TSC freq=");
        crate::serial_write_hex(tsc_freq);
        crate::serial_write(" Hz\n");
    }
}