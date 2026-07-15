#![allow(dead_code)]
//! AMD-V (SVM) implementation.
//!
//! Provides VMCB management, VMRUN/VMLOAD/VMSAVE wrappers, and NPT
//! (Nested Page Tables) setup.

use core::arch::asm;
use x86_64::PhysAddr;

/// MSRs for SVM.
const MSR_AMD_VM_CR: u32 = 0xC001_0114;
const MSR_AMD_VM_HSAVE_PA: u32 = 0xC001_0117;

/// SVM features from CPUID 0x8000_000A:EDX.
const SVM_FEATURE_NPT: u32 = 1 << 0;
const SVM_FEATURE_LBR_VIRT: u32 = 1 << 1;
const SVM_FEATURE_SVML: u32 = 1 << 2;

/// VMCB state-save area offsets (32-bit, 64-bit).
const VMCB_CR0: u64 = 0x400;
const VMCB_CR2: u64 = 0x408;
const VMCB_CR3: u64 = 0x410;
const VMCB_CR4: u64 = 0x418;
const VMCB_RIP: u64 = 0x478;
const VMCB_RFLAGS: u64 = 0x480;
const VMCB_RAX: u64 = 0x4C0;
const VMCB_RSP: u64 = 0x4D8;
const VMCB_CTL_INTERCEPT: u64 = 0x000;
const VMCB_CTL_NP_ENABLE: u64 = 0x0A0;
const VMCB_CTL_NP_CR3: u64 = 0x0A8;
const VMCB_CTL_EVENTINJ: u64 = 0x098;

/// VMCB (Virtual Machine Control Block) — 4KB aligned.
#[repr(C, align(4096))]
pub struct Vmcb {
    pub data: [u8; 4096],
}

impl Vmcb {
    pub fn new() -> Option<&'static mut Vmcb> {
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
        let frame = crate::memory::buddy::BUDDY_ALLOCATOR.lock()
            .allocate_contiguous(0)?;
        let virt = (frame.as_u64() + offset) as *mut Vmcb;
        // SAFETY: frame is valid and zeroed.
        unsafe {
            core::ptr::write_bytes(virt as *mut u8, 0, 4096);
            Some(&mut *virt)
        }
    }

    pub fn phys_addr(&self) -> Option<u64> {
        let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get()?;
        Some((self as *const Vmcb as u64) - offset)
    }
}

/// AMD-V handler.
pub struct SvmHandler {
    pub vmcb_phys: PhysAddr,
    pub host_save_pa: PhysAddr,
}

impl SvmHandler {
    pub fn new() -> Option<Self> {
        let vmcb = Vmcb::new()?;
        let vmcb_phys = PhysAddr::new(vmcb.phys_addr()?);
        let host_save = Vmcb::new()?;
        let host_save_pa = PhysAddr::new(host_save.phys_addr()?);

        Some(SvmHandler {
            vmcb_phys,
            host_save_pa,
        })
    }

    /// Initialize SVM on the current CPU.
    ///
    /// # Safety
    /// Must be called once per CPU. Requires EFER.SVME to be set.
    pub unsafe fn svm_on(&self) -> bool {
        // 1. Set EFER.SVME (bit 12)
        let efer: u64;
        // SAFETY: RDMSR on EFER.
        unsafe {
            asm!("rdmsr", in("ecx") 0xC000_0080u32, lateout("eax") efer, lateout("edx") _);
        }
        let efer_svme = efer | (1 << 12);
        // SAFETY: WRMSR on EFER.
        unsafe {
            asm!("wrmsr", in("ecx") 0xC000_0080u32, in("eax") efer_svme as u32, in("edx") (efer_svme >> 32) as u32);
        }

        // 2. Write host save area
        let host_save_pa = self.host_save_pa.as_u64();
        // SAFETY: WRMSR for VM_HSAVE_PA.
        unsafe {
            asm!("wrmsr", in("ecx") MSR_AMD_VM_HSAVE_PA,
                in("eax") host_save_pa as u32,
                in("edx") (host_save_pa >> 32) as u32);
        }

        true
    }

    /// Disable SVM on the current CPU.
    ///
    /// # Safety
    /// Must be called after VMRUN completes.
    pub unsafe fn svm_off(&self) {
        let efer: u64;
        // SAFETY: RDMSR on EFER.
        unsafe {
            asm!("rdmsr", in("ecx") 0xC000_0080u32, lateout("eax") efer, lateout("edx") _);
        }
        let efer_clear = efer & !(1 << 12);
        // SAFETY: WRMSR on EFER.
        unsafe {
            asm!("wrmsr", in("ecx") 0xC000_0080u32, in("eax") efer_clear as u32, in("edx") (efer_clear >> 32) as u32);
        }
    }

