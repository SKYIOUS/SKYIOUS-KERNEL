# SMP Support Design

SkyOS supports symmetric multiprocessing (SMP) across all available CPU cores, with architecture-specific initialization on x86_64.

## BSP and AP Startup

The bootstrap processor (BSP) initializes the kernel and wakes application processors (APs) using the SIPI (Startup IPI) protocol. Each AP executes a small trampoline routine loaded at `0x7000` (below 1 MiB) that transitions it to protected mode, then long mode, and finally jumps to the Rust kernel entry point.

```rust
pub fn wake_aps(bsp: &mut Cpu) {
    let trampoline = include_bytes!("../../boot/trampoline.bin");
    copy_to_low_memory(0x7000, trampoline);
    for ap_id in 1..num_cpus() {
        send_sipi(ap_id, 0x7000);
        wait_for_ap_ready(ap_id);
    }
}
```

## Per-CPU Data

Each CPU has a per-CPU data region accessible via the `GS` segment base. This region contains:
- CPU ID and feature flags
- Local APIC registers
- Per-CPU allocator cache
- Current task pointer
- Interrupt stack pointers

## CPU Hotplug

CPU hotplug is supported through ACPI. When a CPU is hot-added, the kernel allocates per-CPU structures and sends the new CPU a SIPI. Hot-removal requires migrating all tasks off the CPU before taking it offline.

## Load Balancing

The scheduler uses work-stealing to balance load across CPUs. When a CPU's run queue is empty, it attempts to steal tasks from other CPUs' queues, starting with the most loaded CPU.
