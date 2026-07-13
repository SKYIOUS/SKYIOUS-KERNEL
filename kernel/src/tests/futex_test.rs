use crate::selftest;

/// Verify that the global futex queue and wake_futex logic
/// are wired through the scheduler correctly.
fn test_futex_wake_process() -> Result<(), &'static str> {
    let woken = crate::task::scheduler::wake_process_futex(0xFFFF);
    if woken != 0 {
        return Err("wake_process_futex on non-existent PID should return 0");
    }
    Ok(())
}

/// Verify wake_futex handles zero-waiter case.
fn test_futex_wake_empty() -> Result<(), &'static str> {
    let woken = crate::task::scheduler::wake_futex(0xDEAD_BEEF, 1);
    if woken != 0 {
        return Err("wake_futex on empty queue should return 0");
    }
    Ok(())
}

/// Verify wake_process_blocked handles non-existent PID.
fn test_futex_wake_process_blocked() -> Result<(), &'static str> {
    let woken = crate::task::scheduler::wake_process_blocked(0xFFFF);
    if woken != 0 {
        return Err("wake_process_blocked on non-existent PID should return 0");
    }
    Ok(())
}

/// Verify wake_futex with max_wake=0 returns 0.
fn test_futex_wake_zero() -> Result<(), &'static str> {
    let woken = crate::task::scheduler::wake_futex(0xCAFE, 0);
    if woken != 0 {
        return Err("wake_futex with max_wake=0 should return 0");
    }
    Ok(())
}

/// Verify wake_process_blocked returns 0 on empty queue.
fn test_block_wake_empty() -> Result<(), &'static str> {
    let woken = crate::task::scheduler::wake_process_blocked(0);
    if woken != 0 {
        return Err("wake_process_blocked on empty queue should return 0");
    }
    Ok(())
}

/// Verify boost_thread_priority returns false (stub).
fn test_boost_priority_stub() -> Result<(), &'static str> {
    let boosted = crate::task::scheduler::boost_thread_priority(0, 7);
    if boosted {
        return Err("boost_thread_priority should return false (stub)");
    }
    Ok(())
}

/// Inject threads into futex queue then wake via public API.
fn test_futex_wake_prepopulated() -> Result<(), &'static str> {
    use crate::task::thread::{Thread, ThreadId, ThreadStatus};
    use crate::memory::stack::Stack;
    let t = Thread {
        _id: ThreadId::new(),
        stack: Stack { bottom: 0, top: 0 },
        stack_ptr: 0,
        status: ThreadStatus::Ready,
        process: None,
        priority: 5,
        sleep_until: None,
        futex_wake_addr: Some(0xAAAA),
        pipe_block_key: None,
        fs_base: 0,
    };
    crate::task::scheduler::add_futex_thread(t);
    let t2 = Thread {
        _id: ThreadId::new(),
        stack: Stack { bottom: 0, top: 0 },
        stack_ptr: 0,
        status: ThreadStatus::Ready,
        process: None,
        priority: 5,
        sleep_until: None,
        futex_wake_addr: Some(0xBBBB),
        pipe_block_key: None,
        fs_base: 0,
    };
    crate::task::scheduler::add_futex_thread(t2);
    let woken = crate::task::scheduler::wake_futex(0xAAAA, 10);
    if woken != 1 { return Err("should wake 1 from prepopulated queue"); }
    Ok(())
}

pub fn register_all() {
    selftest::register("futex::wake_empty", test_futex_wake_empty);
    selftest::register("futex::wake_bad_pid", test_futex_wake_process);
    selftest::register("futex::wake_proc_blocked", test_futex_wake_process_blocked);
    selftest::register("futex::wake_zero", test_futex_wake_zero);
    selftest::register("futex::block_wake_empty", test_block_wake_empty);
    selftest::register("futex::boost_stub", test_boost_priority_stub);
    selftest::register("futex::wake_prepopulated", test_futex_wake_prepopulated);
}
