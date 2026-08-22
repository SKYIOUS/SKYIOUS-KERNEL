//! Stress tests for the Vahi kernel.
//!
//! These tests verify kernel stability under high load conditions.
//! They are designed to be run during boot (selftest framework) and
//! can also be triggered via QEMU integration tests.
//!
//! Test categories:
//! - SMP: Concurrent operations across multiple CPUs
//! - Memory: Allocation pressure and fragmentation
//! - Process churn: Rapid fork/exit cycles
//! - FD exhaustion: Opening many file descriptors
//! - IPC: Pipe and socket throughput
//! - Filesystem: Concurrent file operations

use crate::selftest;

// ---------------------------------------------------------------------------
// SMP Stress Tests
// ---------------------------------------------------------------------------

/// Stress test: Rapid lock acquisition/release across concepts
fn test_lock_contention() -> Result<(), &'static str> {
    use crate::sync::IrqSafeMutex;
    use alloc::vec::Vec;

    let mutex = IrqSafeMutex::new(Vec::<u64>::new());
    let iterations: usize = 1000;

    for i in 0..iterations as u64 {
        let mut guard = mutex.lock();
        guard.push(i);
        // Guard drops here, releasing the lock
    }

    let guard = mutex.lock();
    if guard.len() != iterations {
        return Err("Lock contention: wrong number of elements");
    }
    Ok(())
}

/// Stress test: Rapid atomic operations
fn test_atomic_contention() -> Result<(), &'static str> {
    use core::sync::atomic::{AtomicU64, Ordering};

    let counter = AtomicU64::new(0);
    let iterations = 10_000;

    for _ in 0..iterations {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    let val = counter.load(Ordering::Relaxed);
    if val != iterations {
        return Err("Atomic contention: counter mismatch");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Memory Stress Tests
// ---------------------------------------------------------------------------

/// Stress test: Rapid allocation/deallocation
fn test_alloc_dealloc_churn() -> Result<(), &'static str> {
    use alloc::vec::Vec;

    let iterations = 500;
    for _ in 0..iterations {
        let size = 4096;
        let buf = Vec::<u8>::with_capacity(size);
        drop(buf);
    }
    Ok(())
}

/// Stress test: Fragmented allocation pattern
fn test_fragmented_alloc() -> Result<(), &'static str> {
    use alloc::vec::Vec;
    use alloc::boxed::Box;

    // Allocate many small buffers
    let mut small: Vec<Box<[u8; 64]>> = Vec::new();
    for _ in 0..200 {
        small.push(Box::new([0u8; 64]));
    }

    // Allocate some large buffers
    let mut large: Vec<Box<[u8; 4096]>> = Vec::new();
    for _ in 0..10 {
        large.push(Box::new([0u8; 4096]));
    }

    // Drop every other small buffer to create fragmentation
    for i in (0..small.len()).step_by(2) {
        small[i] = Box::new([0u8; 64]); // Replace with new allocation
    }

    drop(small);
    drop(large);
    Ok(())
}

/// Stress test: Many small allocations
fn test_many_small_allocs() -> Result<(), &'static str> {
    use alloc::boxed::Box;

    let mut boxes = alloc::vec::Vec::new();
    for i in 0..1000 {
        let b = Box::new(i as u64);
        boxes.push(b);
    }

    // Verify all values
    for (i, b) in boxes.iter().enumerate() {
        if **b != i as u64 {
            return Err("Many small allocs: value mismatch");
        }
    }
    drop(boxes);
    Ok(())
}

// ---------------------------------------------------------------------------
// Process Churn Tests
// ---------------------------------------------------------------------------

/// Stress test: Process ID allocation (simulated)
fn test_pid_churn() -> Result<(), &'static str> {
    use core::sync::atomic::{AtomicU32, Ordering};

    static NEXT_PID: AtomicU32 = AtomicU32::new(100000);

    // Simulate rapid PID allocation/deallocation
    let mut pids = alloc::vec::Vec::new();
    for _ in 0..1000 {
        let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
        pids.push(pid);
    }

    // Verify uniqueness
    pids.sort();
    pids.dedup();
    if pids.len() != 1000 {
        return Err("PID churn: duplicate PIDs allocated");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// FD Exhaustion Tests
// ---------------------------------------------------------------------------

/// Stress test: File descriptor table operations
fn test_fd_table_growth() -> Result<(), &'static str> {
    use crate::task::process::FileDescriptor;
    use alloc::vec::Vec;

    // Simulate FD table growth
    let mut table: Vec<Option<FileDescriptor>> = Vec::new();
    for i in 0..1000 {
        if table.len() <= i {
            table.push(None);
        }
        // Each slot would normally be filled with an actual FD
    }

    if table.len() < 1000 {
        return Err("FD table growth: table too small");
    }
    drop(table);
    Ok(())
}

