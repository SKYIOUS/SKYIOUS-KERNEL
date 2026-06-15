pub mod vm;
pub mod verifier;
pub mod maps;
pub mod helpers;

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::slice;
use spin::Mutex;
use lazy_static::lazy_static;
use vm::{EbpfVm, EbpfInsn, EbpfRegs, STACK_SIZE};
use verifier::verify;
use maps::{Map, BPF_MAP_TYPE_HASH, BPF_MAP_TYPE_ARRAY, BPF_MAP_TYPE_PERF_EVENT_ARRAY, BPF_MAP_TYPE_RINGBUF};
use crate::syscalls::errno::Errno;

pub const BPF_MAP_CREATE: u32 = 0;
pub const BPF_MAP_LOOKUP_ELEM: u32 = 1;
pub const BPF_MAP_UPDATE_ELEM: u32 = 2;
pub const BPF_MAP_DELETE_ELEM: u32 = 3;
pub const BPF_PROG_LOAD: u32 = 5;
pub const BPF_PROG_ATTACH: u32 = 6;
pub const BPF_PROG_DETACH: u32 = 7;

pub const BPF_ATTACH_KPROBE: u32 = 0;
pub const BPF_ATTACH_TRACEPOINT: u32 = 1;
pub const BPF_ATTACH_XDP: u32 = 2;
pub const BPF_ATTACH_SOCK_FILTER: u32 = 3;

pub struct EbpfProg {
    pub insns: Vec<EbpfInsn>,
    pub licensed: bool,
}

struct AttachedProg {
    prog_id: u64,
    attach_type: u32,
    target: alloc::string::String,
}

lazy_static! {
    static ref PROGRAMS: Mutex<Vec<(u64, EbpfProg)>> = Mutex::new(Vec::new());
    static ref ATTACHMENTS: Mutex<Vec<AttachedProg>> = Mutex::new(Vec::new());
}

// ── Syscall handler ───────────────────────────────────────────────
pub fn sys_bpf(cmd: u32, arg1: u64, arg2: u64, arg3: u64) -> u64 {
    match cmd {
        BPF_MAP_CREATE => bpf_map_create(arg1 as *const u8),
        BPF_MAP_LOOKUP_ELEM => bpf_map_lookup_elem(arg1, arg2 as *const u8, arg3 as *mut u8),
        BPF_MAP_UPDATE_ELEM => bpf_map_update_elem(arg1, arg2 as *const u8, arg3 as *const u8),
        BPF_MAP_DELETE_ELEM => bpf_map_delete_elem(arg1, arg2 as *const u8),
        BPF_PROG_LOAD => bpf_prog_load(arg1 as *const u8, arg2),
        BPF_PROG_ATTACH => bpf_prog_attach(arg1, arg2 as u32, arg3),
        BPF_PROG_DETACH => bpf_prog_detach(arg1, arg2 as u32),
        _ => Errno::EINVAL as u64,
    }
}

// ── Map operations ────────────────────────────────────────────────
fn bpf_map_create(attr_ptr: *const u8) -> u64 {
    if attr_ptr.is_null() { return Errno::EINVAL as u64; }
    let mut attr_buf = [0u8; core::mem::size_of::<BpfMapCreateAttr>()];
    if unsafe { crate::syscalls::user_access::copy_from_user(&mut attr_buf, attr_ptr).is_err() } {
        return Errno::EFAULT as u64;
    }
    let attr: &BpfMapCreateAttr = unsafe { &*(attr_buf.as_ptr() as *const BpfMapCreateAttr) };
    if attr.map_type >= maps::MAX_MAP_TYPE_COUNT || attr.key_size == 0 || attr.value_size == 0 || attr.max_entries == 0 {
        return Errno::EINVAL as u64;
    }
    let map: Arc<dyn Map> = match attr.map_type {
        t if t == BPF_MAP_TYPE_HASH => Arc::new(maps::HashTable::new(attr.key_size, attr.value_size, attr.max_entries)),
        t if t == BPF_MAP_TYPE_ARRAY => Arc::new(maps::ArrayMap::new(attr.value_size, attr.max_entries)),
        t if t == BPF_MAP_TYPE_PERF_EVENT_ARRAY => Arc::new(maps::PerfEventArray::new(attr.max_entries)),
        t if t == BPF_MAP_TYPE_RINGBUF => Arc::new(maps::RingBuf::new((attr.max_entries * attr.value_size) as usize)),
        _ => return Errno::EINVAL as u64,
    };
    maps::register_map(map) as u64
}

fn bpf_map_lookup_elem(map_id: u64, key_ptr: *const u8, value_ptr: *mut u8) -> u64 {
    if key_ptr.is_null() || value_ptr.is_null() { return Errno::EINVAL as u64; }
    let map = maps::get_map(map_id as usize);
    match map {
        Some(m) => {
            let key = unsafe { slice::from_raw_parts(key_ptr, m.key_size()) };
            match m.lookup(key) {
                Some(val) => {
                    let copy_len = val.len().min(m.value_size());
                    unsafe { core::ptr::copy_nonoverlapping(val.as_ptr(), value_ptr, copy_len); }
                    0
                }
                None => Errno::ENOENT as u64,
            }
        }
        None => Errno::ENOENT as u64,
    }
}

