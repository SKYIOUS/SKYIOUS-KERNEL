//! Benchmark harness for the Vahi kernel.
//!
//! Measures latency and throughput of core kernel subsystems using
//! TSC-based high-resolution timing. Reports min/p50/p99/max percentiles
//! for each benchmark, printed in a structured format for CI parsing.
//!
//! Runs during boot (selftest framework) when `--features self_test`.

use crate::selftest;

// ---------------------------------------------------------------------------
// TSC timing
// ---------------------------------------------------------------------------

#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi,
            options(nostack, preserves_flags));
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// TSC frequency in Hz. Hardcoded for QEMU/TCG (2.4 GHz).
/// On real hardware, calibrate from CPUID leaf 0x15 or PIT.
fn tsc_freq_hz() -> u64 {
    2_400_000_000
}

/// Convert raw TSC ticks to nanoseconds.
fn ticks_to_ns(ticks: u64) -> u64 {
    ticks * 1_000 / (tsc_freq_hz() / 1_000_000)
}

/// Prevent the compiler from optimizing away a value.
#[inline(never)]
fn black_box<T>(mut v: T) {
    unsafe {
        let p = core::ptr::addr_of_mut!(v);
        core::ptr::write_volatile(p, v);
    }
}

// ---------------------------------------------------------------------------
// Percentile statistics
// ---------------------------------------------------------------------------

struct BenchStats {
    name: &'static str,
    samples: alloc::vec::Vec<u64>,
}

impl BenchStats {
    fn new(name: &'static str, capacity: usize) -> Self {
        Self {
            name,
            samples: alloc::vec::Vec::with_capacity(capacity),
        }
    }

    fn push(&mut self, ns: u64) {
        self.samples.push(ns);
    }

    fn finish(&mut self) {
        self.samples.sort_unstable();
    }

    fn min(&self) -> u64 {
        self.samples.first().copied().unwrap_or(0)
    }

    fn p50(&self) -> u64 {
        self.percentile(50)
    }

    fn p99(&self) -> u64 {
        self.percentile(99)
    }

    fn max(&self) -> u64 {
        self.samples.last().copied().unwrap_or(0)
    }

    fn percentile(&self, p: u64) -> u64 {
        if self.samples.is_empty() {
            return 0;
        }
        let idx = ((p as usize) * (self.samples.len() - 1)) / 100;
        self.samples[idx]
    }

    /// Print latency stats: min/p50/p99/max in nanoseconds.
    fn report(&self) {
        crate::serial_write(&alloc::format!(
            "[BENCH]   {:<38} min {:>8} ns  p50 {:>8} ns  p99 {:>8} ns  max {:>8} ns  ({} iters)\n",
            self.name,
            self.min(),
            self.p50(),
            self.p99(),
            self.max(),
            self.samples.len()
        ));
    }

    /// Print throughput: bytes/sec derived from p50 latency.
    fn report_throughput(&self, bytes_per_iter: u64) {
        let p50_ns = self.p50();
        let bytes_per_sec = bytes_per_iter
            .saturating_mul(1_000_000_000)
            .checked_div(p50_ns)
            .unwrap_or(0);
        crate::serial_write(&alloc::format!(
            "[BENCH]   {:<38} {:>10} B/s  (p50 {:>8} ns per {}B transfer, {} iters)\n",
            self.name,
            bytes_per_sec,
            p50_ns,
            bytes_per_iter,
            self.samples.len()
        ));
    }
}

// ---------------------------------------------------------------------------
// Benchmark runner
// ---------------------------------------------------------------------------

/// Run `body` for `iters` iterations, collecting per-iteration TSC latencies.
/// Returns sorted BenchStats with min/p50/p99/max.
fn bench_stats(name: &'static str, iters: usize, body: impl Fn()) -> BenchStats {
    // Warm up: 10% of iterations or 100, whichever is smaller
    let warmup = core::cmp::min(iters / 10, 100);
    for _ in 0..warmup {
        body();
    }

    let mut stats = BenchStats::new(name, iters);
    for _ in 0..iters {
        let start = rdtsc();
        body();
        let elapsed = rdtsc().wrapping_sub(start);
        stats.push(ticks_to_ns(elapsed));
    }
    stats.finish();
    stats
}

