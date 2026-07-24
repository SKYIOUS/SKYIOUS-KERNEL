use core::arch::asm;
use x86_64::PhysAddr;

const MSR_IA32_VMX_BASIC: u32 = 0x480;
const MSR_IA32_FEATURE_CONTROL: u32 = 0x3A;
const MSR_IA32_VMX_PINBASED_CTLS: u32 = 0x481;
const MSR_IA32_VMX_PROCBASED_CTLS: u32 = 0x482;
const MSR_IA32_VMX_EXIT_CTLS: u32 = 0x483;
const MSR_IA32_VMX_ENTRY_CTLS: u32 = 0x484;
const MSR_IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;

/// VMCS region (must be 4KB-aligned, physically contiguous).
#[repr(C, align(4096))]
pub struct VmcsRegion {
    pub data: [u8; 4096],
}

impl VmcsRegion {
    pub fn new() -> Option<&'static mut VmcsRegion> {
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let frame = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .allocate_contiguous(0)?;
        let virt = (frame.as_u64() + offset) as *mut VmcsRegion;
        // SAFETY: frame is valid and zeroed, mapped at virt addr.
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, 4096);
            Some(&mut *virt)
        }
    }

    pub fn phys_addr(&self) -> Option<u64> {
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
        Some((self as *const VmcsRegion as u64) - offset)
    }
}

/// VMXON region (must be 4KB-aligned, revision identifier at offset 0).
#[repr(C, align(4096))]
pub struct VmxonRegion {
    pub revision: u32,
    pub data: [u8; 4092],
}

/// VMX handler: manages VMCS regions, MSR bitmaps, and VM entry/exit.
pub struct VmxHandler {
    pub vmcs_region: PhysAddr,
    pub vmm_stack: u64,
    pub msr_bitmap: PhysAddr,
    pub io_bitmap: PhysAddr,
}

/// VM-entry control flags.
pub struct VmxEntryControls {
    pub load_debug: bool,
    pub load_ia32e: bool,
    pub load_perf: bool,
    pub inject_sw: bool,
}

/// VM-execution control flags.
pub struct VmxExecControls {
    pub use_io_bitmap: bool,
    pub use_msr_bitmap: bool,
    pub use_tpr_shadow: bool,
    pub use_secondary: bool,
    pub hlt_exiting: bool,
    pub cr3_load_exiting: bool,
    pub cr3_store_exiting: bool,
    pub cr8_load_exiting: bool,
    pub cr8_store_exiting: bool,
    pub use_vpid: bool,
    pub rdtsc_exiting: bool,
    pub nopage_fault_exiting: bool,
}

impl VmxHandler {
    pub fn new() -> Option<Self> {
        let vmcs = VmcsRegion::new()?;
        let vmcs_phys = PhysAddr::new(vmcs.phys_addr()?);

        let msr_frame = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .allocate_contiguous(0)?;
        let msr_phys = PhysAddr::new(msr_frame.as_u64());
        let io_frame = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .allocate_contiguous(0)?;
        let io_phys = PhysAddr::new(io_frame.as_u64());
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;

        // Zero MSR bitmap
        let msr_virt = msr_phys.as_u64() + offset;
        // SAFETY: allocated frame is mapped.
        unsafe { core::ptr::write_bytes(msr_virt as *mut u8, 0, 4096); }
        let io_virt = io_phys.as_u64() + offset;
        // SAFETY: allocated frame is mapped.
        unsafe { core::ptr::write_bytes(io_virt as *mut u8, 0, 4096); }

        Some(VmxHandler {
            vmcs_region: vmcs_phys,
            vmm_stack: 0,
            msr_bitmap: msr_phys,
            io_bitmap: io_phys,
        })
    }

