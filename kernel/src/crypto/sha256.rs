use core::cmp::min;

// ── SHA-256 implementation (FIPS 180-4) ──────────────────────────────────────

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn rotr(x: u32, n: u32) -> u32 { (x >> n) | (x << (32 - n)) }

/// Compute SHA-256 hash. Input `data` is processed; output `out` must be 32 bytes.
pub fn sha256(data: &[u8], out: &mut [u8]) {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let data_len_bits = (data.len() as u64) * 8;
    let total_blocks = (data.len() + 9 + 63) / 64;
    let mut block = [0u8; 64];

    for b in 0..total_blocks {
        block.iter_mut().for_each(|b| *b = 0);
        let start = b * 64;
        let remaining = data.len().saturating_sub(start);
        let chunk_end = min(remaining, 64);
        if chunk_end > 0 {
            block[..chunk_end].copy_from_slice(&data[start..start + chunk_end]);
        }

        // Padding
        if chunk_end < 64 {
            block[chunk_end] = 0x80;
            if chunk_end + 8 >= 64 {
                // Not enough room for length — process this block, next one will have length
                sha256_block(&mut h, &block);
                block.iter_mut().for_each(|b| *b = 0);
            }
            // Write length in last 8 bytes (big-endian)
            block[56..64].copy_from_slice(&data_len_bits.to_be_bytes());
        }

        if b == total_blocks - 1 || chunk_end < 64 {
            // For the final block (or overflow block), write length
            if total_blocks > 1 && chunk_end < 64 && chunk_end + 8 >= 64 {
                // Length already handled in the overflow block above
            } else if b == total_blocks - 1 {
                block[56..64].copy_from_slice(&data_len_bits.to_be_bytes());
            }
        }

        sha256_block(&mut h, &block);
    }

    // Output (big-endian)
    for i in 0..8 {
        out[i * 4..(i + 1) * 4].copy_from_slice(&h[i].to_be_bytes());
    }
}

fn sha256_block(h: &mut [u32; 8], block: &[u8; 64]) {
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
        w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
    }

    let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
        (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

    for t in 0..64 {
        let s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
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

// ── HMAC-SHA256 (RFC 2104) ──────────────────────────────────────────────────

/// Compute HMAC-SHA256. `key` and `msg` are inputs; `out` must be 32 bytes.
pub fn hmac_sha256(key: &[u8], msg: &[u8], out: &mut [u8]) {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        let mut hashed = [0u8; 32];
        sha256(key, &mut hashed);
        k[..32].copy_from_slice(&hashed);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    // ipad = k XOR 0x36
    let mut ipad = [0u8; 64];
    for i in 0..64 { ipad[i] = k[i] ^ 0x36; }

    // opad = k XOR 0x5c
    let mut opad = [0u8; 64];
    for i in 0..64 { opad[i] = k[i] ^ 0x5c; }

    // Inner: SHA256(ipad || msg)
    let mut inner_digest = [0u8; 32];
    if msg.len() <= 64 {
        let mut buf = [0u8; 128];
        buf[..64].copy_from_slice(&ipad);
        buf[64..64 + msg.len()].copy_from_slice(msg);
        sha256(&buf[..64 + msg.len()], &mut inner_digest);
    } else {
        let mut buf = alloc::vec![0u8; 64 + msg.len()];
        buf[..64].copy_from_slice(&ipad);
        buf[64..64 + msg.len()].copy_from_slice(msg);
        sha256(&buf, &mut inner_digest);
    }

    // Outer: SHA256(opad || inner_digest)
    let mut outer_buf = [0u8; 96]; // opad(64) + inner(32)
    outer_buf[..64].copy_from_slice(&opad);
    outer_buf[64..96].copy_from_slice(&inner_digest);
    sha256(&outer_buf, out);
}

// ── PBKDF2-HMAC-SHA256 (RFC 2898) ──────────────────────────────────────────

/// Derive key using PBKDF2-HMAC-SHA256.
/// `password`, `salt` inputs; `iterations` >= 1; `dk_len` output length.
/// Writes to `out` (must be at least `dk_len` bytes).
pub fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32, dk_len: usize, out: &mut [u8]) {
    let hlen = 32; // SHA256 output length
    let l = (dk_len + hlen - 1) / hlen; // number of blocks
    let mut t = alloc::vec![0u8; l * hlen];
    let mut u = [0u8; 32];

    for block in 1..=l {
        // U1 = HMAC(password, salt || INT_32_BE(block))
        let mut salt_block = alloc::vec![0u8; salt.len() + 4];
        salt_block[..salt.len()].copy_from_slice(salt);
        salt_block[salt.len()..].copy_from_slice(&(block as u32).to_be_bytes());
        hmac_sha256(password, &salt_block, &mut u);

        // T_block = U1
        let block_off = (block - 1) * hlen;
        t[block_off..block_off + hlen].copy_from_slice(&u);

        // U2..Uc
        for _ in 1..iterations {
            let mut u_next = [0u8; 32];
            hmac_sha256(password, &u, &mut u_next);
            u = u_next;
            for j in 0..hlen {
                t[block_off + j] ^= u[j];
            }
        }
    }

    out[..dk_len].copy_from_slice(&t[..dk_len]);
}
