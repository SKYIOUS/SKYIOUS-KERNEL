//! # vahi-hal — Hardware Abstraction Layer
//!
//! Platform-independent interfaces for hardware interaction, decoupling
//! kernel logic from x86_64/aarch64 specifics.
//!
//! ## Exports
//!
//! | Module | Purpose |
//! |--------|---------|
//! | `irq` | IRQ controller trait: EOI, mask, unmask, route, affinity |
//! | `platform` | Platform info: arch, CPU count, frequency, RAM size |
//! | `timer` | TSC-based microsecond clock: `current_time_us()` |
//! | `dma` | DMA buffer allocation: `DmaBuf` (physical-contiguous), `PooledDma` (per-device pool) |
//!
//! ## Clocks
//!
//! `timer::current_time_us()` is the real TSC-based microsecond clock
//! (registered by the kernel at boot). The 100Hz tick counter lives in
//! `vahi_kernel::interrupts` only — other crates declare it `extern`.
//!
//! ## Invariants
//!
//! - DMA buffers must be physically contiguous (allocated via buddy allocator)
//! - DMA buffers must be cache-line aligned (64 bytes) for device DMA
//! - IRQ handlers calling HAL functions must not allocate heap memory

#![no_std]

extern crate alloc;

pub mod dma;
pub mod irq;
pub mod platform;
pub mod timer;
