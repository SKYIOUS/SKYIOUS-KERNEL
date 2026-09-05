//! Early boot logger.
//!
//! Uses serial output for all diagnostics during boot.
//! Initialized before heap allocation is available.

/// Trait for serial output during boot.
pub trait BootLogger: Send + Sync {
    fn write(&self, msg: &str);
}

static mut LOGGER: Option<&'static dyn BootLogger> = None;

/// Initialize the boot logger.
///
/// # Safety
///
/// Must be called exactly once, before any log output.
pub unsafe fn init(logger: &'static dyn BootLogger) {
    LOGGER = Some(logger);
}

/// Log a message during boot.
pub fn log(msg: &str) {
    unsafe {
        if let Some(l) = LOGGER {
            l.write(msg);
        }
    }
}

/// Log with a tag prefix.
pub fn log_tag(tag: &str, msg: &str) {
    log("[");
    log(tag);
    log("] ");
    log(msg);
    log("\n");
}
