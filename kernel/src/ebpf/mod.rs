//! eBPF subsystem.
//!
//! Provides a verifier, JIT compiler, and eBPF virtual machine.

pub mod helpers;
pub mod jit;
pub mod maps;
pub mod tnum;
pub mod verifier;
pub mod vm;

use crate::sync::IrqSafeMutex as Mutex;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use lazy_static::lazy_static;

use crate::syscalls::errno::Errno;
use maps::{
    Map, BPF_MAP_TYPE_ARRAY, BPF_MAP_TYPE_HASH, BPF_MAP_TYPE_PERF_EVENT_ARRAY, BPF_MAP_TYPE_RINGBUF,
};
use verifier::verify;
use vm::EbpfInsn;

pub const BPF_MAP_CREATE: u32 = 0;
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
pub const BPF_MAP_GET_NEXT_KEY: u32 = 4;
pub const BPF_PROG_LOAD: u32 = 5;
pub const BPF_OBJ_PIN: u32 = 6;
pub const BPF_OBJ_GET: u32 = 7;
pub const BPF_PROG_ATTACH: u32 = 8;
pub const BPF_PROG_DETACH: u32 = 9;
pub const BPF_MAP_FREEZE: u32 = 10;

#[derive(Clone, Copy, Debug)]
pub struct BpfAttr {
    pub map_type: u32,
    pub key_size: u32,
    pub value_size: u32,
    pub max_entries: u32,
    pub map_flags: u32,
    pub insns: *const u8,
    pub insn_cnt: u32,
    pub license: *const u8,
    pub log_level: u32,
    pub log_size: u32,
    pub log_buf: *mut u8,
}

#[derive(Clone, Debug)]
pub struct EbpfProg {
    pub insns: Vec<EbpfInsn>,
    pub log: String,
}

#[derive(Clone, Debug)]
pub struct AttachedProg {
    pub attach_type: u32,
    pub target: alloc::string::String,
}

lazy_static! {
    static ref PROGRAMS: Mutex<Vec<(u64, EbpfProg)>> = Mutex::new(Vec::new());
    static ref ATTACHMENTS: Mutex<Vec<AttachedProg>> = Mutex::new(Vec::new());
}

pub fn sys_bpf(cmd: u32, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    if crate::syscalls::get_current_euid() != 0
        && !crate::syscalls::has_capability(crate::syscalls::CAP_SYS_ADMIN)
    {
        return Errno::EPERM as u64;
    }
    match cmd {
        BPF_MAP_CREATE => bpf_map_create(arg1 as *const u8),
        BPF_PROG_LOAD => bpf_prog_load(arg1 as *const BpfAttr, arg2 as *mut u8, arg3 as u32),
        _ => Errno::ENOSYS as u64,
    }
}

fn bpf_map_create(attr_ptr: *const u8) -> u64 {
    let attr = unsafe { &*(attr_ptr as *const BpfAttr) };
    let map: Arc<dyn Map> = match attr.map_type {
        BPF_MAP_TYPE_HASH => Arc::new(maps::HashTable::new(
            attr.key_size,
            attr.value_size,
            attr.max_entries,
        )),
        BPF_MAP_TYPE_ARRAY => Arc::new(maps::ArrayMap::new(attr.value_size, attr.max_entries)),
        BPF_MAP_TYPE_PERF_EVENT_ARRAY => Arc::new(maps::PerfEventArray::new(attr.max_entries)),
        BPF_MAP_TYPE_RINGBUF => Arc::new(maps::RingBuf::new(attr.max_entries as usize)),
        _ => return Errno::EINVAL as u64,
    };
    let id = maps::register_map(map);
    id as u64
}

fn bpf_prog_load(attr_ptr: *const BpfAttr, _log_buf: *mut u8, _log_size: u32) -> u64 {
    let attr = unsafe { &*attr_ptr };
    if attr.insn_cnt == 0 || attr.insns.is_null() {
        return Errno::EINVAL as u64;
    }

    let insn_slice = unsafe {
        core::slice::from_raw_parts(attr.insns as *const EbpfInsn, attr.insn_cnt as usize)
    };
    let insns = insn_slice.to_vec();

    let verify_result = verify(&insns);
    if !verify_result {
        return Errno::EINVAL as u64;
    }

    let prog = EbpfProg {
        insns,
        log: alloc::string::String::new(),
    };

    let mut progs = PROGRAMS.lock();
    let id = progs.len() as u64;
    progs.push((id, prog));
    id
}
