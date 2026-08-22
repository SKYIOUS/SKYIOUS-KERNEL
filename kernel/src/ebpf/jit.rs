use super::vm::*;
use alloc::vec::Vec;

/// eBPF-to-x86_64 JIT compiler for the Vahi kernel.
///
/// Translates eBPF instructions into native x86_64 machine code.
/// Supports: ALU64, ALU32, JMP/JMP32, LDX, ST, STX, EXIT.
///
/// Register mapping (eBPF -> x86_64):
/// R0 -> RAX  (accumulator, return value)
/// R1 -> RDI  (1st function arg)
/// R2 -> RSI  (2nd function arg)
/// R3 -> RDX  (3rd function arg)
/// R4 -> RCX  (4th function arg)
/// R5 -> R8   (5th function arg)
/// R6 -> R9   (callee-saved)
/// R7 -> R10  (callee-saved)
/// R8 -> R11  (callee-saved)
/// R9 -> R12  (callee-saved)
/// R10 -> R13 (frame pointer, callee-saved)
pub struct EbpfJit {
    code: Vec<u8>,
}

impl EbpfJit {
    pub fn new() -> Self {
        EbpfJit { code: Vec::new() }
    }

    /// Compiles eBPF instructions into x86_64 machine code.
    pub fn compile(&mut self, insns: &[EbpfInsn]) -> Result<Vec<u8>, &'static str> {
        self.emit_prologue();

        for insn in insns {
            let cls = insn.code & 0x07;
            match cls {
                BPF_ALU64 => self.compile_alu64(insn)?,
                BPF_ALU => self.compile_alu32(insn)?,
                BPF_JMP => self.compile_jmp(insn)?,
                BPF_JMP32 => self.compile_jmp(insn)?,
                BPF_LDX => self.compile_ldx(insn)?,
                BPF_ST => self.compile_st(insn)?,
                BPF_STX => self.compile_stx(insn)?,
                // BPF_LD (0x00) is not used in eBPF — all loads are LDX
                0 => return Err("BPF_LD class not supported in JIT"),
                _ => return Err("Unsupported eBPF instruction class for JIT"),
            }
        }

