#![allow(unused_imports)]
//! net syscalls — re-exports from submodules for backward compatibility.
//!
//! Original 1328-line monolith split into:
//! - net_helpers.rs: constants, types, statics, helper functions
//! - net_socket.rs: socket, bind, connect, listen, accept, sendto, recvfrom, socketpair
//! - net_options.rs: setsockopt, getsockopt, sendmsg, recvmsg, getsockname

pub use super::net_helpers::*;
pub use super::net_options::*;
pub use super::net_socket::*;