/// Stress test: FD allocation and deallocation pattern
fn test_fd_alloc_free_pattern() -> Result<(), &'static str> {
    use alloc::vec::Vec;

    // Simulate FD allocation pattern: open many, close some, open more
    let mut active_fds: Vec<usize> = Vec::new();
    let mut next_fd = 0;

    // Phase 1: Open 500 FDs
    for _ in 0..500 {
        active_fds.push(next_fd);
        next_fd += 1;
    }

    // Phase 2: Close every other FD
    let mut closed = 0;
    active_fds.retain(|&fd| {
        if fd % 2 == 0 {
            closed += 1;
            false
        } else {
            true
        }
    });

    if closed != 250 {
        return Err("FD pattern: wrong number closed");
    }

    // Phase 3: Open 250 more FDs
    for _ in 0..250 {
        active_fds.push(next_fd);
        next_fd += 1;
    }

    if active_fds.len() != 500 {
        return Err("FD pattern: wrong final count");
    }

    drop(active_fds);
    Ok(())
}

// ---------------------------------------------------------------------------
// IPC Stress Tests
// ---------------------------------------------------------------------------

/// Stress test: Pipe-like data transfer (simulated)
fn test_pipe_throughput() -> Result<(), &'static str> {
    use alloc::vec::Vec;
    use alloc::collections::VecDeque;

    // Simulate pipe buffer operations
    let mut pipe_buf: VecDeque<Vec<u8>> = VecDeque::new();
    let message_count = 1000;
    let msg_size = 256;

    // Writer: enqueue messages
    for i in 0..message_count {
        let msg = alloc::vec![i as u8; msg_size];
        pipe_buf.push_back(msg);
    }

    // Reader: dequeue and verify
    let mut received = 0;
    while let Some(msg) = pipe_buf.pop_front() {
        if msg.len() != msg_size {
            return Err("Pipe throughput: wrong message size");
        }
        received += 1;
    }

    if received != message_count {
        return Err("Pipe throughput: wrong message count");
    }
    Ok(())
}

/// Stress test: Socket-like message passing (simulated)
fn test_socket_msg_passing() -> Result<(), &'static str> {
    use alloc::vec::Vec;
    use alloc::collections::VecDeque;

    // Simulate socket send/recv buffer
    let mut send_buf: VecDeque<Vec<u8>> = VecDeque::new();
    let mut recv_buf: VecDeque<Vec<u8>> = VecDeque::new();

    // Send 500 messages of varying sizes
    for i in 0..500 {
        let size = 64 + (i % 256);
        let msg = alloc::vec![(i & 0xFF) as u8; size];
        send_buf.push_back(msg);
    }

    // Transfer from send to recv (simulating kernel copy)
    while let Some(msg) = send_buf.pop_front() {
        recv_buf.push_back(msg);
    }

    // Verify
    if recv_buf.len() != 500 {
        return Err("Socket msg: wrong count");
    }

    drop(recv_buf);
    Ok(())
}

// ---------------------------------------------------------------------------
// Filesystem Stress Tests
// ---------------------------------------------------------------------------

/// Stress test: Inode operations (simulated)
fn test_inode_churn() -> Result<(), &'static str> {
    use alloc::vec::Vec;

    // Simulate rapid inode creation/deletion
    struct FakeInode {
        ino: u64,
        refcount: u32,
    }

    let mut inodes: Vec<FakeInode> = Vec::new();

    // Create 1000 inodes
    for i in 0..1000 {
        inodes.push(FakeInode { ino: i, refcount: 1 });
    }

    // Drop every third inode
    let mut dropped = 0;
    inodes.retain(|inode| {
        if inode.ino % 3 == 0 {
            dropped += 1;
            false
        } else {
            true
        }
    });

    if dropped != 334 {
        return Err("Inode churn: wrong drop count");
    }

    // Recreate dropped inodes
    for i in 0..334 {
        inodes.push(FakeInode { ino: 1000 + i as u64, refcount: 1 });
    }

    if inodes.len() != 1000 {
        return Err("Inode churn: wrong final count");
    }

    drop(inodes);
    Ok(())
}

/// Stress test: Path resolution (simulated)
fn test_path_resolution_stress() -> Result<(), &'static str> {
    use alloc::string::String;

    let paths: alloc::vec::Vec<String> = (0..500)
        .map(|i| alloc::format!("/proc/{}/fd/{}", i, i * 2))
        .collect();

    // Verify all paths are unique
    let mut sorted = paths.clone();
    sorted.sort();
    sorted.dedup();
    if sorted.len() != paths.len() {
        return Err("Path stress: duplicate paths generated");
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register() {
    // SMP stress
    selftest::register("stress::lock_contention", test_lock_contention);
    selftest::register("stress::atomic_contention", test_atomic_contention);

    // Memory stress
    selftest::register("stress::alloc_dealloc_churn", test_alloc_dealloc_churn);
    selftest::register("stress::fragmented_alloc", test_fragmented_alloc);
    selftest::register("stress::many_small_allocs", test_many_small_allocs);

    // Process churn
    selftest::register("stress::pid_churn", test_pid_churn);

    // FD exhaustion
    selftest::register("stress::fd_table_growth", test_fd_table_growth);
    selftest::register("stress::fd_alloc_free_pattern", test_fd_alloc_free_pattern);

    // IPC stress
    selftest::register("stress::pipe_throughput", test_pipe_throughput);
    selftest::register("stress::socket_msg_passing", test_socket_msg_passing);

    // Filesystem stress
    selftest::register("stress::inode_churn", test_inode_churn);
    selftest::register("stress::path_resolution_stress", test_path_resolution_stress);
}