    /// Configure the VMCS for a guest launch.
    ///
    /// # Safety
    /// Requires VMXON to have been called on this CPU.
    pub unsafe fn create_vmcs(&self, _guest_entry: u64, _guest_rsp: u64) -> bool {
        // VMPTRLD the VMCS region
        let vmcs_pa = self.vmcs_region.as_u64();
        // SAFETY: vmcs_pa points to a valid zeroed VMCS region.
        unsafe {
            asm!("vmptrld [{0}]", in(reg) &vmcs_pa, options(nostack));
        }

        // Write VMCS revision ID
        let basic: u64;
        // SAFETY: RDMSR on VMX_BASIC MSR.
        unsafe {
            asm!("rdmsr", in("ecx") MSR_IA32_VMX_BASIC, lateout("eax") basic, lateout("edx") _);
        }
        let revision = basic as u32;
        // SAFETY: vmcs_pa is mapped, write revision at offset 0.
        unsafe {
            let vmcs_ptr = (vmcs_pa + crate::memory::physical_memory_offset()) as *mut u32;
            vmcs_ptr.write_volatile(revision);
        }

        // Configure VM-execution controls (pin-based)
        // SAFETY: RDMSR for pin-based controls.
        let pin_ctls: u64 = unsafe {
            let (low, high): (u32, u32);
            asm!("rdmsr", in("ecx") MSR_IA32_VMX_PINBASED_CTLS, lateout("eax") low, lateout("edx") high);
            (low as u64) | ((high as u64) << 32)
        };
        // Enable external-interrupt exiting, NMI exiting
        let pin_based = pin_ctls & (1 << 0 | 1 << 3);
        vmwrite(0x4000, pin_based); // PIN_BASED_VM_EXEC_CONTROL

        // Primary processor-based controls
        let proc_ctls: u64 = unsafe {
            let (low, high): (u32, u32);
            asm!("rdmsr", in("ecx") MSR_IA32_VMX_PROCBASED_CTLS, lateout("eax") low, lateout("edx") high);
            (low as u64) | ((high as u64) << 32)
        };
        let primary = proc_ctls
            & (1 << 2   // RDTSC exiting
            | 1 << 7    // HLT exiting
            | 1 << 15   // CPUID exiting
            | 1 << 20   // IO bitmaps
            | 1 << 21   // MSR bitmaps
            | 1 << 28); // Secondary controls
        vmwrite(0x4002, primary); // CPU_BASED_VM_EXEC_CONTROL

        // Secondary processor-based controls
        let proc_ctls2: u64 = unsafe {
            let (low, high): (u32, u32);
            asm!("rdmsr", in("ecx") MSR_IA32_VMX_PROCBASED_CTLS2, lateout("eax") low, lateout("edx") high);
            (low as u64) | ((high as u64) << 32)
        };
        let secondary = proc_ctls2 & (1 << 1); // EPT
        vmwrite(0x401E, secondary); // SECONDARY_VM_EXEC_CONTROL

        // VM-exit controls
        let exit_ctls: u64 = unsafe {
            let (low, high): (u32, u32);
            asm!("rdmsr", in("ecx") MSR_IA32_VMX_EXIT_CTLS, lateout("eax") low, lateout("edx") high);
            (low as u64) | ((high as u64) << 32)
        };
        let exit_based = exit_ctls & (1 << 15); // Long mode (IA-32e) on exit
        vmwrite(0x400C, exit_based); // VM_EXIT_CONTROLS

        // VM-entry controls
        let entry_ctls: u64 = unsafe {
            let (low, high): (u32, u32);
            asm!("rdmsr", in("ecx") MSR_IA32_VMX_ENTRY_CTLS, lateout("eax") low, lateout("edx") high);
            (low as u64) | ((high as u64) << 32)
        };
        let entry_based = entry_ctls & (1 << 9); // IA-32e mode
        vmwrite(0x4012, entry_based); // VM_ENTRY_CONTROLS

        // Set MSR bitmap address
        vmwrite(0x4006, self.msr_bitmap.as_u64()); // MSR_BITMAP
        // Set I/O bitmap addresses
        vmwrite(0x4008, self.io_bitmap.as_u64()); // IO_BITMAP_A
        vmwrite(0x400A, self.io_bitmap.as_u64()); // IO_BITMAP_B

        true
    }

