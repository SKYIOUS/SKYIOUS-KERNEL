//! GDT/IDT/TSS management for the Vahi kernel.
//!
//! Provides global descriptor table initialization, per-CPU TSS management,
//! and interrupt descriptor table setup. Architecture-specific (x86_64).
//!
//! ## Invariants
//!
//! - GDT must be loaded before any segment register manipulation.
//! - TSS privilege stack table[0] points to the current thread's kernel stack.
//! - Per-CPU GDT/TSS instances are allocated and leaked (never freed).

#![no_std]

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

/// Segment selectors for kernel and user code/data.
#[derive(Debug, Clone, Copy)]
pub struct Selectors {
    pub code_selector: u16,
    pub data_selector: u16,
    pub user_code_selector: u16,
    pub user_data_selector: u16,
    pub tss_selector: u16,
}

/// Trait for providing memory allocation to GDT module.
pub trait GdtMemoryProvider: Send + Sync {
    /// Allocate a page-aligned block of the given size.
    fn alloc_aligned(&self, size: usize) -> Option<*mut u8>;

    /// Allocate a stack (returns base and top addresses).
    fn alloc_stack(&self, pages: usize) -> Option<(u64, u64)>;
}

/// Trait for providing SMP information to GDT module.
pub trait SmpProvider: Send + Sync {
    fn cpu_id(&self) -> u32;
    fn cpu_count(&self) -> u32;
}

/// Global selectors, set once during BSP init.
static SELECTORS: spin::Once<Selectors> = spin::Once::new();

/// Per-CPU privilege stack top (for sysret/syscall).
static PER_CPU_RSP0: [AtomicU64; 256] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; 256]
};

/// Set the privilege stack (RSP0) for a given CPU.
pub fn set_privilege_stack(cpu_id: usize, top: u64) {
    if cpu_id < PER_CPU_RSP0.len() {
        PER_CPU_RSP0[cpu_id].store(top, Ordering::Release);
    }
}

/// Get the privilege stack (RSP0) for a given CPU.
pub fn get_privilege_stack(cpu_id: usize) -> u64 {
    if cpu_id < PER_CPU_RSP0.len() {
        PER_CPU_RSP0[cpu_id].load(Ordering::Acquire)
    } else {
        0
    }
}

/// Register selectors (called once during BSP init).
pub fn register_selectors(selectors: Selectors) {
    SELECTORS.call_once(|| selectors);
}

/// Get the registered selectors.
pub fn selectors() -> Option<Selectors> {
    SELECTORS.get().copied()
}
