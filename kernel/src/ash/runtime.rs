use crate::ebpf::vm::{EbpfVm, EbpfRegs, STACK_SIZE};
use crate::ash::{VerifiedAsh, AshResult, AshError};

/// Execute a verified ASH handler with the given context and payload.
/// R1 = pointer to context struct
/// R2 = pointer to payload buffer
/// R3 = payload length
/// Return value maps to AshResult.
pub fn execute_handler(
    handler: &VerifiedAsh,
    context: &[u8],
    payload: &mut [u8],
) -> AshResult {
    let mut regs = EbpfRegs::new();
    regs.set_r(1, context.as_ptr() as u64);
    regs.set_r(2, payload.as_mut_ptr() as u64);
    regs.set_r(3, payload.len() as u64);

    let mut stack = [0u8; STACK_SIZE];
    let mut vm = EbpfVm::new(&handler.insns, false);

    let result = vm.exec_raw(&mut regs, &mut stack);

    map_return(result)
}

/// Execute using JIT-compiled code if available.
#[allow(dead_code)]
pub fn execute_handler_jit(
    handler: &VerifiedAsh,
    context: &[u8],
    payload: &mut [u8],
) -> AshResult {
    if !handler.jited.is_empty() {
        // ponytail: JIT execution path — calls native code directly
        let ctx_ptr = context.as_ptr();
        let payload_ptr = payload.as_mut_ptr();
        let len = payload.len();
        let jit_code = &handler.jited;
        // SAFETY: jited code was verified by the eBPF verifier and
        // only accesses context/payload/scratch memory within bounds.
        let ret: u64 = unsafe {
            let func: extern "sysv64" fn(*const u8, *mut u8, usize) -> u64 =
                core::mem::transmute(jit_code.as_ptr());
            func(ctx_ptr, payload_ptr, len)
        };
        return map_return(ret);
    }
    execute_handler(handler, context, payload)
}

fn map_return(val: u64) -> AshResult {
    match val {
        0 => AshResult::Continue,
        1 => AshResult::Handled,
        2 => AshResult::Drop,
        3 => AshResult::Modified,
        4..=u64::MAX => AshResult::Error(AshError::Unknown),
    }
}
