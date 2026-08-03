use alloc::vec::Vec;
use core::fmt::Write;
use crate::sync::IrqSafeMutex as Mutex;

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
    
    // VGA output for local debugging
    let mut wr = crate::vga_buffer::WRITER.lock();
    wr.write_str("[self-test] ").ok();
    if count == 0 {
        wr.write_str("no tests registered\n").ok();
        crate::serial_write("Bail out! No tests registered\n");
        return;
    }
    
    let mut passed = 0usize;
    for (i, t) in tests.iter().enumerate() {
        let test_num = i + 1;
        match (t.func)() {
            Ok(()) => {
                // TAP ok line to serial
                crate::serial_write(&alloc::format!("ok {} - {}\n", test_num, t.name));
                // VGA output
                wr.write_str("  OK  ").ok();
                wr.write_str(t.name).ok();
                wr.write_str("\n").ok();
                passed += 1;
            }
            Err(msg) => {
                // TAP not ok line to serial (CI will fail)
                crate::serial_write(&alloc::format!("not ok {} - {} # {}\n", test_num, t.name, msg));
                // VGA output
                wr.write_str("  FAIL ").ok();
                wr.write_str(t.name).ok();
                wr.write_str(": ").ok();
                wr.write_str(msg).ok();
                wr.write_str("\n").ok();
            }
        }
    }
    
    // TAP summary to serial
    crate::serial_write(&alloc::format!("# {}/{} passed, {} failed\n", passed, count, count - passed));
    
    // VGA summary
    let summary = alloc::format!("  {}/{} passed, {} failed\n", passed, count, count - passed);
    wr.write_str(&summary).ok();
    drop(wr);
    drop(tests);
    
    if passed < count {
        panic!("self-test: {} test(s) failed", count - passed);
    }
}
