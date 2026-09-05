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
