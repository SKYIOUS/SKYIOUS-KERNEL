use spin::Mutex;
use crate::println;
use crate::interrupts;

const WATCHDOG_TIMEOUT_TICKS: u64 = 500; // ~5 seconds at 100Hz

#[derive(Debug, Clone, Copy)]
struct CpuWatchdog {
    last_tick: u64,
    warned: bool,
}

static WATCHDOGS: Mutex<[Option<CpuWatchdog>; 256]> = Mutex::new([None; 256]);

/// Updates the watchdog for the current CPU.
pub fn pet() {
    #[cfg(feature = "smp")]
    let cpu_id = crate::smp::get_cpu_id();
    #[cfg(not(feature = "smp"))]
    let cpu_id = 0;
    let ticks = interrupts::get_ticks();
    let mut watchdogs = WATCHDOGS.lock();
    
    if let Some(ref mut watchdog) = watchdogs[cpu_id] {
        watchdog.last_tick = ticks;
        watchdog.warned = false;
    } else {
        watchdogs[cpu_id] = Some(CpuWatchdog {
            last_tick: ticks,
            warned: false,
        });
    }
}

/// Checks all active watchdogs for timeouts.
/// Should be called by one CPU core (e.g., in the scheduler or a dedicated task).
pub fn check() {
    let ticks = interrupts::get_ticks();
    let mut watchdogs = WATCHDOGS.lock();
    
    for (i, opt_watchdog) in watchdogs.iter_mut().enumerate() {
        if let Some(ref mut watchdog) = opt_watchdog {
            let elapsed = ticks.saturating_sub(watchdog.last_tick);
            
            if elapsed > WATCHDOG_TIMEOUT_TICKS {
                panic!("WATCHDOG: CPU {} is stuck! Last seen {} ticks ago.", i, elapsed);
            } else if elapsed > WATCHDOG_TIMEOUT_TICKS / 2 && !watchdog.warned {
                println!("WATCHDOG WARNING: CPU {} has not responded for {} ticks.", i, elapsed);
                watchdog.warned = true;
            }
        }
    }
}
