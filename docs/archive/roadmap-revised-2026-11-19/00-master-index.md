# Vahi Kernel — Revised Roadmap

**Architecture-First, Code-Verified, Dependency-Ordered**

## Phase Completion Status

| Phase | Goal | Status |
|-------|------|--------|
| Phase 0: Foundation | tmpfs, SMAP/SMEP, KASLR, CI, fork | ✅ COMPLETE |
| Phase 1: Userspace Bootstrap | ELF loading, exec, shell, 163-binary initrd | ✅ COMPLETE |
| Phase 2: POSIX/Linux Compat | epoll, writev/readv, madvise, procfs, pread/pwrite | ✅ COMPLETE |
| Phase 3: Persistent Storage | SkyFS journaling, FUSE bridge, ext2 write | ✅ COMPLETE |
| Phase 4: Networking | TCP/UDP, IPv6, socket options, network ioctl | ✅ COMPLETE |
| Phase 5: Security | seccomp BPF, Landlock LSM, prctl | ✅ COMPLETE |
| Phase 6: Containers | Namespaces (PID/Mount/Net/IPC/UTS/User), cgroup v2 | ✅ COMPLETE |
| Phase 7: Virtualization | VMX/SVM, EPT/NPT, vCPU, launch_vm | ✅ COMPLETE |
| Phase 8: Performance + SMP | eBPF JIT, RCU, scheduler RT, CPU affinity | ✅ COMPLETE |
| Phase 9: Vahi-Native | ASH JIT, Vahi IPC, capability model, port IPC, job objects | ✅ COMPLETE |

**All 10 phases complete.** Phases 0-9 are fully implemented and verified against code.

## Remaining Future Work

### Phase 10: Network Optimization (P3)
- Zero-copy sends, scatter-gather I/O
- RSS (Receive Side Scaling)
- TCP segmentation offload

### Phase 11: Block I/O Scheduler (P3)
- mq-deadline or BFQ implementation
- Per-device queues, priority classes

### Phase 12: Advanced Features (P3)
- CFI (Control Flow Integrity)
- Stress tests and fuzzing
- EEVDF scheduler migration

## Supporting Documents

| File | Purpose |
|------|---------|
| `00-executive-architecture.md` | Architecture overview |
| `01-current-state.md` | Verified against code |
| `08-performance-smp.md` | Phase 8 details |
| `11-vahi-native.md` | Phase 9 details |
| `12-testing-strategy.md` | Layered testing approach |
| `13-ci-cd.md` | CI/CD pipeline |
| `14-milestone-matrix.md` | Measurable completion criteria |
| `15-technical-debt.md` | Verified debt inventory |
| `16-adr-index.md` | Architectural decision records |
| `17-definition-of-done.md` | Completion criteria |

## Dependency Graph

```text
Phase 0-7: ✅ COMPLETE
    ↓
Phase 8: Performance + SMP ✅ COMPLETE
    ↓
Phase 9: Vahi-Native ✅ COMPLETE
    ↓
Phase 10-12: Future optimizations
```

## Key ADRs

| ADR | Decision | Status |
|-----|----------|--------|
| ADR-001 | Monolithic kernel | Accepted |
| ADR-005 | Stride scheduling (migrate to EEVDF when needed) | Accepted |
| ADR-010 | Syscalls decomposition | Accepted |
| ADR-019 | Kernel object model: Structured rights + credential snapshots | Implemented |
| ADR-020 | Landlock: Linux-compatible ABI with Vahi extensions | Implemented |
| ADR-024 | Containers: Isolation primitives only, no Docker compat | Implemented |
| ADR-025 | Virtualization: Integrated subsystem, already in kernel | Implemented |
