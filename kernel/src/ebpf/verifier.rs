use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::cmp;
use super::tnum::Tnum;
use super::vm::*;

pub fn verify(insns: &[EbpfInsn]) -> bool {
    if insns.is_empty() || insns.len() > 4096 { return false; }
    for (i, insn) in insns.iter().enumerate() {
        let cls = insn.code & 0x07; let dst = insn.dst_reg; let src = insn.src_reg;
        if dst > 10 || src > 10 { return false; }
        if cls == BPF_ALU || cls == BPF_ALU64 || cls == BPF_JMP || cls == BPF_JMP32 {
            if dst == 10 { return false; }
        }
        match cls {
            BPF_LD => {
                if insn.code & 0xe0 != 0x00 { return false; }
                if insn.code & 0x18 == 0x18 {
                    if i + 1 >= insns.len() { return false; }
                    if insns[i + 1].code != 0 { return false; }
                }
            }
            BPF_LDX => {
                if (insn.off as i64) < -512 || (insn.off as i64) > 512 { return false; }
                if dst == 10 { return false; }
            }
            BPF_ST | BPF_STX => {
                if (insn.off as i64) < -512 || (insn.off as i64) > 512 { return false; }
            }
            BPF_ALU | BPF_ALU64 | BPF_JMP | BPF_JMP32 => {
                if insn.code & 0xf0 == BPF_CALL && (insn.imm < 1 || insn.imm > 4) {
                    return false;
                }
            }
            _ => return false,
        }
    }
    if insns.last().map(|i| i.code & 0xf0) != Some(BPF_EXIT) { return false; }
    let mut targets: BTreeSet<usize> = BTreeSet::new();
    targets.insert(0); targets.insert(insns.len() - 1);
    for (i, insn) in insns.iter().enumerate() {
        let op = insn.code & 0xf0; let cls = insn.code & 0x07;
        let is_jmp = cls == BPF_JMP || cls == BPF_JMP32;
        if is_jmp {
            if op == BPF_JA || op == BPF_CALL {
                let target = ((i as i64) + 1 + (insn.off as i64)) as usize;
                if target >= insns.len() { return false; }
                targets.insert(target);
                if op != BPF_JA { targets.insert(i + 1); }
            } else {
                let target = ((i as i64) + 1 + (insn.off as i64)) as usize;
                if target >= insns.len() { return false; }
                targets.insert(target); targets.insert(i + 1);
            }
        } else if i + 1 < insns.len() { targets.insert(i + 1); }
    }
    for &t in &targets {
        if t >= insns.len() { return false; }
        if t > 0 && insns[t - 1].code & 0x1f == 0x18 { return false; }
    }
    let mut visited: BTreeSet<usize> = BTreeSet::new();
    let mut worklist: Vec<usize> = Vec::new();
    worklist.push(0);
    while let Some(cur) = worklist.pop() {
        if !visited.insert(cur) { continue; }
        let insn = &insns[cur]; let op = insn.code & 0xf0; let cls = insn.code & 0x07;
        let is_jmp = cls == BPF_JMP || cls == BPF_JMP32;
        if is_jmp {
            let target = ((cur as i64) + 1 + (insn.off as i64)) as usize;
            if target < insns.len() { worklist.push(target); }
            if op == BPF_JA || op == BPF_EXIT { continue; }
        }
        let next = cur + if insn.code & 0x1f == 0x18 { 2 } else { 1 };
        if next < insns.len() { worklist.push(next); }
    }
    for i in 0..insns.len() { if !visited.contains(&i) { return false; } }
    true
}

#[derive(Debug, Clone, Copy)]
struct RegState { tnum: Tnum }
impl RegState {
    fn exact(v: u64) -> Self { RegState { tnum: Tnum::exact(v) } }
}

#[derive(Debug, Clone)]
struct VState { regs: [RegState; 11], stack: [Tnum; 64] }
impl VState {
    fn new() -> Self {
        VState { regs: [RegState { tnum: Tnum::any() }; 11], stack: [Tnum::any(); 64] }
    }
    fn init() -> Self {
        let mut s = Self::new(); s.regs[10] = RegState::exact(STACK_SIZE as u64); s
    }
    fn write_stack(&mut self, off: u64, size: u64, t: Tnum) {
        if off % 8 != 0 || size % 8 != 0 { return; }
        let slot = (off / 8) as usize;
        let count = cmp::max(1, (size / 8) as usize);
        for i in 0..count { if slot + i < 64 { self.stack[slot + i] = t; } }
    }
    fn merge(&self, o: &Self) -> Self {
        let mut m = self.clone();
        for i in 0..11 { m.regs[i].tnum = self.regs[i].tnum.merge(o.regs[i].tnum); }
        for i in 0..64 { m.stack[i] = self.stack[i].merge(o.stack[i]); }
        m
    }
    fn eq(&self, o: &Self) -> bool {
        for i in 0..11 { if self.regs[i].tnum != o.regs[i].tnum { return false; } }
        for i in 0..64 { if self.stack[i] != o.stack[i] { return false; } }
        true
    }
}

