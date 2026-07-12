use spin::Mutex;

const FIXED: &[u8] = &[32, 33, 44, 43, 250, 251];
const MSI_START: u8 = 0x50;
const MSI_END: u8 = 0xFE;

struct Bits([u64; 4]);

impl Bits {
    fn set(&mut self, v: u8) { self.0[(v / 64) as usize] |= 1 << (v % 64); }
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

    #[allow(dead_code)]
    fn free(&mut self, v: u8) { if v >= MSI_START && v < MSI_END { self.bits.set(v); } }
}

static POOL: Mutex<Option<Pool>> = Mutex::new(None);

pub fn init() {
    *POOL.lock() = Some(Pool::new());
}

pub fn alloc() -> Option<u8> { POOL.lock().as_mut().and_then(|p| p.alloc()) }

#[allow(dead_code)]
pub fn free(v: u8) {
    if let Some(ref mut p) = *POOL.lock() { p.free(v); }
}

pub fn msi_addr(dest: u8) -> u32 { 0xFEE00000 | ((dest as u32) << 12) }
pub fn msi_data(vec: u8) -> u16 { vec as u16 }
