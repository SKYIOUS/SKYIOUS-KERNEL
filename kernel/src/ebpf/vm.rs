pub const STACK_SIZE: usize = 512;

// ── eBPF instruction ──────────────────────────────────────────────
#[repr(C)]
#[derive(Clone, Copy)]
pub struct EbpfInsn {
    pub code: u8,
    pub dst_reg: u8,
    pub src_reg: u8,
    pub off: i16,
    pub imm: i32,
}

impl EbpfInsn {
    pub fn new(code: u8, dst: u8, src: u8, off: i16, imm: i32) -> Self {
        EbpfInsn { code, dst_reg: dst, src_reg: src, off, imm }
    }
}

// ── Register file ─────────────────────────────────────────────────
pub struct EbpfRegs(pub [u64; 11]);

impl EbpfRegs {
    pub fn new() -> Self { EbpfRegs([0u64; 11]) }
    pub fn r0(&self) -> u64 { self.0[0] }
    pub fn set_r0(&mut self, v: u64) { self.0[0] = v; }
    pub fn r1(&self) -> u64 { self.0[1] }
    pub fn set_r1(&mut self, v: u64) { self.0[1] = v; }
    pub fn r(&self, i: usize) -> u64 {
        if i < 11 { self.0[i] } else { 0 }
    }
    pub fn set_r(&mut self, i: usize, v: u64) {
        if i < 11 { self.0[i] = v; }
    }
}

// ── Opcode classes ────────────────────────────────────────────────
pub const BPF_LD: u8 = 0x00;
pub const BPF_LDX: u8 = 0x01;
pub const BPF_ST: u8 = 0x02;
pub const BPF_STX: u8 = 0x03;
pub const BPF_ALU: u8 = 0x04;
pub const BPF_JMP: u8 = 0x05;
pub const BPF_JMP32: u8 = 0x06;
pub const BPF_ALU64: u8 = 0x07;

// ── Instruction modifiers ─────────────────────────────────────────
pub const BPF_W: u8 = 0x00;
pub const BPF_H: u8 = 0x08;
pub const BPF_B: u8 = 0x10;
pub const BPF_DW: u8 = 0x18;
pub const BPF_MOV: u8 = 0xb0;
pub const BPF_ADD: u8 = 0x00;
pub const BPF_SUB: u8 = 0x10;
pub const BPF_MUL: u8 = 0x20;
pub const BPF_DIV: u8 = 0x30;
pub const BPF_OR: u8 = 0x40;
pub const BPF_AND: u8 = 0x50;
pub const BPF_LSH: u8 = 0x60;
pub const BPF_RSH: u8 = 0x70;
pub const BPF_NEG: u8 = 0x80;
pub const BPF_MOD: u8 = 0x90;
pub const BPF_XOR: u8 = 0xa0;
pub const BPF_ARSH: u8 = 0xc0;

// ── Jump conditions ───────────────────────────────────────────────
pub const BPF_JA: u8 = 0x00;
pub const BPF_JEQ: u8 = 0x10;
pub const BPF_JGT: u8 = 0x20;
pub const BPF_JGE: u8 = 0x30;
pub const BPF_JSET: u8 = 0x40;
pub const BPF_JNE: u8 = 0x50;
pub const BPF_JSGT: u8 = 0x60;
pub const BPF_JSGE: u8 = 0x70;
pub const BPF_CALL: u8 = 0x80;
pub const BPF_EXIT: u8 = 0x90;

// ── LD/LDX size modifiers ─────────────────────────────────────────
pub const BPF_SIZE_MASK: u8 = 0x18;
pub const BPF_IMM: u8 = 0x00;

// ── eBPF VM ───────────────────────────────────────────────────────
pub struct EbpfVm<'a> {
    insns: &'a [EbpfInsn],
}

impl<'a> EbpfVm<'a> {
    pub fn new(insns: &'a [EbpfInsn], _licensed: bool) -> Self {
        EbpfVm { insns }
    }