fn sz_bytes(code: u8) -> usize {
    match code & BPF_SIZE_MASK { 0x00 => 8, 0x08 => 4, 0x10 => 2, 0x18 => 1, _ => 0 }
}

fn check_stack_acc(base: &Tnum, off: i16, size: usize) -> bool {
    let bm = base.max();
    if bm > STACK_SIZE as u64 * 8 { return false; }
    let addr = (bm as i64).wrapping_add(off as i64);
    let end = addr.wrapping_add(size as i64 - 1);
    addr >= 0 && end < STACK_SIZE as i64
}

fn alu_op_tnum(op: u8, dst: Tnum, src: Tnum, is_alu32: bool) -> Tnum {
    let r = match op {
        BPF_ADD => dst.add(src),   BPF_SUB => dst.sub(src),
        BPF_MUL => dst.mul(src),   BPF_DIV => Tnum::any(),
        BPF_OR => dst.or(src),     BPF_AND => dst.and(src),
        BPF_LSH => dst.shl(src),   BPF_RSH => dst.lshr(src),
        BPF_NEG => dst.neg(),      BPF_MOD => Tnum::any(),
        BPF_XOR => dst.xor(src),   BPF_MOV => src,
        BPF_ARSH => dst.ashr(src), _ => Tnum::any(),
    };
    if is_alu32 {
        let lo = 0xFFFF_FFFFu64;
        Tnum { value: r.value & lo & !r.mask, mask: r.mask & lo | !lo }
    } else { r }
}

fn mask32(t: Tnum) -> Tnum {
    let lo = 0xFFFF_FFFFu64;
    Tnum { value: t.value & lo, mask: t.mask & lo | !lo }
}

