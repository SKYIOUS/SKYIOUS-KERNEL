//! Cryptographic primitives for the Vahi kernel.
//!
//! Pure `#![no_std]` implementation with no kernel dependencies.
//! - SHA-256 (FIPS 180-4)
//! - HMAC-SHA256 (RFC 2104)
//! - PBKDF2-HMAC-SHA256 (RFC 2898)
//! - Entropy harvester (RDTSC + RDRAND + SHA-256 mixing)

#![no_std]

extern crate alloc;

pub mod entropy;
pub mod sha256;

pub use entropy::GLOBAL_ENTROPY;
pub use sha256::{hmac_sha256, pbkdf2, sha256};
