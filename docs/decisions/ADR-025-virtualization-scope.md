# ADR-025: Virtualization Scope

## Status

**DECISION REQUIRED** — This ADR proposes a virtualization architecture that requires team consensus.

## Context

Vahi already has substantial hypervisor infrastructure:
- **VMX (Intel)** — VMXON, VMCS management, VM entry/exit
- **SVM (AMD)** — VMCB management, VMRUN/VMLOAD/VMSAVE
- **EPT/NPT** — Nested page tables for guest memory
- **vCPU management** — Per-vCPU state, scheduling
- **Device emulation** — UART, PIC, PIT, virtio-net
- **Guest boot protocols** — Linux boot, SkyOS boot
- **Hypercall interface** — Guest-to-host communication
- **Guest memory allocation** — Physical memory mapping

Current syscalls:
- `sys_vm_create(name, mem_size)` — Create a guest VM
- `sys_vm_destroy(guest_id)` — Destroy a guest VM
- `sys_vm_start(guest_id)` — Start a guest VM
- `sys_vm_stop(guest_id)` — Stop a guest VM
- `sys_vm_resume(guest_id)` — Resume a guest VM
- `sys_vm_pause(guest_id)` — Pause a guest VM

The virtualization decision must address:
1. Should virtualization be a separate program or integrated?
2. What is the scope (KVM-level, Xen-level, or minimal)?
3. How does it interact with the host kernel?
4. What guests are supported?

## Decision

**Treat virtualization as a major integrated subsystem, not a separate program.**

### Rationale

1. **Already integrated** — VMX/SVM, EPT/NPT, vCPU scheduling are in the main kernel
2. **Performance** — Integrated hypervisor avoids context-switch overhead
3. **Shared memory** — Guest and host share physical memory directly
4. **Device access** — Host drivers can be exposed to guests via virtio
5. **Vahi-native** — Can provide Vahi-specific virtualization features

### Scope Definition

| Feature | In Scope | Out of Scope |
|---------|----------|--------------|
| VMX/SVM | ✅ | |
| EPT/NPT | ✅ | |
| vCPU abstraction | ✅ | |
| Interrupt virtualization | ✅ | |
| Guest memory | ✅ | |
| VM exits | ✅ | |
| Device model | ✅ (basic) | Full device model |
| Guest boot | ✅ | |
| Live migration | | ❌ |
| Nested virtualization | | ❌ |
| GPU passthrough | | ❌ |
| SR-IOV | | ❌ |

### Architecture

```text
┌─────────────────────────────────────────────┐
│              Guest VM                        │
│  ┌─────────────┐ ┌─────────────┐            │
│  │   vCPU 0    │ │   vCPU 1    │            │
│  └─────────────┘ └─────────────┘            │
│  ┌─────────────┐ ┌─────────────┐            │
│  │  Guest RAM  │ │  VirtIO     │            │
│  └─────────────┘ └─────────────┘            │
└──────────────┬──────────────────────────────┘
               │ VM exit / VM entry
               ▼
┌─────────────────────────────────────────────┐
│           Vahi Hypervisor                    │
│  ┌─────────────────────────────────────┐    │
│  │         VMX / SVM Layer             │    │
│  │  • VMCS / VMCB management           │    │
│  │  • VM entry / exit handling          │    │
│  │  • MSR bitmap                       │    │
│  │  • I/O bitmap                       │    │
│  └─────────────────────────────────────┘    │
│  ┌─────────────────────────────────────┐    │
│  │         EPT / NPT Layer             │    │
│  │  • Guest physical → host physical   │    │
│  │  • Memory mapping                   │    │
│  │  • Dirty page tracking              │    │
│  └─────────────────────────────────────┘    │
│  ┌─────────────────────────────────────┐    │
│  │         Device Model                │    │
│  │  • VirtIO block / net               │    │
│  │  • UART / PIC / PIT                 │    │
│  │  • Hypercall interface              │    │
│  └─────────────────────────────────────┘    │
│  ┌─────────────────────────────────────┐    │
│  │         vCPU Scheduler              │    │
│  │  • Map vCPUs to host threads        │    │
│  │  • Handle VM exits                  │    │
│  │  • Inject interrupts                │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
```

### vCPU Scheduling

```rust
/// vCPU thread — scheduled alongside normal threads.
pub struct VcpuThread {
    /// The vCPU this thread represents.
    pub vcpu: Vcpu,
    /// Host thread context.
    pub host_thread: Thread,
    /// VM exit handler.
    pub exit_handler: Box<dyn VmExitHandler>,
}

impl VcpuThread {
    /// Run the vCPU until next VM exit.
    pub fn run(&mut self) -> VmExitReason {
        // 1. Load guest state from VMCS/VMCB
        // 2. VM entry (VMLAUNCH/VMRUN)
        // 3. VM exit (hardware returns here)
        // 4. Save guest state
        // 5. Return exit reason
        todo!()
    }
}
```

### VM Exit Handling

```rust
pub enum VmExitReason {
    /// External interrupt.
    ExternalInterrupt,
    /// I/O port access.
    IoAccess { port: u16, size: u8, write: bool, data: u64 },
    /// MSR access.
    MsrAccess { msr: u32, write: bool, data: u64 },
    /// CPUID.
    Cpuid { leaf: u32, subleaf: u32 },
    /// HLT.
    Hlt,
    /// Triple fault.
    TripleFault,
    /// EPT violation (memory access).
    EptViolation { guest_phys: u64, write: bool, exec: bool },
}

/// Handle a VM exit.
pub fn handle_vm_exit(vcpu: &mut Vcpu, reason: VmExitReason) -> VmExitAction {
    match reason {
        VmExitReason::IoAccess { port, size, write, data } => {
            // Forward to device model
            DEVICE_MODEL.lock().handle_io(port, size, write, data)
        }
        VmExitReason::MsrAccess { msr, write, data } => {
            // Handle MSR read/write
            todo!()
        }
        VmExitReason::Cpuid { leaf, subleaf } => {
            // Return CPUID results
            todo!()
        }
        VmExitReason::EptViolation { guest_phys, write, exec } => {
            // Handle page fault (demand paging, CoW, etc.)
            todo!()
        }
        _ => VmExitAction::Shutdown,
    }
}
```

