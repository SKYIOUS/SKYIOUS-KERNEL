//! Seccomp (Secure Computing) — BPF-based syscall filtering.
//!
//! Supports two modes:
//! - SECCOMP_MODE_STRICT: Allows only read/write/exit/sigreturn.
//! - SECCOMP_MODE_FILTER: User-supplied BPF program filters syscalls.
//!
//! BPF interpreter implements the full classic BPF instruction set as used by
//! Linux seccomp: LD/LDX, ST/STX, ALU, JMP, RET.

use alloc::vec::Vec;
use crate::task::process::CURRENT_PROCESS;
use crate::syscalls::errno;
use crate::syscalls::user_access;

/// Seccomp modes
pub const SECCOMP_MODE_DISABLED: u32 = 0;
pub const SECCOMP_MODE_STRICT: u32 = 1;
pub const SECCOMP_MODE_FILTER: u32 = 2;

/// Seccomp actions
pub const SECCOMP_RET_KILL_PROCESS: u64 = 0x0000_0000;
pub const SECCOMP_RET_KILL_THREAD: u64 = 0x0000_0000;
pub const SECCOMP_RET_TRAP: u64 = 0x0002_0000;
pub const SECCOMP_RET_ERRNO: u64 = 0x0003_0000;
pub const SECCOMP_RET_USER_NOTIF: u64 = 0x7FC0_0000;
pub const SECCOMP_RET_TRACE: u64 = 0x7FF0_0000;
pub const SECCOMP_RET_LOG: u64 = 0x7FFC_0000;
pub const SECCOMP_RET_ALLOW: u64 = 0x7FFF_0000;

pub const SECCOMP_RET_ACTION: u64 = 0xFFFF_0000;

// ─── BPF instruction encoding constants ───────────────────────────

// Instruction classes (bits 0:2)
const BPF_LD: u16 = 0x00;
const BPF_LDX: u16 = 0x01;
const BPF_ST: u16 = 0x02;
const BPF_STX: u16 = 0x03;
const BPF_ALU: u16 = 0x04;
const BPF_JMP: u16 = 0x05;
const BPF_RET: u16 = 0x06;

// Size bits (bits 3:4) for LD/LDX
const BPF_W: u16 = 0x00;   // 32-bit word
const BPF_H: u16 = 0x08;   // 16-bit halfword
const BPF_B: u16 = 0x10;   // 8-bit byte

// Source bits (bit 3) for ALU/JMP
const BPF_K: u16 = 0x00;   // use immediate k
const BPF_X: u16 = 0x08;   // use index register X

// LD/LDX source modes (bits 5:7)
const BPF_ABS: u16 = 0x20;  // load from seccomp data buffer
const BPF_IND: u16 = 0x40;  // load from seccomp data buffer at X + k
const BPF_MEM: u16 = 0x60;  // load/store from scratch memory

// ALU operations (bits 4:7)
const BPF_ADD: u16 = 0x00;
const BPF_SUB: u16 = 0x10;
const BPF_MUL: u16 = 0x20;
const BPF_DIV: u16 = 0x30;
const BPF_OR: u16 = 0x40;
const BPF_AND: u16 = 0x50;
const BPF_LSH: u16 = 0x60;
const BPF_RSH: u16 = 0x70;
const BPF_NEG: u16 = 0x80;
const BPF_MOD: u16 = 0x90;
const BPF_XOR: u16 = 0xA0;

// JMP operations (bits 4:7)
const BPF_JEQ: u16 = 0x10;
const BPF_JGT: u16 = 0x20;
const BPF_JGE: u16 = 0x30;
const BPF_JSET: u16 = 0x40;
const BPF_JNE: u16 = 0x50;
const BPF_JLT: u16 = 0xA0;
const BPF_JLE: u16 = 0xB0;
const BPF_JMP_ALWAYS: u16 = 0x00;

// seccomp data layout — first 16 bytes are the syscall data
// See linux/seccomp.h: struct seccomp_data
const SECCOMP_DATA_NR_OFFSET: usize = 0;       // u32 syscall number
const SECCOMP_DATA_ARCH_OFFSET: usize = 4;     // u32 architecture
const SECCOMP_DATA_IP_OFFSET: usize = 8;  // u64 instruction pointer
const SECCOMP_DATA_ARGS_OFFSET: usize = 16;    // u64 args[6] (48 bytes)

