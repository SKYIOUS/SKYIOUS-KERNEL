use alloc::string::String;
use alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;
#[cfg(target_arch = "x86_64")]
use crate::hypervisor::vmx::VmcsRegion;
#[cfg(target_arch = "x86_64")]
use crate::hypervisor::ept::EptPml4;

/// A virtual machine instance.
pub struct VirtualMachine {
    pub id: u64,
    pub name: String,
    pub state: VmState,
    #[cfg(target_arch = "x86_64")]
    pub vmcs: Option<&'static mut VmcsRegion>,
    #[cfg(target_arch = "x86_64")]
    pub ept: Option<&'static mut EptPml4>,
    #[cfg(not(target_arch = "x86_64"))]
    pub _vmcs: *mut u8,
    #[cfg(not(target_arch = "x86_64"))]
    pub _ept: *mut u8,
    pub memory_size: usize,
    pub cpu_count: usize,
    pub entry_point: u64,
    pub stack_ptr: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    PoweredOff,
    Running,
    Paused,
    Crashed,
}

impl VirtualMachine {
    pub fn new(id: u64, name: &str, memory_mb: usize) -> Option<Arc<Mutex<Self>>> {
        #[cfg(not(target_arch = "x86_64"))]
        let vm = Arc::new(Mutex::new(VirtualMachine {
            id,
            name: String::from(name),
            state: VmState::PoweredOff,
            _vmcs: core::ptr::null_mut(),
            _ept: core::ptr::null_mut(),
            memory_size: memory_mb * 1024 * 1024,
            cpu_count: 1,
            entry_point: 0,
            stack_ptr: 0,
        }));
        #[cfg(target_arch = "x86_64")]
        let vm = Arc::new(Mutex::new(VirtualMachine {
            id,
            name: String::from(name),
            state: VmState::PoweredOff,
            vmcs: None,
            ept: None,
            memory_size: memory_mb * 1024 * 1024,
            cpu_count: 1,
            entry_point: 0,
            stack_ptr: 0,
        }));
        Some(vm)
    }

    /// Boot the virtual machine: allocate EPT, set up VMCS, launch.
    #[cfg(target_arch = "x86_64")]
    pub fn boot(&mut self) -> bool {
        if self.state != VmState::PoweredOff { return false; }

        let ept = match EptPml4::new() {
            Some(e) => e,
            None => { self.state = VmState::Crashed; return false; }
        };
        self.ept = Some(ept);

        let vmcs = match VmcsRegion::new() {
            Some(v) => v,
            None => { self.state = VmState::Crashed; return false; }
        };
        self.vmcs = Some(vmcs);

        self.state = VmState::Running;
        // SAFETY: VMCS and EPT are allocated and valid.
        if unsafe { launch_vm() } {
            crate::println!("VMM: Guest '{}' booted", self.name);
            true
        } else {
            self.state = VmState::Crashed;
            false
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    pub fn boot(&mut self) -> bool {
        false
    }

    pub fn pause(&mut self) { self.state = VmState::Paused; }
    pub fn resume(&mut self) { self.state = VmState::Running; }
    pub fn shutdown(&mut self) { self.state = VmState::PoweredOff; }
}

/// Execute VMLAUNCH to start guest execution.
///
/// # Safety
/// Requires a valid VMCS loaded with VMPTRLD and EPT configured.
#[cfg(target_arch = "x86_64")]
unsafe fn launch_vm() -> bool {
    let result: u8;
    // SAFETY: VMLAUNCH requires VMCS to be set up by caller.
    unsafe {
        core::arch::asm!(
            "vmlaunch",
            "setc al",
            out("al") result,
            options(nostack),
        );
    }
    result == 0
}

/// Handle a VM exit from a guest VM.
/// Called from the VM exit handler.
pub fn handle_vmexit(_vm: &Arc<Mutex<VirtualMachine>>, exit_reason: u64) {
    match exit_reason {
        0 => { /* Exception or NMI */ }
        1 => { /* External interrupt */ }
        10 => { /* CPUID */ }
        12 => { /* HLT */ }
        18 => { /* VMCALL */ }
        36 => { /* MOV to CR3 */ }
        48 => { /* EPT violation */ }
        _ => {
            crate::println!("VMM: Unhandled VM exit reason {}", exit_reason);
        }
    }
}
