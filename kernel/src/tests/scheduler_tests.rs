use crate::task::scheduler::{GLOBAL, cpu_sched};
use crate::task::thread::{Thread, ThreadId, ThreadStatus, STRIDE_MAX};

fn make_test_thread(priority: u8, sleep_until: Option<u64>) -> Thread {
    Thread {
        _id: ThreadId::new(),
        stack: crate::memory::stack::Stack { bottom: 0, top: 0 },
        stack_ptr: 0,
        status: ThreadStatus::Ready,
        process: None,
        priority,
        sleep_until,
        futex_wake_addr: None,
        pipe_block_key: None,
        fs_base: 0,
        pass: 0,
        stride: STRIDE_MAX / 20,
        tickets: 20,
    }
}

fn test_tick_empty() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    GLOBAL.tick(0, &mut *target);
    GLOBAL.tick(100, &mut *target);
    Ok(())
}

fn test_sleep_queue_add_and_tick() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.sleep_queue.lock().len();
    GLOBAL.add_sleeping_thread(make_test_thread(5, Some(100)));
    if GLOBAL.sleep_queue.lock().len() != before + 1 { return Err("sleep queue should grow by 1"); }
    GLOBAL.tick(100, &mut *target);
    if GLOBAL.sleep_queue.lock().len() != before { return Err("thread should be woken at tick=100"); }
    Ok(())
}

fn test_sleep_queue_not_woken_early() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.sleep_queue.lock().len();
    GLOBAL.add_sleeping_thread(make_test_thread(5, Some(100)));
    GLOBAL.tick(50, &mut *target);
    if GLOBAL.sleep_queue.lock().len() != before + 1 { return Err("thread should still be sleeping"); }
    Ok(())
}

fn test_sleep_queue_multiple_wake() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.sleep_queue.lock().len();
    GLOBAL.add_sleeping_thread(make_test_thread(5, Some(50)));
    GLOBAL.add_sleeping_thread(make_test_thread(3, Some(100)));
    GLOBAL.add_sleeping_thread(make_test_thread(7, Some(150)));
    GLOBAL.tick(100, &mut *target);
    if GLOBAL.sleep_queue.lock().len() != before + 1 { return Err("only the last thread should remain"); }
    GLOBAL.tick(200, &mut *target);
    if GLOBAL.sleep_queue.lock().len() != before { return Err("all should be woken by tick=200"); }
    Ok(())
}

fn test_futex_queue_wake() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.futex_queue.lock().len();
    let mut t1 = make_test_thread(5, None);
    t1.futex_wake_addr = Some(0xCAFE);
    GLOBAL.futex_queue.lock().push_back(alloc::boxed::Box::new(t1));
    let mut t2 = make_test_thread(5, None);
    t2.futex_wake_addr = Some(0xBEEF);
    GLOBAL.futex_queue.lock().push_back(alloc::boxed::Box::new(t2));
    if GLOBAL.futex_queue.lock().len() != before + 2 { return Err("should have 2 in futex queue"); }
    let woken = GLOBAL.wake_futex(0xCAFE, 1, &mut *target);
    if woken != 1 { return Err("should wake 1 thread"); }
    if GLOBAL.futex_queue.lock().len() != before + 1 { return Err("1 should remain in futex queue"); }
    Ok(())
}

fn test_futex_wake_max() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.futex_queue.lock().len();
    for _ in 0..5 {
        let mut t = make_test_thread(5, None);
        t.futex_wake_addr = Some(0xF000);
        GLOBAL.futex_queue.lock().push_back(alloc::boxed::Box::new(t));
    }
    let woken = GLOBAL.wake_futex(0xF000, 3, &mut *target);
    if woken != 3 { return Err("should wake exactly 3"); }
    if GLOBAL.futex_queue.lock().len() != before + 2 { return Err("2 should remain"); }
    Ok(())
}

fn test_block_queue_wake() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.block_queue.lock().len();
    let mut t = make_test_thread(5, None);
    t.pipe_block_key = Some(0x42);
    GLOBAL.block_queue.lock().push_back(alloc::boxed::Box::new(t));
    let woken = GLOBAL.wake_blocked_threads(0x42, 1, &mut *target);
    if woken != 1 { return Err("should wake 1"); }
    if GLOBAL.block_queue.lock().len() != before { return Err("block queue should be empty"); }
    Ok(())
}

fn test_block_queue_wake_wrong_key() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let before = GLOBAL.block_queue.lock().len();
    let mut t = make_test_thread(5, None);
    t.pipe_block_key = Some(0x42);
    GLOBAL.block_queue.lock().push_back(alloc::boxed::Box::new(t));
    let woken = GLOBAL.wake_blocked_threads(0xFF, 1, &mut *target);
    if woken != 0 { return Err("should not wake with wrong key"); }
    if GLOBAL.block_queue.lock().len() != before + 1 { return Err("thread should remain"); }
    Ok(())
}

fn test_pick_next_from_pending() -> Result<(), &'static str> {
    let before = GLOBAL.pending_queue.lock().len();
    GLOBAL.pending_queue.lock().push_back(alloc::boxed::Box::new(make_test_thread(5, None)));
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let thread = target.pick_next();
    if thread.is_none() { return Err("pick_next should return thread from pending"); }
    let t = thread.unwrap();
    if t.priority != 5 { return Err("picked thread priority mismatch"); }
    drop(target);
    if GLOBAL.pending_queue.lock().len() != before { return Err("pending_queue should be drained"); }
    Ok(())
}

fn test_pick_next_empty() -> Result<(), &'static str> {
    let mut target = cpu_sched(0).ok_or("no cpu 0")?.lock();
    let thread = target.pick_next();
    if thread.is_some() { return Err("pick_next on empty should return None"); }
    Ok(())
}

pub fn register() {
    crate::selftest::register("sched::tick_empty", test_tick_empty);
    crate::selftest::register("sched::sleep_add_tick", test_sleep_queue_add_and_tick);
    crate::selftest::register("sched::sleep_not_woken_early", test_sleep_queue_not_woken_early);
    crate::selftest::register("sched::sleep_multiple_wake", test_sleep_queue_multiple_wake);
    crate::selftest::register("sched::futex_wake", test_futex_queue_wake);
    crate::selftest::register("sched::futex_wake_max", test_futex_wake_max);
    crate::selftest::register("sched::block_wake", test_block_queue_wake);
    crate::selftest::register("sched::block_wake_wrong_key", test_block_queue_wake_wrong_key);
    crate::selftest::register("sched::pick_next_from_pending", test_pick_next_from_pending);
    crate::selftest::register("sched::pick_next_empty", test_pick_next_empty);
}
