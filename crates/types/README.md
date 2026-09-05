# vahi-types

**Foundation crate** — shared types and trait interfaces that break circular
dependencies between kernel modules.

## Purpose

Defines the trait boundaries that allow kernel modules to be extracted into
separate crates without creating circular dependencies. The kernel registers
real implementations during boot; extracted crates call through these traits.

## Key Types

| Type | Used By | Purpose |
|------|---------|---------|
| `Pid`, `Tid`, `FdNum` | task, vfs, net | Linux-compatible IDs |
| `Vma`, `VmFlags`, `VmProt` | task, memory | Virtual memory areas |
| `Credentials` | task, syscalls | Unix UID/GID |
| `FileDescriptor`, `FileOps` | task, vfs | File I/O abstraction |
| `IoVec` | syscalls | Scatter/gather I/O |

## Key Traits

| Trait | Breaks Cycle | Implemented By |
|-------|-------------|----------------|
| `ProcessProvider` | task ↔ memory | kernel task module |
| `PageFaultHandler` | memory ↔ interrupts | kernel memory/paging |
| `ArchOps` | boot ↔ arch | kernel arch module |
| `TimerSource` | interrupts ↔ task | kernel timer |
| `FileOps` | task ↔ vfs | kernel vfs |
| `Driver` | drivers ↔ task | kernel drivers |

## Registration

The kernel wires everything at boot:

```rust
static PROVIDER: KernelProcessProvider = KernelProcessProvider;

fn init() {
    vahi_types::register_process_provider(&PROVIDER);
    vahi_types::register_page_fault_handler(&HANDLER);
    vahi_types::register_arch_ops(&ARCH);
    vahi_types::register_timer_source(&TIMER);
}
```

## Rules

- No kernel dependencies. This crate must never import from `vahi_kernel`.
- Trait definitions only. No implementations.
- `no_std` only. Uses `alloc` for `Vec`/`String`.
- API stability: breaking changes require an ADR.

## Dependencies

- `spin` 0.9 (for `Once` statics)