    pub fn exec_raw(&mut self, regs: &mut EbpfRegs, stack: &mut [u8; STACK_SIZE]) -> u64 {
        let mut pc: usize = 0;
        let insns = self.insns;

        while pc < insns.len() {
            let insn = &insns[pc];
            let cls = insn.code & 0x07;
            let src = insn.src_reg as usize;
            let dst = insn.dst_reg as usize;
            let off = insn.off as i64;
            let imm = insn.imm as i64;

            match cls {
                BPF_ALU | BPF_ALU64 => {
                    let is64 = cls == BPF_ALU64;
                    let op = insn.code & 0xf0;
                    let src_val = if insn.code & 0x08 != 0 { imm } else { regs.r(src) as i64 };
                    let dst_val = regs.r(dst) as i64;

                    let result = match op {
                        BPF_ADD => dst_val.wrapping_add(src_val),
                        BPF_SUB => dst_val.wrapping_sub(src_val),
                        BPF_MUL => dst_val.wrapping_mul(src_val),
                        BPF_DIV => { if src_val == 0 { 0 } else { dst_val / src_val } }
                        BPF_OR => dst_val | src_val,
                        BPF_AND => dst_val & src_val,
                        BPF_LSH => dst_val.wrapping_shl(src_val as u32),
                        BPF_RSH => (dst_val as u64).wrapping_shr(src_val as u32) as i64,
                        BPF_MOD => { if src_val == 0 { 0 } else { dst_val % src_val } }
                        BPF_XOR => dst_val ^ src_val,
                        BPF_MOV => src_val,
                        BPF_ARSH => (dst_val as u64).wrapping_shr(src_val as u32) as i64,
                        BPF_NEG => dst_val.wrapping_neg(),
                        _ => 0i64,
                    };

                    let final_val = if is64 { result as u64 } else { (result as i32) as u64 };
                    regs.set_r(dst, final_val);
                    pc += 1;
                }

                BPF_JMP | BPF_JMP32 => {
                    let op = insn.code & 0xf0;
                    let src_val = if op == BPF_JA { 0 }
                        else if cls == BPF_JMP32 { regs.r(src) as i32 as i64 }
                        else { regs.r(src) as i64 };
                    let dst_val = if cls == BPF_JMP32 { regs.r(dst) as i32 as i64 }
                        else { regs.r(dst) as i64 };

                    let taken = match op {
                        BPF_JA => true,
                        BPF_JEQ => dst_val == src_val,
                        BPF_JGT => (dst_val as u64) > (src_val as u64),
                        BPF_JGE => (dst_val as u64) >= (src_val as u64),
                        BPF_JSET => (dst_val & src_val) != 0,
                        BPF_JNE => dst_val != src_val,
                        BPF_JSGT => dst_val > src_val,
                        BPF_JSGE => dst_val >= src_val,
                        BPF_CALL => {
                            match insn.imm {
                                1 => {
                                    let map_fd = regs.r(1) as u64;
                                    let key_ptr = regs.r(2) as *const u8;
                                    let val_ptr = regs.r(3) as *mut u8;
                                    let ret = super::helpers::bpf_helper_map_lookup_elem(map_fd, key_ptr, val_ptr);
                                    regs.set_r0(ret as u64);
                                }
                                2 => {
                                    regs.set_r0(super::helpers::bpf_helper_get_current_pid());
                                }
                                3 => {
                                    regs.set_r0(super::helpers::bpf_helper_get_ticks());
                                }
                                4 => {
                                    let msg_ptr = regs.r(1) as *const u8;
                                    let len = regs.r(2);
                                    super::helpers::bpf_helper_debug_print(msg_ptr, len);
                                }
                                _ => {}
                            }
                            pc += 1;
                            continue;
                        }
                        BPF_EXIT => {
                            return regs.r0();
                        }
                        _ => false,
                    };

                    if taken && op != BPF_CALL {
                        if off == -1 {
                            return regs.r0();
                        }
                        let offset = if off < 0 {
                            pc.wrapping_sub((-off) as usize)
                        } else {
                            pc + (off as usize)
                        };
                        pc = offset;
                    } else {
                        pc += 1;
                    }
                }

                BPF_LD => {
                    if insn.code & BPF_SIZE_MASK == BPF_DW && insn.code & 0xe0 == BPF_IMM {
                        let next = if pc + 1 < insns.len() { &insns[pc + 1] } else { &EbpfInsn::new(0, 0, 0, 0, 0) };
                        let imm64 = (insn.imm as u64) | ((next.imm as u64) << 32);
                        regs.set_r(dst, imm64);
                        pc += 2;
                    } else {
                        pc += 1;
                    }
                }

                BPF_LDX => {
                    let size = insn.code & BPF_SIZE_MASK;
                    let base = regs.r(src) as usize;
                    let addr = base.wrapping_add(off as usize);

                    if addr + size_bytes(size) > STACK_SIZE { pc += 1; continue; }
                    let val = match size {
                        0x00 => unsafe { *(stack.as_ptr().add(addr) as *const u64) },
                        0x08 => unsafe { *(stack.as_ptr().add(addr) as *const u32) as u64 },
                        0x10 => unsafe { *(stack.as_ptr().add(addr) as *const u16) as u64 },
                        0x18 => unsafe { *(stack.as_ptr().add(addr) as *const u8) as u64 },
                        _ => 0,
                    };
                    regs.set_r(dst, val);
                    pc += 1;
                }

                BPF_ST => {
                    let addr = regs.r(dst) as usize + off as usize;
                    if addr + 8 > STACK_SIZE { pc += 1; continue; }
                    let val = imm as u64;
                    unsafe { *(stack.as_mut_ptr().add(addr) as *mut u64) = val; }
                    pc += 1;
                }

                BPF_STX => {
                    let size = insn.code & BPF_SIZE_MASK;
                    let base = regs.r(dst) as usize;
                    let addr = base.wrapping_add(off as usize);
                    if addr + size_bytes(size) > STACK_SIZE { pc += 1; continue; }
                    let val = regs.r(src);
                    match size {
                        0x00 => unsafe { *(stack.as_mut_ptr().add(addr) as *mut u64) = val },
                        0x08 => unsafe { *(stack.as_mut_ptr().add(addr) as *mut u32) = val as u32 },
                        0x10 => unsafe { *(stack.as_mut_ptr().add(addr) as *mut u16) = val as u16 },
                        0x18 => unsafe { *(stack.as_mut_ptr().add(addr) as *mut u8) = val as u8 },
                        _ => {}
                    }
                    pc += 1;
                }

                _ => { pc += 1; }
            }
        }
        regs.r0()
    }
}

fn size_bytes(size: u8) -> usize {
    match size {
        0x00 => 8,
        0x08 => 4,
        0x10 => 2,
        0x18 => 1,
        _ => 0,
    }
}
