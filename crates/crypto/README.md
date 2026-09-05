# vahi-crypto

Pure `#![no_std]` cryptographic primitives for the Vahi kernel.

## Algorithms

| Algorithm | Standard | Use Case |
|-----------|----------|----------|
| SHA-256 | FIPS 180-4 | File integrity, KASLR entropy |
| HMAC-SHA256 | RFC 2104 | Keyed message authentication |
| PBKDF2-HMAC-SHA256 | RFC 2898 | Password hashing |
| RDRAND+TSC entropy | Intel SDM | Kernel RNG seeding |

## API

```rust
use vahi_crypto::{sha256, hmac_sha256, pbkdf2};

let hash = sha256(b"hello world");
let mac = hmac_sha256(key, message);
pbkdf2(password, salt, iterations, &mut output);
```

## Entropy

`GLOBAL_ENTROPY` is a kernel-wide entropy source mixing RDRAND,
TSC jitter, and previous output through SHA-256.

```rust
let random_bytes = vahi_crypto::GLOBAL_ENTROPY.fill(buf)?;
```

## Zero Dependencies

Only uses `alloc` (for HMAC/PBKDF2 output). No heap allocation in
core SHA-256 (stack-based, 256-byte working buffer).
