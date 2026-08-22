# Current State — Verified Against Code

## What Actually Exists

### Memory Subsystem
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| Buddy allocator | ✅ Working | `memory/buddy.rs` | Frame allocation |
| Slab allocator | ✅ Working | `memory/slab.rs` | Size-class blocks 8–4096 bytes |
| Page tables | ✅ Working | `memory/paging.rs` | Address spaces, COW |
| Page cache | ✅ Working | `vfs/page_cache.rs` | Inode-keyed, FIFO eviction, 4096 max pages |
| Frame info | ✅ Working | `memory/frame_info.rs` | Reference counting |
| Stack allocator | ✅ Working | `memory/stack.rs` | Per-thread kernel stacks |
| Swap | ✅ Working | `memory/swap.rs` | Page-out to disk |

### Kernel Object Model
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| KernelObject trait | ✅ Working | `objects/mod.rs` | read/write/ioctl/stat/truncate/poll |
| HandleTable | ✅ Working | `objects/handle.rs` | Per-process, security checks at bind |
| SecurityDescriptor | ✅ Working | `objects/security.rs` | uid/gid/mode/acl |
| Credentials | ✅ Working | `objects/security.rs` | uid/gid/euid/egid/cap_effective |
| ObjectNamespace | ✅ Working | `objects/namespace.rs` | Path-based object registry |

### Process/Thread Model
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| Thread | ✅ Working | `task/thread.rs` | Stack, status, stride fields |
| Process | ✅ Working | `task/process.rs` | Address space, FD table, credentials |
| Scheduler | ✅ Working | `task/scheduler.rs` | Stride heap + work stealing |
| Context switch | ✅ Working | `task/thread.rs` | Callee-saved register save/restore |
| Fork | ✅ Exists | `syscalls/process_lifecycle.rs` | CoW address space clone |
| Execve | ✅ Exists | `syscalls/process_lifecycle.rs` | ELF loading works |
| Signal handling | ✅ Exists | `syscalls/process_signal.rs` | rt_sigaction, rt_sigreturn, kill |
| Credentials | ✅ Exists | `syscalls/process_creds.rs` | uid/gid, capabilities, resource limits |

### VFS
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| VFS trait | ✅ Working | `vfs/mod.rs` | FileSystem, VfsNode |
| devfs | ✅ Working | `vfs/devfs.rs` | /dev/null, /dev/zero, tty |
| SkyFS | ✅ Working | `vfs/skyfs/` | B-tree filesystem |
| ext2 | ✅ Working | `vfs/ext2.rs` | Read + write + truncate |
| ext4 | ⚠️ Read-only | `vfs/ext4.rs` | Feature-gated |
| Pipe | ✅ Working | `vfs/pipe.rs` | Anonymous pipes |
| Page cache | ✅ Working | `vfs/page_cache.rs` | Inode-keyed caching |

### Networking
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| smoltcp | ✅ Working | `net/mod.rs` | TCP/UDP |
| Unix sockets | ✅ Working | `net/unix.rs` | |
| DHCP | ✅ Working | `net/dhcp.rs` | |
| DNS | ✅ Working | `net/dns.rs` | |

### Security
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| seccomp BPF | ✅ Working | `syscalls/seccomp.rs` | Full BPF interpreter (LD/ALU/JMP/RET) |
| Landlock LSM | ✅ Working | `syscalls/landlock.rs` | Path rules, fd-based rulesets |
| prctl | ✅ Working | `syscalls/prctl.rs` | NO_NEW_PRIVS, SECCOMP, DUMPABLE |
| Namespaces | ✅ Working | `syscalls/namespaces.rs` | PID/Mount/Net/IPC/UTS/User |
| Cgroup v2 | ✅ Working | `syscalls/cgroup.rs` | CPU/memory/pids/IO controllers |
| Cgroup enforcement | ✅ Working | `interrupts.rs` | Memory limits at page fault time |
| CFI (Control Flow Integrity) | ✅ Working | `sync/cfi.rs` | Software CFI: static + dynamic target validation |

### Synchronization
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| IrqSafeMutex | ✅ Working | `sync/mod.rs` | IRQ-safe spin mutex |
| RCU | ✅ Working | `sync/rcu.rs` | Read-Copy-Update with grace periods |

