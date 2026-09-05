//! Cryptographic primitives — re-exported from vahi-crypto crate.

pub use vahi_crypto::entropy;
pub use vahi_crypto::sha256;

pub use vahi_crypto::GLOBAL_ENTROPY;
pub use vahi_crypto::{hmac_sha256, pbkdf2, sha256 as sha256_hash};
