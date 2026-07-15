use alloc::vec::Vec;
use crate::ebpf::vm::EbpfInsn;

/// JIT-compile eBPF bytecode to x86_64 native code.
/// Reuses the existing EbpfJit from the eBPF subsystem.
pub fn jit_compile(insns: &[EbpfInsn]) -> Result<Vec<u8>, &'static str> {
    let mut jit = crate::ebpf::jit::EbpfJit::new();
    jit.compile(insns)
}
