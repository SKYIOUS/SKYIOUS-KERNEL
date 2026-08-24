# Vahi Kernel — Strategic Roadmap to Linux Parity

> **Historical planning artifact.** This document may contain superseded scope, estimates, or implementation-status claims. Use `docs/roadmap-revised/00-master-index.md` and `docs/decisions/` for current planning and decisions.

## 1. Current State Grade Card

| Subsystem | Grade | Lines | Status | Critical Gap |
|-----------|-------|-------|--------|--------------|
| **Scheduler** | B+ | 913 | Stride heap + work stealing | No CFS, no RT classes, no CPU affinity |
| **Memory** | B | 1,855 | Buddy + COW + swap | No slab, no huge pages, no KASLR, no SMAP enforcement |
| **VFS** | B- | 5,451 | SkyFS + devfs + ext2/4 | No tmpfs, no pipefs, no procfs, no page cache, no journaling |
| **Syscalls** | B+ | 9,419 | ~150 implemented | No epoll, no io_uring, no clone3, no memfd |
| **Drivers** | B | 7,257 | VirtIO + AHCI + NVMe + XHCI | No AHCI NCQ, no USB mass storage, no USB HID parser |
| **Signals** | B | ~500 | sigaction/kill/signalfd4/timerfd | Full signalfd_siginfo, centralized routing to signalfd |
| **Networking** | C+ | 802 | TCP/UDP via smoltcp | No raw sockets, no IPv6 routing, no netfilter |
| **ASH/eBPF** | B- | 1,304 | Verifier + interpreter | No JIT, no map types, no CO-RE |
| **Hypervisor** | D | 2,643 | Stubs only | No VMCS/VMCB, no EPT/NPT, no VM exit handling |
| **Objects** | C | 1,002 | SocketObject + HandleTable | No unified object model, no security descriptors |
| **IPC** | B- | 573 | Pipe + SysV SHM + eventfd | No POSIX MQ, no semaphores, no futex robust list |
| **Tests** | C+ | ~500 | 92 selftests | No userspace test suite, no stress tests, no fuzzing |
| **Security** | C | ~200 | Capabilities + SMAP | No KASLR, no CFI, no sandbox, no audit |

**Overall: B- (functional but not production-ready)**

---

## 2. The 5 Blockers to Linux Parity

These are the things that prevent Vahi from being taken seriously as a general-purpose kernel:

### Blocker 1: No Userspace ABI Stability
- `fork()` is broken for complex programs
- `execve()` doesn't handle dynamic linking
- No `ld.so` support — can't run musl/glibc binaries
- **Fix:** Stabilize fork+exec, implement ELF loader with dynamic linking, port musl

