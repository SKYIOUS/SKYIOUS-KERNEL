//! Background task entry points spawned by kernel_main.

pub extern "C" fn init_os_task() -> ! {
    crate::boot::state::run_boot()
}

pub extern "C" fn run_async_tasks() -> ! {
    crate::serial_write("[ASYNC] Async Executor Started.\n");
    use crate::task::{executor::Executor, Task};
    let mut executor = Executor::new();

    // ponytail: kernel shell disabled — it writes directly to the framebuffer,
    // clobbering the GUI compositor's rendered output. The GUI handles keyboard.
    #[cfg(feature = "net")]
    let _ = executor.spawn(Task::new(network_poll_task()));
    let _ = executor.spawn(Task::new(crate::gui::input::gui_refresh_task()));
    executor.run();
}

#[cfg(feature = "net")]
pub async fn network_poll_task() {
    loop {
        crate::net::poll();
        core::hint::spin_loop();
        crate::task::YieldNow::new().await;
    }
}

/// Kernel thread that updates the cached RTC time once per second.
/// Runs in process context (not IRQ) to allow CMOS I/O.
/// TSC sync and drift check are now handled in the timer interrupt handler (CPU 0, every tick).
pub extern "C" fn rtc_cache_update_task() -> ! {
    loop {
        crate::vdso::vdso_update_cached_rtc();

        // Sleep for 1 second (100 ticks at 100 Hz)
        let target = crate::interrupts::get_ticks() + 100;
        crate::task::scheduler::sleep_until_tick(target);
    }
}
