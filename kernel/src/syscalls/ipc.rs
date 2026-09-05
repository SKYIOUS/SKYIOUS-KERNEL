#![allow(unused_imports)]
//! ipc syscalls — split from mod.rs (7246 lines).
use super::errno;
use super::numbers;
use super::*;
use crate::objects::KernelObject;
use crate::sync::IrqSafeMutex as Mutex;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::vfs::{Stat, VfsNode, VFS};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
