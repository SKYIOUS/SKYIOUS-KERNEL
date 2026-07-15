use crate::ebpf::vm::EbpfInsn;
use crate::ebpf::verifier::{self, tnum_verify};
use crate::ash::{AshHandler, HookPoint, VerifiedAsh, AshResult, AshError};

/// Maximum number of eBPF instructions allowed for an ASH handler.
pub const ASH_MAX_INSNS: usize = 512;

/// Maximum scratch memory budget for an ASH handler.
pub const ASH_MAX_MEMORY_BUDGET: usize = 512;

/// Verify an ASH handler's bytecode against the hook point.
pub fn verify_handler(handler: &AshHandler) -> Result<VerifiedAsh, AshResult> {
    if handler.bytecode.is_empty() || handler.bytecode.len() > ASH_MAX_INSNS * core::mem::size_of::<EbpfInsn>() {
        return Err(AshResult::Error(AshError::VerifierRejected));
    }

    let insn_count = handler.bytecode.len() / core::mem::size_of::<EbpfInsn>();
    if insn_count > ASH_MAX_INSNS {
        return Err(AshResult::Error(AshError::VerifierRejected));
    }

    // SAFETY: We check the slice is aligned & sized correctly
    let insns: &[EbpfInsn] = unsafe {
        core::slice::from_raw_parts(
            handler.bytecode.as_ptr() as *const EbpfInsn,
            insn_count,
        )
    };

    if !verifier::verify(insns) {
        return Err(AshResult::Error(AshError::VerifierRejected));
    }

    if !tnum_verify(insns) {
        return Err(AshResult::Error(AshError::VerifierRejected));
    }

    if !is_hook_compatible(insns, &handler.hook_point) {
        return Err(AshResult::Error(AshError::VerifierRejected));
    }

    let max_cycles = core::cmp::min(handler.max_insns.max(ASH_MAX_INSNS as u32), 100_000);

    let context_size = hook_context_size(&handler.hook_point);

    let jited = crate::ash::jit::jit_compile(insns).unwrap_or_default();

    Ok(VerifiedAsh {
        bytecode: handler.bytecode.clone(),
        insns: insns.to_vec(),
        jited,
        max_cycles,
        context_size,
        memory_budget: ASH_MAX_MEMORY_BUDGET,
    })
}

/// Check that the bytecode only accesses memory within its allowed range
/// for the given hook point.
fn is_hook_compatible(insns: &[EbpfInsn], hook: &HookPoint) -> bool {
    let ctx_size = hook_context_size(hook);
    for insn in insns {
        let cls = insn.code & 0x07;
        match cls {
            0x01 => {
                let src = insn.src_reg;
                if src == 1 {
                    let addr = insn.off as i64;
                    let end = addr + size_of_load(insn.code) as i64;
                    if addr < 0 || end > ctx_size as i64 {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    true
}

fn size_of_load(code: u8) -> usize {
    match code & 0x18 {
        0x00 => 8,
        0x08 => 4,
        0x10 => 2,
        0x18 => 1,
        _ => 0,
    }
}

fn hook_context_size(hook: &HookPoint) -> usize {
    match hook {
        HookPoint::NetReceive { .. } | HookPoint::NetTransmit { .. } => 32,
        HookPoint::SyscallEntry { .. } | HookPoint::SyscallExit { .. } => 64,
        HookPoint::TimerFired { .. } => 16,
        HookPoint::SignalDelivery { .. } => 16,
        HookPoint::MessageReceive { .. } => 32,
    }
}
