//! vDSO (Virtual Dynamic Shared Object) — Kernel data page mapped into userspace.
//!
//! Provides lock-free access to time, PID, and CPU ID without syscalls.
//! Updated by kernel on timer ticks and context switches; read by userspace via seqlock.
//!
//! Per-CPU pages: each CPU has its own vDSO page at VDSO_BASE + cpu_id * 4096
//! to eliminate cache-line false sharing on timer tick updates.

#![cfg(target_arch = "x86_64")]

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicPtr, Ordering};
use x86_64::structures::paging::{FrameAllocator, Size4KiB, PhysFrame, Mapper, Page, PageTableFlags, OffsetPageTable};
use x86_64::VirtAddr;

/// Maximum number of CPUs (must match crate::task::scheduler::MAX_CPUS)
pub const MAX_CPUS: usize = crate::task::scheduler::MAX_CPUS;

/// Base virtual address for vDSO pages in user space.
/// Each CPU gets a page at VDSO_BASE + cpu_id * 4096.
/// x86_64: 256 KiB below USER_MMAP_TOP (0x0000_7FFF_FFFF_0000)
pub const VDSO_BASE: u64 = 0x0000_7FFF_FFFC_0000;

/// Virtual address of vDSO page for a specific CPU.
pub const fn vdso_virt_addr_for_cpu(cpu_id: usize) -> u64 {
    VDSO_BASE + (cpu_id as u64) * 4096
}

/// Virtual address of vDSO page for the current CPU.
pub fn vdso_virt_addr() -> u64 {
    vdso_virt_addr_for_cpu(crate::smp::get_cpu_id())
}

/// vDSO data structure — exactly one page (4096 bytes), cache-line aligned.
///
/// Layout (offsets verified by seqlock protocol):
///   0x000: seq (AtomicU64) — even = consistent, odd = updating
///   0x008: monotonic_sec (AtomicU64)
///   0x010: monotonic_nsec (AtomicU32) + padding
///   0x018: realtime_sec (AtomicU64)
///   0x020: realtime_nsec (AtomicU32) + padding
///   0x028: tsc_freq_hz (AtomicU64)
///   0x030: tsc_mult (u32) + tsc_shift (u32)
///   0x038: tsc_base (AtomicU64)
///   0x040: pid (AtomicU64)
///   0x048: cpu_id (AtomicU32) + padding
///   0x050-0xFFF: reserved (zero)
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
    pub tsc_mult: u32,
    pub tsc_shift: u32,
    pub tsc_base: AtomicU64,
    pub pid: AtomicU64,
    pub cpu_id: AtomicU32,
    _pad3: [u32; 1],
    _reserved: [u64; 496], // Fill to 4096 bytes
}

// Compile-time size check
const _: () = assert!(core::mem::size_of::<VdsoData>() == 4096);

/// Per-CPU vDSO page info for kernel access via HHDM.
/// Stores physical frame + userspace virtual address for each CPU.
#[derive(Clone, Copy)]
struct VdsoPageInfo {
    frame: PhysFrame<Size4KiB>,
    user_vaddr: u64,
}

/// Per-CPU page info array — write-once after init, lock-free reads via AtomicPtr.
/// Each entry is an AtomicPtr<VdsoPageInfo>, null = not initialized.
static VDSO_PAGES: [AtomicPtr<VdsoPageInfo>; MAX_CPUS] = [const { AtomicPtr::new(core::ptr::null_mut()) }; MAX_CPUS];

/// Heap-allocated VdsoPageInfo storage (write-once). Direct array without Option to avoid pointer casting issues.
static mut VDSO_PAGE_INFOS: [VdsoPageInfo; MAX_CPUS] = unsafe { 
    [VdsoPageInfo { 
        frame: PhysFrame::from_start_address_unchecked(x86_64::PhysAddr::new(0)), 
        user_vaddr: 0 
    }; MAX_CPUS] 
};

