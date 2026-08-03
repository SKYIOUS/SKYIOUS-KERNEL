use core::sync::atomic::AtomicU64;

/// CPU identification (APIC ID on x86, MPIDR on aarch64, hart ID on RISC-V).
pub type CpuId = u64;

/// Abstract CPU context operations.
/// Implementations are registered at boot and dispatched through inline wrappers.
pub trait CpuContext: Send + Sync {
    fn halt(&self);
    fn read_sp(&self) -> u64;
    fn read_fp(&self) -> u64;
    unsafe fn jump_to_usermode(&self, entry: u64, rsp: u64) -> !;
    unsafe fn switch_thread(&self, old_sp: *mut u64, new_sp: u64, new_thread_pointer: u64);
    fn read_thread_pointer(&self) -> u64;
    unsafe fn write_thread_pointer(&self, val: u64);
    fn current_cpu_id(&self) -> CpuId;
    fn cpu_count(&self) -> usize;
}

static CPU_CONTEXT: crate::sync::IrqSafeMutex<Option<&'static dyn CpuContext>> = crate::sync::IrqSafeMutex::new(None);
pub(crate) static CPU_COUNT: AtomicU64 = AtomicU64::new(1);

pub fn register_cpu_context(ctx: &'static dyn CpuContext) {
    *CPU_CONTEXT.lock() = Some(ctx);
}

pub fn set_cpu_count(n: usize) {
    CPU_COUNT.store(n as u64, core::sync::atomic::Ordering::Relaxed);
}

// ── Inline dispatch wrappers ─────────────────────────────────────

#[inline(always)]
pub fn halt() {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        ctx.halt();
        return;
    }
    #[cfg(target_arch = "x86_64")]
    { x86_64::instructions::hlt(); }
    #[cfg(not(target_arch = "x86_64"))]
    { unsafe { core::arch::asm!("wfi"); } }
}

#[inline(always)]
pub fn read_sp() -> u64 {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        return ctx.read_sp();
    }
    let sp: u64;
    #[cfg(target_arch = "x86_64")]
    unsafe { core::arch::asm!("mov {}, rsp", out(reg) sp, options(nostack, preserves_flags)); }
    #[cfg(target_arch = "aarch64")]
    unsafe { core::arch::asm!("mov {}, sp", out(reg) sp); }
    #[cfg(target_arch = "riscv64")]
    unsafe { core::arch::asm!("mv {}, sp", out(reg) sp); }
    sp
}

#[inline(always)]
pub fn read_fp() -> u64 {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        return ctx.read_fp();
    }
    let fp: u64;
    #[cfg(target_arch = "x86_64")]
    unsafe { core::arch::asm!("mov {}, rbp", out(reg) fp, options(nostack, preserves_flags)); }
    #[cfg(target_arch = "aarch64")]
    unsafe { core::arch::asm!("mov {}, x29", out(reg) fp); }
    #[cfg(target_arch = "riscv64")]
    unsafe { core::arch::asm!("mv {}, fp", out(reg) fp); }
    fp
}

pub unsafe fn jump_to_usermode(entry: u64, rsp: u64) -> ! {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        // SAFETY: caller guarantees valid entry point and stack
        ctx.jump_to_usermode(entry, rsp)
    }
    #[cfg(target_arch = "x86_64")]
    { crate::task::thread::jump_to_usermode(entry, rsp) }
    #[cfg(not(target_arch = "x86_64"))]
    { loop {} }
}

pub unsafe fn switch_thread(old_sp: *mut u64, new_sp: u64, new_thread_pointer: u64) {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        // SAFETY: caller guarantees valid saved and new stack pointers
        ctx.switch_thread(old_sp, new_sp, new_thread_pointer);
        return;
    }
    #[cfg(target_arch = "x86_64")]
    { crate::task::thread::switch_thread(old_sp, new_sp, new_thread_pointer); }
    #[cfg(not(target_arch = "x86_64"))]
    { core::hint::spin_loop(); }
}

pub fn read_thread_pointer() -> u64 {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        return ctx.read_thread_pointer();
    }
    #[cfg(target_arch = "x86_64")]
    { crate::task::thread::read_fs_base() }
    #[cfg(target_arch = "aarch64")]
    { let tp: u64; unsafe { core::arch::asm!("mrs {}, tpidr_el0", out(reg) tp); } tp }
    #[cfg(target_arch = "riscv64")]
    { 0 }
}

pub unsafe fn write_thread_pointer(val: u64) {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        // SAFETY: caller ensures thread pointer register is writable
        ctx.write_thread_pointer(val);
        return;
    }
    #[cfg(target_arch = "x86_64")]
    { crate::task::thread::write_fs_base(val); }
    #[cfg(target_arch = "aarch64")]
    { core::arch::asm!("msr tpidr_el0, {}", in(reg) val); }
    #[cfg(target_arch = "riscv64")]
    { }
}

pub fn current_cpu_id() -> CpuId {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        return ctx.current_cpu_id();
    }
    #[cfg(target_arch = "x86_64")]
    {
        crate::apic::current_lapic_id() as u64
    }
    #[cfg(target_arch = "aarch64")]
    {
        let id: u64;
        unsafe { core::arch::asm!("mrs {}, mpidr_el1", out(reg) id); }
        id & 0xFF
    }
    #[cfg(target_arch = "riscv64")]
    { 0 }
}

pub fn cpu_count() -> usize {
    if let Some(ctx) = &*CPU_CONTEXT.lock() {
        return ctx.cpu_count();
    }
    CPU_COUNT.load(core::sync::atomic::Ordering::Relaxed) as usize
}
