# Vahi Kernel — Revised Executive Architecture

## What This Document Is

This is a corrected, architecture-first roadmap for the Vahi kernel. It replaces the previous roadmap's assumption-based planning with dependency-ordered, code-verified development phases.

## What Changed From the Previous Roadmap

| Previous Claim | Reality | Correction |
|----------------|---------|------------|
| "No slab allocator" | `memory/slab.rs` exists with size-class blocks | Already implemented |
| "No page cache" | `vfs/page_cache.rs` exists with inode-keyed cache | Already implemented |
| "fork() broken" | Needs verification, not assumption | Test before assuming broken |
| "No kernel object model" | `KernelObject` trait + `HandleTable` + `SecurityDescriptor` exist | Already implemented |
| "epoll = O(1)" | Oversimplified; avoids O(n) scan but has registration overhead | Corrected |
| "CFS is the target" | EEVDF is more accurate for modern Linux | Corrected |
| "KVM-level hypervisor" | Integrated subsystem (VMX/SVM already in kernel) | Corrected |
| "4 weeks for fork fix" | No basis for estimate; use complexity ratings instead | Corrected |

## Architectural Identity: What Is Vahi-Native

Vahi is NOT "Linux written in Rust." It has its own architectural identity:

### Already Implemented (Vahi-Native)

1. **KernelObject trait** — Unified object model with `read/write/ioctl/stat/truncate/poll_readable/poll_writable/socket_*` methods
2. **HandleTable** — Per-process handle table with security checks at bind time
3. **SecurityDescriptor** — uid/gid/mode/acl attached to every kernel object
4. **Credentials** — uid/gid/euid/egid/fsuid/fsgid/cap_effective snapshot
5. **ASH** — Kernel extension system with eBPF verifier + interpreter (Linux has no equivalent)
6. **Stride scheduler** — Proportional-share scheduling (different from CFS)
7. **IRQ-safe mutex** — `IrqSafeMutex` that disables interrupts during critical sections

### Recently Implemented (Vahi-Native)

8. **Capability/rights model** — 16 per-object rights in `objects/security.rs`: READ, WRITE, EXEC, CREATE, DELETE, MODIFY, ADMIN, CONNECT, LISTEN, BIND, SEND, RECV, IOCTL, MMAP, SHMEM, SIGNAL
9. **Vahi IPC** — Structured messages + zero-copy transfers in `ipc/mod.rs`
10. **Port IPC** — Windows NT-inspired port-based IPC with zero-copy
11. **Job Objects** — Resource limits, kill-on-close in `task/process.rs`
12. **RCU** — Read-Copy-Update synchronization in `sync/rcu.rs`
13. **eBPF JIT** — x86_64 code generation in `ebpf/jit.rs`
14. **RT Scheduling** — SCHED_FIFO, SCHED_RR + 64-bit CPU affinity

### Not Yet Implemented (Low Priority)

1. **WaitableObject abstraction** — Polling is ad-hoc per FD type (low priority, IrqSafeMutex works)
2. **VM object model** — mmap works but no formal VM object abstraction (low priority)
3. **CFI (Control Flow Integrity)** — Not critical for correctness

## The Three Interface Layers

### Layer 1: POSIX Interfaces (Required for Userspace)

```text
open, read, write, close, lseek, stat, fstat, lstat
fork, clone, execve, exit, wait4, getpid, getppid
pipe, dup, dup2, fcntl, ioctl
mmap, munmap, mprotect, brk
rt_sigaction, rt_sigreturn, sigprocmask, kill
nanosleep, clock_gettime, clock_nanosleep
getuid, getgid, geteuid, getegid, setuid, setgid
getcwd, chdir, mkdir, rmdir, unlink, link, symlink, readlink, rename
chmod, chown, fchmod, fchown
statfs, fstatfs, umask
```

### Layer 2: Linux-Compatible Interfaces (Required for Modern Software)

```text
epoll (for nginx, redis, Node.js)
procfs (for ps, top, free, vmstat)
io_uring (for high-performance async I/O)
seccomp (for sandboxing)
Landlock (for filesystem access control)
cgroups (for container resource limits)
Linux namespaces (for container isolation)
clone3 (for containers)
timerfd, signalfd4, eventfd2
memfd_create
sendfile, splice, vmsplice
prlimit64, getrusage
inotify, fanotify
```

