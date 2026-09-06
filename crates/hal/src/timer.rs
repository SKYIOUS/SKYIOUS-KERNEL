//! Timer abstraction — TSC-based timer with trait for other sources.
//!
//! Provides a global timer interface for the scheduler and sleep
//! syscalls. The TSC (Time Stamp Counter) is the default timer
//! on x86_64; other architectures can register alternative timers.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};
use vahi_sync::IrqSafeMutex as Mutex;

/// High-resolution timer source.
///
/// Implement this trait for HPET, ACPI PM timer, ARM generic timer, etc.
pub trait Timer: Send + Sync {
    /// Read the current time in microseconds.
    fn ticks(&self) -> u64;

    /// Set the timer period in microseconds (for periodic mode).
    fn set_period(&self, micros: u64);

    /// Start the timer.
    fn start(&self);

    /// Stop the timer.
    fn stop(&self);

    /// Get the timer resolution in nanoseconds.
    fn resolution_ns(&self) -> u64;

    /// Calibrate the timer (measure actual frequency).
    fn calibrate(&self);
}

#[cfg(target_arch = "x86_64")]
/// TSC-based timer using the x86_64 Time Stamp Counter.
///
/// Resolution is ~1 ns on modern CPUs with constant TSC.
pub struct TscTimer {
    /// TSC frequency in Hz (set during calibration).
    tsc_freq: AtomicU64,
    /// TSC value at initialization (for computing elapsed time).
    tsc_start: AtomicU64,
}

#[cfg(target_arch = "x86_64")]
impl TscTimer {
    /// Create a new uninitialized TSC timer.
    pub const fn new() -> Self {
        TscTimer {
            tsc_freq: AtomicU64::new(0),
            tsc_start: AtomicU64::new(0),
        }
    }

    /// Initialize the timer with the given CPU frequency.
    pub fn init(&self, cpu_freq_hz: u64) {
        self.tsc_freq.store(cpu_freq_hz, Ordering::Relaxed);
        let lo: u32;
        let hi: u32;
        // SAFETY: RDTSC reads the time stamp counter into EDX:EAX.
        // Non-privileged instruction, safe from any privilege level.
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
        self.tsc_start
            .store(((hi as u64) << 32) | lo as u64, Ordering::Relaxed);
    }
}

#[cfg(target_arch = "x86_64")]
impl Default for TscTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "x86_64")]
impl Timer for TscTimer {
    fn ticks(&self) -> u64 {
        let freq = self.tsc_freq.load(Ordering::Relaxed);
        if freq == 0 {
            return 0;
        }
        let lo: u32;
        let hi: u32;
        // SAFETY: RDTSC reads the time stamp counter into EDX:EAX.
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
        let now = ((hi as u64) << 32) | lo as u64;
        let start = self.tsc_start.load(Ordering::Relaxed);
        (now.wrapping_sub(start)) / (freq / 1_000_000)
    }

    fn set_period(&self, _micros: u64) {}
    fn start(&self) {}
    fn stop(&self) {}
    fn resolution_ns(&self) -> u64 {
        1
    }
    fn calibrate(&self) {}
}

/// Global timer instance. Set once during boot.
static CURRENT_TIMER: Mutex<Option<Arc<dyn Timer>>> = Mutex::new(None);

/// Register the system's timer source.
pub fn register_timer(timer: Arc<dyn Timer>) {
    *CURRENT_TIMER.lock() = Some(timer);
}

/// Get the current time in microseconds from the registered timer.
pub fn current_time_us() -> u64 {
    CURRENT_TIMER.lock().as_ref().map_or(0, |t| t.ticks())
}