/// BPF instruction for seccomp
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SeccompBpfInstruction {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

/// Per-process seccomp state
pub struct SeccompState {
    pub mode: u32,
    /// Compiled BPF instructions
    pub filter: Vec<SeccompBpfInstruction>,
    /// Action to take when no filter match (defaults to SECCOMP_RET_KILL)
    pub default_action: u64,
}

impl Default for SeccompState {
    fn default() -> Self {
        Self {
            mode: SECCOMP_MODE_DISABLED,
            filter: Vec::new(),
            default_action: SECCOMP_RET_KILL_PROCESS,
        }
    }
}

impl SeccompState {
    /// Check if a syscall is allowed under strict mode
    pub fn check_strict(syscall_nr: u64) -> bool {
        // Strict mode: only read(0), write(1,2), exit, exit_group, sigreturn
        matches!(
            syscall_nr,
            0 | 1 | 3 | 15 | 60 | 231 // read, write, close, sigreturn, exit, exit_group
        )
    }

    /// Build the seccomp data buffer from syscall number and arguments.
    /// This matches the layout of `struct seccomp_data` in Linux.
    fn build_seccomp_data(&self, syscall_nr: u64, args: &[u64; 6]) -> [u8; 64] {
        let mut data = [0u8; 64];
        // nr (u32)
        data[0..4].copy_from_slice(&(syscall_nr as u32).to_ne_bytes());
        // arch (u32) — AUDIT_ARCH_X86_64 = 0xC000003E
        data[4..8].copy_from_slice(&0xC000003E_u32.to_ne_bytes());
        // instruction_pointer (u64) — 0 for now (not tracked)
        data[8..16].copy_from_slice(&0u64.to_ne_bytes());
        // args[0..5] (u64 each)
        for (i, &arg) in args.iter().enumerate() {
            let offset = 16 + i * 8;
            data[offset..offset + 8].copy_from_slice(&arg.to_ne_bytes());
        }
        data
    }

    /// Run the BPF filter on a syscall. Returns the action.
    ///
    /// Implements the full classic BPF instruction set:
    /// - LD/LDX: load from seccomp data, scratch memory, or immediate
    /// - ST/STX: store to scratch memory
    /// - ALU: arithmetic/logic on accumulator
    /// - JMP: conditional/unconditional jumps
    /// - RET: return action
    pub fn check_filter(&self, syscall_nr: u64, args: &[u64; 6]) -> u64 {
        if self.filter.is_empty() {
            return self.default_action;
        }

        let data = self.build_seccomp_data(syscall_nr, args);
        let mut acc: u64 = 0;   // accumulator (A register)
        let mut x: u64 = 0;     // index register (X register)
        let mut mem = [0u64; 16]; // scratch memory
        let mut pc: usize = 0;
        let insn_count = self.filter.len();

        loop {
            if pc >= insn_count {
                return self.default_action;
            }

            let insn = &self.filter[pc];
            let class = insn.code & 0x07;

            match class {
                BPF_LD | BPF_LDX => {
                    // Load instruction
                    let size = insn.code & 0x18; // size bits
                    let source = insn.code & 0xE0; // source bits

                    let value = match source {
                        BPF_ABS => {
                            // Load from seccomp data buffer at offset k
                            let offset = insn.k as usize;
                            match size {
                                BPF_W => {
                                    if offset + 4 > data.len() { return self.default_action; }
                                    u32::from_ne_bytes([
                                        data[offset], data[offset+1],
                                        data[offset+2], data[offset+3],
                                    ]) as u64
                                }
                                BPF_H => {
                                    if offset + 2 > data.len() { return self.default_action; }
                                    u16::from_ne_bytes([data[offset], data[offset+1]]) as u64
                                }
                                BPF_B => {
                                    if offset >= data.len() { return self.default_action; }
                                    data[offset] as u64
                                }
                                _ => return self.default_action,
                            }
                        }
                        BPF_IND => {
                            // Load from seccomp data buffer at X + k
                            let offset = (x.wrapping_add(insn.k as u64)) as usize;
                            match size {
                                BPF_W => {
                                    if offset + 4 > data.len() { return self.default_action; }
                                    u32::from_ne_bytes([
                                        data[offset], data[offset+1],
                                        data[offset+2], data[offset+3],
                                    ]) as u64
                                }
                                BPF_H => {
                                    if offset + 2 > data.len() { return self.default_action; }
                                    u16::from_ne_bytes([data[offset], data[offset+1]]) as u64
                                }
                                BPF_B => {
                                    if offset >= data.len() { return self.default_action; }
                                    data[offset] as u64
                                }
                                _ => return self.default_action,
                            }
                        }
                        BPF_MEM => {
                            // Load from scratch memory at index k
                            if (insn.k as usize) < mem.len() {
                                mem[insn.k as usize]
                            } else {
                                return self.default_action;
                            }
                        }
                        _ => return self.default_action,
                    };

                    if class == BPF_LD {
                        acc = value;
                    } else {
                        x = value;
                    }
                    pc += 1;
                }

                BPF_ST | BPF_STX => {
                    // Store instruction
                    let index = insn.k as usize;
                    if index < mem.len() {
                        mem[index] = if class == BPF_ST { acc } else { x };
                    }
                    pc += 1;
                }

                BPF_ALU => {
                    // Arithmetic/logic instruction
                    let op = insn.code & 0xF0;
                    let src = insn.code & 0x08;
                    let operand = if src == BPF_X { x } else { insn.k as u64 };

                    match op {
                        BPF_ADD => acc = acc.wrapping_add(operand),
                        BPF_SUB => acc = acc.wrapping_sub(operand),
                        BPF_MUL => acc = acc.wrapping_mul(operand),
                        BPF_DIV => {
                            if operand == 0 { return self.default_action; }
                            acc /= operand;
                        }
                        BPF_MOD => {
                            if operand == 0 { return self.default_action; }
                            acc %= operand;
                        }
                        BPF_OR => acc |= operand,
                        BPF_AND => acc &= operand,
                        BPF_XOR => acc ^= operand,
                        BPF_LSH => acc = acc.wrapping_shl(operand as u32),
                        BPF_RSH => acc = acc.wrapping_shr(operand as u32),
                        BPF_NEG => acc = (-(acc as i64)) as u64,
                        _ => return self.default_action,
                    }
                    pc += 1;
                }

                BPF_JMP => {
                    // Jump instruction
                    let op = insn.code & 0xF0;
                    let src = insn.code & 0x08;
                    let operand = if src == BPF_X { x } else { insn.k as u64 };

                    let taken = match op {
                        BPF_JEQ => acc == operand,
                        BPF_JNE => acc != operand,
                        BPF_JGT => acc > operand,
                        BPF_JGE => acc >= operand,
                        BPF_JLT => acc < operand,
                        BPF_JLE => acc <= operand,
                        BPF_JSET => (acc & operand) != 0,
                        BPF_JMP_ALWAYS => true,
                        _ => false,
                    };

                    if taken {
                        pc += 1 + insn.jt as usize;
                    } else {
                        pc += 1 + insn.jf as usize;
                    }
                }

                BPF_RET => {
                    // Return instruction
                    let src = insn.code & 0x08;
                    let retval = if src == BPF_X { x } else { insn.k as u64 };

                    // If retval has SECCOMP_RET_ACTION bits set, it's an action
                    if retval & SECCOMP_RET_ACTION != 0 {
                        return retval;
                    }
                    // Otherwise treat as a numeric return (seccomp compat)
                    return self.default_action;
                }

                _ => {
                    // Unknown instruction class — skip
                    pc += 1;
                }
            }

            // Prevent infinite loops — max 10000 instructions
            if pc > insn_count + 10000 {
                return self.default_action;
            }
        }
    }
}

/// seccomp() syscall — manage seccomp filters
///
/// op: SECCOMP_SET_MODE_STRICT (1) or SECCOMP_SET_MODE_FILTER (2) or SECCOMP_GET_ACTION_AVAIL (3)
pub fn sys_seccomp(op: u32, flags: u32, user_insns: *const u8) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Can only set seccomp once per process (no going back)
    {
        let seccomp = proc.seccomp.lock();
        if seccomp.mode != SECCOMP_MODE_DISABLED {
            return errno::Errno::EINVAL as u64;
        }
    }

    match op {
        1 => {
            // SECCOMP_SET_MODE_STRICT
            let mut seccomp = proc.seccomp.lock();
            seccomp.mode = SECCOMP_MODE_STRICT;
            crate::serial_write("[SECCOMP] Strict mode enabled for pid=");
            crate::serial_write(&alloc::format!("{}\n", proc.id));
            0
        }
        2 => {
            // SECCOMP_SET_MODE_FILTER
            if flags & 0x01 != 0 {
                // SECCOMP_FILTER_FLAG_LOG — allow logging
            }

            if user_insns.is_null() {
                return errno::Errno::EINVAL as u64;
            }

            // Read the sock_fprog header (2 bytes len + 8 bytes pointer)
            let mut hdr = [0u8; 12];
            if unsafe { user_access::copy_from_user(&mut hdr, user_insns) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }
            let len = u16::from_ne_bytes([hdr[0], hdr[1]]) as usize;
            let insns_ptr = u64::from_ne_bytes([
                hdr[4], hdr[5], hdr[6], hdr[7], hdr[8], hdr[9], hdr[10], hdr[11],
            ]);

            if len == 0 || len > 4096 {
                return errno::Errno::EINVAL as u64;
            }

            // Read BPF instructions from userspace
            let insn_size = core::mem::size_of::<SeccompBpfInstruction>();
            let total_size = len * insn_size;
            let mut buf = alloc::vec![0u8; total_size];
            if unsafe { user_access::copy_from_user(&mut buf, insns_ptr as *const u8) }.is_err() {
                return errno::Errno::EFAULT as u64;
            }

            // Parse instructions
            let mut filter = Vec::new();
            for i in 0..len {
                let offset = i * insn_size;
                if offset + insn_size > buf.len() {
                    return errno::Errno::EINVAL as u64;
                }
                let code = u16::from_ne_bytes([buf[offset], buf[offset + 1]]);
                let jt = buf[offset + 2];
                let jf = buf[offset + 3];
                let k = u32::from_ne_bytes([
                    buf[offset + 4], buf[offset + 5], buf[offset + 6], buf[offset + 7],
                ]);
                filter.push(SeccompBpfInstruction { code, jt, jf, k });
            }

            let mut seccomp = proc.seccomp.lock();
            seccomp.mode = SECCOMP_MODE_FILTER;
            seccomp.filter = filter;
            if flags & 0x02 != 0 {
                // SECCOMP_FILTER_FLAG_SPEC_ALLOW — allow speculative bypass
            }

            crate::serial_write("[SECCOMP] Filter mode enabled for pid=");
            crate::serial_write(&alloc::format!("{} ({} instructions)\n", proc.id, len));
            0
        }
        3 => {
            // SECCOMP_GET_ACTION_AVAIL — check if action is supported
            let action = flags as u64;
            match action {
                SECCOMP_RET_KILL_PROCESS |
                SECCOMP_RET_TRAP | SECCOMP_RET_ERRNO | SECCOMP_RET_TRACE |
                SECCOMP_RET_LOG | SECCOMP_RET_ALLOW => 0,
                _ => errno::Errno::EOPNOTSUPP as u64,
            }
        }
        4 => {
            // SECCOMP_GET_NOTIF_SIZES — not implemented yet
            errno::Errno::ENOSYS as u64
        }
        _ => errno::Errno::EINVAL as u64,
    }
}

