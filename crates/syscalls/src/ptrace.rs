//! ptrace state for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/ptrace.rs`.

/// Reason a tracee is stopped.
#[derive(Debug, Clone, Copy)]
pub enum PtraceStop {
    SignalDelivered(u8),
    SyscallEntry,
    SyscallExit,
    Exec,
    Exit,
    Clone,
    GroupStop,
    Seccomp,
}

/// Ptrace state: tracer/tracee relationship, stop reasons, flags.
#[derive(Debug, Clone, Default)]
pub struct PtraceState {
    /// PID of the tracer process (0 = not traced).
    pub tracer_pid: u64,
    /// PID of the tracee process (0 = not a tracee).
    pub tracee_pid: u64,
    /// Ptrace option flags (PT_PTRACED, etc.).
    pub flags: u64,
    /// Current stop reason, if any.
    pub stop_reason: Option<PtraceStop>,
}
