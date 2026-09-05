#![allow(unused_imports)]
//! process syscalls — re-exports from submodules for backward compatibility.
//!
//! Original 1637-line monolith split into:
//! - process_lifecycle.rs: fork, clone, execve, exit, wait, sched, time
//! - process_signal.rs: rt_sigaction, rt_sigreturn, kill, sigprocmask, sigaltstack, signalfd4
//! - process_creds.rs: uid, gid, capabilities, process groups, resource limits

pub use super::process_creds::*;
pub use super::process_lifecycle::*;
pub use super::process_signal::*;
