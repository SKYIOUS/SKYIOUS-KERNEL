use crate::sync::IrqSafeMutex as Mutex;

/// Vectors permanently reserved by the kernel and never handed out to PCI
/// devices: 0..=31 (CPU exceptions and reserved vectors), 32..=47 (the fixed
/// legacy PIC/APIC vectors: timer=32, keyboard=33, mouse=44, network=43), and
/// 250..=251 (IPI vectors reserved by the SMP layer: TLB shootdown=250,
/// function call=251).
const FIXED: &[u8] = &[32, 33, 44, 43, 250, 251];
/// First dynamically-allocatable MSI vector. MSI targets 0x50..=0xFE
/// (above the exception/legacy range, below the 255 spurious-vector cap).
const MSI_START: u8 = 0x50;
/// Exclusive upper bound of the MSI vector range.
const MSI_END: u8 = 0xFE;

/// Bitmap-backed vector allocator used by the MSI layer.
///
/// `first_zero` is a linear scan: with only ~208 allocatable bits (0x50..0xFE)
/// this is cheap in practice. If the allocatable vector space ever grows (e.g.
/// wider MSI-X tables or a second allocator for high vectors), replace the scan
/// with an intrinsic such as `u64::bit_smear + trailing_zeros` /
/// `find_first_zero_set` per platform rather than a full 256-bit scan.
struct Bits([u64; 4]);

impl Bits {
    fn set(&mut self, v: u8) { self.0[(v / 64) as usize] |= 1 << (v % 64); }
    fn clear(&mut self, v: u8) { self.0[(v / 64) as usize] &= !(1 << (v % 64)); }
    fn test(&self, v: u8) -> bool { (self.0[(v / 64) as usize] >> (v % 64)) & 1 != 0 }
    fn first_zero(&self, s: u8, e: u8) -> Option<u8> { (s..e).find(|&v| !self.test(v)) }
}

struct Pool {
    bits: Bits,
    next: u8,
}

impl Pool {
    fn new() -> Self {
        let mut b = Bits([0; 4]);
        for i in 0..32u8 { b.set(i); }
        for &v in FIXED { b.set(v); }
        Pool { bits: b, next: MSI_START }
    }

    fn alloc(&mut self) -> Option<u8> {
        let v = self.bits.first_zero(self.next, MSI_END)
            .or_else(|| self.bits.first_zero(MSI_START, self.next))?;
        self.bits.set(v);
        self.next = v.wrapping_add(1);
        if self.next < MSI_START || self.next >= MSI_END { self.next = MSI_START; }
        Some(v)
    }

    /// Allocate `count` contiguous vectors. Returns the base vector or None.
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
                if self.next >= MSI_END { self.next = MSI_START; }
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

    #[allow(dead_code)]
    fn free(&mut self, v: u8) { if v >= MSI_START && v < MSI_END { self.bits.clear(v); } }
}

/// `alloc()` hands out one MSI vector from the `MSI_START..MSI_END` window,
/// skipping the `FIXED` reserved vectors (see above). Returns `None` when the
/// MSI/MSI-X vector space is exhausted. Paired with `pci_enable_msi` and
/// `pci_route_legacy_irq`; every allocated vector must eventually be freed
/// via `free()` when the device is detached.
static POOL: Mutex<Option<Pool>> = Mutex::new(None);

/// Initialize the MSI vector pool. Called once from `apic::init()` during boot,
/// before any PCI device is enumerated (PCI MSI allocation depends on this).
pub fn init() {
    *POOL.lock() = Some(Pool::new());
}

/// Allocate a single MSI vector. See `Pool::alloc`.
pub fn alloc() -> Option<u8> { POOL.lock().as_mut().and_then(|p| p.alloc()) }

/// Release a previously-allocated MSI vector back to the pool.
pub fn free(v: u8) {
    if let Some(ref mut p) = *POOL.lock() { p.free(v); }
}

/// Release a contiguous range of MSI vectors (used by MSI-X).
pub fn free_range(base: u8, count: u32) {
    if let Some(ref mut p) = *POOL.lock() {
        p.free_contiguous(base, count);
    }
}

pub fn msi_addr(dest: u8) -> u32 { super::LAPIC_PHYS_BASE as u32 | ((dest as u32) << 12) }
pub fn msi_data(vec: u8) -> u16 { vec as u16 }


