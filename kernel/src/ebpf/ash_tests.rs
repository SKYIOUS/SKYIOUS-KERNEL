use crate::ebpf::ash::{self, AshAction, AshPerCpu};
use crate::ebpf::vm::{EbpfInsn, EbpfRegs, STACK_SIZE};
use crate::ebpf::verifier;

fn mk(code: u8, dst: u8, src: u8, off: i16, imm: i32) -> EbpfInsn {
    EbpfInsn::new(code, dst, src, off, imm)
}

fn exit() -> EbpfInsn { mk(0x95, 0, 0, 0, 0) }

pub fn register() {
    crate::selftest::register("ash_pass_no_handlers", ash_pass_no_handlers);
    crate::selftest::register("ash_drop_program", ash_drop_program);
    crate::selftest::register("ash_protocol_mismatch_skips", ash_protocol_mismatch_skips);
    crate::selftest::register("ash_fail_too_many_insns", ash_fail_too_many_insns);
    crate::selftest::register("ash_fail_unsafe_helper", ash_fail_unsafe_helper);
    crate::selftest::register("ash_port_filter", ash_port_filter);
    crate::selftest::register("ash_remove_handler", ash_remove_handler);
}

fn ash_pass_no_handlers() -> Result<(), &'static str> {
    let mut cpu = AshPerCpu { stack: [0u8; STACK_SIZE], regs: EbpfRegs::new() };
    let action = unsafe { ash::run_ash_handlers(&[0u8; 64], 1, 0, &mut cpu) };
    if action == AshAction::Pass { Ok(()) } else { Err("no handlers should pass") }
}

// Program: r0 = 1 (drop), exit
fn ash_drop_program() -> Result<(), &'static str> {
    let p = &[
        mk(0xb7, 0, 0, 0, 1), // r0 = 1
        exit(),
    ];
    if !ash::install_handler(100, p, 0, 0) {
        return Err("install should succeed");
    }
    let mut cpu = AshPerCpu { stack: [0u8; STACK_SIZE], regs: EbpfRegs::new() };
    let action = unsafe { ash::run_ash_handlers(&[0u8; 64], 1, 0, &mut cpu) };
    ash::remove_handler(100);
    if action == AshAction::Drop { Ok(()) } else { Err("should drop") }
}

// Install handler for TCP (6), call with UDP (17) — should skip
fn ash_protocol_mismatch_skips() -> Result<(), &'static str> {
    let p = &[
        mk(0xb7, 0, 0, 0, 1), // r0 = 1 (drop)
        exit(),
    ];
    if !ash::install_handler(101, p, 6, 0) {
        return Err("install should succeed");
    }
    let mut cpu = AshPerCpu { stack: [0u8; STACK_SIZE], regs: EbpfRegs::new() };
    // UDP (17) — should skip TCP-only handler
    let action = unsafe { ash::run_ash_handlers(&[0u8; 64], 17, 0, &mut cpu) };
    ash::remove_handler(101);
    if action == AshAction::Pass { Ok(()) } else { Err("protocol mismatch should pass") }
}

fn ash_fail_too_many_insns() -> Result<(), &'static str> {
    let p = alloc::vec![exit(); 513];
    if ash::install_handler(102, &p, 0, 0) {
        ash::remove_handler(102);
        Err(">512 insns should fail install")
    } else {
        Ok(())
    }
}

// Helper 4 (debug_print) is not IRQ-safe — should be rejected
fn ash_fail_unsafe_helper() -> Result<(), &'static str> {
    // BPF_CALL with imm=4 (debug_print)
    let p = &[
        mk(0x85, 1, 0, 0, 4), // call helper 4 (debug_print)
        mk(0xb7, 0, 0, 0, 0), // r0 = 0
        exit(),
    ];
    if ash::install_handler(103, p, 0, 0) {
        ash::remove_handler(103);
        Err("unsafe helper should fail install")
    } else {
        Ok(())
    }
}

// Install handler for port 80, call with port 8080 — should skip
fn ash_port_filter() -> Result<(), &'static str> {
    let p = &[
        mk(0xb7, 0, 0, 0, 1), // r0 = 1 (drop)
        exit(),
    ];
    if !ash::install_handler(104, p, 6, 80) {
        return Err("install should succeed");
    }
    let mut cpu = AshPerCpu { stack: [0u8; STACK_SIZE], regs: EbpfRegs::new() };
    // Port 8080 — should skip port-80-only handler
    let action = unsafe { ash::run_ash_handlers(&[0u8; 64], 6, 8080, &mut cpu) };
    ash::remove_handler(104);
    if action == AshAction::Pass { Ok(()) } else { Err("port mismatch should pass") }
}

fn ash_remove_handler() -> Result<(), &'static str> {
    let p = &[
        mk(0xb7, 0, 0, 0, 1),
        exit(),
    ];
    if !ash::install_handler(105, p, 0, 0) {
        return Err("install should succeed");
    }
    if !ash::remove_handler(105) {
        return Err("remove should succeed");
    }
    if ash::remove_handler(105) {
        return Err("double remove should fail");
    }
    let mut cpu = AshPerCpu { stack: [0u8; STACK_SIZE], regs: EbpfRegs::new() };
    let action = unsafe { ash::run_ash_handlers(&[0u8; 64], 1, 0, &mut cpu) };
    if action == AshAction::Pass { Ok(()) } else { Err("after removal should pass") }
}
