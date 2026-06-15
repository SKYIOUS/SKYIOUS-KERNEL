use crate::ebpf::verifier;
use crate::ebpf::vm::EbpfInsn;

pub fn register() {
    crate::selftest::register("ebpf_pass_empty_prog", ebpf_pass_empty_prog);
    crate::selftest::register("ebpf_pass_simple_alu", ebpf_pass_simple_alu);
    crate::selftest::register("ebpf_fail_too_many_insns", ebpf_fail_too_many_insns);
    crate::selftest::register("ebpf_fail_bad_reg", ebpf_fail_bad_reg);
    crate::selftest::register("ebpf_fail_write_r10", ebpf_fail_write_r10);
    crate::selftest::register("ebpf_fail_no_exit", ebpf_fail_no_exit);
    crate::selftest::register("ebpf_pass_lddw", ebpf_pass_lddw);
    crate::selftest::register("ebpf_fail_bad_lddw_slot", ebpf_fail_bad_lddw_slot);
    crate::selftest::register("ebpf_pass_conditional_jump", ebpf_pass_conditional_jump);
    crate::selftest::register("ebpf_pass_unconditional_jump", ebpf_pass_unconditional_jump);
    crate::selftest::register("ebpf_fail_ldx_r10", ebpf_fail_ldx_r10);
    crate::selftest::register("ebpf_pass_call_helper", ebpf_pass_call_helper);
    crate::selftest::register("ebpf_fail_call_bad_helper", ebpf_fail_call_bad_helper);
    crate::selftest::register("ebpf_fail_jump_out_of_bounds", ebpf_fail_jump_out_of_bounds);
}

fn mk(code: u8, dst: u8, src: u8, off: i16, imm: i32) -> EbpfInsn {
    EbpfInsn::new(code, dst, src, off, imm)
}

fn exit() -> EbpfInsn { mk(0x95, 0, 0, 0, 0) } // BPF_EXIT

// ── Pass tests ────────────────────────────────────────────────────
fn ebpf_pass_empty_prog() -> Result<(), &'static str> {
    let p = &[exit()];
    if verifier::verify(p) { Ok(()) } else { Err("empty prog should pass") }
}

fn ebpf_pass_simple_alu() -> Result<(), &'static str> {
    let p = &[
        mk(0x07, 0, 0, 0, 42),  // r0 += 42 (ALU64 ADD)
        exit(),
    ];
    if verifier::verify(p) { Ok(()) } else { Err("simple ALU should pass") }
}

fn ebpf_pass_lddw() -> Result<(), &'static str> {
    let p = &[
        mk(0x18, 0, 0, 0, 0x1234),  // LD_DW_IMM r0, low
        mk(0x00, 0, 0, 0, 0x5678),  // ld_dw continuation
        exit(),
    ];
    if verifier::verify(p) { Ok(()) } else { Err("LD_DW should pass") }
}

fn ebpf_pass_conditional_jump() -> Result<(), &'static str> {
    let p = &[
        mk(0x15, 0, 1, 2, 0),       // if r0 == r1, pc += 2
        mk(0x07, 0, 0, 0, 1),       // r0 += 1
        exit(),
    ];
    if verifier::verify(p) { Ok(()) } else { Err("conditional jump should pass") }
}

fn ebpf_pass_unconditional_jump() -> Result<(), &'static str> {
    let p = &[
        mk(0x05, 0, 0, 0, 0),       // ja +0 (no-op, jumps to next insn)
        mk(0x07, 0, 0, 0, 1),       // r0 += 1
        exit(),
    ];
    if verifier::verify(p) { Ok(()) } else { Err("unconditional jump should pass") }
}

// ── Fail tests ────────────────────────────────────────────────────
fn ebpf_fail_too_many_insns() -> Result<(), &'static str> {
    let p = alloc::vec![exit(); 4097];
    if !verifier::verify(&p) { Ok(()) } else { Err(">4096 insns should fail") }
}

fn ebpf_fail_bad_reg() -> Result<(), &'static str> {
    let p = &[
        mk(0x07, 11, 0, 0, 1),      // r11 += 1 (dst > 10)
        exit(),
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("dst=11 should fail") }
}

fn ebpf_fail_write_r10() -> Result<(), &'static str> {
    let p = &[
        mk(0x07, 10, 0, 0, 1),      // r10 += 1 (r10 is read-only)
        exit(),
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("write to r10 should fail") }
}

fn ebpf_fail_no_exit() -> Result<(), &'static str> {
    let p = &[
        mk(0x07, 0, 0, 0, 1),       // r0 += 1 (no exit)
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("no exit should fail") }
}

fn ebpf_fail_bad_lddw_slot() -> Result<(), &'static str> {
    let p = &[
        mk(0x18, 0, 0, 0, 1),       // LD_DW (needs continuation, but EXIT after)
        exit(),
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("LD_DW missing continuation should fail") }
}

fn ebpf_pass_call_helper() -> Result<(), &'static str> {
    let p = &[
        mk(0x85, 0, 0, 0, 1),       // CALL helper #1
        exit(),
    ];
    if verifier::verify(p) { Ok(()) } else { Err("CALL helper #1 should pass") }
}

fn ebpf_fail_call_bad_helper() -> Result<(), &'static str> {
    let p = &[
        mk(0x85, 0, 0, 0, 99),      // CALL helper #99 (invalid)
        exit(),
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("CALL helper #99 should fail") }
}

fn ebpf_fail_ldx_r10() -> Result<(), &'static str> {
    let p = &[
        mk(0x61, 10, 1, 0, 0),      // LDX r10, [r1+0] (R10 is read-only)
        exit(),
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("LDX with dst=R10 should fail") }
}

fn ebpf_fail_jump_out_of_bounds() -> Result<(), &'static str> {
    let p = &[
        mk(0x05, 0, 0, 100, 0),     // ja +100 (out of bounds)
        exit(),
    ];
    if !verifier::verify(p) { Ok(()) } else { Err("jump out of bounds should fail") }
}
