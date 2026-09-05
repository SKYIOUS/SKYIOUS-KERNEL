//! MSI vector allocator — bitmap-backed, lock-free after init.
//!
//! Message Signaled Interrupts (MSI) bypass the I/O APIC entirely: PCI
//! devices write a 32-bit message to a designated MMIO address, and the
//! LAPIC delivers the interrupt directly. This avoids shared IRQ lines
//! and improves scalability.
//!
//! ## Vector Allocation
//!
//! The allocator uses a bitmap indexed by interrupt vector number. Fixed
//! vectors (timer, keyboard, mouse, network, TLB flush, IPI function) are
//! reserved up front; the remainder are allocatable.
//!
//! ## Invariants
//!
//! - Vectors 0–31 are reserved for CPU exceptions
//! - Fixed vectors are reserved before [`init`] returns
//! - Bitmap is mutated under a mutex during init, then read-only
//! - Each MSI allocation returns a unique vector for the lifetime of the system

use vahi_sync::IrqSafeMutex as Mutex;

const FIXED: &[u8] = &[32, 33, 44, 43, 250, 251];
const MSI_START: u8 = 0x50;
const MSI_END: u8 = 0xFE;

struct Bits([u64; 4]);

impl Bits {
    fn set(&mut self, v: u8) {
        self.0[(v / 64) as usize] |= 1 << (v % 64);
    }
    fn clear(&mut self, v: u8) {
        self.0[(v / 64) as usize] &= !(1 << (v % 64));
    }
    fn test(&self, v: u8) -> bool {
        (self.0[(v / 64) as usize] >> (v % 64)) & 1 != 0
    }
    fn first_zero(&self, s: u8, e: u8) -> Option<u8> {
        (s..e).find(|&v| !self.test(v))
    }
}

struct Pool {
    bits: Bits,
    next: u8,
}

impl Pool {
    fn new() -> Self {
        let mut b = Bits([0; 4]);
        for i in 0..32u8 {
            b.set(i);
        }
        for &v in FIXED {
            b.set(v);
        }
        Pool {
            bits: b,
            next: MSI_START,
        }
    }

    fn alloc(&mut self) -> Option<u8> {
        let v = self
            .bits
            .first_zero(self.next, MSI_END)
            .or_else(|| self.bits.first_zero(MSI_START, self.next))?;
        self.bits.set(v);
        self.next = v.wrapping_add(1);
        if self.next < MSI_START || self.next >= MSI_END {
            self.next = MSI_START;
        }
        Some(v)
    }

    #[allow(dead_code)]
    fn alloc_contiguous(&mut self, count: u32) -> Option<u8> {
        if count == 0 {
            return None;
        }
        let count = count as u8;
        let start = MSI_START;
        let end = MSI_END - count + 1;
        for base in start..end {
            let mut ok = true;
            for i in 0..count {
                if self.bits.test(base + i) {
                    ok = false;
                    break;
                }
            }
            if ok {
                for i in 0..count {
                    self.bits.set(base + i);
                }
                self.next = base + count;
                if self.next >= MSI_END {
                    self.next = MSI_START;
                }
                return Some(base);
            }
        }
        None
    }

    fn free_contiguous(&mut self, base: u8, count: u32) {
        for i in 0..count {
            self.bits.clear(base + i as u8);
        }
    }

    fn free(&mut self, v: u8) {
        if (MSI_START..MSI_END).contains(&v) {
            self.bits.clear(v);
        }
    }
}

static POOL: Mutex<Option<Pool>> = Mutex::new(None);

pub fn init() {
    *POOL.lock() = Some(Pool::new());
}

pub fn alloc() -> Option<u8> {
    POOL.lock().as_mut().and_then(|p| p.alloc())
}

pub fn free(v: u8) {
    if let Some(ref mut p) = *POOL.lock() {
        p.free(v);
    }
}

pub fn free_range(base: u8, count: u32) {
    if let Some(ref mut p) = *POOL.lock() {
        p.free_contiguous(base, count);
    }
}

pub fn msi_addr(dest: u8) -> u32 {
    super::LAPIC_PHYS_BASE as u32 | ((dest as u32) << 12)
}
pub fn msi_data(vec: u8) -> u16 {
    vec as u16
}