/// Create a kernel page table mapper for temporary mappings.
fn kernel_mapper() -> Option<OffsetPageTable<'static>> {
    use x86_64::registers::control::Cr3;
    use x86_64::VirtAddr;
    
    // Get the kernel PML4 from the current CR3
    let (frame, _) = Cr3::read();
    let phys_offset = VirtAddr::new(*crate::memory::PHYSICAL_MEMORY_OFFSET.get()?);
    let pml4_virt = VirtAddr::new(phys_offset.as_u64() + frame.start_address().as_u64());
    
    // SAFETY: We're reading the kernel page tables which are always valid.
    Some(unsafe { OffsetPageTable::new(&mut *(pml4_virt.as_mut_ptr()), phys_offset) })
}

/// Initialize the vDSO pages (one per CPU).
///
/// Allocates one physical frame per CPU, zeroes them, initializes TSC calibration
/// fields, and stores the references globally. Must be called early in boot
/// (after frame allocator is ready, before first process creation).
pub fn init_vdso(frame_allocator: &mut impl FrameAllocator<Size4KiB>) {
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
    let (lo, hi) = read_tsc();
    let tsc_base = ((hi as u64) << 32) | lo as u64;

    // Allocate and initialize one page per CPU
    for cpu_id in 0..MAX_CPUS {
        let frame = frame_allocator
            .allocate_frame()
            .expect("vDSO: failed to allocate frame");

        let user_vaddr = vdso_virt_addr_for_cpu(cpu_id);

        // Map it temporarily in kernel space to initialize
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(user_vaddr));
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

        let mut mapper = kernel_mapper().expect("vDSO: kernel mapper not available");
        unsafe { mapper.map_to(page, frame, flags, frame_allocator) }
            .expect("vDSO: failed to map page")
            .flush();

        // Initialize the page via HHDM (same mapping used by readers) to ensure cache coherency
        let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().expect("HHDM offset not set");
        let hhdm_vaddr = phys_offset + frame.start_address().as_u64();
        let vdso_ptr = hhdm_vaddr as *mut VdsoData;
        unsafe { core::ptr::write_bytes(vdso_ptr, 0, 1) };
        let vdso = unsafe { &mut *vdso_ptr };

        // Initialize TSC calibration fields (read-only after boot)
        vdso.tsc_freq_hz.store(tsc_freq, Ordering::Release);
        vdso.tsc_mult = mult;
        vdso.tsc_shift = shift;
        vdso.tsc_base.store(tsc_base, Ordering::Release);
        vdso.cpu_id.store(cpu_id as u32, Ordering::Release);

        // Initialize time fields from current tick counter and RTC
        let ticks = crate::interrupts::get_ticks();
        let total_ms = ticks * 10;
        vdso.monotonic_sec.store((total_ms / 1000) as u64, Ordering::Release);
        vdso.monotonic_nsec.store(((total_ms % 1000) * 1_000_000) as u32, Ordering::Release);
        let (rt_sec, rt_nsec) = crate::drivers::rtc::read_realtime();
        vdso.realtime_sec.store(rt_sec as u64, Ordering::Release);
        vdso.realtime_nsec.store(rt_nsec as u32, Ordering::Release);

        // Ensure all initialization writes are visible before publishing the page pointer
        core::sync::atomic::fence(Ordering::SeqCst);

        // Store frame info in static array
        let info = VdsoPageInfo { frame, user_vaddr };
        unsafe {
            VDSO_PAGE_INFOS[cpu_id] = info;
            let ptr = &raw mut VDSO_PAGE_INFOS[cpu_id] as *mut VdsoPageInfo;
            VDSO_PAGES[cpu_id].store(ptr, Ordering::Release);
        }

        // Unmap from kernel space
        let (_, _flush) = mapper.unmap(page).expect("vDSO: failed to unmap");

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

    for cpu_id in 0..MAX_CPUS {
        let ptr = VDSO_PAGES[cpu_id].load(Ordering::Acquire);
        if ptr.is_null() {
            return Err(());
        }
        // SAFETY: ptr is valid after init
        let info = unsafe { &*ptr };
        let page = Page::<Size4KiB>::containing_address(VirtAddr::new(info.user_vaddr));

        // SAFETY: frame was allocated by init_vdso and is valid.
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
    let info = unsafe { &*ptr };
    let phys_offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(offset) => *offset,
        None => return core::ptr::null(),
    };
    let hhdm_vaddr = phys_offset + info.frame.start_address().as_u64();
    hhdm_vaddr as *const VdsoData
}

