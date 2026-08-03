# ADR-001: Monolithic Kernel Architecture

## Status
Accepted

## Date
2026-08-01

## Context
The Vahi kernel needed a fundamental architecture decision: monolithic vs microkernel vs hybrid. Requirements included:
- All core OS services must run in a single address space for performance
- Zero-copy data paths between subsystems (scheduler, memory, VFS, network, GUI)
- No IPC penalty for inter-service communication
- Simple synchronization model within kernel space
- Multi-architecture future (x86_64, aarch64, RISC-V)

## Decision
Use a monolithic kernel architecture with clean internal module boundaries.

All core services (scheduler, memory manager, VFS, network stack, GUI compositor, drivers) run in kernel space with a single page table. Internal boundaries are enforced by Rust's module system and trait abstractions, not by address-space isolation.

## Alternatives Considered

### Microkernel
- Pros: Strong isolation, fault containment, driver crashes don't take down the system
- Cons: IPC overhead for every cross-service call, complex capability passing, performance penalty for high-throughput paths (network, GUI compositing)
- Rejected: Performance requirements for 30fps compositing and zero-copy networking make IPC overhead unacceptable

### Hybrid kernel
- Pros: Some drivers in userspace, core services in kernel
- Cons: Still pays IPC cost for driver communication, more complex than pure monolithic
- Rejected: Adds complexity without sufficient benefit for this project's goals

## Consequences
- Inter-module calls are direct function calls — minimal overhead
- A single fault in any kernel component crashes the system (mitigated by Rust's safety guarantees)
- No need for IPC marshaling between subsystems
- Simpler synchronization: spinlocks and mutexes within a single address space
- Kernel size is ~50K+ lines of Rust — manageable with modular organization
- The `Arch` trait provides a clean abstraction for porting to new architectures
- The `VfsNode` and `FileSystem` traits provide clean module boundaries within the monolith