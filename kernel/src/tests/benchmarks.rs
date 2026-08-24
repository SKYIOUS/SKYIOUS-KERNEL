//! Benchmark harness for the Vahi kernel.
//!
//! Measures throughput of core kernel operations using TSC-based timing.
//! Results are printed to serial in a machine-parseable format for CI.
//!
//! All benchmarks run during boot (selftest framework) and report:
//! - Operations per second
//! - Latency per operation (microseconds)
//! - Total elapsed time

use crate::selftest;

/// Read the TSC (timestamp counter) directly for high-resolution timing.
/// Returns raw TSC ticks — convert to microseconds using the TSC frequency.
#[inline]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags));
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// TSC frequency in Hz. Hardcoded to 2.4 GHz for QEMU/TCG.
/// On real hardware, this should be calibrated from CPUID or PIT.
fn tsc_freq_hz() -> u64 {
    2_400_000_000
}

/// Convert raw TSC ticks to microseconds
fn tsc_to_us(ticks: u64) -> u64 {
    ticks / (tsc_freq_hz() / 1_000_000)
}

/// Result of a single benchmark run
struct BenchResult {
    name: &'static str,
    iterations: u64,
    elapsed_us: u64,
    ops_per_sec: u64,
    us_per_op: u64,
}

impl BenchResult {
    fn report(&self) {
        crate::serial_write(&alloc::format!(
            "[BENCH] {:<35} {:>8} iters  {:>10} us  {:>10} ops/s  {:>8} us/op\n",
            self.name, self.iterations, self.elapsed_us, self.ops_per_sec, self.us_per_op
        ));
    }
}

fn bench(name: &'static str, iterations: u64, body: impl Fn()) -> BenchResult {
    // Warm up
    for _ in 0..core::cmp::min(iterations / 10, 100) {
        body();
    }

    let start = rdtsc();
    for _ in 0..iterations {
        body();
    }
    let elapsed_ticks = rdtsc().wrapping_sub(start);
    let elapsed_us = tsc_to_us(elapsed_ticks);
    let ops_per_sec = if elapsed_us > 0 {
        (iterations * 1_000_000) / elapsed_us
    } else {
        0
    };
    let us_per_op = if iterations > 0 {
        elapsed_us / iterations
    } else {
        0
    };

    BenchResult { name, iterations, elapsed_us, ops_per_sec, us_per_op }
}

// ---------------------------------------------------------------------------
// Benchmark: Heap allocation/deallocation
// ---------------------------------------------------------------------------

fn bench_box_alloc_dealloc() {
    let b = alloc::boxed::Box::new(42u64);
    drop(b);
}

fn bench_vec_push_drop() {
    let mut v = alloc::vec::Vec::with_capacity(256);
    for i in 0..256u64 {
        v.push(i);
    }
    drop(v);
}

fn bench_large_alloc() {
    // Heap-allocated, not stack — safe for kernel context
    let b = alloc::vec![0u8; 4096];
    drop(b);
}

// ---------------------------------------------------------------------------
// Benchmark: Lock operations
// ---------------------------------------------------------------------------

fn bench_mutex_lock_unlock() {
    use crate::sync::IrqSafeMutex;

    static LOCK: IrqSafeMutex<u64> = IrqSafeMutex::new(0);
    let mut guard = LOCK.lock();
    *guard += 1;
    drop(guard);
}

fn bench_atomic_increment() {
    use core::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Benchmark: Scheduler operations
// ---------------------------------------------------------------------------

fn bench_yield_now() {
    // Measure the cost of a YieldNow construction (not the actual yield,
    // since we can't yield in selftest context)
    let _yield = crate::task::YieldNow::new();
}

fn bench_spin_loop() {
    core::hint::spin_loop();
}

// ---------------------------------------------------------------------------
// Benchmark: String/format operations
// ---------------------------------------------------------------------------

fn bench_format_u64() {
    // Measure pure formatting cost (no heap allocation) by writing to a stack buffer
    use core::fmt::Write;
    struct Buf([u8; 32], usize);
    impl Write for Buf {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let end = self.1 + bytes.len();
            if end <= self.0.len() {
                self.0[self.1..end].copy_from_slice(bytes);
                self.1 = end;
            }
            Ok(())
        }
    }
    let mut buf = Buf([0u8; 32], 0);
    write!(buf, "{}", 12345678u64).ok();
    black_box(buf);
}