/// Alias for vdso_page_ptr — kept for compatibility with tests.
#[inline]
pub fn vdso_page_for_cpu(cpu_id: usize) -> *const VdsoData {
    vdso_page_ptr(cpu_id)
}

/// Compute TSC multiplier and shift for nanosecond conversion.
/// Algorithm from Linux: mult = (1e9 << shift) / freq
fn compute_tsc_conversion(freq_hz: u64) -> (u32, u32) {
    const NSEC_PER_SEC: u64 = 1_000_000_000;
    if freq_hz == 0 {
        return (0, 0);
    }
    let mut shift = 0u32;
    let mut mult = (NSEC_PER_SEC << shift) / freq_hz;

    // Increase shift until mult fits in u32
    while mult > u32::MAX as u64 && shift < 32 {
        shift += 1;
        mult = (NSEC_PER_SEC << shift) / freq_hz;
    }

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

/// Update cached RTC time (called from kernel thread/workqueue, NOT from IRQ).
/// Does CMOS I/O — must NOT be called from interrupt context.
pub fn vdso_update_cached_rtc() {
    let (sec, nsec) = crate::drivers::rtc::read_realtime();
    CACHED_RTC[0].store(sec as u64, Ordering::Release);
    CACHED_RTC[1].store(nsec as u32 as u64, Ordering::Release);
}

/// Get cached RTC time (for vDSO updates). Lock-free, IRQ-safe.
fn get_cached_rtc() -> (u64, u32) {
    let sec = CACHED_RTC[0].load(Ordering::Acquire);
    let nsec = CACHED_RTC[1].load(Ordering::Acquire) as u32;
    if sec == 0 && nsec == 0 {
        // Cache not initialized — read RTC directly (slow, but rare)
        let (sec, nsec) = crate::drivers::rtc::read_realtime();
        (sec as u64, nsec as u32)
    } else {
        (sec, nsec)
    }
}

/// Update vDSO time fields for the current CPU (called from timer interrupt, 100 Hz).
///
/// Uses seqlock: odd seq = writing, even seq = consistent.
/// Fields updated: monotonic_sec, monotonic_nsec, realtime_sec, realtime_nsec.
pub fn vdso_update_time() {
    let ticks = crate::interrupts::get_ticks();
    let total_ms = ticks * 10; // 100 Hz = 10ms per tick
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
    let info = unsafe { &*ptr };
    let phys_offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(offset) => *offset,
        None => return,
    };
    let hhdm_vaddr = phys_offset + info.frame.start_address().as_u64();
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };

    // Start write: increment seq to odd (writer entering)
    // Use AcqRel on fetch_add to order prior stores before the seq increment
    let seq = vdso.seq.fetch_add(1, Ordering::AcqRel) + 1;
    debug_assert!(seq % 2 == 1, "vDSO seq should be odd during write");

    // Update time fields with Release ordering (visible before final seq store)
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
    
    let ptr = VDSO_PAGES[cpu_idx].load(Ordering::Acquire);
    if ptr.is_null() {
        return;
    }
    let info = unsafe { &*ptr };

    let phys_offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(offset) => *offset,
        None => return,
    };
    let hhdm_vaddr = phys_offset + info.frame.start_address().as_u64();
    let vdso = unsafe { &*(hhdm_vaddr as *const VdsoData) };

    // Start write: increment seq to odd (writer entering)
    let seq = vdso.seq.fetch_add(1, Ordering::AcqRel) + 1;
    debug_assert!(seq % 2 == 1);

    // Update data fields with Release ordering
    vdso.pid.store(pid, Ordering::Release);
    vdso.cpu_id.store(cpu_id, Ordering::Release);

    // End write: increment seq to even (writer done) with Release ordering
    vdso.seq.store(seq + 1, Ordering::Release);
}