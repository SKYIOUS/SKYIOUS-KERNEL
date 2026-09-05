//! Signal types for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/signal.rs`.

extern crate alloc;

/// Signal enumeration (subset used by crate interfaces).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGTRAP = 5,
    SIGABRT = 6,
    SIGBUS = 7,
    SIGFPE = 8,
    SIGKILL = 9,
    SIGUSR1 = 10,
    SIGSEGV = 11,
    SIGUSR2 = 12,
    SIGPIPE = 13,
    SIGALRM = 14,
    SIGTERM = 15,
    SIGCHLD = 17,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTSTP = 20,
    SIGTTIN = 21,
    SIGTTOU = 22,
    SIGURG = 23,
    SIGXCPU = 24,
    SIGXFSZ = 25,
    SIGVTALRM = 26,
    SIGPROF = 27,
    SIGWINCH = 28,
    SIGIO = 29,
    SIGPWR = 30,
    SIGSYS = 31,
}

/// Saved CPU context for signal handler return.
#[derive(Debug, Clone, Default)]
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

/// Per-process signal state.
#[derive(Debug, Clone, Default)]
pub struct SignalState {
    pub pending: u64,
    pub blocked: u64,
    /// Saved context for signal handler trampoline.
    pub saved_context: Option<SignalContext>,
}

impl SignalState {
    pub fn new() -> Self {
        SignalState {
            pending: 0,
            blocked: 0,
            saved_context: None,
        }
    }

    pub fn raise(&mut self, sig: Signal) {
        self.pending |= 1u64 << (sig as u32 - 1);
    }

    pub fn has_pending(&self) -> bool {
        self.pending != 0
    }

    pub fn has_unmasked_pending(&self, mask: u64) -> bool {
        (self.pending & !mask) != 0
    }

    pub fn pop_unmasked(&mut self, mask: u64) -> Option<u32> {
        let available = self.pending & !mask;
        if available == 0 {
            return None;
        }
        let bit = available.trailing_zeros();
        self.pending &= !(1 << bit);
        Some(bit + 1)
    }

    #[allow(dead_code)]
    pub fn pop_any(&mut self) -> Option<u32> {
        if self.pending == 0 {
            return None;
        }
        let bit = self.pending.trailing_zeros();
        self.pending &= !(1 << bit);
        Some(bit + 1)
    }

    pub fn restore_context(&mut self) -> Option<SignalContext> {
        self.saved_context.take()
    }
}

/// Intentionally always `false` for now. The kernel's real check takes a
/// blocking `CURRENT_PROCESS` lock, which syscall entry already holds, so
/// wiring it here would self-deadlock the polling pipe-read loop that calls
/// it. ponytail: replace with a try_lock-based check once the syscall-entry
/// lock is removed (see the SMP freeze campaign notes).
pub fn has_pending_signal() -> bool {
    false
}
