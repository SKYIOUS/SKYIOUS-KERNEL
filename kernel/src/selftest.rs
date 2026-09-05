use crate::sync::IrqSafeMutex as Mutex;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

pub type TestFn = fn() -> Result<(), &'static str>;

struct RegisteredTest {
    pub name: &'static str,
    pub func: TestFn,
}

static TESTS: Mutex<Vec<RegisteredTest>> = Mutex::new(Vec::new());

pub fn register(name: &'static str, func: TestFn) {
    TESTS.lock().push(RegisteredTest { name, func });
}

pub fn run_all() {
    let tests = TESTS.lock();
    let count = tests.len();

    // TAP header to serial (CI gate)
    crate::serial_write("TAP version 13\n");
    crate::serial_write(&alloc::format!("1..{}\n", count));

    if count == 0 {
        crate::serial_write("Bail out! No tests registered\n");
        return;
    }

    let mut passed = 0usize;
    crate::task::scheduler::SCHED_QUIESCE.store(true, Ordering::Relaxed);
    for i in 0..count {
        let t = &tests[i];
        let test_num = i + 1;
        crate::serial_write(&alloc::format!(
            "[SELF-TEST] running test {}/{}: {}\n",
            test_num,
            count,
            t.name
        ));
        if (t.func as usize) == 0 {
            crate::serial_write(&alloc::format!(
                "not ok {} - {} # NULL function pointer\n",
                test_num,
                t.name
            ));
            continue;
        }
        match (t.func)() {
            Ok(()) => {
                crate::serial_write(&alloc::format!("ok {} - {}\n", test_num, t.name));
                passed += 1;
            }
            Err(msg) => {
                crate::serial_write(&alloc::format!(
                    "not ok {} - {} # {}\n",
                    test_num,
                    t.name,
                    msg
                ));
            }
        }
    }

    // TAP summary to serial
    crate::serial_write(&alloc::format!(
        "# {}/{} passed, {} failed\n",
        passed,
        count,
        count - passed
    ));
    crate::task::scheduler::SCHED_QUIESCE.store(false, Ordering::Relaxed);

    // Drain test-injected threads from the global queues: with wake paths now
    // marking ready queues dirty, woken test threads (bogus contexts) would be
    // picked by the real scheduler once quiesce clears.
    for q in [
        &crate::task::scheduler::GLOBAL.pending_queue,
        &crate::task::scheduler::GLOBAL.sleep_queue,
        &crate::task::scheduler::GLOBAL.block_queue,
        &crate::task::scheduler::GLOBAL.futex_queue,
    ] {
        while q.lock().pop_front().is_some() {}
    }
    for i in 0..crate::task::scheduler::MAX_CPUS {
        if let Some(s) = crate::task::scheduler::cpu_sched(i) {
            s.lock().reset_runnable_state();
        }
    }

    drop(tests);

    if passed < count {
        panic!("self-test: {} test(s) failed", count - passed);
    }
}
