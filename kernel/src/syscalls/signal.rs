#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Signal {
    SIGHUP = 1,
    SIGINT = 2,
    _SIGQUIT = 3,
    _SIGILL = 4,
    _SIGTRAP = 5,
    _SIGABRT = 6,
    _SIGBUS = 7,
    _SIGFPE = 8,
    _SIGKILL = 9,
    _SIGUSR1 = 10,
    _SIGSEGV = 11,
    _SIGUSR2 = 12,
    _SIGPIPE = 13,
    _SIGALRM = 14,
    _SIGTERM = 15,
    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
}

pub struct SignalState {
    pub pending: u64,
    pub blocked: u64,       // bitmask of blocked signals (sigprocmask)
    pub saved_context: Option<SignalContext>,
}

pub struct SignalContext {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rflags: u64,
    /// Saved FPU/SSE state (heap-allocated, not Copy).
    pub fpu_state: Option<alloc::vec::Vec<u8>>,
}

impl SignalState {
    pub fn new() -> Self {
        SignalState { pending: 0, blocked: 0, saved_context: None }
    }

    pub fn raise(&mut self, sig: Signal) {
        self.pending |= 1 << (sig as u32 - 1);
    }

    pub fn has_pending(&self) -> bool {
        self.pending != 0
    }

    pub fn has_unmasked_pending(&self, mask: u64) -> bool {
        (self.pending & !mask) != 0
    }

    pub fn pop_unmasked(&mut self, mask: u64) -> Option<u32> {
        let available = self.pending & !mask;
        if available == 0 { return None; }
        let bit = available.trailing_zeros();
        self.pending &= !(1 << bit);
        Some(bit + 1)
    }

    #[allow(dead_code)]
    pub fn pop_any(&mut self) -> Option<u32> {
        if self.pending == 0 { return None; }
        let bit = self.pending.trailing_zeros();
        self.pending &= !(1 << bit);
        Some(bit + 1)
    }

    pub fn restore_context(&mut self) -> Option<SignalContext> {
        self.saved_context.take()
    }
}

#[repr(C)]
pub struct SignalFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub rip: u64,
    pub rflags: u64,
    pub rsp: u64,
}

/// struct sigaction { sa_handler(u64), sa_flags(u64), sa_restorer(u64), sa_mask(u64) }
#[repr(C)]
pub struct SigAction {
    pub sa_handler: u64,
    pub sa_flags: u64,
    pub sa_restorer: u64,
    pub sa_mask: u64,
}

/// Returns true if the current process has any unmasked pending signal.
pub fn has_pending_signal() -> bool {
    let lock = crate::task::process::CURRENT_PROCESS.lock();
    if let Some(ref p) = *lock {
        let sig = p.signals.lock();
        sig.has_unmasked_pending(sig.blocked)
    } else {
        false
    }
}

// ── Default signal restorer trampoline ─────────────────────────────

/// The default restorer is a small piece of code mapped into userspace
/// that issues `syscall` with __NR_rt_sigreturn (15) to restore the
/// saved register context. If sa_restorer is not set by the application,
/// the kernel provides this trampoline.
///
/// Layout (x86_64):
///   mov eax, 15   (B8 0F 00 00 00)
///   syscall        (0F 05)
///   int3           (CC)  — unreachable, debug trap if we return here
#[cfg(target_arch = "x86_64")]
pub static SIGNAL_RESTORER: [u8; 7] = [
    0xB8, 0x0F, 0x00, 0x00, 0x00, // mov eax, 15 (sys_rt_sigreturn)
    0x0F, 0x05,                     // syscall
];

/// aarch64 restorer: mov x8, #__NR_rt_sigreturn; svc #0
#[cfg(target_arch = "aarch64")]
pub static SIGNAL_RESTORER: [u8; 8] = [
    0x08, 0x00, 0x80, 0xD2, // mov x8, #15
    0x01, 0x00, 0x00, 0xD4, // svc #0
];

/// Get the restorer address. Uses sa_restorer if set, otherwise the
/// kernel-provided trampoline.
pub fn get_restorer(handler: u64, sa_restorer: u64) -> u64 {
    if sa_restorer != 0 {
        sa_restorer
    } else {
        // Map the static trampoline into userspace. For simplicity,
        // we return the address of the static — it must be in userspace
        // mapped memory. In a production kernel, we'd mmap a small
        // page with the trampoline. Here we rely on the fact that the
        // signal frame setup copies the restorer code to the user stack.
        SIGNAL_RESTORER.as_ptr() as u64
    }
}

// ── FPU state for signal delivery ──────────────────────────────────

/// Save FPU state into a heap buffer. Called before signal delivery
/// to preserve floating-point state across the handler.
#[cfg(target_arch = "x86_64")]
pub fn save_fpu_state_for_signal() -> Option<alloc::vec::Vec<u8>> {
    let mut area = crate::task::thread::FpuArea::new();
    unsafe {
        crate::task::thread::save_fpu(&mut area);
    }
    Some(alloc::vec::Vec::from(area.data.as_slice()))
}

#[cfg(target_arch = "aarch64")]
pub fn save_fpu_state_for_signal() -> Option<alloc::vec::Vec<u8>> {
    let mut state = crate::arch::arch_aarch64::AArch64FpuState::new();
    unsafe {
        crate::arch::arch_aarch64::save_fpu_aarch64(&mut state);
    }
    Some(alloc::vec::Vec::from(state.data.as_slice()))
}