pub fn tnum_verify(insns: &[EbpfInsn]) -> bool {
    if insns.is_empty() || insns.len() > 4096 { return false; }
    let mut state_map: BTreeMap<usize, VState> = BTreeMap::new();
    state_map.insert(0, VState::init());
    let mut worklist: Vec<usize> = Vec::new();
    worklist.push(0);
    while let Some(cur) = worklist.pop() {
        let Some(pre) = state_map.get(&cur).cloned() else { continue; };
        let insn = &insns[cur]; let cls = insn.code & 0x07; let op = insn.code & 0xf0;
        match cls {
            BPF_ALU | BPF_ALU64 => {
                let mut post = pre.clone();
                let is64 = cls == BPF_ALU64; let dst = insn.dst_reg as usize;
                let use_imm = insn.code & 0x08 != 0;
                let src_tnum = if use_imm {
                    if is64 { Tnum::exact(insn.imm as i64 as u64) }
                    else { Tnum::exact(insn.imm as u32 as u64) }
                } else {
                    if is64 { pre.regs[insn.src_reg as usize].tnum }
                    else { mask32(pre.regs[insn.src_reg as usize].tnum) }
                };
                let dst_tnum = if is64 { pre.regs[dst].tnum }
                    else { mask32(pre.regs[dst].tnum) };
                if (op == BPF_DIV || op == BPF_MOD) && src_tnum.could_be_zero() { return false; }
                post.regs[dst].tnum = alu_op_tnum(op, dst_tnum, src_tnum, !is64);
                let next = cur + 1;
                if next < insns.len() { push_merged(&mut state_map, &mut worklist, next, &post); }
            }
            BPF_LDX => {
                let base = &pre.regs[insn.src_reg as usize].tnum;
                let size = sz_bytes(insn.code);
                if !check_stack_acc(base, insn.off, size) { return false; }
                let mut post = pre.clone();
                post.regs[insn.dst_reg as usize].tnum = Tnum::any();
                let next = cur + 1;
                if next < insns.len() { push_merged(&mut state_map, &mut worklist, next, &post); }
            }
            BPF_ST => {
                let base = &pre.regs[insn.dst_reg as usize].tnum;
                let size = if insn.code & BPF_SIZE_MASK == 0x00 { 8 } else { 4 };
                if !check_stack_acc(base, insn.off, size) { return false; }
                let mut post = pre.clone();
                let addr = (base.min() as i64).wrapping_add(insn.off as i64) as u64;
                post.write_stack(addr, size as u64, Tnum::exact(insn.imm as u64));
                let next = cur + 1;
                if next < insns.len() { push_merged(&mut state_map, &mut worklist, next, &post); }
            }
            BPF_STX => {
                let base = &pre.regs[insn.dst_reg as usize].tnum;
                let size = sz_bytes(insn.code);
                if !check_stack_acc(base, insn.off, size) { return false; }
                let mut post = pre.clone();
                let src_tnum = post.regs[insn.src_reg as usize].tnum;
                let addr = (base.min() as i64).wrapping_add(insn.off as i64) as u64;
                post.write_stack(addr, size as u64, src_tnum);
                let next = cur + 1;
                if next < insns.len() { push_merged(&mut state_map, &mut worklist, next, &post); }
            }
            BPF_JMP | BPF_JMP32 => {
                match op {
                    BPF_EXIT => { if pre.regs[0].tnum == Tnum::any() { return false; } }
                    BPF_CALL => {
                        let mut post = pre.clone();
                        match insn.imm {
                            1..=3 => post.regs[0].tnum = Tnum::any(),
                            4 => post.regs[0].tnum = Tnum::exact(0),
                            _ => return false,
                        }
                        for r in 1..=5 { post.regs[r].tnum = Tnum::any(); }
                        let next = cur + 1;
                        if next < insns.len() { push_merged(&mut state_map, &mut worklist, next, &post); }
                    }
                    BPF_JA => {
                        let target = ((cur as i64) + 1 + (insn.off as i64)) as usize;
                        if target >= insns.len() { return false; }
                        push_merged(&mut state_map, &mut worklist, target, &pre);
                    }
                    _ => {
                        let target = ((cur as i64) + 1 + (insn.off as i64)) as usize;
                        if target >= insns.len() { return false; }
                        let is32 = cls == BPF_JMP32;
                        let taken = refine_cond(&pre, insn, true, is32);
                        let fallthru = refine_cond(&pre, insn, false, is32);
                        push_merged(&mut state_map, &mut worklist, target, &taken);
                        push_merged(&mut state_map, &mut worklist, cur + 1, &fallthru);
                    }
                }
            }
            BPF_LD => {
                if insn.code & BPF_SIZE_MASK == BPF_DW && insn.code & 0xe0 == 0x00 {
                    if cur + 1 >= insns.len() { return false; }
                    let n = &insns[cur + 1];
                    let imm64 = (insn.imm as u64) | ((n.imm as u64) << 32);
                    let mut post = pre.clone();
                    post.regs[insn.dst_reg as usize] = RegState::exact(imm64);
                    let next = cur + 2;
                    if next < insns.len() { push_merged(&mut state_map, &mut worklist, next, &post); }
                } else { return false; }
            }
            _ => return false,
        }
    }
    let last = insns.len() - 1;
    if let Some(state) = state_map.get(&last) {
        if state.regs[0].tnum == Tnum::any() { return false; }
    }
    true
}

fn refine_cond(pre: &VState, insn: &EbpfInsn, taken: bool, is32: bool) -> VState {
    let op = insn.code & 0xf0; let mut state = pre.clone();
    match op {
        BPF_JEQ | BPF_JNE => {
            let use_imm = insn.code & 0x08 != 0; let dst = insn.dst_reg as usize;
            let eq = (op == BPF_JEQ && taken) || (op == BPF_JNE && !taken);
            if use_imm && eq {
                state.regs[dst].tnum = if is32 { Tnum::exact(insn.imm as u32 as u64) }
                    else { Tnum::exact(insn.imm as i64 as u64) };
            } else if !use_imm && eq && insn.src_reg != insn.dst_reg {
                let s = state.regs[insn.src_reg as usize].tnum;
                let d = state.regs[dst].tnum;
                let r = d.intersect(s);
                state.regs[dst].tnum = r; state.regs[insn.src_reg as usize].tnum = r;
            }
        }
        _ => {}
    }
    state
}

fn push_merged(state_map: &mut BTreeMap<usize, VState>, worklist: &mut Vec<usize>,
               target: usize, new_state: &VState) {
    match state_map.get(&target).cloned() {
        Some(existing) => {
            let merged = existing.merge(new_state);
            if !existing.eq(&merged) {
                state_map.insert(target, merged); worklist.push(target);
            }
        }
        None => { state_map.insert(target, new_state.clone()); worklist.push(target); }
    }
}

