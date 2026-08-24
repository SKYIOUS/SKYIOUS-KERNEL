# ADR-024: Container Scope

## Status

**DECISION REQUIRED** — This ADR proposes a container architecture that requires team consensus.

## Context

Vahi needs container support for cloud-native workloads. The container decision must address:
1. What level of isolation is needed?
2. Should Vahi be Docker-compatible?
3. Should Vahi support Kubernetes?
4. What namespaces are required?
5. What resource limits are needed?

Current state:
- No namespace implementation exists
- No cgroup implementation exists
- No clone flags (CLONE_NEWPID, CLONE_NEWNS, etc.)
- No container runtime interface

The container decision must balance:
- **Isolation** — Process isolation, filesystem isolation, network isolation
- **Compatibility** — Docker, Kubernetes, OCI runtime
- **Complexity** — Implementation cost, maintenance burden
- **Performance** — Overhead of namespace/cgroup operations

## Decision

**Implement isolation primitives only. Do not implement Docker/Kubernetes compatibility.**

### Rationale

1. **Isolation is fundamental** — Every modern OS needs process isolation
2. **Docker compatibility is complex** — Requires full OCI runtime, containerd, runc
3. **Kubernetes is massive** — Requires cgroups, namespaces, networking, storage, scheduling
4. **Vahi-native approach** — Can provide isolation without Linux container ABI
5. **Incremental** — Start with namespaces, add cgroups later

### Scope Definition

| Feature | In Scope | Out of Scope |
|---------|----------|--------------|
| PID namespaces | ✅ | |
| Mount namespaces | ✅ | |
| Network namespaces | ✅ | |
| IPC namespaces | ✅ | |
| UTS namespaces | ✅ | |
| User namespaces | ✅ | |
| cgroups v1/v2 | ✅ (Phase 2) | |
| Container runtime (OCI) | | ❌ |
| Docker compatibility | | ❌ |
| Kubernetes support | | ❌ |
| containerd integration | | ❌ |

### Architecture

```text
┌─────────────────────────────────────────────┐
│              Userspace                       │
│  clone(CLONE_NEWPID | CLONE_NEWNS | ...)     │
│  unshare(CLONE_NEWNET)                       │
│  setns(fd, CLONE_NEWPID)                     │
└──────────────┬──────────────────────────────┘
               │ syscall
               ▼
┌─────────────────────────────────────────────┐
│           Namespace Layer                    │
│  ┌─────────────┐ ┌─────────────┐            │
│  │   PID NS    │ │  Mount NS   │            │
│  └─────────────┘ └─────────────┘            │
│  ┌─────────────┐ ┌─────────────┐            │
│  │  Network NS │ │   IPC NS    │            │
│  └─────────────┘ └─────────────┘            │
│  ┌─────────────┐ ┌─────────────┐            │
│  │   UTS NS    │ │  User NS    │            │
│  └─────────────┘ └─────────────┘            │
└──────────────┬──────────────────────────────┘
               │
               ▼
┌─────────────────────────────────────────────┐
│           Resource Limits                    │
│  ┌─────────────┐ ┌─────────────┐            │
│  │  cgroups    │ │  rlimits    │            │
│  └─────────────┘ └─────────────┘            │
└─────────────────────────────────────────────┘
```

### New Types

```rust
/// Namespace types.
pub enum NamespaceType {
    Pid,
    Mount,
    Network,
    Ipc,
    Uts,
    User,
}

/// A namespace instance.
pub struct Namespace {
    pub ns_type: NamespaceType,
    pub id: u64,
    pub owner: u64,  // PID of creating process
    pub procs: Vec<u64>,  // PIDs in this namespace
}

/// Per-process namespace set.
pub struct NamespaceSet {
    pub pid_ns: Arc<Namespace>,
    pub mount_ns: Arc<Namespace>,
    pub net_ns: Arc<Namespace>,
    pub ipc_ns: Arc<Namespace>,
    pub uts_ns: Arc<Namespace>,
    pub user_ns: Arc<Namespace>,
}

/// cgroup controller (Phase 2).
pub struct Cgroup {
    pub path: Vec<u8>,
    pub controllers: HashMap<Vec<u8>, CgroupController>,
}

pub struct CgroupController {
    pub cpu_max: u64,
    pub memory_max: u64,
    pub pids_max: u64,
    pub io_max: u64,
}
```

