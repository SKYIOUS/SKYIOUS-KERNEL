#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! fs syscalls — re-exports from submodules for backward compatibility.
//!
//! Original 2625-line monolith split into:
//! - fs_open.rs: open, close, dup, fcntl, pipe, access, chdir, getcwd
//! - fs_stat.rs: stat, chmod, chown, link, symlink, rename, mkdir, unlink
//! - fs_mount.rs: mount, umount, mkfs, swapon, swapoff, sync
//! - fs_io.rs: read, write, lseek, brk, mmap, munmap, mprotect, ioctl, getdents, fallocate, sendfile

pub use super::fs_open::*;
pub use super::fs_stat::*;
pub use super::fs_mount::*;
pub use super::fs_io::*;