// ---------------------------------------------------------------------------
// Benchmark: VFS operations (simulated)
// ---------------------------------------------------------------------------

fn bench_path_parse() {
    // Simulate path parsing by iterating bytes
    let path = b"/proc/1234/fd/5";
    let mut components = 0;
    let mut in_component = false;
    for &b in path {
        if b == b'/' {
            if in_component {
                components += 1;
            }
            in_component = false;
        } else {
            in_component = true;
        }
    }
    if in_component {
        components += 1;
    }
    // Prevent optimization
    black_box(components);
}

/// Prevent the compiler from optimizing away a value.
/// Uses a volatile write through a raw pointer to force the compiler to
/// retain the computation without eliding it.
#[inline(never)]
fn black_box<T>(mut v: T) {
    unsafe {
        let p = core::ptr::addr_of_mut!(v);
        core::ptr::write_volatile(p, v);
    }
}

// ---------------------------------------------------------------------------
// Benchmark: Crypto operations
// ---------------------------------------------------------------------------

fn bench_entropy_read() {
    let _val = crate::crypto::GLOBAL_ENTROPY.get_u64();
}

// ---------------------------------------------------------------------------
// Benchmark: Memory operations
// ---------------------------------------------------------------------------

fn bench_memcpy_4k() {
    let src = [0u8; 4096];
    let mut dst = [0u8; 4096];
    dst.copy_from_slice(&src);
    black_box(dst);
}

fn bench_memset_4k() {
    let mut buf = [0u8; 4096];
    buf.fill(0xAB);
    black_box(buf);
}

// ---------------------------------------------------------------------------
// Benchmark: Pipe bandwidth
// ---------------------------------------------------------------------------

fn bench_pipe_throughput_4k() {
    use crate::vfs::pipe::Pipe;
    use crate::vfs::VfsNode;

    let (reader, writer) = Pipe::new();
    let data = alloc::vec![0xABu8; 4096];
    let _ = writer.write(&data);
    let _ = reader.read(4096);
}

fn bench_pipe_throughput_64k() {
    use crate::vfs::pipe::Pipe;
    use crate::vfs::VfsNode;

    let (reader, writer) = Pipe::new();
    let data = alloc::vec![0xCDu8; 65536];
    let _ = writer.write(&data);
    let _ = reader.read(65536);
}

fn bench_pipe_create_destroy() {
    use crate::vfs::pipe::Pipe;
    let (reader, writer) = Pipe::new();
    drop(reader);
    drop(writer);
}

// ---------------------------------------------------------------------------
// Benchmark: Fork prerequisites (CoW clone, process creation, VMA setup)
// ---------------------------------------------------------------------------

