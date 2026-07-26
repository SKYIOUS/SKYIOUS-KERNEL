//! Boot-time diagnostic logger with millisecond timestamps.

use crate::boot::BootContext;

pub struct BootLogger;

impl BootLogger {
    fn timestamp_ms(context: &BootContext) -> u64 {
        // Use the 100Hz tick counter; divide by 10 for approximate ms
        // ponytail: Ticks at 100Hz = 10ms granularity, good enough for boot diag
        let ticks = crate::interrupts::get_ticks();
        let elapsed = ticks.wrapping_sub(context.boot_start_tick);
        elapsed * 10
    }

    pub fn info(context: &BootContext, msg: &str) {
        let ts = Self::timestamp_ms(context);
        crate::serial_write(&alloc::format!("[{}] BOOT  {}\n", ts, msg));
    }

    pub fn warn(context: &BootContext, msg: &str) {
        let ts = Self::timestamp_ms(context);
        crate::serial_write(&alloc::format!("[{}] BOOT  WARNING {}\n", ts, msg));
    }

    pub fn error(context: &BootContext, msg: &str) {
        let ts = Self::timestamp_ms(context);
        crate::serial_write(&alloc::format!("[{}] BOOT  ERROR {}\n", ts, msg));
    }
}
