//! SHA-256 (FIPS 180-4), HMAC-SHA256 (RFC 2104), PBKDF2-HMAC-SHA256 (RFC 2898).
//!
//! Pure `#![no_std]` implementation. Stack-based for small inputs;
//! heap-allocated only when HMAC or PBKDF2 input exceeds 64 bytes.

// ── Constants ───────────────────────────────────────────────────────────────

/// SHA-256 block size in bytes.
const BLOCK_SIZE: usize = 64;

/// SHA-256 digest size in bytes.
pub const DIGEST_SIZE: usize = 32;

/// HMAC inner padding byte.
const IPAD: u8 = 0x36;

/// HMAC outer padding byte.
const OPAD: u8 = 0x5c;

/// HMAC key block size (same as SHA-256 block size).
const HMAC_BLOCK_SIZE: usize = BLOCK_SIZE;

/// SHA-256 initial hash values (FIPS 180-4 §5.3.3).
const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// SHA-256 round constants (FIPS 180-4 §4.2.2).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

// ── SHA-256 Core ─────────────────────────────────────────────────────────────

/// Right rotation by `n` bits.
#[inline(always)]
const fn rotr(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

/// Process a single 64-byte SHA-256 block.
#[inline]
fn sha256_block(h: &mut [u32; 8], block: &[u8; BLOCK_SIZE]) {
    let mut w = [0u32; 64];

    // Prepare message schedule
    for t in 0..16 {
        w[t] = u32::from_be_bytes([
            block[t * 4],
            block[t * 4 + 1],
            block[t * 4 + 2],
            block[t * 4 + 3],
        ]);
    }
    for t in 16..64 {
        let s0 = rotr(w[t - 15], 7) ^ rotr(w[t - 15], 18) ^ (w[t - 15] >> 3);
        let s1 = rotr(w[t - 2], 17) ^ rotr(w[t - 2], 19) ^ (w[t - 2] >> 10);
        w[t] = w[t - 16]
            .wrapping_add(s0)
            .wrapping_add(w[t - 7])
            .wrapping_add(s1);
    }

    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

    for t in 0..64 {
        let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[t])
            .wrapping_add(w[t]);
        let s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

/// Compute SHA-256 hash.
///
/// Writes the 32-byte digest to `out`.
///
/// # Panics
///
/// Panics if `out` is shorter than 32 bytes.
pub fn sha256(data: &[u8], out: &mut [u8]) {
    assert!(
        out.len() >= DIGEST_SIZE,
        "sha256: output buffer must be >= 32 bytes"
    );

    let mut h = H0;
    let data_len_bits = (data.len() as u64) * 8;

    // Process complete 64-byte blocks (excluding padding)
    let complete_blocks = data.len() / BLOCK_SIZE;
    let mut block = [0u8; BLOCK_SIZE];
    for i in 0..complete_blocks {
        let start = i * BLOCK_SIZE;
        block.copy_from_slice(&data[start..start + BLOCK_SIZE]);
        sha256_block(&mut h, &block);
    }

    // Pad the final (possibly partial) block per FIPS 180-4 §5.1.1
    let remaining = data.len() % BLOCK_SIZE;
    block.fill(0);
    block[..remaining].copy_from_slice(&data[complete_blocks * BLOCK_SIZE..]);

    // Append 0x80 bit
    block[remaining] = 0x80;

    // If the length (8 bytes) doesn't fit in this block, process it first
    if remaining + 1 + 8 > BLOCK_SIZE {
        sha256_block(&mut h, &block);
        block.fill(0);
    }

    // Append the 64-bit big-endian bit length in the last 8 bytes
    block[BLOCK_SIZE - 8..].copy_from_slice(&data_len_bits.to_be_bytes());
    sha256_block(&mut h, &block);

    // Write output (big-endian)
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
    }
}

// ── HMAC-SHA256 (RFC 2104) ───────────────────────────────────────────────────

/// Compute HMAC-SHA256.
///
/// Writes the 32-byte MAC to `out`.
///
/// # Panics
///
/// Panics if `out` is shorter than 32 bytes.
pub fn hmac_sha256(key: &[u8], msg: &[u8], out: &mut [u8]) {
    assert!(
        out.len() >= DIGEST_SIZE,
        "hmac_sha256: output buffer must be >= 32 bytes"
    );

    // Step 1: Hash key if longer than block size
    let mut k = [0u8; HMAC_BLOCK_SIZE];
    if key.len() > HMAC_BLOCK_SIZE {
        let mut hashed = [0u8; DIGEST_SIZE];
        sha256(key, &mut hashed);
        k[..DIGEST_SIZE].copy_from_slice(&hashed);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    // Step 2: Compute ipad XOR and opad XOR
    let mut ipad = [0u8; HMAC_BLOCK_SIZE];
    let mut opad = [0u8; HMAC_BLOCK_SIZE];
    for i in 0..HMAC_BLOCK_SIZE {
        ipad[i] = k[i] ^ IPAD;
        opad[i] = k[i] ^ OPAD;
    }

    // Step 3: Inner hash — SHA256(ipad || msg)
    let inner_len = HMAC_BLOCK_SIZE + msg.len();
    let mut inner_input = alloc::vec![0u8; inner_len];
    inner_input[..HMAC_BLOCK_SIZE].copy_from_slice(&ipad);
    inner_input[HMAC_BLOCK_SIZE..].copy_from_slice(msg);
    let mut inner_hash = [0u8; DIGEST_SIZE];
    sha256(&inner_input, &mut inner_hash);

    // Step 4: Outer hash — SHA256(opad || inner_hash)
    let mut outer_input = [0u8; HMAC_BLOCK_SIZE + DIGEST_SIZE];
    outer_input[..HMAC_BLOCK_SIZE].copy_from_slice(&opad);
    outer_input[HMAC_BLOCK_SIZE..].copy_from_slice(&inner_hash);
    sha256(&outer_input, out);
}

// ── PBKDF2-HMAC-SHA256 (RFC 2898) ───────────────────────────────────────────

/// Derive key using PBKDF2-HMAC-SHA256.
///
/// # Arguments
/// * `password` - Password bytes
/// * `salt` - Salt bytes
/// * `iterations` - Number of PBKDF2 iterations (must be >= 1)
/// * `out` - Output buffer (derived key is written here)
///
/// # Panics
///
/// Panics if `iterations` is 0.
pub fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    assert!(iterations >= 1, "pbkdf2: iterations must be >= 1");

    let hlen = DIGEST_SIZE;
    let block_count = out.len().div_ceil(hlen);
    let mut u = [0u8; DIGEST_SIZE];

    for block in 1..=block_count {
        // U1 = HMAC(password, salt || INT_32_BE(block))
        let mut salt_block = alloc::vec![0u8; salt.len() + 4];
        salt_block[..salt.len()].copy_from_slice(salt);
        salt_block[salt.len()..].copy_from_slice(&(block as u32).to_be_bytes());
        hmac_sha256(password, &salt_block, &mut u);

        // Accumulate: T_block = XOR(U1, U2, ..., Uc)
        let block_off = (block - 1) * hlen;
        let end = core::cmp::min(block_off + hlen, out.len());
        out[block_off..end].copy_from_slice(&u[..end - block_off]);

        for _ in 1..iterations {
            let mut u_next = [0u8; DIGEST_SIZE];
            hmac_sha256(password, &u, &mut u_next);
            u = u_next;
            for j in block_off..end {
                out[j] ^= u[j - block_off];
            }
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