    pub fn launch_vm(&self, vcpu: &mut crate::hypervisor::vcpu::Vcpu) -> Option<VmExitReason> {
        let vmcs_pa = self.vmcs_region.as_u64();
        // SAFETY: VMPTRLD then VMLAUNCH.
        unsafe {
            asm!("vmptrld [{0}]", in(reg) &vmcs_pa, options(nostack));
            let result: u8;
            asm!(
                "vmlaunch",
                "setc al",
                out("al") result,
                options(nostack),
            );
            if result != 0 {
                // Read VM-instruction error
                let error: u64 = vmread(0x4400);
                crate::serial_write(&alloc::format!("[VMX] VMLAUNCH failed: error={}\n", error));
                return None;
            }
        }

        vcpu.state = crate::hypervisor::vcpu::VcpuState::Running;
        let exit_reason = vmread(0x4402); // VM_EXIT_REASON
        Some(decode_exit_reason(exit_reason))
    }

    pub fn handle_vmexit(&self, vcpu: &mut crate::hypervisor::vcpu::Vcpu, reason: VmExitReason) -> bool {
        match reason {
            VmExitReason::EptViolation { gpa } => {
                crate::serial_write(&alloc::format!("[VMX] EPT violation at GPA 0x{:x}\n", gpa));
                // ponytail: handle EPT violation by checking EPT manager
                // add when EPT manager is fully wired
                false
            }
            VmExitReason::IoInstruction { port, size, direction, data: _ } => {
                crate::serial_write(&alloc::format!("[VMX] I/O port 0x{:x} size={} dir={:?}\n", port, size, direction));
                // ponytail: emulate I/O via device model
                // add device dispatch when virtio devices are registered
                true
            }
            VmExitReason::Cpuid => {
                emulate_cpuid(vcpu);
                true
            }
            VmExitReason::MsrRead { msr } | VmExitReason::MsrWrite { msr, value: _ } => {
                emulate_msr(vcpu, msr);
                true
            }
            VmExitReason::Hlt => {
                vcpu.state = crate::hypervisor::vcpu::VcpuState::Halted;
                crate::serial_write("[VMX] Guest HLT\n");
                true
            }
            VmExitReason::ExternalInterrupt => {
                // Resume guest — the interrupt was already handled by host
                true
            }
            VmExitReason::Exception { vector, code } => {
                crate::serial_write(&alloc::format!("[VMX] Guest exception #{} code={}\n", vector, code));
                false
            }
            VmExitReason::VmxCall => {
                let rax = vcpu.regs.rax;
                let args = [vcpu.regs.rdi, vcpu.regs.rsi, vcpu.regs.rdx, vcpu.regs.r10, vcpu.regs.r8, vcpu.regs.r9];
                let result = crate::hypervisor::hypercalls::handle_hypercall(vcpu, rax, args);
                match result {
                    crate::hypervisor::hypercalls::HypercallResult::Success(val) => {
                        vcpu.regs.rax = val;
                    }
                    crate::hypervisor::hypercalls::HypercallResult::Error(e) => {
                        vcpu.regs.rax = e;
                    }
                }
                true
            }
            VmExitReason::TripleFault => {
                crate::serial_write("[VMX] Guest triple fault\n");
                false
            }
            VmExitReason::EptMisconfig { gpa } => {
                crate::serial_write(&alloc::format!("[VMX] EPT misconfig at GPA 0x{:x}\n", gpa));
                false
            }
            VmExitReason::Unknown(code) => {
                crate::serial_write(&alloc::format!("[VMX] Unknown exit reason {}\n", code));
                false
            }
        }
    }
}