### Scheduling
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| Stride scheduler | ✅ Working | `task/scheduler.rs` | Proportional-share |
| SCHED_FIFO | ✅ Working | `task/thread.rs` | Real-time FIFO |
| SCHED_RR | ✅ Working | `task/thread.rs` | Real-time round-robin |
| CPU affinity | ✅ Working | `task/thread.rs` | 64-bit mask |

### IPC
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| Vahi IPC | ✅ Working | `ipc/mod.rs` | Structured messages, zero-copy |

### Capabilities
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| Capability model | ✅ Working | `objects/security.rs` | 16 rights, compose/fork/drop |

### ASH (Kernel Extensions)
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| Verifier | ✅ Working | `ash/verifier.rs` | Safety-critical |
| Interpreter | ✅ Working | `ash/runtime.rs` | |
| Manager | ✅ Working | `ash/manager.rs` | Hook registration |
| Hooks | ✅ Working | `ash/hooks/` | Net, syscall hooks |
| JIT | ✅ Working | `ebpf/jit.rs` | x86_64 code gen: ALU64/32, JMP/JMP32, LD/ST, memory ops |

### eBPF
| Component | Status | File | Notes |
|-----------|--------|------|-------|
| VM | ✅ Working | `ebpf/vm.rs` | Full instruction set |
| JIT compiler | ✅ Working | `ebpf/jit.rs` | x86_64 native code generation |
| Verifier | ✅ Working | `ebpf/verifier.rs` | Safety checks |
| Helpers | ✅ Working | `ebpf/helpers.rs` | Map lookup, pid, ticks, debug print |

### Drivers
| Driver | Status | Notes |
|--------|--------|-------|
| Serial (16550A) | ✅ Working | |
| AHCI/SATA | ✅ Working | |
| NVMe | ✅ Working | With DMA pool |
| VirtIO (block, net, GPU) | ✅ Working | |
| E1000 | ✅ Working | |
| XHCI (USB 3.0) | ✅ Working | With pending_dma pool |
| PS/2 Mouse/Keyboard | ✅ Working | |
| HDA Audio | ✅ Working | |

## What Is Actually Missing (Verified)

| Gap | Verification | Priority |
|-----|-------------|----------|
| tmpfs | ✅ Exists at `vfs/ramfs.rs` (struct Tmpfs), mounted at `/tmp` | **Phase 0 DONE** |
| SMAP/SMEP | ✅ CR4 flags set, STAC/CLAC enforced | **Phase 0 DONE** |
| KASLR | ✅ `KERNEL_SLIDE` + `init_kaslr()` in `main.rs` | **Phase 0 DONE** |
| CI | ✅ `.github/workflows/ci.yml` — build, QEMU selftest, clippy | **Phase 0 DONE** |
| fork() | ✅ `sys_fork()` with COW, FD clone, credential copy | **Phase 0 DONE** |
| ELF loading | ✅ `load_elf()` with static + dynamic support | **Phase 1 DONE** |
| execve | ✅ Full implementation: path, perms, setuid, FD_CLOEXEC | **Phase 1 DONE** |
| Userspace init | ✅ Boot state machine, 8 states, `jump_to_usermode()` | **Phase 1 DONE** |
| initrd | ✅ 163 statically-linked binaries in `kernel/initrd.tar` | **Phase 1 DONE** |
| Signal delivery | ✅ rt_sigaction, signal frame setup, restorer | **Phase 1 DONE** |
| Linux emulation | ✅ Detects Linux ELF, maps syscalls to Vahi | **Phase 1 DONE** |
| procfs | ✅ `syscalls/procfs.rs` exists | Phase 2 DONE |
| epoll | ✅ `syscalls/epoll.rs` exists | Phase 2 DONE |
| writev/readv | ✅ Standalone syscalls | Phase 2 DONE |
| madvise | ✅ Implemented | Phase 2 DONE |
| pread/pwrite | ✅ Added to emulation | Phase 2 DONE |
| FUSE bridge | ✅ `vfs/fuse.rs` — full protocol implementation | Phase 3 DONE |
| SkyFS journaling | ✅ Fixed — actually replays data to target blocks | Phase 3 DONE |
| Socket options | ✅ 25+ options (SO_REUSEADDR, TCP_NODELAY, etc.) | Phase 4 DONE |
| Network ioctl | ✅ 7 ioctl commands for interface config | Phase 4 DONE |
| seccomp | ✅ Full BPF interpreter + strict mode | Phase 5 DONE |
| Landlock | ✅ Path rules, fd-based rulesets | Phase 5 DONE |
| prctl | ✅ NO_NEW_PRIVS, SECCOMP, DUMPABLE | Phase 5 DONE |
| Namespaces | ✅ PID/Mount/Net/IPC/UTS/User + unshare/setns | Phase 6 DONE |
| Cgroup v2 | ✅ CPU/memory/pids/IO + enforcement at page fault | Phase 6 DONE |
| Hypervisor | ✅ VMX/SVM, EPT/NPT, vCPU, launch_vm | Phase 7 DONE |
| io_uring | ✅ Working | `syscalls/io_uring.rs` | SQE processing, readv/writev, accept/connect, send/recv, GUI ops |
| CFI | ✅ Working | `sync/cfi.rs` | Software CFI: validation tables, binary search, violation logging |
| Dynamic linking kernel-side | ✅ Exists | `syscalls/process_lifecycle.rs` | Static + dynamic ELF loading |

