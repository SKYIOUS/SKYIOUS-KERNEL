/// Driver module initialization
/// Initializes all hardware drivers with proper error handling
pub fn init() -> Result<(), &'static str> {
    // Initialize serial driver
    crate::drivers::serial::init(0x3F8).map_err(|()| "Failed to initialize serial driver")?;
    
    // Initialize RTC
    crate::drivers::rtc::init().map_err(|()| "Failed to initialize RTC")?;
    
    Ok(())
}

/// Driver module cleanup
/// Properly shuts down all drivers
pub fn cleanup() {
    crate::drivers::serial::cleanup();
    crate::drivers::rtc::cleanup();
}

pub mod storage;
pub mod net;
pub mod graphics;
pub mod gpu;
pub mod mouse;
pub mod ps2;
pub mod block;
pub mod usb;
pub mod watchdog;
pub mod audio;
pub mod input;
pub mod rtc;
pub mod serial;
