use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;

pub trait Timer: Send + Sync {
    fn ticks(&self) -> u64;
    fn set_period(&self, micros: u64);
    fn start(&self);
    fn stop(&self);
    fn resolution_ns(&self) -> u64;
    fn calibrate(&self);
}

pub struct TscTimer {
    tsc_freq: AtomicU64,
    tsc_start: AtomicU64,
}

impl TscTimer {
    pub const fn new() -> Self {
        TscTimer {
            tsc_freq: AtomicU64::new(0),
            tsc_start: AtomicU64::new(0),
        }
    }

    pub fn init(&self, cpu_freq_hz: u64) {
        self.tsc_freq.store(cpu_freq_hz, Ordering::Relaxed);
        let lo: u32;
        let hi: u32;
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
        self.tsc_start.store(((hi as u64) << 32) | lo as u64, Ordering::Relaxed);
    }
}

impl Timer for TscTimer {
    fn ticks(&self) -> u64 {
        let freq = self.tsc_freq.load(Ordering::Relaxed);
        if freq == 0 { return 0; }
        let lo: u32; let hi: u32;
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi) };
        let now = ((hi as u64) << 32) | lo as u64;
        let start = self.tsc_start.load(Ordering::Relaxed);
        (now.wrapping_sub(start)) / (freq / 1_000_000)
    }

    fn set_period(&self, _micros: u64) { }
    fn start(&self) { }
    fn stop(&self) { }
    fn resolution_ns(&self) -> u64 { 1 }
    fn calibrate(&self) { }
}

static CURRENT_TIMER: Mutex<Option<Arc<dyn Timer>>> = Mutex::new(None);

pub fn register_timer(timer: Arc<dyn Timer>) {
    *CURRENT_TIMER.lock() = Some(timer);
}

pub fn get_ticks() -> u64 {
    CURRENT_TIMER.lock().as_ref().map_or(0, |t| t.ticks())
}

pub fn current_time_us() -> u64 {
    CURRENT_TIMER.lock().as_ref().map_or(0, |t| t.ticks())
}
