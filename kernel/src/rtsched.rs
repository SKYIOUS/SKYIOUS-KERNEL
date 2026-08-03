// ponytail: rtsched targets Cortex-M; this module wraps it behind a feature flag.
// The crate's `HostPlatform` fallback works on x86_64 for non-arch-specific features
// (timer queue, wait queue, CFS run queue).  Porting ContextSwitchPort/SchedulerTimerPort
// to x86_64 would enable preemptive RT scheduling.  Add when a real-time workload demands it.

use core::sync::atomic::{AtomicBool, Ordering};

/// Minimal rtsched integration point.
pub struct RtSched;

#[cfg(feature = "rtsched")]
static INITIALIZED: AtomicBool = AtomicBool::new(false);

impl RtSched {
    /// Initialise the rtsched ktimer queue and CFS scheduler.
    /// `period_ticks`: CFS scheduler period in timer ticks.
    /// `exec_ticks`:   CFS scheduling quantum in timer ticks.
    /// Stub when the feature is disabled.
    pub fn init(period_ticks: u32, exec_ticks: u32) {
        #[cfg(feature = "rtsched")]
        {
            if !INITIALIZED.swap(true, Ordering::Relaxed) {
                unsafe {
                    rtsched::init_ktimer_queue();
                    rtsched::init_cfs(period_ticks, exec_ticks);
                }
            }
        }
    }

    /// Sleep the current thread for `ms` milliseconds.
    /// Falls back to a busy-wait stub when the feature is disabled.
    pub fn msleep(ms: u32) {
        #[cfg(feature = "rtsched")]
        rtsched::msleepyi(ms);

        #[cfg(not(feature = "rtsched"))]
        {
            for _ in 0..(ms as u64) * 100_000 {
                core::hint::spin_loop();
            }
        }
    }

    /// Cooperative yield — hint the scheduler another thread may run.
    pub fn yield_cpu() {
        #[cfg(feature = "rtsched")]
        rtsched::yieldyi();
    }

    /// Handle one scheduler tick — delegates to rtsched when enabled.
    #[allow(dead_code)]
    pub fn handle_tick() {
        #[cfg(feature = "rtsched")]
        rtsched::handle_sched_tick();
    }
}