## Previous Roadmap Errors Corrected

1. **"No slab allocator"** → Slab exists at `memory/slab.rs` with size-class blocks
2. **"No page cache"** → Page cache exists at `vfs/page_cache.rs` with inode-keyed caching
3. **"fork() broken"** → Fork exists; needs testing, not assumption of broken
4. **"No kernel object model"** → KernelObject trait + HandleTable + SecurityDescriptor exist
5. **"Add slab in Phase 4"** → Already exists; remove from roadmap
6. **"Add page cache in Phase 0"** → Already exists; remove from roadmap
7. **"epoll = O(1)"** → Oversimplified; corrected in revised text
8. **"CFS is the target"** → EEVDF is more accurate for modern Linux
9. **"KASLR not in boot path"** → KASLR is in `main.rs:63` via `init_kaslr()`
10. **"SMAP/SMEP not enforced"** → Fully enforced: CR4 flags + STAC/CLAC in user_access.rs

## Phase Completion Status

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 0: Foundation | ✅ COMPLETE | tmpfs, SMAP/SMEP, KASLR, CI, fork() all verified |
| Phase 1: Userspace Bootstrap | ✅ COMPLETE | ELF, exec, init, 163-binary initrd, signal delivery |
| Phase 2: POSIX/Linux Compat | ✅ COMPLETE | epoll, writev/readv, madvise, pipe2, dup3, procfs, pread/pwrite |
| Phase 3: Persistent Storage | ✅ COMPLETE | SkyFS journaling fixed, FUSE bridge created, ext2 write works |
| Phase 4: Networking | ✅ COMPLETE | TCP/UDP, IPv6, Unix sockets, raw sockets, DHCP, DNS, socket options, network ioctl |
| Phase 5: Security | ✅ COMPLETE | SMAP/SMEP, DAC, signals, seccomp, Landlock, prctl, capability dropping |
| Phase 6: Containers | ✅ COMPLETE | PID/Mount/Net/IPC/UTS/User namespaces, unshare/setns, cgroup v2 primitives |
| Phase 7: Virtualization | ✅ COMPLETE | VMX/SVM, EPT/NPT, vCPU, launch_vm, 10+ VM syscalls (integrated subsystem per ADR-025) |

## Key Findings from Phase 1 Audit

### The kernel boots to a real userspace shell

The initrd contains 163 statically-linked ELF x86-64 binaries:
- `bin/init` — init process
- `bin/sash` — shell
- `bin/ls`, `bin/cat`, `bin/mkdir`, `bin/rm` — core utilities
- `bin/login`, `bin/passwd` — authentication
- `bin/sarga-term`, `bin/skyedit` — GUI applications
- `etc/passwd`, `etc/fstab`, `etc/hostname` — configuration

### Linux emulation layer works

The `emulation.rs` module detects Linux ELF binaries (via `set_emulation()`) and maps Linux syscall numbers to Vahi syscalls via `map_linux_to_vahi()`. This allows statically-linked Linux binaries to run on Vahi.

### Signal delivery is fully functional

The dispatch table (lines 301-456 of `dispatch.rs`) shows complete signal frame setup:
- Save all registers to user stack
- Set up restorer trampoline
- Modify instruction pointer to handler
- `rt_sigreturn` restores context

### The biggest gap: epoll

Without `epoll_create1`/`epoll_ctl`/`epoll_wait`, Vahi cannot run:
- nginx
- node.js
- Go runtime's netpoller
- Most modern event-driven servers

This is the single highest-priority item for Phase 2.