fn emulate_cpuid(vcpu: &mut crate::hypervisor::vcpu::Vcpu) {
    let eax: u32;
    let _ecx = vcpu.regs.rcx as u32;
    // SAFETY: CPUID execution.
    unsafe {
        asm!(
            "mov eax, {l}eax",
            "xchg rbx, {tmp}",
            "cpuid",
            "xchg rbx, {tmp}",
            l = in(reg) vcpu.regs.rax,
            tmp = inout(reg) 0u64 => _,
            out("eax") eax,
            out("ecx") _,
            out("edx") _,
        );
    }

    match vcpu.regs.rax {
        1 => {
            // Mask hypervisor bit (bit 31 of ECX)
            vcpu.regs.rax = eax as u64;
            vcpu.regs.rbx = 0;
            vcpu.regs.rcx = 0;
            vcpu.regs.rdx = 0;
        }
        0x4000_0000..=0x4FFF_FFFF => {
            // Hypervisor CPUID leaves — report Vahi
            vcpu.regs.rax = 0x4D_564100; // " VAHI"
            vcpu.regs.rbx = 0x0069_6861_56; // "Vahi\0"
            vcpu.regs.rcx = 0x0001; // Interface version
            vcpu.regs.rdx = 0;
        }
        _ => {}
    }
}

fn emulate_msr(vcpu: &mut crate::hypervisor::vcpu::Vcpu, _msr: u32) {
    // ponytail: pass through most MSRs; emulate TSC, APIC base, EFER
    // add when paravirtualized MSR handling is needed
    vcpu.regs.rax = 0;
    vcpu.regs.rdx = 0;
}

/// Write a VMCS field.
fn vmwrite(field: u64, value: u64) {
    // SAFETY: VMX root operation required.
    unsafe {
        asm!(
            "vmwrite {1}, {0}",
            in(reg) field,
            in(reg) value,
            options(nostack),
        );
    }
}

/// Read a VMCS field.
fn vmread(field: u64) -> u64 {
    let value: u64;
    // SAFETY: VMX root operation required.
    unsafe {
        asm!(
            "vmread {0}, {1}",
            out(reg) value,
            in(reg) field,
            options(nostack, readonly),
        );
    }
    value
}

/// Enable VMX on the current CPU.
///
/// # Safety
/// Must be called on each CPU that will run virtual machines.
pub unsafe fn vmx_on() -> bool {
    let offset = match crate::memory::PHYSICAL_MEMORY_OFFSET.get() {
        Some(o) => *o,
        None => return false,
    };

    // 1. Check/Set VMX lock (IA32_FEATURE_CONTROL MSR)
    let feature_ctl: u64;
    // SAFETY: RDMSR on IA32_FEATURE_CONTROL.
    unsafe {
        asm!("rdmsr", in("ecx") MSR_IA32_FEATURE_CONTROL, lateout("eax") feature_ctl, lateout("edx") _);
    }
    if (feature_ctl & 0x5) != 0x5 {
        // SAFETY: WRMSR to enable VMXON.
        unsafe {
            asm!("wrmsr", in("ecx") MSR_IA32_FEATURE_CONTROL, in("eax") 0x5u32, in("edx") 0u32);
        }
    }

    // 2. Enable VMX in CR4 (bit 13 = VMXE)
    let cr4: u64;
    // SAFETY: Reading CR4.
    unsafe {
        asm!("mov {}, cr4", out(reg) cr4);
    }
    // SAFETY: Setting CR4.VMXE.
    unsafe {
        asm!("mov cr4, {}", in(reg) cr4 | (1 << 13));
    }

    // 3. Allocate VMXON region
    let vmxon_phys = {
        let frame = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .allocate_contiguous(0);
        match frame {
            Some(addr) => addr.as_u64(),
            None => return false,
        }
    };

    // 4. Write VMCS revision ID to VMXON region
    let basic: u64;
    // SAFETY: RDMSR on VMX_BASIC MSR.
    unsafe {
        asm!("rdmsr", in("ecx") MSR_IA32_VMX_BASIC, lateout("eax") basic, lateout("edx") _);
    }
    let revision = basic as u32;
    let vmxon_virt = vmxon_phys + offset;
    // SAFETY: vmxon_virt is the valid virtual mapping of the allocated frame.
    unsafe {
        *(vmxon_virt as *mut u32) = revision;
    }

    // 5. Execute VMXON — operand is a memory reference to the physical address of the VMXON region
    let vmxon_pa = vmxon_phys;
    let result: u8;
    // SAFETY: VMXON requires the VMXON region to be valid and CR4.VMXE set.
    unsafe {
        let vmxon_ptr = &vmxon_pa as *const u64;
        asm!(
            "vmxon [{0}]",
            "setc {1}",
            in(reg) vmxon_ptr,
            out(reg_byte) result,
            options(nostack, preserves_flags),
        );
    }

    result == 0
}

