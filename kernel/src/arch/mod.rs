//! Architecture abstraction layer.
//!
//! The `Arch` trait defines the interface that each target architecture must
//! implement. This allows the kernel to be ported to new architectures
//! (aarch64, RISC-V) by providing a new implementation of this trait.
//!
//! Current targets: x86_64 (mature), aarch64 (in progress)

use crate::hal::platform::PlatformInfo;

/// Architecture-specific operations required by the kernel.
pub trait Arch: Send + Sync {
    /// Initialize boot-time architecture (GDT/IDT for x86, vector table for aarch64).
    ///
    /// # Safety
    /// Called once on BSP during early boot. Must only be invoked with a valid
    /// boot context and before any other threads exist.
    unsafe fn init_boot();

    /// Initialize the syscall entry mechanism.
    ///
    /// # Safety
    /// Configures CPU MSRs (x86_64) or vector table (aarch64) for syscall entry.
    /// Must be called once per CPU after GDT/IDT is initialized.
    unsafe fn init_syscalls();

    /// Initialize features for the current CPU (SSE, SMEP, etc.).
    ///
    /// # Safety
    /// Modifies CPU control registers (CR0/CR4 on x86_64). Must be called on
    /// each CPU during bring-up with interrupts disabled.
    unsafe fn init_cpu();

    /// Read the current stack pointer.
    fn read_sp() -> u64;

    /// Read the frame pointer (if available).
    fn read_fp() -> u64;

    /// Halt the CPU until the next interrupt.
    fn halt();

    /// Halt the CPU indefinitely (panic or shutdown).
    fn halt_loop() -> ! {
        loop {
            Self::halt();
        }
    }

    /// Jump to userspace at the given entry point and stack pointer.
    ///
    /// # Safety
    /// Transfers control to user mode. `entry` and `rsp` must be valid user
    /// virtual addresses. Caller must have set up user page tables and
    /// privileges (CPL=3).
    unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> !;

    /// Switch threads: save old context, restore new context.
    ///
    /// # Safety
    /// `old_sp` must be a valid pointer to a saved context. `new_sp` and
    /// `new_fs_base` must be valid for the target thread. Called from
    /// scheduler with interrupts disabled.
    unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_fs_base: u64);

    /// Read the current thread pointer (FS/GS base on x86, TPIDR on aarch64).
    fn read_thread_pointer() -> u64;

    /// Write the current thread pointer.
    ///
    /// # Safety
    /// Updates FS/GS base (x86_64) or TPIDR (aarch64). Must be called with
    /// a valid thread pointer value during context switch.
    unsafe fn write_thread_pointer(val: u64);

    /// Probe platform information (CPU count, frequency, features).
    fn probe_platform() -> PlatformInfo;

    /// Initialize and register the HAL interrupt controller for this arch.
    fn init_hal_irq();

    /// Initialize and register the HAL timer for this arch.
    fn init_hal_timer();
}

#[cfg(target_arch = "x86_64")]
pub mod arch_x86_64;

#[cfg(target_arch = "x86_64")]
pub use self::arch_x86_64::X86_64Arch;

#[cfg(target_arch = "aarch64")]
pub mod arch_aarch64;

#[cfg(target_arch = "aarch64")]
pub use self::arch_aarch64::AArch64Arch;

#[cfg(target_arch = "riscv64")]
pub mod arch_riscv64;

#[cfg(target_arch = "riscv64")]
pub use self::arch_riscv64::RiscV64Arch;

// Re-export the current architecture's implementation as `CurrentArch`.
#[cfg(target_arch = "x86_64")]
pub type CurrentArch = self::arch_x86_64::X86_64Arch;

#[cfg(target_arch = "aarch64")]
pub type CurrentArch = self::arch_aarch64::AArch64Arch;

#[cfg(target_arch = "riscv64")]
pub type CurrentArch = self::arch_riscv64::RiscV64Arch;
