use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformArch {
    X86_64,
    AArch64,
    RiscV64,
}

#[derive(Debug, Clone, Copy)]
pub struct PlatformInfo {
    pub arch: PlatformArch,
    pub cpu_count: usize,
    pub cpu_freq_hz: u64,
    pub ram_size: u64,
    pub has_fpu: bool,
    pub has_simd: bool,
    pub boot_time_ticks: u64,
}

impl PlatformInfo {
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

static PLATFORM: Mutex<PlatformInfo> = Mutex::new(PlatformInfo::unknown());

pub fn init(info: PlatformInfo) {
    *PLATFORM.lock() = info;
}

pub fn get() -> PlatformInfo {
    PLATFORM.lock().clone()
}