fn bpf_map_update_elem(map_id: u64, key_ptr: *const u8, value_ptr: *const u8) -> u64 {
    if key_ptr.is_null() || value_ptr.is_null() { return Errno::EINVAL as u64; }
    let map = maps::get_map(map_id as usize);
    match map {
        Some(m) => {
            let key = unsafe { slice::from_raw_parts(key_ptr, m.key_size()) };
            let value = unsafe { slice::from_raw_parts(value_ptr, m.value_size()) };
            if m.update(key, value) { 0 } else { Errno::ENOMEM as u64 }
        }
        None => Errno::ENOENT as u64,
    }
}

fn bpf_map_delete_elem(map_id: u64, key_ptr: *const u8) -> u64 {
    if key_ptr.is_null() { return Errno::EINVAL as u64; }
    let map = maps::get_map(map_id as usize);
    match map {
        Some(m) => {
            let key = unsafe { slice::from_raw_parts(key_ptr, m.key_size()) };
            if m.delete(key) { 0 } else { Errno::ENOENT as u64 }
        }
        None => Errno::ENOENT as u64,
    }
}

// ── Program operations ────────────────────────────────────────────
fn bpf_prog_load(attr_ptr: *const u8, _prog_type: u64) -> u64 {
    if attr_ptr.is_null() { return Errno::EINVAL as u64; }

    let mut attr_buf = [0u8; core::mem::size_of::<BpfProgLoadAttr>()];
    if unsafe { crate::syscalls::user_access::copy_from_user(&mut attr_buf, attr_ptr).is_err() } {
        return Errno::EFAULT as u64;
    }
    let attr: &BpfProgLoadAttr = unsafe { &*(attr_buf.as_ptr() as *const BpfProgLoadAttr) };

    if attr.insn_cnt == 0 || attr.insns.is_null() || attr.license.is_null() {
        return Errno::EINVAL as u64;
    }

    let insns_size = attr.insn_cnt as usize * core::mem::size_of::<vm::EbpfInsn>();
    let mut insns_buf = alloc::vec![0u8; insns_size];
    if unsafe { crate::syscalls::user_access::copy_from_user(&mut insns_buf, attr.insns as *const u8).is_err() } {
        return Errno::EFAULT as u64;
    }
    let insns: &[vm::EbpfInsn] = unsafe {
        slice::from_raw_parts(insns_buf.as_ptr() as *const vm::EbpfInsn, attr.insn_cnt as usize)
    };

    let license = unsafe { crate::syscalls::user_access::read_user_string(attr.license, 128).unwrap_or_default() };

    if !verify(insns) {
        return Errno::EINVAL as u64;
    }

    let licensed = license.contains("GPL");
    let prog = EbpfProg {
        insns: insns.to_vec(),
        licensed,
    };

    let mut progs = PROGRAMS.lock();
    let prog_id = progs.len() as u64 + 1;
    progs.push((prog_id, prog));
    prog_id
}

fn bpf_prog_attach(prog_id: u64, attach_type: u32, target: u64) -> u64 {
    let progs = PROGRAMS.lock();
    if !progs.iter().any(|(id, _)| *id == prog_id) {
        return Errno::ENOENT as u64;
    }
    let target_name = if target != 0 {
        unsafe { crate::syscalls::user_access::read_user_string(target as *const u8, 256).unwrap_or_default() }
    } else {
        alloc::string::String::new()
    };
    drop(progs);
    let mut att = ATTACHMENTS.lock();
    att.push(AttachedProg { prog_id, attach_type, target: target_name });
    0
}

fn bpf_prog_detach(prog_id: u64, attach_type: u32) -> u64 {
    let mut att = ATTACHMENTS.lock();
    let before = att.len();
    att.retain(|a| !(a.prog_id == prog_id && a.attach_type == attach_type));
    if att.len() < before { 0 } else { Errno::ENOENT as u64 }
}

// ── Execution ─────────────────────────────────────────────────────
pub fn execute_prog(prog_id: u64, regs: &mut EbpfRegs, stack: &mut [u8; STACK_SIZE]) -> u64 {
    let progs = PROGRAMS.lock();
    for (id, prog) in progs.iter() {
        if *id == prog_id {
            let mut vm = EbpfVm::new(&prog.insns, prog.licensed);
            return vm.exec_raw(regs, stack);
        }
    }
    0
}

pub fn execute_all_kprobe(name: &str, regs: &mut EbpfRegs, stack: &mut [u8; STACK_SIZE]) {
    let att = ATTACHMENTS.lock();
    for a in att.iter() {
        if a.attach_type == BPF_ATTACH_KPROBE && (a.target.is_empty() || a.target == name) {
            execute_prog(a.prog_id, regs, stack);
        }
    }
}

// ── BPF attributes ────────────────────────────────────────────────
#[repr(C)]
pub struct BpfMapCreateAttr {
    pub map_type: u32,
    pub key_size: u32,
    pub value_size: u32,
    pub max_entries: u32,
}

#[repr(C)]
pub struct BpfProgLoadAttr {
    pub prog_type: u32,
    pub insn_cnt: u32,
    pub insns: *const u8,
    pub license: *const u8,
    pub log_level: u32,
    pub log_size: u32,
    pub log_buf: *mut u8,
    pub kern_version: u32,
}
