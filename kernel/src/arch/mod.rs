//! Architecture abstraction layer.
//!
//! The `Arch` trait defines the interface that each target architecture must
//! implement. This allows the kernel to be ported to new architectures
//! (aarch64, RISC-V) by providing a new implementation of this trait.
//!
//! Current targets: x86_64 (mature), aarch64 (in progress)

/// Architecture-specific operations required by the kernel.
pub trait Arch: Send + Sync {
    /// Initialize boot-time architecture (GDT/IDT for x86, vector table for aarch64).
    unsafe fn init_boot();

    /// Initialize the syscall entry mechanism.
    unsafe fn init_syscalls();

    /// Initialize features for the current CPU (SSE, SMEP, etc.).
    unsafe fn init_cpu();

    /// Read the current stack pointer.
    fn read_sp() -> u64;

    /// Read the frame pointer (if available).
    fn read_fp() -> u64;

    /// Halt the CPU until the next interrupt.
    fn halt();

    /// Halt the CPU indefinitely (panic or shutdown).
    fn halt_loop() -> ! {
        loop { Self::halt(); }
    }

    /// Jump to userspace at the given entry point and stack pointer.
    unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> !;

    /// Switch threads: save old context, restore new context.
    unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_fs_base: u64);

    /// Read the current thread pointer (FS/GS base on x86, TPIDR on aarch64).
    fn read_thread_pointer() -> u64;

    /// Write the current thread pointer.
    unsafe fn write_thread_pointer(val: u64);
}

#[cfg(target_arch = "x86_64")]
pub mod arch_x86_64;

#[cfg(target_arch = "x86_64")]
pub use self::arch_x86_64::X86_64Arch;

#[cfg(target_arch = "aarch64")]
pub mod arch_aarch64;

#[cfg(target_arch = "aarch64")]
pub use self::arch_aarch64::AArch64Arch;

// Re-export the current architecture's implementation as `CurrentArch`.
#[cfg(target_arch = "x86_64")]
pub type CurrentArch = self::arch_x86_64::X86_64Arch;

#[cfg(target_arch = "aarch64")]
pub type CurrentArch = self::arch_aarch64::AArch64Arch;