/// Execute VMXOFF on the current CPU.
///
/// # Safety
/// Must be called after a successful vmx_on() on this CPU.
pub unsafe fn vmx_off() {
    // SAFETY: VMXOFF exits VMX root operation.
    unsafe {
        asm!("vmxoff", options(nostack));
    }
}

/// VM-exit reasons decoded from the VMCS exit reason field.
#[derive(Debug, Clone, Copy)]
pub enum VmExitReason {
    Exception { vector: u8, code: u32 },
    ExternalInterrupt,
    TripleFault,
    Cpuid,
    Hlt,
    IoInstruction { port: u16, size: u8, direction: IoDirection, data: u32 },
    MsrRead { msr: u32 },
    MsrWrite { msr: u32, value: u64 },
    EptViolation { gpa: u64 },
    EptMisconfig { gpa: u64 },
    VmxCall,
    Unknown(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDirection {
    In,
    Out,
}

/// Decode a VM-exit reason from the VMCS field (0x4402).
pub fn decode_exit_reason(reason: u64) -> VmExitReason {
    let basic = reason & 0xFFFF;
    match basic {
        0 => VmExitReason::Exception { vector: ((reason >> 8) & 0xFF) as u8, code: ((reason >> 32) & 0xFFFFFFFF) as u32 },
        1 => VmExitReason::ExternalInterrupt,
        2 => VmExitReason::TripleFault,
        10 => VmExitReason::Cpuid,
        12 => VmExitReason::Hlt,
        18 => VmExitReason::VmxCall,
        30 => {
            // I/O instruction
            let port = ((reason >> 24) & 0xFFFF) as u16;
            let size = ((reason >> 40) & 0x7) as u8 + 1;
            let dir = if (reason & (1 << 20)) != 0 { IoDirection::Out } else { IoDirection::In };
            VmExitReason::IoInstruction { port, size, direction: dir, data: 0 }
        }
        31 => {
            // MSR read — ponytail: extract MSR from VMCS
            VmExitReason::MsrRead { msr: 0 }
        }
        32 => {
            // MSR write
            VmExitReason::MsrWrite { msr: 0, value: 0 }
        }
        48 => VmExitReason::EptViolation { gpa: 0 },
        49 => VmExitReason::EptMisconfig { gpa: 0 },
        _ => VmExitReason::Unknown(reason),
    }
}

/// Get EPT violation guest physical address from VMCS.
pub fn get_ept_gpa() -> u64 {
    vmread(0x2400) // GUEST_PHYSICAL_ADDRESS
}

/// Get I/O instruction information from VMCS.
pub fn get_io_info() -> (u16, u8, IoDirection) {
    let info = vmread(0x6402); // IO_INSNS_INFO
    let port = ((info >> 24) & 0xFFFF) as u16;
    let size = ((info >> 40) & 0x7) as u8 + 1;
    let dir = if (info & (1 << 20)) != 0 { IoDirection::Out } else { IoDirection::In };
    (port, size, dir)
}