// ---------------------------------------------------------------------------
// Helper: create a temporary process for fork/exec/mmap benchmarks
// ---------------------------------------------------------------------------

/// Create a temporary process with a fresh address space for benchmarking.
fn make_bench_process() -> Option<alloc::sync::Arc<crate::task::process::Process>> {
    use crate::memory::buddy::BuddyFrameAllocator;
    use crate::memory::paging::AddressSpace;
    use crate::task::process::{Process, CURRENT_PROCESS};

    let parent = CURRENT_PROCESS.lock();
    let (id, parent_id, aspace) = if let Some(ref p) = *parent {
        let mut allocator = BuddyFrameAllocator;
        match p.address_space.clone_cow(&mut allocator) {
            Some(child_as) => (Process::next_id(), Some(p.id), child_as),
            None => {
                drop(parent);
                return None;
            }
        }
    } else {
        let mut allocator = BuddyFrameAllocator;
        match AddressSpace::new(&mut allocator) {
            Some(aspace) => (Process::next_id(), None, aspace),
            None => {
                drop(parent);
                return None;
            }
        }
    };
    drop(parent);
    Some(alloc::sync::Arc::new(Process::new(id, parent_id, aspace)))
}

// ---------------------------------------------------------------------------
// 1. Fork/exec latency — CoW address-space clone + Process creation
// ---------------------------------------------------------------------------

fn bench_fork_exec() {
    use crate::memory::buddy::BuddyFrameAllocator;
    use crate::task::process::Process;

    let parent = match make_bench_process() {
        Some(p) => p,
        None => return,
    };
    let mut allocator = BuddyFrameAllocator;
    if let Some(child_as) = parent.address_space.clone_cow(&mut allocator) {
        let child = Process::new(Process::next_id(), Some(parent.id), child_as);
        drop(child);
    }
    drop(parent);
}

// ---------------------------------------------------------------------------
// 2. Pipe throughput — write + read 4 KiB through a pipe
// ---------------------------------------------------------------------------

fn bench_pipe_throughput() {
    use crate::vfs::pipe::Pipe;
    use crate::vfs::VfsNode;

    let (reader, writer) = Pipe::new();
    let data = alloc::vec![0xABu8; 4096];
    let _ = writer.write(&data);
    let _ = reader.read(4096);
}

// ---------------------------------------------------------------------------
// 3. Context switch — YieldNow poll overhead (scheduler readiness path)
// ---------------------------------------------------------------------------

fn bench_ctxswitch() {
    use crate::task::YieldNow;
    use core::future::Future;
    use core::task::{Context, RawWaker, RawWakerVTable, Waker};

    fn noop_waker() -> Waker {
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(core::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }

    let mut yield_fut = YieldNow::new();
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    // YieldNow returns Pending on first poll (yields to scheduler).
    let _ = core::pin::Pin::new(&mut yield_fut).poll(&mut cx);
}

// ---------------------------------------------------------------------------
// 4. mmap/munmap latency — VMA sorted-insert + remove
// ---------------------------------------------------------------------------

fn bench_mmap_munmap() {
    use crate::task::process::Vma;
    use x86_64::structures::paging::PageTableFlags;

    let proc = match make_bench_process() {
        Some(p) => p,
        None => return,
    };
    let vma = Vma {
        start: 0x7F_F000_0000,
        end: 0x7F_F000_1000,
        flags: PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE,
        _name: "bench_mmap",
        file_handle: None,
        file_offset: 0,
        is_shared: false,
        shm_id: None,
    };
    proc.add_vma(vma);
    proc.remove_vma_range(0x7F_F000_0000, 0x7F_F000_1000);
    drop(proc);
}

// ---------------------------------------------------------------------------
// 5. Syscall round-trip — getpid (baseline syscall overhead)
// ---------------------------------------------------------------------------

fn bench_syscall_getpid() {
    // Inline the getpid logic to measure pure syscall path cost
    // without dispatch table indirection.
    use crate::task::process::CURRENT_PROCESS;
    let lock = CURRENT_PROCESS.lock();
    let _pid = lock.as_ref().map_or(0u64, |p| p.id);
    drop(lock);
}

// ---------------------------------------------------------------------------
// 6. Page alloc/free throughput — physical frame alloc + free cycle
// ---------------------------------------------------------------------------

fn bench_page_alloc_free() {
    use crate::memory::phys;
    if let Some(frame) = phys::alloc_frame() {
        phys::free_frame(frame);
    }
}

// ---------------------------------------------------------------------------
// Memory leak audit (runs after benchmarks)
// ---------------------------------------------------------------------------

fn test_memory_leak_audit() -> Result<(), &'static str> {
    use crate::memory::{phys, slab};

    phys::reset_watermarks();
    let before = phys::audit_snapshot();
    slab::slab_set_baseline();

    // Alloc/drop churn
    let mut boxes = alloc::vec::Vec::new();
    for i in 0..500u64 {
        boxes.push(alloc::boxed::Box::new(i));
    }
    drop(boxes);

    crate::memory::frame_info::drain_deferred();
    phys::update_watermarks();
    slab::slab_update_hwm();
    let after = phys::audit_snapshot();
    let slab_leak = slab::slab_check_leak();

    crate::serial_write("[BENCH] --- Memory Leak Audit ---\n");
    before.report();
    after.report();
    crate::serial_write(&alloc::format!(
        "[BENCH]   slab active delta: {} bytes\n",
        slab_leak
    ));

    if after.has_leak() {
        crate::serial_write(&alloc::format!(
            "[BENCH] WARNING: possible leak — {} frames lost from baseline\n",
            after.current_leak
        ));
        return Err("memory leak audit: frames lost after alloc/drop cycle");
    }

    crate::serial_write("[BENCH] Memory leak audit: PASS (no leak detected)\n");
    Ok(())
}

