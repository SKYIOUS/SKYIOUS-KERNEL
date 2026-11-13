//! Timer abstraction — TSC-based timer with trait for other sources.
//!
//! Provides a global timer interface for the scheduler and sleep
//! syscalls. The TSC (Time Stamp Counter) is the default timer
//! on x86_64; other architectures can register alternative timers.

use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicU64, Ordering};

/// High-resolution timer source.
///
/// Implement this trait for HPET, ACPI PM timer, ARM generic timer, etc.
/// The `'static` bound is required because `as_any()` returns `&dyn Any`,
/// which requires the implementor to be `'static`. This means timers cannot
/// borrow non-'static data (e.g., references to platform resources with
/// shorter lifetimes).
pub trait Timer: Send + Sync + 'static {
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

    /// Downcast to Any for frequency access (vDSO).
    /// Only implemented for concrete types that support downcasting (e.g., TscTimer).
    /// Returns None if the timer type does not support downcasting.
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        None
    }
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
        self.set_frequency(cpu_freq_hz);
        let lo: u32;
        let hi: u32;
        // SAFETY: RDTSC reads the time stamp counter into EDX:EAX.
        // Non-privileged instruction, safe from any privilege level.
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
        self.tsc_start
            .store(((hi as u64) << 32) | lo as u64, Ordering::Relaxed);
    }

    /// Update the TSC frequency without resetting the time base.
    /// Useful when frequency is calibrated after initial registration.
    pub fn set_frequency(&self, freq_hz: u64) {
        // Relaxed is safe: synchronized via PLATFORM mutex in set_cpu_frequency()/platform::get().
        // The caller must hold the platform lock or otherwise synchronize.
        self.tsc_freq.store(freq_hz, Ordering::Relaxed);
    }

    /// Get the TSC frequency in Hz.
    pub fn tsc_frequency(&self) -> u64 {
        // Relaxed is safe: synchronized via PLATFORM mutex in set_cpu_frequency()/platform::get().
        self.tsc_freq.load(Ordering::Relaxed)
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
        if freq < 1_000_000 {
            return 0;
        }
        let lo: u32;
        let hi: u32;
        // SAFETY: RDTSC reads the time stamp counter into EDX:EAX.
        // Non-privileged instruction, safe from any privilege level.
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
        let now = ((hi as u64) << 32) | lo as u64;
        let start = self.tsc_start.load(Ordering::Relaxed);
        // freq >= 1_000_000 is guaranteed by the check above, so division is safe
        (now.wrapping_sub(start)) / (freq / 1_000_000)
    }

    fn set_period(&self, _micros: u64) {}
    fn start(&self) {}
    fn stop(&self) {}
    fn resolution_ns(&self) -> u64 {
        1
    }
    fn calibrate(&self) {
        // TSC calibration is performed externally via APIC bus frequency measurement
        // (see crate::apic::LapicTimer::calibrate_bus_frequency).
        // The measured frequency is then passed to init() or set_cpu_frequency().
        // This method exists to satisfy the Timer trait but performs no action.
    }

    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }
}

/// Global timer instance. Set once during boot.
static CURRENT_TIMER: Mutex<Option<Arc<dyn Timer>>> = Mutex::new(None);

/// Register the system's timer source.
pub fn register_timer(timer: Arc<dyn Timer>) {
    *CURRENT_TIMER.lock() = Some(timer);
}

/// Get the current timer instance for advanced operations (e.g., frequency updates).
/// Prefer `current_time_us()` for time readings.
pub fn current_timer() -> Option<Arc<dyn Timer>> {
    CURRENT_TIMER.lock().clone()
}

/// Get the current time in microseconds from the registered timer.
pub fn current_time_us() -> u64 {
    CURRENT_TIMER.lock().as_ref().map_or(0, |t| t.ticks())
}

/// Get the TSC frequency in Hz (x86_64 only).
/// Returns 0 if TSC timer not initialized or not on x86_64.
#[cfg(target_arch = "x86_64")]
pub fn get_tsc_frequency() -> u64 {
    CURRENT_TIMER
        .lock()
        .as_ref()
        .and_then(|t| t.as_any())
        .and_then(|a| a.downcast_ref::<TscTimer>())
        .map(|tsc| tsc.tsc_frequency())
        .unwrap_or(0)
}

