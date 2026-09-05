//! Platform information — architecture, CPU count, frequency, RAM size.
//!
//! Set once during boot, read by drivers and scheduler for configuration.

use vahi_sync::IrqSafeMutex as Mutex;

/// Supported CPU architectures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformArch {
    /// x86_64 (AMD64)
    X86_64,
    /// AArch64 (ARM 64-bit)
    AArch64,
    /// RISC-V 64-bit
    RiscV64,
}

/// Hardware platform description, populated during early boot.
#[derive(Debug, Clone, Copy)]
pub struct PlatformInfo {
    /// CPU architecture.
    pub arch: PlatformArch,
    /// Number of CPUs (BSP + APs).
    pub cpu_count: usize,
    /// CPU frequency in Hz (from ACPI or TSC calibration).
    pub cpu_freq_hz: u64,
    /// Total physical RAM in bytes.
    pub ram_size: u64,
    /// Whether FPU is available.
    pub has_fpu: bool,
    /// Whether SIMD (SSE/AVX) is available.
    pub has_simd: bool,
    /// Boot time in timer ticks (for monotonic clock).
    pub boot_time_ticks: u64,
}

impl PlatformInfo {
    /// Default platform info (unknown hardware, single CPU).
    pub const fn unknown() -> Self {
        PlatformInfo {
            arch: PlatformArch::X86_64,
            cpu_count: 1,
            cpu_freq_hz: 0,
            ram_size: 0,
            has_fpu: false,
            has_simd: false,
            boot_time_ticks: 0,
        }
    }
}

/// Global platform info. Set once during boot.
static PLATFORM: Mutex<PlatformInfo> = Mutex::new(PlatformInfo::unknown());

/// Initialize the platform info (called once during boot).
pub fn init(info: PlatformInfo) {
    *PLATFORM.lock() = info;
}

/// Get the current platform info.
pub fn get() -> PlatformInfo {
    *PLATFORM.lock()
}
