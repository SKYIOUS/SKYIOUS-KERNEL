//! # vahi-types — Shared Types & Cycle-Breaking Interfaces
//!
//! Foundation crate that defines shared types and trait interfaces used across
//! kernel modules. Breaking these into a separate crate eliminates the circular
//! dependencies that prevent individual module extraction.
//!
//! Each module below is one concern of the kernel boundary; the root
//! re-exports everything flat so consumers keep using `vahi_types::X`.
//!
//! ## Crate Rules
//!
//! - **No kernel dependencies.** This crate must never depend on `vahi_kernel`.
//! - **Trait definitions only.** No implementations — the kernel provides them.
//! - **`no_std` only.** Uses `alloc` for `Vec`/`String` where needed.
//! - **API stability.** Breaking changes require an ADR.

#![no_std]

extern crate alloc;

pub mod errno;
pub mod identity;
pub mod provider;
pub mod registry;
pub mod vm;

pub use errno::*;
pub use identity::*;
pub use provider::*;
pub use registry::*;
pub use vm::*;

#[cfg(test)]
mod tests;
