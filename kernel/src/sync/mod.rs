//! Synchronization primitives for the Vahi kernel.
//!
//! `IrqSafeMutex` is provided by the `vahi-sync` crate and re-exported here
//! so existing `use crate::sync::IrqSafeMutex` imports continue to work.
//!
//! RCU and CFI remain in-kernel because they depend on `alloc` and kernel
//! subsystems.

// Re-export the core type from the external crate.
pub use vahi_sync::{IrqSafeMutex, IrqSafeMutexGuard};

// RCU (Read-Copy-Update) synchronization
pub mod rcu;

// Control Flow Integrity
pub mod cfi;
