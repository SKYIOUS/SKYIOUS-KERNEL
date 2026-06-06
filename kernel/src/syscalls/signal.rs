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
    pub masked: u64,
}

impl SignalState {
        pub fn new() -> Self {
        SignalState { pending: 0, masked: 0 }
    }
    
        pub fn raise(&mut self, sig: Signal) {
        self.pending |= 1 << (sig as u32 - 1);
    }
    
        pub fn has_pending(&self) -> bool {
        (self.pending & !self.masked) != 0
    }
}