### New Syscalls

```rust
/// Clone with namespace flags.
pub fn sys_clone(
    flags: u64,  // CLONE_NEWPID, CLONE_NEWNS, etc.
    child_stack: u64,
    ptid: u64,
    ctid: u64,
    tls: u64,
) -> u64;

/// Unshare current namespace.
pub fn sys_unshare(flags: u64) -> u64;

/// Join a namespace.
pub fn sys_setns(fd: u64, nstype: u64) -> u64;
```

### Clone Flags

```rust
const CLONE_NEWPID: u64 = 1 << 0;
const CLONE_NEWNS: u64 = 1 << 1;
const CLONE_NEWNET: u64 = 1 << 2;
const CLONE_NEWIPC: u64 = 1 << 3;
const CLONE_NEWUTS: u64 = 1 << 4;
const CLONE_NEWUSER: u64 = 1 << 5;
const CLONE_PARENT: u64 = 1 << 6;
const CLONE_THREAD: u64 = 1 << 7;
```

### Namespace Implementation

**PID Namespace:**
- Each PID namespace has its own PID numbering
- PID 1 is the namespace init process
- Children of PID 1 are reaped automatically
- PID namespaces are hierarchical (parent can see child PIDs)

**Mount Namespace:**
- Each mount namespace has its own mount table
- `mount()` and `umount()` affect only the current namespace
- Bind mounts can be shared between namespaces

**Network Namespace:**
- Each network namespace has its own network stack
- Interfaces, routes, iptables are namespace-local
- veth pairs connect namespaces

**IPC Namespace:**
- Each IPC namespace has its own System V IPC and POSIX message queues
- Shared memory segments are namespace-local

**UTS Namespace:**
- Each UTS namespace has its own hostname and domain name
- `sethostname()` and `setdomainname()` affect only the current namespace

**User Namespace:**
- Each user namespace has its own UID/GID mapping
- Unprivileged users can create user namespaces
- UID 0 in a user namespace maps to an unprivileged UID in the parent

## Consequences

### Positive

1. **Process isolation** — PID namespaces isolate process trees
2. **Filesystem isolation** — Mount namespaces isolate mount points
3. **Network isolation** — Network namespaces isolate network stacks
4. **Resource limits** — cgroups limit CPU, memory, I/O
5. **Security** — User namespaces enable unprivileged isolation

### Negative

1. **Complexity** — Namespaces require deep kernel integration
2. **Performance overhead** — Namespace operations add latency
3. **No Docker compatibility** — Linux containers won't run unmodified
4. **No Kubernetes support** — Cloud-native workloads need k8s

### Risks

1. **Incomplete implementation** — Partial namespaces are worse than none
2. **Security holes** — Namespace escapes are critical vulnerabilities
3. **Maintenance burden** — Namespaces interact with every subsystem

## Alternatives Considered

### Alternative 1: Full Docker Compatibility

**Rejected.** Requires OCI runtime, containerd, runc, and full Linux container ABI. Massive scope.

### Alternative 2: No Namespaces

**Rejected.** Every modern OS needs process isolation. Without namespaces, Vahi cannot run untrusted code safely.

### Alternative 3: Vahi-Native Isolation Only

**Rejected.** Linux programs using namespaces would not work. Limits compatibility.

## Implementation Plan

1. **Phase 1:** Add PID namespaces (isolate process trees)
2. **Phase 2:** Add mount namespaces (isolate filesystem)
3. **Phase 3:** Add network namespaces (isolate network)
4. **Phase 4:** Add IPC/UTS namespaces
5. **Phase 5:** Add user namespaces
6. **Phase 6:** Add cgroups v2

## References

- Linux namespaces: `include/linux/nsproxy.h`
- cgroups: `kernel/cgroup/`
- `docs/roadmap-revised/09-containers-isolation.md` — Container architecture
