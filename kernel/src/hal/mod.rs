//! Hardware abstraction layer — partially re-exported from vahi-hal crate.
//!
//! `irq`, `timer`, `platform` are extracted to `vahi-hal`.
//! `dma` and `exec_mem` remain in-kernel (depend on kernel memory subsystem).

pub use vahi_hal::irq;
pub use vahi_hal::platform;
pub use vahi_hal::timer;

pub mod dma;
pub mod exec_mem;