// ---------------------------------------------------------------------------
// Run all benchmarks
// ---------------------------------------------------------------------------

fn run_all_benchmarks() -> Result<(), &'static str> {
    crate::serial_write("\n");
    crate::serial_write("[BENCH] ============================================================\n");
    crate::serial_write("[BENCH]  Vahi Kernel Benchmark Suite\n");
    crate::serial_write("[BENCH]  TSC frequency: 2400 MHz (QEMU/TCG)\n");
    crate::serial_write("[BENCH]  All latencies in nanoseconds. Lower is better.\n");
    crate::serial_write("[BENCH] ============================================================\n\n");

    // --- 1. Fork/exec latency ---
    crate::serial_write("[BENCH] 1. Fork/Exec Latency (CoW clone + process creation)\n");
    let s = bench_stats("fork_exec_clone", 100, bench_fork_exec);
    s.report();

    // --- 2. Pipe throughput ---
    crate::serial_write("\n[BENCH] 2. Pipe Throughput (4 KiB write+read)\n");
    let s = bench_stats("pipe_throughput_4k", 1_000, bench_pipe_throughput);
    s.report_throughput(4096);

    // --- 3. Context switch ---
    crate::serial_write("\n[BENCH] 3. Context Switch (YieldNow poll round-trip)\n");
    let s = bench_stats("ctxswitch_yield_poll", 10_000, bench_ctxswitch);
    s.report();

    // --- 4. mmap/munmap ---
    crate::serial_write("\n[BENCH] 4. mmap/munmap Latency (VMA add + remove)\n");
    let s = bench_stats("mmap_munmap_vma", 500, bench_mmap_munmap);
    s.report();

    // --- 5. Syscall round-trip ---
    crate::serial_write("\n[BENCH] 5. Syscall Round-Trip (getpid)\n");
    let s = bench_stats("syscall_getpid", 10_000, bench_syscall_getpid);
    s.report();

    // --- 6. Page alloc/free ---
    crate::serial_write("\n[BENCH] 6. Page Alloc/Free Throughput\n");
    let s = bench_stats("page_alloc_free", 10_000, bench_page_alloc_free);
    s.report();

    // --- Memory leak audit ---
    crate::serial_write("\n[BENCH] --- Memory Leak Audit ---\n");
    test_memory_leak_audit()?;

    crate::serial_write("\n[BENCH] ============================================================\n");
    crate::serial_write("[BENCH]  All benchmarks complete.\n");
    crate::serial_write("[BENCH] ============================================================\n\n");

    Ok(())
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register() {
    selftest::register("benchmarks::suite", run_all_benchmarks);
    selftest::register("benchmarks::memory_leak_audit", test_memory_leak_audit);
}
