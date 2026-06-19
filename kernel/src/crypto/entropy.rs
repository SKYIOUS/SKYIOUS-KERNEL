use core::sync::atomic::{AtomicU64, Ordering};
use crate::crypto::sha256;

/// A robust entropy harvester for SARGA OS.
/// It mixes multiple sources of entropy to provide high-quality random seeds.
pub struct EntropyHarvester {
    pool: AtomicU64,
}

impl EntropyHarvester {
    pub const fn new() -> Self {
        EntropyHarvester {
            pool: AtomicU64::new(0),
        }
    }

    /// Adds entropy from a raw source.
    pub fn add_entropy(&self, val: u64) {
        // Simple mixing using a prime multiplier and XOR
        self.pool.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |old| {
            Some(old.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(val) ^ (val >> 13))
        }).ok();
    }

    /// Generates a 64-bit random value by harvesting available hardware sources.
    pub fn get_u64(&self) -> u64 {
        // 1. RDTSC (Always available)
        let lo: u32;
        let hi: u32;
        unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags)); }
        let tsc = ((hi as u64) << 32) | (lo as u64);
        self.add_entropy(tsc);

        // 2. RDRAND (If supported)
        let mut rdrand_val: u64 = 0;
        let success: u8;
        unsafe {
            core::arch::asm!(
                "rdrand {0}",
                "setc {1}",
                out(reg) rdrand_val,
                out(reg_byte) success,
                options(nostack, preserves_flags)
            );
        }
        if success != 0 {
            self.add_entropy(rdrand_val);
        }

        // 3. Current pool value mixed with a hash
        let current = self.pool.load(Ordering::SeqCst);
        let mut hash_input = [0u8; 16];
        hash_input[..8].copy_from_slice(&current.to_le_bytes());
        hash_input[8..16].copy_from_slice(&tsc.to_le_bytes());

        let mut output = [0u8; 32];
        sha256(&hash_input, &mut output);
        let mut result = 0u64;
        for i in 0..8 {
            result = (result << 8) | (output[i] as u64);
        }

        self.add_entropy(result);
        result
    }
}

pub static GLOBAL_ENTROPY: EntropyHarvester = EntropyHarvester::new();
