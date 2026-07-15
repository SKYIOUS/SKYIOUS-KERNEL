//! VCPU lifecycle and register management.

use x86_64::PhysAddr;

/// VCPU register state.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct VcpuRegs {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub cs_sel: u16,
    pub ds_sel: u16,
    pub es_sel: u16,
    pub ss_sel: u16,
    pub gdtr: u64,
    pub idtr: u64,
    pub efer: u64,
    pub fs_base: u64,
    pub gs_base: u64,
    pub kernel_gs_base: u64,
}

impl VcpuRegs {
    pub fn new() -> Self {
        VcpuRegs {
            rax: 0, rbx: 0, rcx: 0, rdx: 0,
            rsi: 0, rdi: 0, rbp: 0, rsp: 0,
            r8: 0, r9: 0, r10: 0, r11: 0,
            r12: 0, r13: 0, r14: 0, r15: 0,
            rip: 0, rflags: 0x2, // reserved bit 1 always set
            cr0: 0x80000001, cr2: 0, cr3: 0, cr4: 0x2000,
            cs_sel: 0x10, ds_sel: 0x18, es_sel: 0x18, ss_sel: 0x18,
            gdtr: 0, idtr: 0, efer: 0,
            fs_base: 0, gs_base: 0, kernel_gs_base: 0,
        }
    }
}

/// VCPU state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpuState {
    Running,
    Halted,
    Blocked,
    Sipi,
    Stopped,
}

/// A virtual CPU.
pub struct Vcpu {
    pub id: u32,
    pub guest_id: u64,
    pub state: VcpuState,
    pub regs: VcpuRegs,
    pub vmcs_phys: PhysAddr,
    pub stack: u64,
    pub tsc_offset: u64,
    /// Number of VM-exits since last launch
    pub exit_count: u64,
}

impl Vcpu {
    pub fn new(id: u32, guest_id: u64) -> Self {
        Vcpu {
            id,
            guest_id,
            state: VcpuState::Stopped,
            regs: VcpuRegs::new(),
            vmcs_phys: PhysAddr::new(0),
            stack: 0,
            tsc_offset: 0,
            exit_count: 0,
        }
    }

    /// Run the VCPU. Returns the VM-exit reason.
    /// On Intel: VMLAUNCH/VMRESUME. On AMD: VMRUN.
    pub fn run(&mut self) -> Option<crate::hypervisor::vmx::VmExitReason> {
        #[cfg(target_arch = "x86_64")]
        {
            if let Some(ref hv) = *crate::hypervisor::HYPERVISOR.lock() {
                if hv.hardware_cap.has_vmx {
                    let handler = crate::hypervisor::vmx::VmxHandler::new()?;
                    // SAFETY: create_vmcs is unsafe because it touches VMXON/VMCS regions.
                    unsafe { handler.create_vmcs(self.regs.rip, self.regs.rsp); }
                    let reason = handler.launch_vm(self)?;
                    self.exit_count += 1;

                    let handled = handler.handle_vmexit(self, reason);
                    if !handled {
                        self.state = VcpuState::Stopped;
                    }
                    return Some(reason);
                }
            }
        }
        None
    }

    /// Inject an interrupt into the guest.
    /// On Intel: set VM-entry interruption-information field.
    /// On AMD: set VMCB event injection field.
    pub fn inject_interrupt(&mut self, vector: u8) -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            // Write to VM-entry interruption-information field (0x4016)
            let info = (vector as u64) | (0x0 << 8) | (0x0 << 11); // type=external, valid
            // SAFETY: VMX root operation.
            unsafe {
                core::arch::asm!(
                    "vmwrite {0}, {1}",
                    in(reg) 0x4016u64,
                    in(reg) info,
                    options(nostack),
                );
            }
            true
        }
        #[cfg(not(target_arch = "x86_64"))]
        { false }
    }

    /// Set initial register state for a Linux boot.
    pub fn setup_linux_boot(&mut self, entry: u64, dtb: u64, cmdline: u64) {
        self.regs.rip = entry;
        self.regs.rsp = 0x8000; // Temporary stack for boot setup
        self.regs.rsi = dtb;    // x86: rsi = DTB; aarch64: x0 = DTB
        self.regs.rdi = cmdline; // x86: rdi = cmdline
        self.regs.cr0 = 0x80000001; // Enable paging + protected mode
        self.regs.cr3 = 0;  // ponytail: set by boot protocol
        self.regs.cr4 = 0x2000 | (1 << 5); // PAE + PGE
        self.regs.efer = 0x500; // LME + LMA (long mode)
        self.regs.rflags = 0x2;
    }
}
