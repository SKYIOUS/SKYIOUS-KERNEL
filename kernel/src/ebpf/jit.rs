use super::vm::*;
use alloc::vec::Vec;

/// A very simple eBPF-to-x86_64 JIT compiler for the Vahi kernel.
/// It translates eBPF instructions into native x86_64 machine code.
pub struct EbpfJit {
    code: Vec<u8>,
}

impl EbpfJit {
    pub fn new() -> Self {
        EbpfJit { code: Vec::new() }
    }

    /// Compiles eBPF instructions into x86_64 machine code.
    /// Maps eBPF R0-R10 to x86_64 registers:
    /// R0 -> RAX
    /// R1 -> RDI
    /// R2 -> RSI
    /// R3 -> RDX
    /// R4 -> RCX
    /// R5 -> R8
    /// R6 -> R9
    /// R7 -> R10
    /// R8 -> R11
    /// R9 -> R12
    /// R10 -> R13 (Stack pointer / Frame pointer base)
    pub fn compile(&mut self, insns: &[EbpfInsn]) -> Result<Vec<u8>, &'static str> {
        // Prologue: save callee-saved registers we use
        self.emit_prologue();

        for insn in insns {
            let cls = insn.code & 0x07;
            match cls {
                BPF_ALU64 => self.compile_alu64(insn)?,
                BPF_JMP => self.compile_jmp(insn)?,
                BPF_EXIT => {
                    self.emit_epilogue();
                    self.emit_byte(0xC3); // RET
                }
                _ => return Err("Unsupported eBPF instruction for JIT"),
            }
        }

        Ok(self.code.clone())
    }

    fn emit_prologue(&mut self) {
        // push r12; push r13
        self.emit_byte(0x41); self.emit_byte(0x54);
        self.emit_byte(0x41); self.emit_byte(0x55);
        // mov r13, rdi (assuming R10/FP is passed as 1st arg for now, or just dummy)
    }

    fn emit_epilogue(&mut self) {
        // pop r13; pop r12
        self.emit_byte(0x41); self.emit_byte(0x5D);
        self.emit_byte(0x41); self.emit_byte(0x5C);
    }

    fn compile_alu64(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let op = insn.code & 0xf0;
        let dst = insn.dst_reg;
        let src = insn.src_reg;
        let imm = insn.imm;

        match op {
            BPF_MOV => {
                if insn.code & 0x08 != 0 {
                    // MOV R_dst, imm32
                    self.emit_mov_imm64(dst, imm as u64);
                } else {
                    // MOV R_dst, R_src
                    self.emit_mov_reg64(dst, src);
                }
            }
            BPF_ADD => {
                if insn.code & 0x08 != 0 {
                    self.emit_add_imm64(dst, imm as u64);
                } else {
                    self.emit_add_reg64(dst, src);
                }
            }
            _ => return Err("Unsupported ALU64 op"),
        }
        Ok(())
    }

    fn compile_jmp(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let op = insn.code & 0xf0;
        if op == BPF_EXIT {
            // Handled in main loop
        } else {
            return Err("Jumps not yet fully implemented in JIT");
        }
        Ok(())
    }

    fn emit_byte(&mut self, b: u8) { self.code.push(b); }

    fn emit_mov_imm64(&mut self, _dst: u8, _imm: u64) {
        // REX.W mov reg, imm64 is 10 bytes. For now simplified.
        self.emit_byte(0x48);
        self.emit_byte(0xC7); // MOV r/m64, imm32 (sign extended)
        self.emit_byte(0xC0 | (_dst & 7));
        let bytes = (_imm as u32).to_le_bytes();
        for b in bytes { self.emit_byte(b); }
    }

    fn emit_mov_reg64(&mut self, _dst: u8, _src: u8) {
        self.emit_byte(0x48);
        self.emit_byte(0x89);
        self.emit_byte(0xC0 | ((_src & 7) << 3) | (_dst & 7));
    }

    fn emit_add_imm64(&mut self, _dst: u8, _imm: u64) {
        self.emit_byte(0x48);
        self.emit_byte(0x81);
        self.emit_byte(0xC0 | (_dst & 7));
        let bytes = (_imm as u32).to_le_bytes();
        for b in bytes { self.emit_byte(b); }
    }

    fn emit_add_reg64(&mut self, _dst: u8, _src: u8) {
        self.emit_byte(0x48);
        self.emit_byte(0x01);
        self.emit_byte(0xC0 | ((_src & 7) << 3) | (_dst & 7));
    }
}