### Blocker 2: No Production Filesystem
- SkyFS has no journaling (data loss on crash)
- No page cache (every read goes to disk)
- No tmpfs (can't boot without tmpfs)
- ext4 is partial (read-only, no journal replay)
- **Fix:** Add journaling to SkyFS, implement page cache, add tmpfs

### Blocker 3: No Memory Safety Enforcement
- SMAP/SMEP not enforced in page fault handler
- No KASLR (kernel base is predictable)
- No CFI (return-oriented programming possible)
- **Fix:** Enforce SMAP/SMEP, add KASLR, add shadow call stack

### Blocker 4: No Production Scheduler
- Stride scheduling is fair but not optimal
- No real-time classes (SCHED_FIFO/SCHED_RR)
- No CPU affinity (can't pin threads to cores)
- No load balancing (work stealing is reactive, not proactive)
- **Fix:** Add RT classes, CPU affinity, periodic load balancing

### Blocker 5: Incomplete POSIX Compliance
- No `epoll` (only select/poll)
- No `io_uring` (only stubs)
- No `clone3` with namespaces
- No `procfs`/`sysfs` (no introspection)
- **Fix:** Implement epoll, complete io_uring, add procfs

---

## 3. Phased Roadmap

### Phase 0: Foundation (Months 0–2)
**Goal:** Every existing feature works correctly. Zero regressions.

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| Fix fork() for complex programs | P0 | 2 weeks | Unblocks userspace |
| Fix execve() ELF loading | P0 | 2 weeks | Unblocks userspace |
| Add tmpfs | P0 | 1 week | Required for /tmp, /run |
| Add page cache for VFS reads | P0 | 2 weeks | 10x read performance |
| Enforce SMAP/SMEP in page fault | P0 | 1 week | Security baseline |
| Add KASLR | P1 | 1 week | Security baseline |
| Fix all clippy warnings | P1 | 1 week | Code quality |
| Add CI with QEMU boot test | P0 | 1 week | Prevent regressions |

**Exit criteria:** Boot to shell, run `ls`, `cat`, `echo`, `mkdir` without crashes.

### Phase 1: POSIX Compliance (Months 2–6)
**Goal:** Run real userspace programs (musl-compiled).

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| Port musl libc to Vahi | P0 | 4 weeks | Unblocks all userspace |
| Implement `epoll` | P0 | 2 weeks | Required for nginx, redis |
| Implement `io_uring` | P1 | 4 weeks | Modern async I/O |
| Add `procfs` (minimal) | P1 | 2 weeks | /proc/pid/status, /proc/meminfo |
| Add `sysfs` (minimal) | P2 | 2 weeks | /sys/block, /sys/class |
| Implement `clone3` + namespaces | P2 | 4 weeks | Container foundation |
| Add `pipefs` | P1 | 1 week | Proper pipe implementation |
| Add `socketfs` | P2 | 1 week | Unix socket namespace |
| Implement POSIX MQ | P2 | 2 weeks | IPC completeness |
| Implement `timerfd` | P2 | 1 week | Timer completeness |
| Implement `signalfd4` | P1 | 1 week | Signal completeness |
| Add `memfd_create` | P2 | 1 week | Shared memory |

**Exit criteria:** Boot musl-linked shell, run basic utilities (ls, cat, grep, find).

### Phase 2: Filesystem Robustness (Months 6–9)
**Goal:** Data integrity under crash conditions.

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| Add journaling to SkyFS | P0 | 4 weeks | Crash safety |
| Complete ext4 read-write | P0 | 4 weeks | Standard filesystem |
| Add ext4 journal replay | P0 | 2 weeks | Crash recovery |
| Implement FUSE | P1 | 4 weeks | User-space filesystems |
| Add filesystem caching | P0 | 2 weeks | Performance |
| Add `sendfile` optimization | P1 | 1 week | Zero-copy I/O |
| Add `splice`/`vmsplice` | P2 | 2 weeks | Advanced I/O |

**Exit criteria:** Write files, crash kernel, reboot, verify data integrity.

### Phase 3: Security Hardening (Months 9–12)
**Goal:** Production-grade security.

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| Add KASLR | P0 | 1 week | ASLR baseline |
| Add CFI (shadow call stack) | P0 | 2 weeks | ROP prevention |
| Implement landlock sandboxing | P1 | 3 weeks | User-space security |
| Add seccomp | P1 | 2 weeks | Syscall filtering |
| Add kernel module signing | P2 | 2 weeks | Trust chain |
| Add ASH handler signing | P2 | 1 week | Extension security |
| Implement audit subsystem | P2 | 2 weeks | Compliance |
| Add encrypted swap | P2 | 2 weeks | Data at rest |

**Exit criteria:** Pass basic security audit, run sandboxed workloads.

### Phase 4: Performance (Months 12–18)
**Goal:** Match Linux 5.x on equivalent workloads.

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| Implement slab allocator | P0 | 3 weeks | 2x allocator performance |
| Add eBPF JIT compiler | P0 | 4 weeks | 10x eBPF performance |
| Implement CFS scheduler | P1 | 4 weeks | Better fairness |
| Add RCU (read-copy-update) | P1 | 4 weeks | Lockless reads |
| Add huge pages (2MiB) | P1 | 3 weeks | TLB performance |
| Add NUMA awareness | P2 | 4 weeks | Multi-socket support |
| Optimize network stack | P1 | 4 weeks | Throughput |
| Add block I/O scheduler | P2 | 2 weeks | I/O fairness |

**Exit criteria:** Within 2x of Linux on LMBench, Hackbench, netperf.

### Phase 5: Advanced Features (Months 18–24)
**Goal:** Feature parity with Linux/Windows NT.

| Task | Priority | Effort | Impact |
|------|----------|--------|--------|
| Complete hypervisor (KVM-level) | P1 | 12 weeks | Virtualization |
| Implement containers (cgroups) | P1 | 6 weeks | Cloud native |
| Add live kernel patching | P2 | 4 weeks | Zero-downtime updates |
| Add hotplug (CPU, memory, PCI) | P2 | 4 weeks | Dynamic hardware |
| Add power management | P2 | 4 weeks | Laptop support |
| Add GPU compute | P3 | 8 weeks | HPC/AI |

**Exit criteria:** Run Docker containers, support live patching, manage power.

---

## 4. Architecture Decisions Needed

### ADR-013: Slab Allocator
- **Decision:** Add per-size slab caches for kernel objects (Thread, Vma, FileDescriptor)
- **Rationale:** Current Box allocators are 2-4x slower than slab for small objects
- **Impact:** memory/, task/, vfs/

### ADR-014: Page Cache
- **Decision:** Add unified page cache between VFS and block devices
- **Rationale:** Every VFS read currently goes to disk; page cache would cache recent reads
- **Impact:** vfs/, memory/

### ADR-015: epoll Implementation
- **Decision:** Implement epoll as a file descriptor with interest list + ready list
- **Rationale:** select/poll are O(n); epoll is O(1) for event notification
- **Impact:** syscalls/

### ADR-016: CFS Scheduler (Optional)
- **Decision:** Add CFS as an alternative to stride scheduling, selectable per-process
- **Rationale:** CFS is more efficient for mixed workloads; stride is simpler for real-time
- **Impact:** task/scheduler.rs

### ADR-017: KASLR
- **Decision:** Randomize kernel base address at boot using RDRAND
- **Rationale:** Predictable kernel addresses enable ROP attacks
- **Impact:** boot/, memory/

### ADR-018: Journaling Strategy
- **Decision:** Add write-ahead journaling to SkyFS with ordered mode
- **Rationale:** Current SkyFS loses data on crash; journaling ensures consistency
- **Impact:** vfs/skyfs/

---

## 5. What Makes Vahi Better Than Linux

These are Vahi's structural advantages that Linux cannot easily match:

| Advantage | Why It Matters |
|-----------|---------------|
| **Rust memory safety** | Eliminates buffer overflows, use-after-free, data races — the #1 source of Linux CVEs |
| **ASH/eBPF verifier** | Safe kernel extension without KEXT-style risks; user-space accessible |
| **Monolithic + modular** | No microkernel overhead, but feature flags enable minimal builds |
| **Clean architecture trait** | Multi-arch support without #ifdef spaghetti |
| **Modern IPC** | eventfd, io_uring, ASH hooks — not legacy SysV baggage |
| **Built-in hypervisor** | KVM-level virtualization without separate module |

---

## 6. What Linux Has That Vahi Needs

| Linux Feature | Vahi Status | Priority |
|---------------|-------------|----------|
| 350+ syscalls | ~150 | P0-P1 |
| ext4/btrfs/xfs | SkyFS + partial ext2/4 | P0 |
| CFS scheduler | Stride heap | P1 |
| KVM hypervisor | Stubs | P2 |
| cgroups/containers | None | P2 |
| SELinux/AppArmor | Capabilities only | P2 |
| io_uring | Stubs | P1 |
| FUSE | None | P1 |
| /proc, /sys | None | P1 |
| Module loading | None | P2 |
| KPTI/Retpoline | SMAP only | P0 |

---

## 7. Immediate Next Steps (This Week)

1. **Fix fork()** — The single most critical bug. Without working fork, no userspace.
2. **Add tmpfs** — Required for /tmp, /run, and many programs.
3. **Add CI** — QEMU boot test on every commit to prevent regressions.
4. **Fix clippy warnings** — Code quality baseline.

---

## 8. Success Metrics

| Metric | Current | 6-Month Target | 12-Month Target |
|--------|---------|----------------|-----------------|
| Syscalls implemented | ~150 | ~200 | ~300 |
| Selftests passing | 92 | 150 | 300 |
| Files over 1k lines | 3 | 0 | 0 |
| Userspace programs runnable | 0 | 10 | 50 |
| Boot to shell | Yes | Yes (stable) | Yes (with services) |
| Crash recovery | No | Yes (journaling) | Yes (full) |
| Security hardening | Basic | KASLR + CFI | Full audit |

---

*Generated: 2026-08-21*
*Kernel version: vahi_kernel 0.3.0*
*Based on: CONTEXT.md, kernel-future-plan.md, ADR-001 through ADR-012*