    /// Configure the VMCB for a guest launch.
    ///
    /// # Safety
    /// vmcb_phys must point to a valid zeroed VMCB.
    pub unsafe fn setup_vmcb(&self, _entry: u64, _rsp: u64) -> bool {
        // ponytail: full VMCB setup with intercept bitmap, NPT enable, and
        // segment state — add when VMRUN is ready to be tested

        // Enable NPT if available
        let npt_enabled = crate::hypervisor::has_npt_capability();
        if npt_enabled {
            vmcb_write_64(self.vmcb_phys.as_u64(), VMCB_CTL_NP_ENABLE, 1);
        }

        // Set intercepts: CPUID, HLT, IO, MSR, CR accesses
        let intercept_cr = 0xFFFF_FFFFu64; // Intercept all CR0-CR15 accesses
        let intercept_dr = 0xFFFF_FFFFu64;
        let intercept_exc = 0;
        // SAFETY: Write intercept sets to VMCB control area.
        unsafe {
            let vmcb = self.vmcb_phys.as_u64();
            core::ptr::write_volatile((vmcb + 0x010) as *mut u64, intercept_cr);   // CR intercepts
            core::ptr::write_volatile((vmcb + 0x018) as *mut u64, intercept_dr);   // DR intercepts
            core::ptr::write_volatile((vmcb + 0x020) as *mut u64, intercept_exc);  // Exception intercepts
        }

        true
    }

    /// Launch the guest using VMRUN.
    ///
    /// # Safety
    /// Requires VMCB to be fully configured.
    pub unsafe fn vmrun(&self, vcpu: &mut crate::hypervisor::vcpu::Vcpu) -> Option<VmExitReason> {
        // VMLOAD loads host state from VMCB
        let vmcb_pa = self.vmcb_phys.as_u64();
        // SAFETY: VMRUN with valid VMCB.
        unsafe {
            asm!("vmload {}", in(reg) vmcb_pa, options(nostack));
            asm!("vmrun {}", in(reg) vmcb_pa, options(nostack));
            asm!("vmsave {}", in(reg) vmcb_pa, options(nostack));
        }

        vcpu.state = crate::hypervisor::vcpu::VcpuState::Running;

        // Read exit reason from VMCB control area
        let exit_code: u64;
        // SAFETY: Read VMCB exit code field.
        unsafe {
            exit_code = core::ptr::read_volatile((vmcb_pa + 0x08C) as *const u64);
        }

        Some(decode_exit_code(exit_code))
    }
}

/// Write a 64-bit value to a VMCB offset.
fn vmcb_write_64(vmcb_pa: u64, offset: u64, value: u64) {
    // SAFETY: vmcb_pa + offset must be within the 4KB VMCB.
    unsafe {
        core::ptr::write_volatile((vmcb_pa + offset) as *mut u64, value);
    }
}

fn vmcb_read_64(vmcb_pa: u64, offset: u64) -> u64 {
    // SAFETY: vmcb_pa + offset must be within the 4KB VMCB.
    unsafe {
        core::ptr::read_volatile((vmcb_pa + offset) as *const u64)
    }
}

/// SVM VM-exit codes (subset).
#[derive(Debug, Clone, Copy)]
pub enum VmExitReason {
    Cpuid,
    Hlt,
    IoInstruction { port: u16, size: u8, write: bool },
    MsrRead,
    MsrWrite,
    Vmcall,
    NptViolation { gpa: u64 },
    Exception { vector: u8 },
    ExternalInterrupt,
    Unknown(u64),
}

fn decode_exit_code(code: u64) -> VmExitReason {
    match code {
        0x72 => VmExitReason::Cpuid,
        0x78 => VmExitReason::Hlt,
        0x7C => VmExitReason::Vmcall,
        0x7B => VmExitReason::IoInstruction { port: 0, size: 1, write: false },
        0x7F => VmExitReason::MsrRead,
        0x80 => VmExitReason::MsrWrite,
        0x400 => VmExitReason::NptViolation { gpa: 0 },
        0x60..=0x6F => VmExitReason::Exception { vector: (code & 0xFF) as u8 },
        0x40 => VmExitReason::ExternalInterrupt,
        _ => VmExitReason::Unknown(code),
    }
}

/// Query SVM features.
pub fn svm_features() -> u32 {
    let edx: u32;
    // SAFETY: CPUID leaf 0x8000_000A.
    unsafe {
        asm!(
            "mov eax, 0x8000000A",
            "xchg rbx, {tmp}",
            "cpuid",
            "xchg rbx, {tmp}",
            tmp = inout(reg) 0u64 => _,
            in("eax") 0x8000_000Au32,
            out("edx") edx,
            out("ecx") _,
        );
    }
    edx
}

/// Check if NPT is supported.
pub fn has_npt() -> bool {
    (svm_features() & SVM_FEATURE_NPT) != 0
}
