//! Boot-time diagnostic logger with millisecond timestamps.

use crate::boot::BootContext;

pub struct BootLogger;

impl BootLogger {
    fn timestamp_ms(context: &BootContext) -> alloc::string::String {
        let tick = crate::interrupts::get_ticks();
        if tick == 0 && context.boot_start_tick == 0 {
            alloc::string::String::from("?")
        } else {
            let elapsed = tick.wrapping_sub(context.boot_start_tick);
            alloc::format!("{}", elapsed * 10)
        }
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
