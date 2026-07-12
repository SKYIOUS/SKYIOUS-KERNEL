use crate::selftest;

/// Verify that the global futex queue and wake_futex logic
/// are wired through the scheduler correctly.
fn test_futex_wake_process() -> Result<(), &'static str> {
    // Simply verify that wake_process_futex does not panic
    // when called with a non-existent PID.
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

pub fn register_all() {
    selftest::register("futex::wake_empty", test_futex_wake_empty);
    selftest::register("futex::wake_bad_pid", test_futex_wake_process);
    selftest::register("futex::wake_proc_blocked", test_futex_wake_process_blocked);
}
