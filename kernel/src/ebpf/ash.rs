use alloc::vec::Vec;
use spin::Mutex;
use super::vm::{EbpfVm, EbpfInsn, EbpfRegs, STACK_SIZE};
use super::verifier;

/// ASH handler state — installed per-NIC for packet-level filtering.
pub struct AshHandler {
    /// Program ID in the BPF program table
    pub prog_id: u64,
    /// Pre-verified bytecode (copied for IRQ-safety)
    pub insns: Vec<EbpfInsn>,
    /// Whether this handler can initiate replies (XDP-style)
    pub can_initiate: bool,
    /// Protocol filter (0 = all, 1 = ICMP, 6 = TCP, 17 = UDP)
    pub protocol: u8,
    /// Destination port filter (0 = all)
    pub dst_port: u16,
}

/// Result from an ASH execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AshAction {
    /// Pass packet to normal stack
    Pass,
    /// Drop packet silently
    Drop,
    /// Drop and send ICMP Unreachable
    DropWithReply,
    /// Redirect to a specific socket handle
    Redirect(u64),
}

/// Pre-allocated per-CPU ASH stack (interrupt-safe, no allocation in IRQ).
pub struct AshPerCpu {
    pub stack: [u8; STACK_SIZE],
    pub regs: EbpfRegs,
}

// Global list of ASH handlers
static ASH_HANDLERS: Mutex<Vec<AshHandler>> = Mutex::new(Vec::new());

/// Install an ASH handler from a verified eBPF program.
/// Called during `BPF_PROG_ATTACH` with `BPF_ATTACH_ASH`.
pub fn install_handler(prog_id: u64, insns: &[EbpfInsn], protocol: u8, dst_port: u16) -> bool {
    if !verifier::is_ash_safe(insns) {
        return false;
    }
    let mut handlers = ASH_HANDLERS.lock();
    handlers.push(AshHandler {
        prog_id,
        insns: insns.to_vec(),
        can_initiate: true,
        protocol,
        dst_port,
    });
    true
}

/// Remove an ASH handler by program ID.
pub fn remove_handler(prog_id: u64) -> bool {
    let mut handlers = ASH_HANDLERS.lock();
    let before = handlers.len();
    handlers.retain(|h| h.prog_id != prog_id);
    handlers.len() < before
}

/// Execute ASH handlers for an incoming packet.
/// Called from the NIC interrupt handler (IRQ context).
///
/// SAFETY: Must be called with interrupts disabled. No allocation, no blocking.
pub unsafe fn run_ash_handlers(
    packet: &[u8],
    protocol: u8,
    dst_port: u16,
    cpu_data: &mut AshPerCpu,
) -> AshAction {
    let handlers = ASH_HANDLERS.lock();
    if handlers.is_empty() {
        return AshAction::Pass;
    }

    for handler in handlers.iter() {
        if handler.protocol != 0 && handler.protocol != protocol {
            continue;
        }
        if handler.dst_port != 0 && handler.dst_port != dst_port {
            continue;
        }

        // R1 = packet data pointer, R2 = packet length
        // R3 = protocol, R4 = destination port
        cpu_data.regs = EbpfRegs::new();
        cpu_data.regs.set_r(1, packet.as_ptr() as u64);
        cpu_data.regs.set_r(2, packet.len() as u64);
        cpu_data.regs.set_r(3, protocol as u64);
        cpu_data.regs.set_r(4, dst_port as u64);

        // ponytail: uses per-CPU pre-allocated stack, no heap allocation
        let mut vm = EbpfVm::new(&handler.insns, false);
        let result = vm.exec_raw(&mut cpu_data.regs, &mut cpu_data.stack);

        match result {
            0 => {} // Pass — continue to next handler
            1 => return AshAction::Drop,
            2 => {
                // Read reply parameters from R5-R7
                // R5 = destination IP (u32 for IPv4)
                // R6 = destination port
                // R7 = reply data length (must be in packet buffer)
                return AshAction::DropWithReply;
            }
            _ => {} // Unknown — pass
        }
    }

    AshAction::Pass
}

/// Send a UDP reply from within the ASH handler.
/// Uses pre-allocated buffers so it's safe for IRQ context.
// ponytail: stub — needs TX DMA descriptor setup and checksum offload per NIC
pub unsafe fn ash_send_udp(
    _dst_ip: u32,
    _dst_port: u16,
    _src_port: u16,
    _data: &[u8],
) -> bool {
    false
}
