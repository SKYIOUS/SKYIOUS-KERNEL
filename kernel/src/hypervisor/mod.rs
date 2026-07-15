#[cfg(target_arch = "x86_64")]
pub mod vmx;
#[cfg(target_arch = "x86_64")]
pub mod svm;
#[cfg(target_arch = "x86_64")]
pub mod ept;
pub mod vmm;
pub mod guest;
pub mod vcpu;
pub mod memory;
pub mod hypercalls;
pub mod sched;
pub mod devices;
pub mod boot;

use core::sync::atomic::AtomicBool;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

/// Check if virtualization is supported on this CPU.
pub fn is_virtualization_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let ecx: u32;
        // SAFETY: CPUID leaf 1 is always supported on x86_64.
        unsafe {
            core::arch::asm!(
                "mov eax, 1",
                "xchg rbx, {tmp}",
                "cpuid",
                "xchg rbx, {tmp}",
                tmp = inout(reg) 0u64 => _,
                in("eax") 1u32,
                out("ecx") ecx,
                out("edx") _,
            );
        }
        (ecx & (1 << 5)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

/// Global hypervisor state.
pub static HYPERVISOR_ENABLED: AtomicBool = AtomicBool::new(false);

/// Hypervisor singleton.
pub static HYPERVISOR: Mutex<Option<Hypervisor>> = Mutex::new(None);

pub struct Hypervisor {
    pub enabled: bool,
    pub vcpu_count: u32,
    pub guests: BTreeMap<u64, GuestVm>,
    pub hardware_cap: HwCapabilities,
}

pub struct HwCapabilities {
    pub has_vmx: bool,
    pub has_svm: bool,
    pub has_ept: bool,
    pub has_npt: bool,
    pub has_vpid: bool,
    pub max_vcpus: u32,
    pub iommu_present: bool,
}

pub struct GuestVm {
    pub id: u64,
    pub name: String,
    pub vcpus: Vec<vcpu::Vcpu>,
    pub memory_regions: Vec<memory::MemoryRegion>,
    pub devices: Vec<Box<dyn devices::VirtDevice>>,
    pub state: VmState,
    pub os_type: OsType,
}

#[derive(PartialEq)]
pub enum VmState {
    Created,
    Running,
    Paused,
    Stopped,
    Crashed(GuestCrashInfo),
}

#[derive(PartialEq)]
pub struct GuestCrashInfo {
    pub reason: String,
    pub vcpu_id: u32,
    pub exit_reason: u64,
    pub guest_rip: u64,
}

pub enum OsType {
    Linux { kernel: u64, initrd: u64, cmdline: String },
    Windows { kernel: u64 },
    SkyOS { bootinfo: u64 },
    BareMetal { entry: u64 },
}

/// Probe hardware virtualization capabilities via CPUID.
pub fn probe_hardware_caps() -> HwCapabilities {
    HwCapabilities {
        has_vmx: is_vm_supported(),
        has_svm: is_svm_supported(),
        has_ept: has_ept_capability(),
        has_npt: has_npt_capability(),
        has_vpid: has_vpid_capability(),
        max_vcpus: 64,
        iommu_present: false,
    }
}

fn is_vm_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let ecx: u32;
        // SAFETY: CPUID leaf 1.
        unsafe {
            core::arch::asm!(
                "mov eax, 1",
                "xchg rbx, {tmp}",
                "cpuid",
                "xchg rbx, {tmp}",
                tmp = inout(reg) 0u64 => _,
                in("eax") 1u32,
                out("ecx") ecx,
                out("edx") _,
            );
        }
        (ecx & (1 << 5)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

fn is_svm_supported() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let ecx: u32;
        // SAFETY: CPUID leaf 0x8000_0001.
        unsafe {
            core::arch::asm!(
                "mov eax, 0x80000001",
                "xchg rbx, {tmp}",
                "cpuid",
                "xchg rbx, {tmp}",
                tmp = inout(reg) 0u64 => _,
                in("eax") 0x8000_0001u32,
                out("ecx") ecx,
                out("edx") _,
            );
        }
        (ecx & (1 << 2)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

fn has_ept_capability() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let msr_val: u64;
        // SAFETY: IA32_VMX_PROCBASED_CTLS2 MSR (0x48B).
        unsafe {
            core::arch::asm!(
                "rdmsr",
                in("ecx") 0x48Bu32,
                lateout("eax") msr_val,
                lateout("edx") _,
            );
        }
        (msr_val & (1 << 1)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

fn has_npt_capability() -> bool {
    // NPT on AMD = SVM feature flag bit (CPUID 0x8000_000A:EDX[0])
    #[cfg(target_arch = "x86_64")]
    {
        let edx: u32;
        // SAFETY: CPUID leaf 0x8000_000A (SVM revision/features).
        unsafe {
            core::arch::asm!(
                "mov eax, 0x8000000A",
                "xchg rbx, {tmp}",
                "cpuid",
                "xchg rbx, {tmp}",
                tmp = inout(reg) 0u64 => _,
                in("eax") 0x8000_000Au32,
                out("ecx") _,
                out("edx") edx,
            );
        }
        (edx & 1) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

fn has_vpid_capability() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        let msr_val: u64;
        // SAFETY: IA32_VMX_PROCBASED_CTLS2 MSR (0x48B), VPID = bit 5.
        unsafe {
            core::arch::asm!(
                "rdmsr",
                in("ecx") 0x48Bu32,
                lateout("eax") msr_val,
                lateout("edx") _,
            );
        }
        (msr_val & (1 << 5)) != 0
    }
    #[cfg(not(target_arch = "x86_64"))]
    { false }
}

/// Initialize the hypervisor (probe capabilities, enable VMX/SVM on BSP).
pub fn init() -> bool {
    if !is_virtualization_available() {
        crate::println!("Hypervisor: VMX/SVM not available on this CPU");
        return false;
    }

    let caps = probe_hardware_caps();
    crate::serial_write("[HYPERVISOR] Hardware capabilities detected\n");

    if caps.has_vmx {
        crate::serial_write("[HYPERVISOR] Intel VT-x available\n");
    }
    if caps.has_svm {
        crate::serial_write("[HYPERVISOR] AMD-V available\n");
    }
    if caps.has_ept {
        crate::serial_write("[HYPERVISOR] EPT (nested paging) available\n");
    }
    if caps.has_npt {
        crate::serial_write("[HYPERVISOR] NPT (nested paging) available\n");
    }
    if caps.has_vpid {
        crate::serial_write("[HYPERVISOR] VPID available\n");
    }

    #[cfg(target_arch = "x86_64")]
    {
        if caps.has_vmx {
            // SAFETY: VMXON on BSP — single CPU at boot time.
            if unsafe { vmx::vmx_on() } {
                crate::serial_write("[HYPERVISOR] VMXON successful on BSP\n");
            } else {
                crate::serial_write("[HYPERVISOR] VMXON failed\n");
                return false;
            }
        }
    }

    *HYPERVISOR.lock() = Some(Hypervisor {
        enabled: true,
        vcpu_count: 0,
        guests: BTreeMap::new(),
        hardware_cap: caps,
    });

    HYPERVISOR_ENABLED.store(true, core::sync::atomic::Ordering::SeqCst);
    crate::println!("Hypervisor: initialized");
    true
}

/// Create a new guest VM.
pub fn create_guest(name: &str, os_type: OsType, mem_size: usize) -> Option<u64> {
    let mut hv = HYPERVISOR.lock();
    let hv = hv.as_mut()?;
    if !hv.enabled {
        return None;
    }

    let guest_id = hv.guests.len() as u64;

    let _guest_memory = memory::GuestMemory::allocate_guest(mem_size)?;
    let mut memory_regions = Vec::new();
    for region in &_guest_memory.regions {
        memory_regions.push(region.clone());
    }

    let vcpu0 = vcpu::Vcpu::new(0, guest_id);
    let vcpus = alloc::vec![vcpu0];

    // ponytail: single VCPU per guest; SMP guest support requires IPI forwarding
    let vm = GuestVm {
        id: guest_id,
        name: alloc::string::String::from(name),
        vcpus,
        memory_regions,
        devices: Vec::new(),
        state: VmState::Created,
        os_type,
    };

    hv.guests.insert(guest_id, vm);
    hv.vcpu_count += 1;

    Some(guest_id)
}

/// Destroy a guest VM and free all resources.
pub fn destroy_guest(guest_id: u64) -> bool {
    let mut hv = HYPERVISOR.lock();
    if let Some(hv) = hv.as_mut() {
        if hv.guests.remove(&guest_id).is_some() {
            hv.vcpu_count = hv.vcpu_count.saturating_sub(1);
            return true;
        }
    }
    false
}
