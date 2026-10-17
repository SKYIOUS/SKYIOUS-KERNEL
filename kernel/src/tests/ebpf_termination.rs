//! Termination bound for the eBPF tnum verifier's worklist.
//!
//! `tnum_verify` drives a worklist of instruction indices. A conditional
//! jump pushes both a taken target and a fall-through successor, and
//! `push_merged` re-pushes whenever the merged state differs. Nothing in the
//! loop bounds the number of iterations or rejects a back edge, so any program
//! whose jump target points at or before the current instruction (a loop) is
//! re-queued forever and the kernel stops making progress inside the self-test.
//!
//! Linux's verifier has the same exposure and answers it structurally: the
//! instruction graph is walked with a processed-instruction set, so a back edge
//! terminates because the target is already marked.

use crate::ebpf::verifier::tnum_verify;
use crate::ebpf::vm::EbpfInsn;
use crate::selftest;
use alloc::vec::Vec;

/// An unconditional backward jump forms a cycle the worklist must not follow.
fn test_backward_jump_terminates() -> Result<(), &'static str> {
    // insn 0: r0 = 42; insn 1: ja -2 (back to insn 0); insn 2: exit
    let insns = [
        EbpfInsn {
            code: 0xb7,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 42,
        },
        EbpfInsn {
            code: 0x05,
            dst_reg: 0,
            src_reg: 0,
            off: -2,
            imm: 0,
        },
        EbpfInsn {
            code: 0x95,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 0,
        },
    ];
    // Must return, whatever the verdict. A hang here is the defect.
    let _ = tnum_verify(&insns);
    Ok(())
}

/// A conditional backward jump with a state change on every pass must also
/// terminate; merging the same state repeatedly is what makes this loop.
fn test_conditional_backward_jump_terminates() -> Result<(), &'static str> {
    let insns = [
        EbpfInsn {
            code: 0xb7,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 1,
        },
        EbpfInsn {
            code: 0x05,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 0,
        }, // ja -> 3
        EbpfInsn {
            code: 0x95,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 0,
        }, // exit
        EbpfInsn {
            code: 0x55,
            dst_reg: 0,
            src_reg: 0,
            off: -3,
            imm: 0,
        }, // jne -> 1
    ];
    let _ = tnum_verify(&insns);
    Ok(())
}

/// A long forward chain of conditional jumps must terminate as well; this
/// pins the iteration bound rather than only the cycle case.
fn test_long_forward_chain_terminates() -> Result<(), &'static str> {
    let mut insns = Vec::new();
    for _ in 0..64usize {
        // r0 += 1; jne +0 (never taken, falls through)
        insns.push(EbpfInsn {
            code: 0x07,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 1,
        });
        insns.push(EbpfInsn {
            code: 0x55,
            dst_reg: 0,
            src_reg: 0,
            off: 0,
            imm: 0,
        });
    }
    insns.push(EbpfInsn {
        code: 0x95,
        dst_reg: 0,
        src_reg: 0,
        off: 0,
        imm: 0,
    });
    let _ = tnum_verify(&insns);
    Ok(())
}

pub fn register() {
    selftest::register(
        "tnum:backward_jump_terminates",
        test_backward_jump_terminates,
    );
    selftest::register(
        "tnum:conditional_backward_jump_terminates",
        test_conditional_backward_jump_terminates,
    );
    selftest::register(
        "tnum:long_forward_chain_terminates",
        test_long_forward_chain_terminates,
    );
}
