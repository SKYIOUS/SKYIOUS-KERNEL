//! vDSO (Virtual Dynamic Shared Object) — Kernel data page mapped into userspace.
//!
//! Provides lock-free access to time, PID, and CPU ID without syscalls.
//! Updated by kernel on timer ticks and context switches; read by userspace via seqlock.
//!
//! Per-CPU pages: each CPU has its own vDSO page at VDSO_BASE + cpu_id * 4096
//! to eliminate cache-line false sharing on timer tick updates.

#![cfg(target_arch = "x86_64")]

use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use vahi_hal::sync::IrqSafeMutex;
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

/// Per-CPU vDSO pages — kernel-accessible via HHDM.
/// Stores physical frame + userspace virtual address for each CPU.
#[derive(Clone, Copy)]
struct VdsoPageInfo {
    frame: PhysFrame<Size4KiB>,
    user_vaddr: u64,
}

/// Per-CPU page info array — write-once after init. Protected by IrqSafeMutex
/// for init; reads after init use lock() which is fast when uncontended.
static VDSO_PAGES: IrqSafeMutex<[Option<VdsoPageInfo>; MAX_CPUS]> = IrqSafeMutex::new([const { None }; MAX_CPUS]);

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

        // Initialize the page via the temporary mapping
        let vdso_ptr = user_vaddr as *mut VdsoData;
        unsafe { core::ptr::write_bytes(vdso_ptr, 0, 1) };
        let vdso = unsafe { &mut *vdso_ptr };

        // Initialize TSC calibration fields (read-only after boot)
        vdso.tsc_freq_hz.store(tsc_freq, Ordering::Relaxed);
        vdso.tsc_mult = mult;
        vdso.tsc_shift = shift;
        vdso.tsc_base.store(tsc_base, Ordering::Relaxed);
        vdso.cpu_id.store(cpu_id as u32, Ordering::Relaxed);

        // Initialize time fields from current tick counter and RTC
        let ticks = crate::interrupts::get_ticks();
        let total_ms = ticks * 10;
        vdso.monotonic_sec.store((total_ms / 1000) as u64, Ordering::Relaxed);
        vdso.monotonic_nsec.store(((total_ms % 1000) * 1_000_000) as u32, Ordering::Relaxed);
        let (rt_sec, rt_nsec) = crate::drivers::rtc::read_realtime();
        vdso.realtime_sec.store(rt_sec as u64, Ordering::Relaxed);
        vdso.realtime_nsec.store(rt_nsec as u32, Ordering::Relaxed);

        // Store frame info for later process mapping and kernel access via HHDM
        VDSO_PAGES.lock()[cpu_id] = Some(VdsoPageInfo { frame, user_vaddr });

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

    let pages = VDSO_PAGES.lock();
    for cpu_id in 0..MAX_CPUS {
        let info = pages[cpu_id].as_ref().ok_or(())?;
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

/// Get reference to the current CPU's vDSO page (via HHDM for kernel access).
/// Note: This is for IRQ-safe read-only access. The seqlock in VdsoData protects data consistency.
#[inline]
pub fn vdso_page() -> Option<&'static VdsoData> {
    let cpu_id = crate::smp::get_cpu_id();
    vdso_page_via_hhdm(cpu_id)
}

/// Get reference to a specific CPU's vDSO page (via HHDM for kernel access).
/// Note: This is for IRQ-safe read-only access. The seqlock in VdsoData protects data consistency.
pub fn vdso_page_for_cpu(cpu_id: usize) -> Option<&'static VdsoData> {
    vdso_page_via_hhdm(cpu_id)
}

/// Internal: access vDSO page via HHDM (kernel direct map).
/// The seqlock in VdsoData protects data consistency for concurrent readers/writers.
fn vdso_page_via_hhdm(cpu_id: usize) -> Option<&'static VdsoData> {
    let pages = VDSO_PAGES.lock();
    let info = pages[cpu_id].as_ref()?;
    let phys_offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
    let hhdm_vaddr = phys_offset + info.frame.start_address().as_u64();
    let ptr = hhdm_vaddr as u64 as *const VdsoData;
    Some(unsafe { &*ptr })
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
static CACHED_RTC: IrqSafeMutex<Option<(u64, u32)>> = IrqSafeMutex::new(None);

/// Update cached RTC time (called once per second from timer tick or dedicated thread)
pub fn vdso_update_cached_rtc() {
    let (sec, nsec) = crate::drivers::rtc::read_realtime();
    *CACHED_RTC.lock() = Some((sec as u64, nsec as u32));
}

/// Get cached RTC time (for vDSO updates).
/// Returns None if cache not yet initialized.
fn get_cached_rtc() -> Option<(u64, u32)> {
    CACHED_RTC.lock().as_ref().copied()
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
    let (rt_sec, rt_nsec) = get_cached_rtc().unwrap_or_else(|| {
        let (sec, nsec) = crate::drivers::rtc::read_realtime();
        (sec as u64, nsec as u32)
    });

    // Update ONLY the current CPU's page (timer interrupt runs on one CPU)
    let cpu_id = crate::smp::get_cpu_id();
    let pages = VDSO_PAGES.lock();
    let info = match pages[cpu_id].as_ref() {
        Some(i) => i,
        None => return,
    };

    let phys_offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(offset) => *offset,
        None => return,
    };
    let hhdm_vaddr = phys_offset + info.frame.start_address().as_u64();
    let vdso = unsafe { &*(hhdm_vaddr as u64 as *const VdsoData) };

    // Start write: increment seq to odd
    let seq = vdso.seq.fetch_add(1, Ordering::Acquire) + 1;
    debug_assert!(seq % 2 == 1, "vDSO seq should be odd during write");

    // Update time fields
    vdso.monotonic_sec.store(monotonic_sec, Ordering::Relaxed);
    vdso.monotonic_nsec.store(monotonic_nsec, Ordering::Relaxed);
    vdso.realtime_sec.store(rt_sec as u64, Ordering::Relaxed);
    vdso.realtime_nsec.store(rt_nsec as u32, Ordering::Relaxed);

    // End write: increment seq to even (release)
    vdso.seq.store(seq + 1, Ordering::Release);
}

/// Update vDSO PID and CPU ID for a specific CPU (called from context switch).
pub fn vdso_update_pid_cpu(pid: u64, cpu_id: u32) {
    let cpu_idx = cpu_id as usize;
    
    // No lock needed - VDSO_PAGES is write-once after init
    let pages = VDSO_PAGES.lock();
    let info = match pages[cpu_idx].as_ref() {
        Some(i) => i,
        None => return,
    };

    let phys_offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(offset) => *offset,
        None => return,
    };
    let hhdm_vaddr = phys_offset + info.frame.start_address().as_u64();
    let vdso = unsafe { &*(hhdm_vaddr as u64 as *const VdsoData) };

    let seq = vdso.seq.fetch_add(1, Ordering::Acquire) + 1;
    debug_assert!(seq % 2 == 1);

    vdso.pid.store(pid, Ordering::Relaxed);
    vdso.cpu_id.store(cpu_id, Ordering::Relaxed);

    vdso.seq.store(seq + 1, Ordering::Release);
}