### Layer 3: Vahi-Native Interfaces (Differentiators) ✅ IMPLEMENTED

```text
ASH (kernel extensions with eBPF verifier + JIT) ✅
Vahi IPC (structured messages + zero-copy + port IPC) ✅
Capability-based authority model (16 per-object rights) ✅
Job Objects (resource limits, kill-on-close) ✅
Built-in hypervisor (VMX/SVM, EPT/NPT, vCPU) ✅
```

## Dependency Graph

```text
Boot / Architecture
    │
    ├── UEFI boot protocol
    ├── Architecture init (x86_64, aarch64 stubs)
    ├── Early memory (physical memory map)
    ├── Page tables (kernel mapping)
    ├── Kernel relocation
    ├── Early entropy (RDRAND)
    └── CPU bring-up (BSP + APs)
    │
    ▼
Physical Memory Manager
    │
    ├── Buddy allocator (EXISTS: memory/buddy.rs)
    ├── Frame info / refcounting (EXISTS: memory/frame_info.rs)
    └── Physical memory offset (EXISTS: memory/phys.rs)
    │
    ▼
Virtual Memory Manager
    │
    ├── Page tables (EXISTS: memory/paging.rs)
    ├── Address spaces (EXISTS: memory/paging.rs)
    ├── Page permissions (EXISTS: PageTableFlags)
    ├── Page fault handler (EXISTS: interrupts.rs)
    ├── Anonymous memory (EXISTS: mmap/brk)
    └── COW (EXISTS: memory/paging.rs)
    │
    ▼
Kernel Heap
    │
    ├── Slab allocator (EXISTS: memory/slab.rs)
    ├── Fixed-size blocks (8–4096 bytes)
    └── Fallback to linked-list allocator
    │
    ▼
Kernel Object Model
    │
    ├── KernelObject trait (EXISTS: objects/mod.rs)
    ├── ObjectHeader (EXISTS: objects/mod.rs)
    ├── ObjectTypeId (EXISTS: objects/mod.rs)
    ├── HandleTable (EXISTS: objects/handle.rs)
    ├── SecurityDescriptor (EXISTS: objects/security.rs)
    └── Credentials (EXISTS: objects/security.rs)
    │
    ▼
Threads / Processes
    │
    ├── Thread model (EXISTS: task/thread.rs)
    ├── Process model (EXISTS: task/process.rs)
    ├── Address-space ownership (EXISTS: task/process.rs)
    ├── Context switching (EXISTS: task/thread.rs)
    ├── Scheduler (EXISTS: task/scheduler.rs)
    ├── Process lifecycle (EXISTS: task/process.rs)
    └── Exit/reap semantics (EXISTS: task/scheduler.rs)
    │
    ▼
Synchronization
    │
    ├── IrqSafeMutex (EXISTS: sync.rs)
    ├── Atomics (EXISTS: core::sync::atomic)
    ├── Wait queues (PARTIAL: sleep/futex/pipe queues)
    ├── Futex (EXISTS: syscalls/futex.rs)
    └── Timers (EXISTS: syscalls/posix_timers.rs)
    │
    ▼
VFS / File Objects
    │
    ├── VFS trait (EXISTS: vfs/mod.rs)
    ├── VfsNode trait (EXISTS: vfs/mod.rs)
    ├── FileSystem trait (EXISTS: vfs/mod.rs)
    ├── devfs (EXISTS: vfs/devfs.rs)
    ├── SkyFS (EXISTS: vfs/skyfs/)
    ├── ext2 (EXISTS: vfs/ext2.rs, read-only)
    ├── ext4 (EXISTS: vfs/ext4.rs, read-only, feature-gated)
    ├── Page cache (EXISTS: vfs/page_cache.rs)
    ├── Pipe (EXISTS: vfs/pipe.rs)
    └── Path resolution (EXISTS: vfs/mod.rs)
    │
    ▼
Executable Loading
    │
    ├── ELF loading (EXISTS: task/process.rs)
    ├── Static linking (EXISTS: task/process.rs)
    └── Dynamic linking (NOT YET: ld.so is userspace)
    │
    ▼
Userspace Bootstrap
    │
    ├── fork (EXISTS: syscalls/process.rs)
    ├── execve (EXISTS: syscalls/process.rs)
    ├── exit/wait (EXISTS: syscalls/process.rs)
    ├── signals (EXISTS: syscalls/signal.rs)
    ├── pipes (EXISTS: vfs/pipe.rs)
    ├── TTY/console (EXISTS: vfs/devfs.rs)
    └── init (NEEDS: first userspace process)
    │
    ▼
Networking
    │
    ├── smoltcp integration (EXISTS: net/mod.rs)
    ├── TCP/UDP (EXISTS: net/mod.rs)
    ├── Unix sockets (EXISTS: net/unix.rs)
    ├── DHCP (EXISTS: net/dhcp.rs)
    └── DNS (EXISTS: net/dns.rs)
    │
    ▼
Linux/POSIX Compatibility ✅
    │
    ├── epoll (EXISTS: syscalls/epoll.rs)
    ├── procfs (EXISTS: syscalls/procfs.rs)
    ├── io_uring (EXISTS: syscalls/io_uring.rs — SQE processing, readv/writev, accept/connect, send/recv)
    ├── seccomp (EXISTS: syscalls/seccomp.rs — full BPF interpreter)
    └── Landlock (EXISTS: syscalls/landlock.rs — path rules, fd-based rulesets)
    │
    ▼
Security Architecture ✅
    │
    ├── Capabilities (EXISTS: syscalls/numbers.rs CAP_*)
    ├── DAC (EXISTS: objects/security.rs)
    ├── ASH (EXISTS: ash/ — verifier, interpreter, manager, hooks)
    ├── SMAP/SMEP (EXISTS: CR4 flags + STAC/CLAC enforced)
    ├── KASLR (EXISTS: main.rs init_kaslr())
    └── CFI (NOT YET — low priority)
    │
    ▼
Performance / Scalability ✅
    │
    ├── SMP (EXISTS: smp.rs — AP trampoline, per-CPU schedulers)
    ├── Work stealing (EXISTS: task/scheduler.rs)
    ├── DMA pool (EXISTS: hal/dma.rs)
    ├── RCU (EXISTS: sync/rcu.rs — read-side critical sections, grace periods)
    ├── eBPF JIT (EXISTS: ebpf/jit.rs — x86_64 code generation)
    └── RT Scheduling (EXISTS: task/thread.rs — SCHED_FIFO, SCHED_RR, affinity)
    │
    ▼
Containers / Isolation ✅
    │
    ├── Namespaces (EXISTS: syscalls/namespaces.rs — PID/Mount/Net/IPC/UTS/User)
    ├── cgroups (EXISTS: syscalls/cgroup.rs — CPU/memory/pids/IO + enforcement)
    └── Container runtime (NOT YET — low priority)
    │
    ▼
Virtualization (INTEGRATED SUBSYSTEM) ✅
    │
    ├── VMX/SVM (EXISTS: hypervisor/vmx.rs)
    ├── EPT/NPT (EXISTS: hypervisor/)
    ├── vCPU (EXISTS: hypervisor/)
    └── launch_vm (EXISTS: syscalls/vm.rs)
    │
    ▼
Vahi-Native Features ✅
    │
    ├── ASH JIT (EXISTS: ebpf/jit.rs — x86_64 code generation)
    ├── Vahi IPC (EXISTS: ipc/mod.rs — structured messages, zero-copy, port IPC)
    ├── Capability model (EXISTS: objects/security.rs — 16 rights, compose/fork/drop)
    └── Job Objects (EXISTS: task/process.rs — resource limits, kill-on-close)
```

## Dependency Matrix

| Subsystem | Depends On | Depended By |
|-----------|------------|-------------|
| Boot | Nothing | Everything |
| Physical Memory | Boot | Virtual Memory, Kernel Heap |
| Virtual Memory | Physical Memory | Processes, VFS, mmap |
| Kernel Heap | Physical Memory | Everything |
| Kernel Objects | Kernel Heap | VFS, Processes, Networking |
| Threads/Processes | Virtual Memory, Kernel Objects | Syscalls, Scheduler |
| Synchronization | Threads | Everything |
| VFS | Virtual Memory, Kernel Objects | Syscalls, Executable Loading |
| Executable Loading | VFS, Virtual Memory | Userspace |
| Userspace | Executable Loading | POSIX/Linux compat |
| Networking | Kernel Objects, Sockets | POSIX/Linux compat |
| Security | Kernel Objects, Credentials | Everything |
| Containers | Namespaces, cgroups | Docker/OCI |
| Virtualization | Everything | Guest OS |
