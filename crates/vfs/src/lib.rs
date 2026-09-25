//! # vahi-vfs — Virtual Filesystem Layer
//!
//! Unified filesystem abstraction with mount management, inode/dentry
//! cache, and support for ext2, ext4, SkyFS, FAT32, ramfs, devfs, FUSE.

#![no_std]
#![allow(dead_code, unused_variables, unused_imports)]
#![allow(
    clippy::result_unit_err,
    clippy::missing_safety_doc,
    clippy::not_unsafe_ptr_arg_deref
)]
#![allow(clippy::manual_range_contains, clippy::needless_range_loop)]
#![allow(clippy::unnecessary_cast, clippy::new_without_default)]
#![allow(clippy::identity_op, clippy::declare_interior_mutable_const)]
#![allow(clippy::type_complexity, clippy::redundant_field_names)]
#![allow(clippy::needless_bool, clippy::manual_clamp)]
#![allow(clippy::explicit_auto_deref, clippy::needless_lifetimes)]
#![allow(
    clippy::never_loop,
    clippy::needless_question_mark,
    clippy::question_mark
)]
#![allow(clippy::byte_char_slices, clippy::manual_checked_ops)]
#![allow(clippy::manual_repeat_n, clippy::only_used_in_recursion)]
#![allow(clippy::unnecessary_sort_by, clippy::needless_borrow)]
#![allow(clippy::new_ret_no_self)]

extern crate alloc;

// ─── Kernel-provided symbols (must precede module declarations) ──

/// Debug print macro — no-op by default; the kernel can opt in.
#[macro_export]
macro_rules! vfs_debug {
    ($($arg:tt)*) => {};
}

#[cfg(all(not(test), target_os = "none"))]
extern "Rust" {
    fn vahi_kernel_serial_write(msg: &str);
}

/// Serial line output, provided by `vahi_kernel`.
///
/// `vahi_kernel::main` defines the `#[no_mangle] fn
/// vahi_kernel_serial_write` symbol this wrapper forwards to, so VFS
/// debug output (ext4 self-test, tarfs, fuse) reaches the real serial
/// port. Host builds/tests call a local no-op instead.
#[cfg(all(not(test), target_os = "none"))]
pub fn vfs_serial_write(msg: &str) {
    // SAFETY: `vahi_kernel_serial_write` is the Rust-ABI #[no_mangle]
    // function vahi_kernel::main defines; the kernel binary always links
    // it, so this extern symbol is defined whenever this crate is compiled
    // into the kernel (the cfg(all(not(test), target_os = "none")) arm).
    unsafe { vahi_kernel_serial_write(msg) }
}

#[cfg(not(all(not(test), target_os = "none")))]
pub fn vfs_serial_write(_msg: &str) {}

#[cfg(all(not(test), target_os = "none"))]
extern "Rust" {
    fn vahi_kernel_serial_putc(c: u8);
}

/// Per-character serial output, provided by `vahi_kernel`.
///
/// `vahi_kernel::main` defines the `#[no_mangle] fn
/// vahi_kernel_serial_putc` symbol this wrapper forwards to. It is the
/// write sink for `/dev/tty0` (userspace stdin/stdout/stderr). Host
/// builds/tests call a local no-op instead.
#[cfg(all(not(test), target_os = "none"))]
pub fn vfs_serial_putc(c: u8) {
    // SAFETY: `vahi_kernel_serial_putc` is the Rust-ABI #[no_mangle]
    // function vahi_kernel::main defines; the kernel binary always links
    // it, so this extern symbol is defined whenever this crate is compiled
    // into the kernel (the cfg(all(not(test), target_os = "none")) arm).
    unsafe { vahi_kernel_serial_putc(c) }
}

#[cfg(not(all(not(test), target_os = "none")))]
pub fn vfs_serial_putc(_c: u8) {}

#[cfg(all(not(test), target_os = "none"))]
extern "Rust" {
    fn vahi_kernel_get_ticks() -> u64;
}

/// Monotonic 100Hz tick counter, provided by `vahi_kernel` (the single
/// owner; per-crate stubs silently return 0).
#[cfg(all(not(test), target_os = "none"))]
pub fn get_ticks() -> u64 {
    // SAFETY: vahi_kernel::interrupts defines the Rust-ABI #[no_mangle]
    // `vahi_kernel_get_ticks`; the kernel binary always links it.
    unsafe { vahi_kernel_get_ticks() }
}

/// Host builds/tests have no kernel linked — return 0 as before.
#[cfg(not(all(not(test), target_os = "none")))]
pub fn get_ticks() -> u64 {
    0
}

// ─── Sub-modules ────────────────────────────────────────────────

pub mod ctlfs;
pub mod defs;
pub mod devfs;
pub mod ext2;
#[cfg(feature = "ext4")]
pub mod ext4;
pub mod fat;
pub mod fuse;
pub mod inode;
pub mod mount;
pub mod page_cache;
pub mod path;
pub mod pipe;
pub mod ramfs;
pub mod skyfs;
pub mod tarfs;

pub use defs::*;

/// TTY input stub — overridden by vahi_kernel at link time.
pub mod tty {
    use crossbeam_queue::ArrayQueue;
    use spin::Once;
    static TTY_INPUT_ONCE: Once<ArrayQueue<u8>> = Once::new();
    pub fn tty_input() -> &'static ArrayQueue<u8> {
        TTY_INPUT_ONCE.call_once(|| ArrayQueue::new(4096))
    }
}

/// Verification stubs — feature-gated, overridden by vahi_kernel.
/// ponytail: the real journal verifier lives in the not-yet-extracted
/// `vahi-verified` crate (kernel/src/verified). These stubs only satisfy
/// vahi-vfs's own `verification` cfg so the crate compiles standalone; the
/// kernel supplies the genuine implementation and does not call these.
#[cfg(feature = "verification")]
pub mod verified {
    pub mod journal {
        #[derive(Debug)]
        pub enum JournalEvent {
            BeginTxn,
            TxnPersisted,
            Crash,
            RecoveryComplete,
        }
        pub struct JournalStateMachine;
        impl JournalStateMachine {
            pub fn new() -> Self {
                Self
            }
            pub fn apply(&mut self, _event: JournalEvent) -> Result<(), JournalEvent> {
                Ok(())
            }
            pub fn record_failure(&self, _name: &str, _msg: &str) {
                // ponytail: stub — real recorder supplied by vahi_kernel.
            }
        }
    }
    pub mod runner {
        use super::journal::JournalStateMachine;
        pub struct VerificationRunner;
        struct JournalStateMachineGuard(JournalStateMachine);
        impl core::ops::Deref for JournalStateMachineGuard {
            type Target = JournalStateMachine;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }
        impl VerificationRunner {
            pub fn lock(&self) -> impl core::ops::Deref<Target = JournalStateMachine> {
                JournalStateMachineGuard(JournalStateMachine::new())
            }
        }
        pub static VERIFICATION_RUNNER: VerificationRunner = VerificationRunner;
    }
}