/// Create a temporary process with a fresh address space for benchmarking.
/// The caller must drop it when done. Note: AddressSpace has no Drop impl,
/// so the PML4 frame leaks — acceptable for boot-time QEMU benchmarks.
fn make_bench_process() -> Option<alloc::sync::Arc<crate::task::process::Process>> {
    use crate::task::process::{Process, CURRENT_PROCESS};
    use crate::memory::paging::AddressSpace;
    use crate::memory::buddy::BuddyFrameAllocator;

    // If CURRENT_PROCESS is set (post-boot), use it as parent for CoW clone.
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
        // During selftest (before userspace), create a bare address space.
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

fn bench_address_space_clone_cow() {
    use crate::memory::buddy::BuddyFrameAllocator;

    let proc = match make_bench_process() {
        Some(p) => p,
        None => return,
    };
    let mut allocator = BuddyFrameAllocator;
    if let Some(cloned) = proc.address_space.clone_cow(&mut allocator) {
        drop(cloned);
    }
    drop(proc);
}

fn bench_process_creation() {
    use crate::task::process::Process;
    use crate::memory::buddy::BuddyFrameAllocator;

    let parent = match make_bench_process() {
        Some(p) => p,
        None => return,
    };
    // Simulate full fork path: CoW clone address space + create Process struct.
    let mut allocator = BuddyFrameAllocator;
    if let Some(child_as) = parent.address_space.clone_cow(&mut allocator) {
        let child_id = Process::next_id();
        let child = Process::new(child_id, Some(parent.id), child_as);
        drop(child);
    }
    drop(parent);
}

fn bench_vma_add_remove() {
    use crate::task::process::Vma;
    use x86_64::structures::paging::PageTableFlags;

    let proc = match make_bench_process() {
        Some(p) => p,
        None => return,
    };
    // Add a VMA, then remove it
    let vma = Vma {
        start: 0x7F_F000_0000,
        end: 0x7F_F000_1000,
        flags: PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE,
        _name: "bench_vma",
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
// Benchmark: Scheduler context-switch (YieldNow poll overhead)
// ---------------------------------------------------------------------------

fn bench_ctxswitch_via_scheduler() {
    // Measure the overhead of constructing + polling a YieldNow future.
    use crate::task::YieldNow;
    // This captures the scheduler readiness check path without actually yielding.
    let mut yield_fut = YieldNow::new();
    use core::future::Future;
    use core::task::{Context, RawWaker, RawWakerVTable, Waker};

    fn noop_waker() -> Waker {
        fn clone(_: *const ()) -> RawWaker { RawWaker::new(core::ptr::null(), &VTABLE) }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }

    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    // Poll once — YieldNow returns Pending the first time (it yields on first poll)
    let _ = core::pin::Pin::new(&mut yield_fut).poll(&mut cx);
}

// ---------------------------------------------------------------------------
// Benchmark: mmap/munmap churn (syscall path via sys_mmap + sys_munmap)
// ---------------------------------------------------------------------------

fn bench_mmap_munmap_churn() {
    use crate::task::process::Vma;
    use x86_64::structures::paging::PageTableFlags;

    let proc = match make_bench_process() {
        Some(p) => p,
        None => return,
    };

    // Simulate mmap+munmap by rapidly adding/removing VMAs.
    // This benchmarks the VMA management path (sorted insert + merge + split).
    // Simulate 16 pages of mmap churn (64 KiB)
    let base = 0x7F_E000_0000u64;
    for i in 0..16u64 {
        let addr = base + i * 4096;
        let vma = Vma {
            start: addr,
            end: addr + 4096,
            flags: PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE | PageTableFlags::WRITABLE,
            _name: "bench_mmap",
            file_handle: None,
            file_offset: 0,
            is_shared: false,
            shm_id: None,
        };
        proc.add_vma(vma);
    }
    proc.remove_vma_range(base, base + 16 * 4096);
    drop(proc);
}

// ---------------------------------------------------------------------------
// Memory leak audit (runs after benchmarks)
// ---------------------------------------------------------------------------

fn test_memory_leak_audit() -> Result<(), &'static str> {
    use crate::memory::phys;

    // Take a snapshot before heavy allocation churn
    phys::reset_watermarks();
    let before = phys::audit_snapshot();

    // Do some allocation work
    let mut boxes = alloc::vec::Vec::new();
    for i in 0..500u64 {
        boxes.push(alloc::boxed::Box::new(i));
    }
    drop(boxes);

    // Force deferral processing
    crate::memory::frame_info::drain_deferred();

    // Update watermarks
    phys::update_watermarks();
    let after = phys::audit_snapshot();

    crate::serial_write("[BENCH] --- Memory Leak Audit ---\n");
    before.report();
    after.report();

    // After dropping all boxes and draining deferred frees, we should be
    // back to roughly the same free frame count. Allow some slack (64 frames = 256 KiB)
    // for internal allocator metadata.
    if after.has_leak() {
        crate::serial_write(&alloc::format!(
            "[BENCH] WARNING: possible leak detected — {} frames lost from baseline\n",
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
    crate::serial_write("\n[BENCH] === Vahi Kernel Benchmark Suite ===\n");
    crate::serial_write("[BENCH] All times in microseconds. Higher ops/s = better.\n\n");

    let iters = 10_000u64;

    // Allocation benchmarks
    crate::serial_write("[BENCH] --- Allocation ---\n");
    bench("box_alloc_dealloc", iters, bench_box_alloc_dealloc).report();
    bench("vec_push_drop_256", 1_000, bench_vec_push_drop).report();
    bench("large_alloc_4k", iters, bench_large_alloc).report();

    // Synchronization benchmarks
    crate::serial_write("\n[BENCH] --- Synchronization ---\n");
    bench("mutex_lock_unlock", iters / 10, bench_mutex_lock_unlock).report();
    bench("atomic_increment", iters, bench_atomic_increment).report();

    // Scheduler benchmarks
    crate::serial_write("\n[BENCH] --- Scheduler ---\n");
    bench("yield_now_construct", iters, bench_yield_now).report();
    bench("spin_loop", iters, bench_spin_loop).report();

    // String/format benchmarks
    crate::serial_write("\n[BENCH] --- String/Format ---\n");
    bench("format_u64_stack", iters, bench_format_u64).report();

    // VFS benchmarks
    crate::serial_write("\n[BENCH] --- VFS ---\n");
    bench("path_parse", iters, bench_path_parse).report();

    // Crypto benchmarks
    crate::serial_write("\n[BENCH] --- Crypto ---\n");
    bench("entropy_read", iters, bench_entropy_read).report();

    // Memory benchmarks
    crate::serial_write("\n[BENCH] --- Memory ---\n");
    bench("memcpy_4k", 1_000, bench_memcpy_4k).report();
    bench("memset_4k", 1_000, bench_memset_4k).report();

    // Pipe bandwidth benchmarks
    crate::serial_write("\n[BENCH] --- Pipe Bandwidth ---\n");
    bench("pipe_create_destroy", 10_000, bench_pipe_create_destroy).report();
    bench("pipe_throughput_4k", 1_000, bench_pipe_throughput_4k).report();
    bench("pipe_throughput_64k", 200, bench_pipe_throughput_64k).report();

    // Fork/exec prerequisite benchmarks (CoW clone, process create, VMA setup)
    crate::serial_write("\n[BENCH] --- Fork / Exec Prerequisites ---\n");
    bench("addr_space_clone_cow", 50, bench_address_space_clone_cow).report();
    bench("process_creation", 50, bench_process_creation).report();
    bench("vma_add_remove", 1_000, bench_vma_add_remove).report();

    // Context-switch latency benchmarks
    crate::serial_write("\n[BENCH] --- Context Switch ---\n");
    bench("ctxswitch_scheduler_poll", 10_000, bench_ctxswitch_via_scheduler).report();

    // mmap/munmap churn benchmark
    crate::serial_write("\n[BENCH] --- mmap/munmap Churn ---\n");
    bench("mmap_munmap_churn_16pg", 500, bench_mmap_munmap_churn).report();

    // Memory leak audit — failures here are serious, propagate them
    crate::serial_write("\n[BENCH] --- Memory Leak Audit ---\n");
    test_memory_leak_audit()?;

    crate::serial_write("\n[BENCH] === Benchmarks Complete ===\n\n");

    Ok(())
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register() {
    selftest::register("benchmarks::suite", run_all_benchmarks);
    selftest::register("benchmarks::memory_leak_audit", test_memory_leak_audit);
}
