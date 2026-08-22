#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
//! ipc syscalls — split from mod.rs (7246 lines).
use super::errno;
use super::numbers;
use super::*;
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;
