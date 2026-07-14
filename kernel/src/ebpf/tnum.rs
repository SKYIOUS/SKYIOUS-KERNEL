#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tnum {
    pub value: u64,
    pub mask: u64,
}

impl Tnum {
    pub const fn exact(v: u64) -> Self { Tnum { value: v, mask: 0 } }
    pub const fn any() -> Self { Tnum { value: 0, mask: u64::MAX } }
    pub fn from_const(v: u64, bit_count: u8) -> Self {
        if bit_count >= 64 { Self::exact(v) }
        else { let mask = u64::MAX << bit_count; Tnum { value: v & !mask, mask } }
    }
    pub fn from_range(lo: u64, hi: u64) -> Self {
        if lo == hi { return Self::exact(lo); }
        if lo > hi { return Self::any(); }
        let diff = lo ^ hi;
        let mask = (1u64 << (64 - diff.leading_zeros())) - 1;
        Tnum { value: lo & !mask, mask }
    }
    pub fn for_aligned_access(offset: u64, size: u64) -> Self {
        let end = offset + size - 1;
        Tnum { value: offset & !(size - 1), mask: end & (size - 1) }
    }
    pub fn add(self, rhs: Self) -> Self {
        let value = self.value.wrapping_add(rhs.value);
        let unknown_carry = (self.mask | rhs.mask) != 0;
        let mask = if unknown_carry {
            let carry_mask = (self.value & rhs.value) | (self.mask | rhs.mask);
            let propagate = (carry_mask & 1) | ((carry_mask >> 1) & (self.mask | rhs.mask));
            self.mask | rhs.mask | propagate
        } else { 0 };
        Tnum { value, mask }
    }
    pub fn sub(self, rhs: Self) -> Self {
        Tnum { value: self.value.wrapping_sub(rhs.value), mask: self.mask | rhs.mask }
    }
    pub fn mul(self, rhs: Self) -> Self {
        if self.mask == 0 && rhs.mask == 0 {
            Self::exact(self.value.wrapping_mul(rhs.value))
        } else { Self::any() }
    }
    pub fn and(self, rhs: Self) -> Self {
        Tnum {
            value: self.value & rhs.value,
            mask: (self.mask & !rhs.value) | (rhs.mask & !self.value) | (self.mask & rhs.mask),
        }
    }
    pub fn or(self, rhs: Self) -> Self {
        Tnum { value: (self.value | rhs.value) & !(self.mask | rhs.mask), mask: self.mask | rhs.mask }
    }
    pub fn xor(self, rhs: Self) -> Self {
        Tnum { value: (self.value ^ rhs.value) & !(self.mask | rhs.mask), mask: self.mask | rhs.mask }
    }
    pub fn shl(self, rhs: Self) -> Self {
        if rhs.mask != 0 { return Self::any(); }
        let shift = rhs.value;
        if shift >= 64 { return Self::any(); }
        Tnum { value: self.value << shift, mask: self.mask << shift }
    }
    pub fn lshr(self, rhs: Self) -> Self {
        if rhs.mask != 0 { return Self::any(); }
        let shift = rhs.value;
        if shift >= 64 { return Self::any(); }
        Tnum { value: self.value >> shift, mask: self.mask >> shift }
    }
    pub fn ashr(self, rhs: Self) -> Self {
        if rhs.mask != 0 { return Self::any(); }
        Tnum { value: (self.value as i64 >> rhs.value) as u64, mask: u64::MAX }
    }
    pub fn neg(self) -> Self {
        Tnum { value: (!self.value).wrapping_add(1), mask: self.mask }
    }
    pub fn min(&self) -> u64 { self.value }
    pub fn max(&self) -> u64 { self.value | self.mask }
    pub fn could_be_zero(&self) -> bool { (self.value & !self.mask) == 0 }
    pub fn could_be_nonzero(&self) -> bool { self.value != 0 || self.mask != 0 }
    pub fn is_exact(&self) -> bool { self.mask == 0 }
    pub fn is_subrange(&self, lo: u64, hi: u64) -> bool { self.min() >= lo && self.max() <= hi }
    pub fn is_valid(&self) -> bool { self.value & self.mask == 0 }
    pub fn merge(self, other: Self) -> Self {
        Tnum { value: self.value & other.value, mask: self.mask | other.mask | (self.value ^ other.value) }
    }
    pub fn intersect(self, other: Self) -> Self {
        let nv = self.value | other.value;
        let nm = self.mask & other.mask;
        Tnum { value: nv & !nm, mask: nm }
    }
}
