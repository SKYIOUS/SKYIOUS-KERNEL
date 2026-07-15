use core::sync::atomic::Ordering;

const FPS_SAMPLES: usize = 16;

pub struct FpsCounter {
    timestamps: [u64; FPS_SAMPLES],
    index: usize,
    count: usize,
    last_fps: core::sync::atomic::AtomicU32,
}

impl FpsCounter {
    pub const fn new() -> Self {
        FpsCounter {
            timestamps: [0; FPS_SAMPLES],
            index: 0,
            count: 0,
            last_fps: core::sync::atomic::AtomicU32::new(60),
        }
    }

    pub fn tick(&mut self, now: u64) {
        self.timestamps[self.index] = now;
        self.index = (self.index + 1) % FPS_SAMPLES;
        if self.count < FPS_SAMPLES {
            self.count += 1;
        }
        if self.count >= 2 {
            let newest = self.timestamps[(self.index + FPS_SAMPLES - 1) % FPS_SAMPLES];
            let oldest = self.timestamps[(self.index + FPS_SAMPLES - self.count) % FPS_SAMPLES];
            let elapsed = newest.saturating_sub(oldest);
            if elapsed > 0 {
                let fps = (self.count as u64 * 1000 / elapsed * 100) as u32;
                self.last_fps.store(fps, Ordering::Relaxed);
            }
        }
    }

    pub fn fps(&self) -> f32 {
        self.last_fps.load(Ordering::Relaxed) as f32 / 100.0
    }

    pub fn refresh_rate(&self) -> u32 {
        60
    }
}

pub fn wait_vsync() {
    let target = crate::interrupts::get_ticks() + 1;
    loop {
        let now = crate::interrupts::get_ticks();
        if now >= target {
            break;
        }
        core::hint::spin_loop();
    }
}
