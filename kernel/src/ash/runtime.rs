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

fn map_return(val: u64) -> AshResult {
    match val {
        0 => AshResult::Continue,
        1 => AshResult::Handled,
        2 => AshResult::Drop,
        3 => AshResult::Modified,
        4..=u64::MAX => AshResult::Error(AshError::Unknown),
    }
}
