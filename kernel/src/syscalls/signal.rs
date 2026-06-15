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
}

pub struct SignalState {
    pub pending: u64,
    pub saved_context: Option<SignalContext>,
}

#[derive(Clone, Copy)]
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
}

impl SignalState {
    pub fn new() -> Self {
        SignalState { pending: 0, saved_context: None }
    }

    pub fn raise(&mut self, sig: Signal) {
        self.pending |= 1 << (sig as u32 - 1);
    }

    pub fn has_pending(&self) -> bool {
        self.pending != 0
    }

    #[allow(dead_code)]
    pub fn has_unmasked_pending(&self, mask: u64) -> bool {
        (self.pending & !mask) != 0
    }

    #[allow(dead_code)]
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
