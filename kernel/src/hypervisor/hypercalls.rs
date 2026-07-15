//! Hypercall interface.
//!
//! Guests use VMCALL (Intel) or VMMCALL (AMD) to request services from
//! the Vahi hypervisor. The hypercall number is passed in RAX, with up
//! to six arguments in RDI, RSI, RDX, R10, R8, R9 (matching the Linux
//! syscall convention). The result is returned in RAX.

use crate::hypervisor::vcpu::{Vcpu, VcpuState};

/// Hypercall numbers.
pub enum Hypercall {
    Halt = 0,
    Shutdown = 1,
    ConsoleWrite = 2,
    ConsoleRead = 3,
    AllocateMemory = 4,
    MapDevice = 5,
    VmCall = 6,
    NotifyEvent = 7,
    GetTime = 8,
    SetTscOffset = 9,
    DebugOutput = 10,
}

impl Hypercall {
    pub fn from_u64(n: u64) -> Option<Self> {
        match n {
            0 => Some(Hypercall::Halt),
            1 => Some(Hypercall::Shutdown),
            2 => Some(Hypercall::ConsoleWrite),
            3 => Some(Hypercall::ConsoleRead),
            4 => Some(Hypercall::AllocateMemory),
            5 => Some(Hypercall::MapDevice),
            6 => Some(Hypercall::VmCall),
            7 => Some(Hypercall::NotifyEvent),
            8 => Some(Hypercall::GetTime),
            9 => Some(Hypercall::SetTscOffset),
            10 => Some(Hypercall::DebugOutput),
            _ => None,
        }
    }
}

/// Result of a hypercall.
pub enum HypercallResult {
    Success(u64),
    Error(u64),
}

/// Handle a hypercall from a guest VM.
pub fn handle_hypercall(vcpu: &mut Vcpu, num: u64, args: [u64; 6]) -> HypercallResult {
    match Hypercall::from_u64(num) {
        Some(Hypercall::Halt) => {
            vcpu.state = VcpuState::Halted;
            HypercallResult::Success(0)
        }
        Some(Hypercall::Shutdown) => {
            // Signal guest shutdown to the VM manager
            crate::serial_write("[HYPERCALL] Guest requested shutdown\n");
            vcpu.state = VcpuState::Stopped;
            HypercallResult::Success(0)
        }
        Some(Hypercall::ConsoleWrite) => {
            // arg0 = string address in guest space, arg1 = length
            let _addr = args[0];
            let _len = args[1] as usize;
            // ponytail: translate guest address and write to host serial
            // add when guest address translation is wired
            HypercallResult::Success(0)
        }
        Some(Hypercall::ConsoleRead) => {
            // arg0 = buffer address, arg1 = max length
            HypercallResult::Success(0)
        }
        Some(Hypercall::AllocateMemory) => {
            // arg0 = size. Return host physical address.
            let _size = args[0] as usize;
            if let Some(frame) = crate::memory::buddy::BUDDY_ALLOCATOR.lock().allocate_contiguous(0) {
                HypercallResult::Success(frame.as_u64())
            } else {
                HypercallResult::Error(1)
            }
        }
        Some(Hypercall::MapDevice) => {
            // arg0 = device MMIO base, arg1 = size
            let _mmio_base = args[0];
            let _size = args[1];
            // ponytail: map device into guest EPT
            // add when EPT manager accepts dynamic mappings
            HypercallResult::Success(0)
        }
        Some(Hypercall::VmCall) => {
            // Inter-guest communication: arg0 = target guest ID, arg1 = message
            let _target_guest = args[0];
            let _message = args[1];
            // ponytail: deliver message to target guest via shared memory
            // add when inter-guest channels exist
            HypercallResult::Success(0)
        }
        Some(Hypercall::NotifyEvent) => {
            // arg0 = event ID, arg1 = value
            let _event_id = args[0];
            let _value = args[1];
            // ponytail: event signaling to host or other guests
            HypercallResult::Success(0)
        }
        Some(Hypercall::GetTime) => {
            let ticks = crate::interrupts::get_ticks();
            HypercallResult::Success(ticks)
        }
        Some(Hypercall::SetTscOffset) => {
            vcpu.tsc_offset = args[0];
            HypercallResult::Success(0)
        }
        Some(Hypercall::DebugOutput) => {
            let _val = args[0];
            crate::serial_write(&alloc::format!("[GUEST-DEBUG] {}\n", _val));
            HypercallResult::Success(0)
        }
        None => {
            crate::serial_write(&alloc::format!("[HYPERCALL] Unknown hypercall {}\n", num));
            HypercallResult::Error(0xFF)
        }
    }
}