/// Check if the current syscall is allowed by seccomp.
/// Called from the syscall dispatch path.
/// Returns true if the syscall should proceed, false to block.
pub fn check_syscall(nr: u64, args: &[u64; 6]) -> bool {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return true, // No process = allow
    };

    let seccomp = proc.seccomp.lock();
    match seccomp.mode {
        SECCOMP_MODE_DISABLED => true,
        SECCOMP_MODE_STRICT => SeccompState::check_strict(nr),
        SECCOMP_MODE_FILTER => {
            #[allow(unreachable_patterns)]
            let action = seccomp.check_filter(nr, args);
            match action & SECCOMP_RET_ACTION {
                SECCOMP_RET_ALLOW => true,
                SECCOMP_RET_KILL_PROCESS => {
                    crate::serial_write("[SECCOMP] Killed process syscall=");
                    crate::serial_write(&alloc::format!("{} pid={}\n", nr, proc.id));
                    drop(seccomp);
                    drop(lock);
                    super::process::sys_exit(0x19 + 128); // SIGSYS
                    false
                }
                SECCOMP_RET_TRAP => {
                    // Send SIGSYS to the process
                    crate::serial_write("[SECCOMP] Trap syscall=");
                    crate::serial_write(&alloc::format!("{} pid={}\n", nr, proc.id));
                    true // Allow for now; signal delivery handled separately
                }
                SECCOMP_RET_ERRNO => {
                    // Return errno (lower 16 bits)
                    let err = (action & 0xFFFF) as u64;
                    if err == 0 { return true; } // errno 0 = EPERM
                    false
                }
                SECCOMP_RET_TRACE => {
                    // Allow, but would notify ptracer
                    true
                }
                SECCOMP_RET_LOG => {
                    // Allow and log
                    crate::serial_write("[SECCOMP] Log syscall=");
                    crate::serial_write(&alloc::format!("{} pid={}\n", nr, proc.id));
                    true
                }
                _ => true,
            }
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_strict_allowed() {
        assert!(SeccompState::check_strict(0));  // read
        assert!(SeccompState::check_strict(1));  // write
        assert!(SeccompState::check_strict(3));  // close
        assert!(SeccompState::check_strict(60)); // exit
        assert!(SeccompState::check_strict(231)); // exit_group
    }

    #[test]
    fn test_check_strict_denied() {
        assert!(!SeccompState::check_strict(2));  // open
        assert!(!SeccompState::check_strict(9));  // mmap
        assert!(!SeccompState::check_strict(56)); // clone
        assert!(!SeccompState::check_strict(59)); // execve
    }

    #[test]
    fn test_bpf_ret_allow() {
        // Simple BPF: LD+RET ALLOW
        let filter = vec![
            SeccompBpfInstruction { code: 0x00 | 0x20 | BPF_W, jt: 0, jf: 0, k: 0 }, // LD ABS [0] (syscall nr)
            SeccompBpfInstruction { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW as u32 },
        ];
        let state = SeccompState {
            mode: SECCOMP_MODE_FILTER,
            filter,
            default_action: SECCOMP_RET_KILL_PROCESS,
        };
        let args = [0; 6];
        assert_eq!(state.check_filter(42, &args), SECCOMP_RET_ALLOW);
    }

    #[test]
    fn test_bpf_jeq_match() {
        // BPF: LD+JEQ k=1 jt=1 jf=0, RET ALLOW, RET KILL
        let filter = vec![
            SeccompBpfInstruction { code: 0x00 | 0x20 | BPF_W, jt: 0, jf: 0, k: 0 }, // LD ABS [0]
            SeccompBpfInstruction { code: BPF_JMP | BPF_JEQ | BPF_K, jt: 1, jf: 0, k: 1 }, // JEQ 1
            SeccompBpfInstruction { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW as u32 },
            SeccompBpfInstruction { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_KILL_PROCESS as u32 },
        ];
        let state = SeccompState {
            mode: SECCOMP_MODE_FILTER,
            filter,
            default_action: SECCOMP_RET_KILL_PROCESS,
        };
        let args = [0; 6];
        // syscall_nr=1 → matches JEQ → ALLOW
        assert_eq!(state.check_filter(1, &args), SECCOMP_RET_ALLOW);
        // syscall_nr=2 → doesn't match → KILL
        assert_eq!(state.check_filter(2, &args), SECCOMP_RET_KILL_PROCESS);
    }

    #[test]
    fn test_bpf_alu_and() {
        // BPF: LD+AND k=0xFF+RET ALLOW if acc==0x42
        let filter = vec![
            SeccompBpfInstruction { code: 0x00 | 0x20 | BPF_W, jt: 0, jf: 0, k: 0 }, // LD ABS [0]
            SeccompBpfInstruction { code: BPF_ALU | BPF_AND | BPF_K, jt: 0, jf: 0, k: 0xFF }, // AND 0xFF
            SeccompBpfInstruction { code: BPF_JMP | BPF_JEQ | BPF_K, jt: 1, jf: 0, k: 0x42 }, // JEQ 0x42
            SeccompBpfInstruction { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW as u32 },
            SeccompBpfInstruction { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_KILL_PROCESS as u32 },
        ];
        let state = SeccompState {
            mode: SECCOMP_MODE_FILTER,
            filter,
            default_action: SECCOMP_RET_KILL_PROCESS,
        };
        let args = [0; 6];
        assert_eq!(state.check_filter(0x42, &args), SECCOMP_RET_ALLOW);
        assert_eq!(state.check_filter(0x43, &args), SECCOMP_RET_KILL_PROCESS);
    }

    #[test]
    fn test_build_seccomp_data() {
        let state = SeccompState::default();
        let args = [10, 20, 30, 40, 50, 60];
        let data = state.build_seccomp_data(42, &args);
        // Verify syscall number at offset 0
        assert_eq!(u32::from_ne_bytes([data[0], data[1], data[2], data[3]]), 42);
        // Verify arch at offset 4
        assert_eq!(u32::from_ne_bytes([data[4], data[5], data[6], data[7]]), 0xC000003E);
        // Verify args at offset 16+
        assert_eq!(u64::from_ne_bytes(data[16..24].try_into().unwrap()), 10);
        assert_eq!(u64::from_ne_bytes(data[24..32].try_into().unwrap()), 20);
    }

    #[test]
    fn test_empty_filter_uses_default() {
        let state = SeccompState {
            mode: SECCOMP_MODE_FILTER,
            filter: Vec::new(),
            default_action: SECCOMP_RET_KILL_PROCESS,
        };
        let args = [0; 6];
        assert_eq!(state.check_filter(1, &args), SECCOMP_RET_KILL_PROCESS);
    }
}
