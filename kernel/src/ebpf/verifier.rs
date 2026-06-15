use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use super::vm::{EbpfInsn, BPF_ALU, BPF_ALU64, BPF_JMP, BPF_JMP32, BPF_LD, BPF_LDX, BPF_ST, BPF_STX};
use super::vm::{BPF_EXIT, BPF_JA, BPF_CALL};

// ── eBPF verifier with CFG analysis ───────────────────────────────
// Phase 1: Structural verification (instruction bounds, stack offsets)
// Phase 2: CFG analysis (reachability, jump target validity)

pub fn verify(insns: &[EbpfInsn]) -> bool {
    if insns.is_empty() || insns.len() > 4096 { return false; }

    // ── Phase 1: Structural checks ─────────────────────────────────
    for (i, insn) in insns.iter().enumerate() {
        let cls = insn.code & 0x07;
        let dst = insn.dst_reg;
        let src = insn.src_reg;

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
                let size = insn.code & 0x18;
                if size > 0x18 { return false; }
                let offset = insn.off as i64;
                if offset < -512 || offset > 512 { return false; }
                if dst == 10 { return false; } // R10 is read-only frame pointer
            }
            BPF_ST | BPF_STX => {
                let offset = insn.off as i64;
                if offset < -512 || offset > 512 { return false; }
            }
            BPF_ALU | BPF_ALU64 | BPF_JMP | BPF_JMP32 => {
                // Validate CALL helper number
                if insn.code & 0xf0 == BPF_CALL && (insn.imm < 1 || insn.imm > 4) {
                    return false;
                }
            }
            _ => return false,
        }
    }

    if insns.last().map(|i| i.code & 0xf0) != Some(BPF_EXIT) {
        return false;
    }

    // ── Phase 2: CFG analysis ──────────────────────────────────────
    // Build set of valid jump targets (all instruction start positions)
    let mut targets: BTreeSet<usize> = BTreeSet::new();
    targets.insert(0); // entry point
    targets.insert(insns.len() - 1); // last instruction (must be EXIT)

    // Collect all jump offsets
    for (i, insn) in insns.iter().enumerate() {
        let op = insn.code & 0xf0;
        let cls = insn.code & 0x07;

        // Check if this is a jump instruction
        let is_jmp = cls == BPF_JMP || cls == BPF_JMP32;
        if is_jmp {
            if op == BPF_JA || op == BPF_CALL {
                // Unconditional jump/call: target = i + 1 + off
                let target = ((i as i64) + 1 + (insn.off as i64)) as usize;
                if target >= insns.len() { return false; }
                targets.insert(target);
                // Fall-through only for CALL, not JA
                if op != BPF_JA {
                    targets.insert(i + 1);
                }
            } else {
                // Conditional jump: target = i + 1 + off, and fall-through
                let target = ((i as i64) + 1 + (insn.off as i64)) as usize;
                if target >= insns.len() { return false; }
                targets.insert(target);
                targets.insert(i + 1);
            }
        } else {
            // Non-jump: fall-through to next
            if i + 1 < insns.len() {
                targets.insert(i + 1);
            }
        }
    }

    // Verify all targets point to valid instruction boundaries
    for &t in &targets {
        if t >= insns.len() { return false; }
        // Check target is not the second slot of LD_DW
        if t > 0 && insns[t - 1].code & 0x1f == 0x18 {
            return false;
        }
    }

    // ── Phase 3: Reachability ──────────────────────────────────────
    // Walk from entry (target 0), following all edges, marking visited
    let mut visited: BTreeSet<usize> = BTreeSet::new();
    let mut worklist: Vec<usize> = Vec::new();
    worklist.push(0);

    while let Some(cur) = worklist.pop() {
        if !visited.insert(cur) { continue; }

        let insn = &insns[cur];
        let op = insn.code & 0xf0;
        let cls = insn.code & 0x07;
        let is_jmp = cls == BPF_JMP || cls == BPF_JMP32;

        if is_jmp {
            let target = ((cur as i64) + 1 + (insn.off as i64)) as usize;
            if target < insns.len() { worklist.push(target); }
            if op == BPF_JA {
                // Unconditional jump: no fall-through
                continue;
            } else if op == BPF_EXIT {
                // EXIT: no fall-through
                continue;
            }
        }
        // Fall-through to next instruction
        let next = cur + if insn.code & 0x1f == 0x18 { 2 } else { 1 };
        if next < insns.len() { worklist.push(next); }
    }

    // Verify all instructions are reachable
    for i in 0..insns.len() {
        if !visited.contains(&i) { return false; }
    }

    true
}
