use alloc::vec::Vec;
use core::fmt::Write;
use spin::Mutex;

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
    let mut wr = crate::vga_buffer::WRITER.lock();
    wr.write_str("[self-test] ").ok();
    if count == 0 {
        wr.write_str("no tests registered\n").ok();
        return;
    }
    let mut passed = 0usize;
    for t in tests.iter() {
        match (t.func)() {
            Ok(()) => {
                wr.write_str("  OK  ").ok();
                wr.write_str(t.name).ok();
                wr.write_str("\n").ok();
                passed += 1;
            }
            Err(msg) => {
                wr.write_str("  FAIL ").ok();
                wr.write_str(t.name).ok();
                wr.write_str(": ").ok();
                wr.write_str(msg).ok();
                wr.write_str("\n").ok();
            }
        }
    }
    let summary = alloc::format!("  {}/{} passed, {} failed\n", passed, count, count - passed);
    wr.write_str(&summary).ok();
    drop(wr);
    drop(tests);
    if passed < count {
        panic!("self-test: {} test(s) failed", count - passed);
    }
}