#[cfg(not(target_arch = "x86_64"))]
pub fn get_tsc_frequency() -> u64 {
    0
}

#[cfg(target_arch = "aarch64")]
/// ARM Generic Timer (CNTVCT_EL0) timer for aarch64.
pub struct CntrvctTimer {
    /// Timer frequency in Hz (typically 50-100 MHz from CNTFRQ_EL0).
    freq_hz: AtomicU64,
    /// Timer value at initialization.
    start_ticks: AtomicU64,
}

#[cfg(target_arch = "aarch64")]
impl CntrvctTimer {
    pub const fn new() -> Self {
        CntrvctTimer {
            freq_hz: AtomicU64::new(0),
            start_ticks: AtomicU64::new(0),
        }
    }

    pub fn init(&self, freq_hz: u64) {
        self.set_frequency(freq_hz);
        // Read CNTVCT_EL0
        let val: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) val);
        }
        self.start_ticks.store(val, Ordering::Relaxed);
    }

    pub fn set_frequency(&self, freq_hz: u64) {
        self.freq_hz.store(freq_hz, Ordering::Relaxed);
    }

    pub fn frequency(&self) -> u64 {
        self.freq_hz.load(Ordering::Relaxed)
    }

    fn read_cntvct(&self) -> u64 {
        let val: u64;
        unsafe {
            core::arch::asm!("mrs {}, cntvct_el0", out(reg) val);
        }
        val
    }
}

#[cfg(target_arch = "aarch64")]
impl Timer for CntrvctTimer {
    fn ticks(&self) -> u64 {
        let freq = self.freq_hz.load(Ordering::Relaxed);
        if freq < 1_000_000 {
            return 0;
        }
        let now = self.read_cntvct();
        let start = self.start_ticks.load(Ordering::Relaxed);
        (now.wrapping_sub(start)) / (freq / 1_000_000)
    }

    fn set_period(&self, _micros: u64) {}
    fn start(&self) {}
    fn stop(&self) {}
    fn resolution_ns(&self) -> u64 {
        // ARM generic timer resolution depends on frequency, typically ~1-20 ns
        1
    }
    fn calibrate(&self) {
        // Frequency is read from CNTFRQ_EL0
    }
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }
}

#[cfg(target_arch = "riscv64")]
/// RISC-V Time CSR / CLINT timer for riscv64.
pub struct RiscvTimeTimer {
    /// Timer frequency in Hz (from DT or CLINT).
    freq_hz: AtomicU64,
    /// Timer value at initialization.
    start_ticks: AtomicU64,
}

#[cfg(target_arch = "riscv64")]
impl RiscvTimeTimer {
    pub const fn new() -> Self {
        RiscvTimeTimer {
            freq_hz: AtomicU64::new(0),
            start_ticks: AtomicU64::new(0),
        }
    }

    pub fn init(&self, freq_hz: u64) {
        self.set_frequency(freq_hz);
        // Read `time` CSR (RV64) or CLINT mtime
        let val: u64;
        unsafe {
            core::arch::asm!("rdtime {}", out(reg) val);
        }
        self.start_ticks.store(val, Ordering::Relaxed);
    }

    pub fn set_frequency(&self, freq_hz: u64) {
        self.freq_hz.store(freq_hz, Ordering::Relaxed);
    }

    pub fn frequency(&self) -> u64 {
        self.freq_hz.load(Ordering::Relaxed)
    }

    fn read_time(&self) -> u64 {
        let val: u64;
        unsafe {
            core::arch::asm!("rdtime {}", out(reg) val);
        }
        val
    }
}

#[cfg(target_arch = "riscv64")]
impl Timer for RiscvTimeTimer {
    fn ticks(&self) -> u64 {
        let freq = self.freq_hz.load(Ordering::Relaxed);
        if freq < 1_000_000 {
            return 0;
        }
        let now = self.read_time();
        let start = self.start_ticks.load(Ordering::Relaxed);
        (now.wrapping_sub(start)) / (freq / 1_000_000)
    }

    fn set_period(&self, _micros: u64) {}
    fn start(&self) {}
    fn stop(&self) {}
    fn resolution_ns(&self) -> u64 {
        1
    }
    fn calibrate(&self) {
        // Frequency is typically fixed or from DT
    }
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        Some(self)
    }
}