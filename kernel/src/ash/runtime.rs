use crate::ebpf::vm::{EbpfVm, EbpfRegs, STACK_SIZE};
use crate::hal::exec_mem::ExecRegion;
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
    // Try JIT execution first if JIT-compiled code is available
    if !handler.jited.is_empty() {
        if let Ok(result) = execute_jit(handler, context, payload) {
            return map_return(result);
        }
        // Fall through to interpreter on JIT failure
    }

    // Interpreter fallback
    let mut regs = EbpfRegs::new();
    regs.set_r(1, context.as_ptr() as u64);
    regs.set_r(2, payload.as_mut_ptr() as u64);
    regs.set_r(3, payload.len() as u64);

    let mut stack = [0u8; STACK_SIZE];
    let mut vm = EbpfVm::new(&handler.insns, false);

    let result = vm.exec_raw(&mut regs, &mut stack);

    map_return(result)
}

/// Execute JIT-compiled ASH handler via W^X exec-memory allocator.
/// Returns the handler's return value on success, or Err if JIT execution fails.
fn execute_jit(
    handler: &VerifiedAsh,
    context: &[u8],
    payload: &mut [u8],
) -> Result<u64, &'static str> {
    // 1. Allocate executable memory region (initially RW)
    let mut region = ExecRegion::alloc()?;

    // 2. Copy JIT code to the RW region
    // Safety: we just allocated this region and have exclusive access
    let dest = region.as_mut_ptr();
    unsafe {
        core::ptr::copy_nonoverlapping(handler.jited.as_ptr(), dest, handler.jited.len());
    }

    // 3. Flip to RX (executable, no write)
    region.flip_to_rx()?;

    // 4. Get function pointer and call it
    // JIT calling convention (from jit.rs):
    // R1 (context) -> RDI, R2 (payload) -> RSI, R3 (payload_len) -> RDX
    // Returns in RAX (R0)
    type JitFn = extern "sysv64" fn(*const u8, *mut u8, u64) -> u64;
    // Use union for function pointer cast (avoids transmute, not an NX violation
    // since the page was flipped to RX via flip_to_rx() before this point).
    union FnPtrCast {
        ptr: *const u8,
        func: JitFn,
    }
    let code_ptr = region.get_fn::<*const u8>() as *const u8;
    let jit_fn = unsafe { FnPtrCast { ptr: code_ptr }.func };

    let result = jit_fn(context.as_ptr(), payload.as_mut_ptr(), payload.len() as u64);

    // 5. Region is dropped here, freeing the executable memory
    Ok(result)
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