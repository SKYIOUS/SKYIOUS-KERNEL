//! # vahi-boot — Boot Sequence & Initialization
//!
//! Manages the boot state machine, early memory setup, ACPI table
//! discovery, and module initialization ordering. ~587 lines.
//!
//! ## Boot State Machine
//!
//! ```text
//! Firmware → Loader → Memory → Heap → Scheduler → SMP → Drivers → Userspace
//!    0         1        2       3        4         5       6          7
//! ```
//!
//! Each state is strictly monotonic — the boot sequence never goes backward.
//!
//! ## Initialization Order
//!
//! | Step | State | What | Depends On |
//! |------|-------|------|------------|
//! | 1 | Firmware | CPU mode, GDT | Nothing |
//! | 2 | Loader | Limine protocol handshake | Firmware |
//! | 3 | Memory | Frame allocator, page tables | Loader |
//! | 4 | Heap | Global heap allocator | Memory |
//! | 5 | Scheduler | Process table, init process | Heap |
//! | 6 | SMP | AP startup via SIPI | Scheduler |
//! | 7 | Drivers | PCI enumeration, driver probe | SMP |
//! | 8 | Userspace | exec /init | Drivers |
//!
//! ## Invariants
//!
//! - Boot state transitions are **monotonic** (never go backward)
//! - Memory init must complete before heap init
//! - Frame allocator must be initialized before any allocation
//! - Scheduler must be initialized before SMP (APs need run queues)
//!
//! ## Dependency Breaking
//!
//! ```text
//! Original:     boot → arch + memory + interrupts + task + drivers
//! With traits:  boot → vahi_types::{ArchOps, register_*}
//! ```

#![no_std]

extern crate alloc;

pub mod logger;
pub mod state;

// ─── Boot State ─────────────────────────────────────────────────────

/// Boot progress state (monotonic — never goes backward).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum BootState {
    Firmware = 0,
    Loader = 1,
    Memory = 2,
    Heap = 3,
    Scheduler = 4,
    Smp = 5,
    Drivers = 6,
    Userspace = 7,
    Complete = 8,
}

impl BootState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Firmware => "Firmware",
            Self::Loader => "Loader",
            Self::Memory => "Memory",
            Self::Heap => "Heap",
            Self::Scheduler => "Scheduler",
            Self::Smp => "SMP",
            Self::Drivers => "Drivers",
            Self::Userspace => "Userspace",
            Self::Complete => "Complete",
        }
    }
}

/// Global boot state tracker.
static mut CURRENT_STATE: BootState = BootState::Firmware;

/// Get the current boot state.
pub fn current_state() -> BootState {
    // SAFETY: CURRENT_STATE is only mutated by advance_state(), which
    // is called sequentially during boot with no concurrent readers.
    unsafe { CURRENT_STATE }
}

/// Advance to the next boot state.
///
/// # Safety
///
/// Must only be called from the boot sequence, exactly once per state
/// transition. The caller must ensure the previous state is complete.
pub unsafe fn advance_state() -> BootState {
    // SAFETY: Called only from boot sequence with single-threaded access.
    let next_val = current_state() as u8 + 1;
    let next = match next_val {
        0 => BootState::Firmware,
        1 => BootState::Loader,
        2 => BootState::Memory,
        3 => BootState::Heap,
        4 => BootState::Scheduler,
        5 => BootState::Smp,
        6 => BootState::Drivers,
        7 => BootState::Userspace,
        8 => BootState::Complete,
        _ => BootState::Complete,
    };
    CURRENT_STATE = next;
    next
}
