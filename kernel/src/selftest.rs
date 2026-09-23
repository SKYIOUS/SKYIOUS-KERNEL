use crate::sync::IrqSafeMutex as Mutex;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

pub type TestFn = fn() -> Result<(), &'static str>;

// QEMU isa-debug-exit exit codes, distinguished by the host runner:
//   0x10 = suite ran to completion (per-test results are parsed from TAP)
//   0x11 = kernel panic reported by the panic handler
//   0x30 = run_all() exceeded its own guard timeout (host also enforces its
//          own wall-clock TIMEOUT, so a hang here is caught twice)
// Host runners must map QEMU exit `(code & 0x7f) - 1`: 0x0f = PASS_BASE,
// 0x10 = PANIC_BASE, 0x2f = TIMEOUT_BASE.
pub const QEMU_EXIT_PASS_BASE: u32 = 0x10;
pub const QEMU_EXIT_PANIC_BASE: u32 = 0x11;
pub const QEMU_EXIT_TIMEOUT_BASE: u32 = 0x30;

/// Explicit QEMU exit via the isa-debug-exit device: writes `code` to port
/// 0xf4, making QEMU terminate with exit status `(code & 0x7f) - 1`.
/// No-op on aarch64 (the device is x86-only) and harmless on real hardware
/// (no device at 0xf4; caller should still halt afterwards).
pub fn qemu_exit(code: u32) {
    #[cfg(target_arch = "x86_64")]
    {
        // SAFETY: writing a 32-bit value to I/O port 0xf4 (QEMU isa-debug-exit
        // device) is always well-defined; without the device it is a no-op.
        let mut port = x86_64::instructions::port::Port::<u32>::new(0xf4);
        unsafe {
            port.write(code);
        }
    }
    let _ = code;
}

/// Halt the current CPU forever (fallback when isa-debug-exit is absent,
/// e.g. on real hardware).
#[cfg(target_arch = "x86_64")]
fn halt_forever() -> ! {
    x86_64::instructions::interrupts::disable();
    loop {
        x86_64::instructions::hlt();
    }
}

/// Halt the current CPU forever (aarch64: no ISA-debug-exit device exists).
#[cfg(target_arch = "aarch64")]
fn halt_forever() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

/// Fatal-error exit used by the panic handler: emit the machine-readable
/// marker, then exit QEMU with the panic code. Falls back to halt on aarch64
/// or if the isa-debug-exit device is absent.
pub fn qemu_exit_panic() -> ! {
    crate::serial_write("TAP version 13\n");
    crate::serial_write("Bail out! KERNEL PANIC\n");
    qemu_exit(QEMU_EXIT_PANIC_BASE);
    halt_forever()
}

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

    // All tests passed: terminate QEMU deterministically with the pass code so
    // the host runner gets an explicit exit status instead of having to kill
    // the emulator. Test-only configuration — normal (non-self_test) boots
    // never reach this code and continue into GUI/login as before.
    qemu_exit(QEMU_EXIT_PASS_BASE);
    halt_forever()
}