        Ok(self.code.clone())
    }

    fn emit_prologue(&mut self) {
        // push rbp; mov rbp, rsp
        self.emit_byte(0x55);
        self.emit_byte(0x48); self.emit_byte(0x89); self.emit_byte(0xE5);
        // push r12; push r13; push r14; push r15
        self.emit_byte(0x41); self.emit_byte(0x54);
        self.emit_byte(0x41); self.emit_byte(0x55);
        self.emit_byte(0x41); self.emit_byte(0x56);
        self.emit_byte(0x41); self.emit_byte(0x57);
    }

    fn emit_epilogue(&mut self) {
        // pop r15; pop r14; pop r13; pop r12
        self.emit_byte(0x41); self.emit_byte(0x5F);
        self.emit_byte(0x41); self.emit_byte(0x5E);
        self.emit_byte(0x41); self.emit_byte(0x5D);
        self.emit_byte(0x41); self.emit_byte(0x5C);
        // pop rbp; ret
        self.emit_byte(0x5D);
    }

    /// Map eBPF register number to x86_64 register encoding.
    fn x86reg(&self, r: u8) -> Result<u8, &'static str> {
        match r {
            0 => Ok(0),   // RAX
            1 => Ok(7),   // RDI
            2 => Ok(6),   // RSI
            3 => Ok(2),   // RDX
            4 => Ok(1),   // RCX
            5 => Ok(0),   // R8  (needs REX.B)
            6 => Ok(1),   // R9  (needs REX.B)
            7 => Ok(2),   // R10 (needs REX.B)
            8 => Ok(3),   // R11 (needs REX.B)
            9 => Ok(4),   // R12 (needs REX.B)
            10 => Ok(5),  // R13 (needs REX.B)
            _ => Err("Invalid eBPF register"),
        }
    }

    /// Returns true if the eBPF register needs REX.B in ModRM encoding.
    fn needs_rexb(&self, r: u8) -> bool { r >= 5 }

    /// Returns true if the eBPF register needs REX.R in ModRM encoding.
    fn needs_rexr(&self, r: u8) -> bool { r >= 5 }

    fn emit_byte(&mut self, b: u8) { self.code.push(b); }

    fn emit_u32(&mut self, v: u32) {
        for b in v.to_le_bytes() { self.emit_byte(b); }
    }

    /// Emit: MOV r64, imm64 (10 bytes)
    fn emit_mov_imm64(&mut self, dst: u8, imm: u64) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        self.emit_byte(rex);
        self.emit_byte(0xB8 | (d & 7));
        self.emit_u32(imm as u32);
        self.emit_u32((imm >> 32) as u32);
        Ok(())
    }

    /// Emit: MOV r/m64, r64
    fn emit_mov_reg64(&mut self, dst: u8, src: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let s = self.x86reg(src)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        if self.needs_rexb(src) { rex |= 0x01; }
        self.emit_byte(rex);
        self.emit_byte(0x89);
        self.emit_byte(0xC0 | ((s & 7) << 3) | (d & 7));
        Ok(())
    }

    /// Emit: MOV r/m32, r32 (zero-extends to 64-bit)
    fn emit_mov_reg32(&mut self, dst: u8, src: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let s = self.x86reg(src)?;
        let mut rex = 0x40;
        if self.needs_rexr(dst) { rex |= 0x04; }
        if self.needs_rexb(src) { rex |= 0x01; }
        self.emit_byte(rex);
        self.emit_byte(0x89);
        self.emit_byte(0xC0 | ((s & 7) << 3) | (d & 7));
        Ok(())
    }

    /// Emit ALU64 reg, imm32 using opcode extension.
    /// opcode_ext is the /r field: ADD=/0, OR=/1, ADC=/2, SBB=/3,
    /// AND=/4, SUB=/5, XOR=/6
    fn emit_alu_imm32(&mut self, dst: u8, imm: i32, opcode_ext: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        self.emit_byte(rex);
        self.emit_byte(0x81);
        self.emit_byte(0xC0 | ((opcode_ext & 7) << 3) | (d & 7));
        self.emit_u32(imm as u32);
        Ok(())
    }

    /// Emit ALU64 r/m64, r64.
    /// opcode is the base: ADD=0x01, OR=0x09, AND=0x21, SUB=0x29, XOR=0x31
    fn emit_alu_reg(&mut self, dst: u8, src: u8, opcode: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let s = self.x86reg(src)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        if self.needs_rexb(src) { rex |= 0x01; }
        self.emit_byte(rex);
        self.emit_byte(opcode);
        self.emit_byte(0xC0 | ((s & 7) << 3) | (d & 7));
        Ok(())
    }

    /// Emit shift r/m64, imm8.
    /// opcode_ext: /4=SHL, /5=SHR, /7=SAR
    fn emit_shift_imm(&mut self, dst: u8, imm: u8, opcode_ext: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        self.emit_byte(rex);
        self.emit_byte(0xC1);
        self.emit_byte(0xE0 | ((opcode_ext & 7) << 3) | (d & 7));
        self.emit_byte(imm);
        Ok(())
    }

    /// Emit CMP r/m64, imm32
    fn emit_cmp_imm(&mut self, dst: u8, imm: i32) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        self.emit_byte(rex);
        self.emit_byte(0x81);
        self.emit_byte(0xF8 | (d & 7));
        self.emit_u32(imm as u32);
        Ok(())
    }

    /// Emit CMP r/m64, r64
    fn emit_cmp_reg(&mut self, dst: u8, src: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let s = self.x86reg(src)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        if self.needs_rexb(src) { rex |= 0x01; }
        self.emit_byte(rex);
        self.emit_byte(0x39); // CMP r/m64, r64
        self.emit_byte(0xC0 | ((s & 7) << 3) | (d & 7));
        Ok(())
    }

    /// Emit NEG r/m64
    fn emit_neg(&mut self, dst: u8) -> Result<(), &'static str> {
        let d = self.x86reg(dst)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        self.emit_byte(rex);
        self.emit_byte(0xF7);
        self.emit_byte(0xD8 | (d & 7));
        Ok(())
    }

    // ─── ALU64 ───────────────────────────────────────────────────────

    fn compile_alu64(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let op = insn.code & 0xF0;
        let dst = insn.dst_reg;
        let src = insn.src_reg;
        let imm = insn.imm;
        let is_imm = (insn.code & 0x08) != 0;

        match op {
            BPF_ADD if is_imm => self.emit_alu_imm32(dst, imm, 0)?,
            BPF_ADD           => self.emit_alu_reg(dst, src, 0x01)?,
            BPF_SUB if is_imm => self.emit_alu_imm32(dst, imm, 5)?,
            BPF_SUB           => self.emit_alu_reg(dst, src, 0x29)?,
            BPF_AND if is_imm => self.emit_alu_imm32(dst, imm, 4)?,
            BPF_AND           => self.emit_alu_reg(dst, src, 0x21)?,
            BPF_OR  if is_imm => self.emit_alu_imm32(dst, imm, 1)?,
            BPF_OR            => self.emit_alu_reg(dst, src, 0x09)?,
            BPF_XOR if is_imm => self.emit_alu_imm32(dst, imm, 6)?,
            BPF_XOR           => self.emit_alu_reg(dst, src, 0x31)?,
            BPF_LSH if is_imm => self.emit_shift_imm(dst, imm as u8, 4)?,
            BPF_LSH => {
                // SHL by CL (RCX). Move src into ECX first.
                self.emit_mov_reg32(4, src)?; // ECX = src
                let d = self.x86reg(dst)?;
                let mut rex = 0x48;
                if self.needs_rexr(dst) { rex |= 0x04; }
                self.emit_byte(rex);
                self.emit_byte(0xD3);
                self.emit_byte(0xE4 | (d & 7)); // SHL r/m64, CL
            }
            BPF_RSH if is_imm => self.emit_shift_imm(dst, imm as u8, 5)?,
            BPF_RSH => {
                self.emit_mov_reg32(4, src)?;
                let d = self.x86reg(dst)?;
                let mut rex = 0x48;
                if self.needs_rexr(dst) { rex |= 0x04; }
                self.emit_byte(rex);
                self.emit_byte(0xD3);
                self.emit_byte(0xEC | (d & 7)); // SHR r/m64, CL
            }
            BPF_NEG => self.emit_neg(dst)?,
            BPF_MOV if is_imm => self.emit_mov_imm64(dst, imm as u64)?,
            BPF_MOV           => self.emit_mov_reg64(dst, src)?,
            BPF_MUL if is_imm => {
                // IMUL r64, r/m64, imm32
                let d = self.x86reg(dst)?;
                let mut rex = 0x48;
                if self.needs_rexr(dst) { rex |= 0x04; }
                self.emit_byte(rex);
                self.emit_byte(0x69);
                self.emit_byte(0xC0 | (d & 7) << 3 | (d & 7));
                self.emit_u32(imm as u32);
            }
            BPF_MUL => {
                // IMUL r64, r/m64
                let d = self.x86reg(dst)?;
                let s = self.x86reg(src)?;
                let mut rex = 0x48;
                if self.needs_rexr(dst) { rex |= 0x04; }
                if self.needs_rexb(src) { rex |= 0x01; }
                self.emit_byte(rex);
                self.emit_byte(0x0F);
                self.emit_byte(0xAF);
                self.emit_byte(0xC0 | ((s & 7) << 3) | (d & 7));
            }
            BPF_DIV | BPF_MOD => {
                // DIV r/m64: RDX:RAX / src -> RAX(quot), RDX(rem)
                // Move dst to RAX, zero RDX, DIV src, move result back.
                let is_mod = op == BPF_MOD;
                self.emit_mov_reg64(0, dst)?; // RAX = dst
                // XOR RDX, RDX
                self.emit_byte(0x48); self.emit_byte(0x31); self.emit_byte(0xD2);
                if is_imm {
                    // Load imm into R11, then DIV R11
                    self.emit_mov_imm64(8, imm as u64)?;
                    let s = self.x86reg(8)?;
                    self.emit_byte(0x49);
                    self.emit_byte(0xF7);
                    self.emit_byte(0xF0 | (s & 7));
                } else {
                    let s = self.x86reg(src)?;
                    let mut rex = 0x48;
                    if self.needs_rexb(src) { rex |= 0x01; }
                    self.emit_byte(rex);
                    self.emit_byte(0xF7);
                    self.emit_byte(0xF0 | (s & 7));
                }
                // dst = RAX (div) or RDX (mod)
                if is_mod {
                    self.emit_mov_reg64(dst, 3)?; // dst = RDX
                }
                // else: dst = RAX (already there)
            }
            _ => return Err("Unsupported ALU64 op in JIT"),
        }
        Ok(())
    }

    // ─── ALU32 ───────────────────────────────────────────────────────

    fn compile_alu32(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let op = insn.code & 0xF0;
        let dst = insn.dst_reg;
        let src = insn.src_reg;
        let imm = insn.imm;
        let is_imm = (insn.code & 0x08) != 0;

        match op {
            BPF_MOV if is_imm => self.emit_mov_imm64(dst, imm as u64)?,
            BPF_MOV           => self.emit_mov_reg32(dst, src)?,
            BPF_ADD if is_imm => self.emit_alu_imm32(dst, imm, 0)?,
            BPF_ADD           => self.emit_alu_reg(dst, src, 0x01)?,
            BPF_SUB if is_imm => self.emit_alu_imm32(dst, imm, 5)?,
            BPF_SUB           => self.emit_alu_reg(dst, src, 0x29)?,
            BPF_AND if is_imm => self.emit_alu_imm32(dst, imm, 4)?,
            BPF_AND           => self.emit_alu_reg(dst, src, 0x21)?,
            BPF_OR  if is_imm => self.emit_alu_imm32(dst, imm, 1)?,
            BPF_OR            => self.emit_alu_reg(dst, src, 0x09)?,
            BPF_XOR if is_imm => self.emit_alu_imm32(dst, imm, 6)?,
            BPF_XOR           => self.emit_alu_reg(dst, src, 0x31)?,
            _ => return Err("Unsupported ALU32 op in JIT"),
        }
        Ok(())
    }

    // ─── JMP / JMP32 ────────────────────────────────────────────────

    fn compile_jmp(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let op = insn.code & 0xF0;
        match op {
            BPF_JA => {
                let offset_bytes = (1 + insn.off as i32) * 8;
                self.emit_byte(0xE9);
                self.emit_u32(offset_bytes as u32);
            }
            BPF_EXIT => {
                self.emit_epilogue();
                self.emit_byte(0xC3);
            }
            BPF_CALL => {
                // For now, emit a NOP (CALL not fully supported in JIT)
                // In production this would emit CALL with relocation
                self.emit_byte(0x90); // NOP
            }
            _ => self.emit_cond_jmp(insn)?,
        }
        Ok(())
    }

    fn emit_cond_jmp(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let op = insn.code & 0xF0;
        let dst = insn.dst_reg;
        let is_imm = (insn.code & 0x08) != 0;

        // Emit CMP
        if is_imm {
            self.emit_cmp_imm(dst, insn.imm)?;
        } else {
            self.emit_cmp_reg(dst, insn.src_reg)?;
        }

        // Jcc target: skip (1 + off) instructions * 8 bytes
        let target_bytes = (1 + insn.off as i32) * 8;

        let cc = match op {
            BPF_JEQ  => 0x84u8, // JE
            BPF_JNE  => 0x85,   // JNE
            BPF_JGT  => 0x87,   // JA (above, unsigned)
            BPF_JGE  => 0x8D,   // JAE (above or equal, unsigned)
            BPF_JSET => 0x85,   // JNZ (after TEST — special case below)
            BPF_JSGT => 0x8F,   // JG (greater, signed)
            BPF_JSGE => 0x8D,   // JGE (greater or equal, signed)
            _ => return Err("Unsupported JMP condition"),
        };

        if op == BPF_JSET {
            // JSET: jump if (dst & src/imm) != 0
            // We already emitted CMP. Replace with TEST.
            // Actually, we need to re-emit. For simplicity, use TEST r/m64, r64
            // and JNZ.
            // Rewind the CMP we just emitted (7 bytes for CMP r/m64, imm32
            // or 3 bytes for CMP r/m64, r64).
            let rewind = if is_imm { 7 } else { 3 };
            self.code.truncate(self.code.len() - rewind);

            if is_imm {
                // TEST r/m64, imm32
                let d = self.x86reg(dst)?;
                let mut rex = 0x48;
                if self.needs_rexr(dst) { rex |= 0x04; }
                self.emit_byte(rex);
                self.emit_byte(0xF7);
                self.emit_byte(0xC0 | (d & 7)); // TEST r/m64, imm32
                self.emit_u32(insn.imm as u32);
            } else {
                // TEST r/m64, r64
                let d = self.x86reg(dst)?;
                let s = self.x86reg(insn.src_reg)?;
                let mut rex = 0x48;
                if self.needs_rexr(dst) { rex |= 0x04; }
                if self.needs_rexb(insn.src_reg) { rex |= 0x01; }
                self.emit_byte(rex);
                self.emit_byte(0x85); // TEST r/m64, r64
                self.emit_byte(0xC0 | ((s & 7) << 3) | (d & 7));
            }

            // JNZ
            self.emit_byte(0x0F);
            self.emit_byte(0x85);
            self.emit_u32(target_bytes as u32);
        } else {
            // Jcc target
            self.emit_byte(0x0F);
            self.emit_byte(cc);
            self.emit_u32(target_bytes as u32);
        }

        Ok(())
    }

    // ─── LD/LDX ──────────────────────────────────────────────────────

    fn compile_ldx(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let size = insn.code & 0x18;
        let dst = insn.dst_reg;
        let src = insn.src_reg;
        let offset = insn.off as i32;

        let d = self.x86reg(dst)?;
        let s = self.x86reg(src)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        if self.needs_rexb(src) { rex |= 0x01; }
        let modrm = 0x80 | ((s & 7) << 3) | (d & 7); // [reg + disp32]

        match size {
            BPF_W => {
                // MOV r32, [r64 + off32] (zero-extends)
                let rex32 = rex & !0x08; // No REX.W
                self.emit_byte(rex32);
                self.emit_byte(0x8B); // MOV r32, r/m32
                self.emit_byte(modrm);
                self.emit_u32(offset as u32);
            }
            BPF_H => {
                // MOVZX r64, r16 [r64 + off32]
                self.emit_byte(rex);
                self.emit_byte(0x0F);
                self.emit_byte(0xB7); // MOVZX r64, r/m16
                self.emit_byte(modrm);
                self.emit_u32(offset as u32);
            }
            BPF_B => {
                // MOVZX r64, r8 [r64 + off32]
                self.emit_byte(rex);
                self.emit_byte(0x0F);
                self.emit_byte(0xB6); // MOVZX r64, r/m8
                self.emit_byte(modrm);
                self.emit_u32(offset as u32);
            }
            BPF_DW => {
                // MOV r64, [r64 + off32]
                self.emit_byte(rex);
                self.emit_byte(0x8B); // MOV r64, r/m64
                self.emit_byte(modrm);
                self.emit_u32(offset as u32);
            }
            _ => return Err("Unsupported LDX size"),
        }
        Ok(())
    }

    // ─── ST ──────────────────────────────────────────────────────────

    fn compile_st(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let size = insn.code & 0x18;
        let dst = insn.dst_reg;
        let offset = insn.off as i32;

        let d = self.x86reg(dst)?;
        let mut rex = 0x40;
        if self.needs_rexr(dst) { rex |= 0x04; }

        match size {
            BPF_W => {
                // MOV dword [r + off], imm32
                self.emit_byte(rex);
                self.emit_byte(0xC7);
                self.emit_byte(0x80 | (d & 7));
                self.emit_u32(offset as u32);
                self.emit_u32(insn.imm as u32);
            }
            BPF_B => {
                // MOV byte [r + off], imm8
                self.emit_byte(rex);
                self.emit_byte(0xC6);
                self.emit_byte(0x80 | (d & 7));
                self.emit_u32(offset as u32);
                self.emit_byte(insn.imm as u8);
            }
            _ => {
                // Default: 64-bit store
                self.emit_byte(rex | 0x08); // REX.W
                self.emit_byte(0xC7);
                self.emit_byte(0x80 | (d & 7));
                self.emit_u32(offset as u32);
                self.emit_u32(insn.imm as u32);
            }
        }
        Ok(())
    }

    // ─── STX ─────────────────────────────────────────────────────────

    fn compile_stx(&mut self, insn: &EbpfInsn) -> Result<(), &'static str> {
        let size = insn.code & 0x18;
        let dst = insn.dst_reg;
        let src = insn.src_reg;
        let offset = insn.off as i32;

        let d = self.x86reg(dst)?;
        let s = self.x86reg(src)?;
        let mut rex = 0x48;
        if self.needs_rexr(dst) { rex |= 0x04; }
        if self.needs_rexb(src) { rex |= 0x01; }

        match size {
            BPF_W => {
                // MOV dword [r + off], r32
                self.emit_byte(rex & !0x08);
                self.emit_byte(0x89);
                self.emit_byte(0x80 | ((s & 7) << 3) | (d & 7));
                self.emit_u32(offset as u32);
            }
            BPF_B => {
                // MOV byte [r + off], r8
                self.emit_byte(rex & !0x08);
                self.emit_byte(0x88);
                self.emit_byte(0x80 | ((s & 7) << 3) | (d & 7));
                self.emit_u32(offset as u32);
            }
            _ => {
                // Default: 64-bit store
                self.emit_byte(rex);
                self.emit_byte(0x89);
                self.emit_byte(0x80 | ((s & 7) << 3) | (d & 7));
                self.emit_u32(offset as u32);
            }
        }
        Ok(())
    }
}