### Device Model

```rust
/// Device model — emulates hardware for guests.
pub struct DeviceModel {
    /// VirtIO block devices.
    pub block_devices: Vec<VirtioBlock>,
    /// VirtIO network devices.
    pub net_devices: Vec<VirtioNet>,
    /// UART serial ports.
    pub uart: Vec<UartEmulation>,
    /// PIC (8259A).
    pub pic: PicEmulation,
    /// PIT (8253).
    pub pit: PitEmulation,
}

impl DeviceModel {
    /// Handle I/O port access from guest.
    pub fn handle_io(&mut self, port: u16, size: u8, write: bool, data: u64) -> VmExitAction {
        match port {
            0x01F0..=0x01F7 => self.block_devices[0].handle_io(port, size, write, data),
            0x3F8..=0x3FF => self.uart[0].handle_io(port, size, write, data),
            0x20..=0x21 => self.pic.handle_io(port, size, write, data),
            0x40..=0x43 => self.pit.handle_io(port, size, write, data),
            _ => VmExitAction::Continue,
        }
    }
}
```

### Guest Boot Protocol

```rust
/// Boot a guest VM.
pub fn boot_guest(guest: &mut GuestVm) -> bool {
    // 1. Allocate guest memory
    let mem = GuestMemory::allocate_guest(guest.memory_size)?;
    
    // 2. Load kernel/initrd into guest memory
    match guest.os_type {
        OsType::Linux { kernel_path, initrd_path, cmdline } => {
            boot::linux::load_linux(&mut mem, &kernel_path, &initrd_path, &cmdline)?;
        }
        OsType::SkyOS { entry } => {
            boot::skyos::load_skyos(&mut mem, entry)?;
        }
        OsType::BareMetal { entry } => {
            // Load raw binary at entry point
        }
    }
    
    // 3. Set up EPT/NPT
    let mut ept = EptManager::new();
    for region in &mem.regions {
        ept.map_guest(region.guest_phys, region.host_phys, region.size, EptFlags::READ | EptFlags::WRITE);
    }
    
    // 4. Create vCPUs
    for i in 0..guest.vcpu_count {
        let vcpu = Vcpu::new(i, guest.id);
        guest.vcpus.push(vcpu);
    }
    
    // 5. Start vCPU threads
    for vcpu in &mut guest.vcpus {
        // Spawn host thread for vCPU
        spawn_vcpu_thread(vcpu);
    }
    
    true
}
```

## Consequences

### Positive

1. **Performance** — Integrated hypervisor avoids context-switch overhead
2. **Shared memory** — Guest and host share physical memory directly
3. **Device access** — Host drivers can be exposed to guests via virtio
4. **Vahi-native** — Can provide Vahi-specific virtualization features
5. **Already implemented** — VMX/SVM, EPT/NPT, vCPU are in the codebase

### Negative

1. **Kernel complexity** — Hypervisor code increases kernel size
2. **Security surface** — VM escapes are critical vulnerabilities
3. **Maintenance burden** — Device model requires ongoing maintenance
4. **No live migration** — Cannot move running VMs between hosts
5. **No nested virtualization** — Cannot run VMs inside VMs

### Risks

1. **VM escapes** — Hypervisor bugs can compromise the host
2. **Performance overhead** — VM exits are expensive (1000+ cycles each)
3. **Device emulation bugs** — Incorrect emulation can crash guests
4. **Memory pressure** — Guest memory competes with host memory

## Alternatives Considered

### Alternative 1: Separate Hypervisor Program

**Rejected.** The hypervisor is already integrated. Separating it would require:
- Moving VMX/SVM code to a separate binary
- Adding IPC between host kernel and hypervisor
- Duplicating memory management
- Increasing context-switch overhead

### Alternative 2: Optional Module (Feature-Gated)

**Partially adopted.** Hypervisor code is already feature-gated with `#[cfg(feature = "hypervisor")]`. But the core VMX/SVM code should always be compiled (for security and testing).

### Alternative 3: Xen-Style Split Driver Model

**Rejected.** Xen requires a separate dom0 kernel for device drivers. Vahi's integrated model is simpler and more performant.

## Implementation Plan

1. **Phase 1:** Complete VMX/SVM implementation (VMCS/VMCB setup, VM entry/exit)
2. **Phase 2:** Complete EPT/NPT implementation (guest memory mapping)
3. **Phase 3:** Implement vCPU scheduling (map vCPUs to host threads)
4. **Phase 4:** Implement device model (UART, PIC, PIT, VirtIO)
5. **Phase 5:** Implement guest boot (Linux, SkyOS, bare-metal)
6. **Phase 6:** Implement hypercall interface (guest-to-host communication)
7. **Phase 7:** Implement VM migration (optional, future)

## References

- Linux KVM: `arch/x86/kvm/`
- Intel VMX: Intel SDM Volume 3, Chapter 23
- AMD SVM: AMD64 Architecture Programmer's Manual, Volume 2
- `docs/roadmap-revised/10-virtualization.md` — Virtualization architecture
