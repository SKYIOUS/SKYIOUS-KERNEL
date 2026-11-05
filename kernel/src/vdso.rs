//! vDSO (Virtual Dynamic Shared Object) — Kernel data page mapped into userspace.
//!
//! Provides lock-free access to time, PID, and CPU ID without syscalls.
//! Updated by kernel on timer ticks and context switches; read by userspace via seqlock.
//!
//! Per-CPU pages: each CPU has its own vDSO page at VDSO_BASE + cpu_id * 4096
//! to eliminate cache-line false sharing on timer tick updates.

#![cfg(target_arch = "x86_64")]

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicPtr, Ordering};
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

/// Virtual address of vDSO page for the current CPU.
#[allow(dead_code)]
pub(crate) fn vdso_virt_addr() -> u64 {
    vdso_virt_addr_for_cpu(crate::smp::get_cpu_id())
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
///   0x058-0xFFF: reserved (zero)
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
    _reserved: [u64; 501], // Fill to 4096 bytes exactly (8+8+4+4+8+4+4+8+4+8+8+4+4+501*8 = 4096)
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

        // Initialize CACHED_RTC for the first CPU (only need to do this once)
        if cpu_id == 0 {
            CACHED_RTC[0].store(rt_sec as u64, Ordering::Release);
            CACHED_RTC[1].store(rt_nsec as u64, Ordering::Release);
            CACHED_RTC_INITIALIZED.store(true, Ordering::Release);
        }

        crate::serial_write("[VDSO] CPU ");
        crate::serial_write_hex(cpu_id as u64);
        crate::serial_write(" page at 0x");
        crate::serial_write_hex(user_vaddr);
        crate::serial_write("\n");
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

    // Increase shift until mult >= 1 (handles freq > 1 GHz)
    while mult == 0 && shift < 32 {
        shift += 1;
        mult = (NSEC_PER_SEC << shift) / freq_hz;
    }

    // Increase shift until mult fits in u32 (handles very low frequencies)
    while mult > u32::MAX as u64 && shift < 32 {
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

/// Cached RTC time for vDSO updates (updated once per second to avoid CMOS I/O on every tick)
static CACHED_RTC: [AtomicU64; 2] = [AtomicU64::new(0), AtomicU64::new(0)];

/// Tracks whether CACHED_RTC has been initialized (set in init_vdso, read in vdso_update_time)
static CACHED_RTC_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Timer tick interval in milliseconds (100 Hz = 10ms)
const TICK_MS: u64 = 10;

/// Update cached RTC time (called from kernel thread/workqueue, NOT from IRQ).
/// Does CMOS I/O — must NOT be called from interrupt context.
pub fn vdso_update_cached_rtc() {
    // CMOS I/O is slow and must not run in interrupt context
    let (sec, nsec) = crate::drivers::rtc::read_realtime();
    CACHED_RTC[0].store(sec as u64, Ordering::Release);
    CACHED_RTC[1].store(nsec as u32 as u64, Ordering::Release);
    // Ensure CACHED_RTC stores are visible before INITIALIZED flag (Release fence suffices)
    core::sync::atomic::fence(Ordering::Release);
    // Mark initialized after storing values
    CACHED_RTC_INITIALIZED.store(true, Ordering::Release);
}

/// Get cached RTC time (for vDSO updates). Lock-free, IRQ-safe.
/// Returns (0, 0) if not yet initialized (before init_vdso runs).
fn get_cached_rtc() -> (u64, u32) {
    // Fast check: if not initialized, return zero
    if !CACHED_RTC_INITIALIZED.load(Ordering::Acquire) {
        return (0, 0);
    }
    let sec = CACHED_RTC[0].load(Ordering::Acquire);
    let nsec = CACHED_RTC[1].load(Ordering::Acquire) as u32;
    (sec, nsec)
}

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