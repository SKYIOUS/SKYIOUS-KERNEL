# ADR-020: Landlock / Sandboxing Approach

## Status

**DECISION REQUIRED** — This ADR proposes a sandboxing architecture that requires team consensus.

## Context

Vahi needs a sandboxing mechanism to restrict process access to filesystems, network, and other resources. Linux provides Landlock (since 5.13) as a mountable LSM for unprivileged sandboxing.

Current state:
- No Landlock implementation exists
- No seccomp implementation exists
- No namespace implementation exists
- `SecurityDescriptor` provides basic DAC (uid/gid/mode)
- `HandleTable` provides bind-time access checks
- No use-time access checks (Finding 2 in kernel object model review)

The sandboxing decision must address:
1. Who can create sandbox policies?
2. What resources can be restricted?
3. How do policies compose (union, intersection, override)?
4. How do policies interact with capabilities and DAC?
5. Should the ABI be Linux-compatible?

## Decision

**Implement Linux-compatible Landlock ABI with Vahi-native extensions.**

### Rationale

1. **Linux compatibility** — Many programs (browsers, containers, package managers) use Landlock
2. **Proven design** — Landlock's unprivileged, composable model is well-tested
3. **Vahi extensions** — Can add Vahi-native resource types (ASH, IPC endpoints)
4. **Incremental** — Start with filesystem, add network later

### Architecture

```text
┌─────────────────────────────────────────────┐
│              Userspace                       │
│  landlandock_create_rule()                   │
│  landlandock_add_rule()                      │
│  landlandock_restrict_self()                 │
└──────────────┬──────────────────────────────┘
               │ syscall
               ▼
┌─────────────────────────────────────────────┐
│           Kernel Sandbox Layer               │
│  ┌─────────────────────────────────────┐    │
│  │        SandboxPolicy                │    │
│  │  ┌─────────────┐ ┌─────────────┐   │    │
│  │  │ Filesystem  │ │   Network   │   │    │
│  │  │   Rules     │ │   Rules     │   │    │
│  │  └─────────────┘ └─────────────┘   │    │
│  │  ┌─────────────┐ ┌─────────────┐   │    │
│  │  │   ASH       │ │    IPC      │   │    │
│  │  │   Rules     │ │   Rules     │   │    │
│  │  └─────────────┘ └─────────────┘   │    │
│  └─────────────────────────────────────┘    │
│                                             │
│  ┌─────────────────────────────────────┐    │
│  │       Check Engine                  │    │
│  │  • Per-syscall policy check         │    │
│  │  • Union of all stacked policies    │    │
│  │  • Deny-first semantics             │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
```

### New Types

```rust
/// A sandbox policy attached to a process.
pub struct SandboxPolicy {
    /// Filesystem access rules.
    pub fs_rules: Vec<FsRule>,
    /// Network access rules.
    pub net_rules: Vec<NetRule>,
    /// ASH access rules (Vahi-native).
    pub ash_rules: Vec<AshRule>,
    /// IPC access rules (Vahi-native).
    pub ipc_rules: Vec<IpcRule>,
}

/// Filesystem access rule.
pub struct FsRule {
    /// Allowed access (read, write, execute).
    pub access: u32,
    /// Path prefix to match (e.g., "/tmp", "/home/user").
    pub path_prefix: Vec<u8>,
    /// Whether this is a deny rule.
    pub deny: bool,
}

/// Network access rule.
pub struct NetRule {
    /// Allowed access (bind, connect, send, recv).
    pub access: u32,
    /// Address family (AF_INET, AF_UNIX, etc.).
    pub family: u16,
    /// Port range (0 = any).
    pub port_start: u16,
    pub port_end: u16,
}

/// ASH access rule (Vahi-native).
pub struct AshRule {
    /// Allowed ASH operations (load, execute, unload).
    pub access: u32,
    /// ASH module name pattern.
    pub module_pattern: Vec<u8>,
}
```

### New Syscalls

```rust
/// Create a new sandbox policy.
pub fn sys_landlandock_create_rule(
    rule_type: u32,  // LANDLOCK_RULE_FS, LANDLOCK_RULE_NET, etc.
    access: u64,     // allowed access mask
    path_fd: i32,    // file descriptor for path-based rules
) -> u64;

/// Add a rule to an existing policy.
pub fn sys_landlandock_add_rule(
    policy_fd: u32,
    rule_type: u32,
    access: u64,
    path_fd: i32,
) -> u64;

/// Restrict the current process with the given policy.
pub fn sys_landlandock_restrict_self(
    policy_fd: u32,
    flags: u64,
) -> u64;
```

### Policy Composition

Policies stack (union of restrictions):
- Process starts with no restrictions
- Each `restrict_self()` adds a policy
- Effective restrictions = intersection of all stacked policies
- Deny rules override allow rules (deny-first)

### Check Engine

```rust
impl SandboxPolicy {
    /// Check if a filesystem access is allowed.
    pub fn check_fs_access(&self, path: &str, access: u32) -> bool {
        // Deny-first: check deny rules first
        for rule in &self.fs_rules {
            if rule.deny && path.starts_with(&rule.path_prefix) {
                return false;
            }
        }
        // Then check allow rules
        for rule in &self.fs_rules {
            if !rule.deny && path.starts_with(&rule.path_prefix) {
                return (rule.access & access) == access;
            }
        }
        // Default: deny
        false
    }
    
    /// Check if a network access is allowed.
    pub fn check_net_access(&self, family: u16, port: u16) -> bool {
        // Similar deny-first logic
        true // default: allow (for backwards compatibility)
    }
}
```

### Integration Points

1. **sys_open / sys_openat** — Check `check_fs_access()` before opening
2. **sys_read / sys_write** — Check `check_fs_access()` on file path
3. **sys_socket / sys_bind / sys_connect** — Check `check_net_access()`
4. **ASH load/execute** — Check `check_ash_access()`

## Consequences

### Positive

1. **Linux compatibility** — Programs using Landlock work without modification
2. **Unprivileged** — Any process can create policies (no root required)
3. **Composable** — Policies stack, enabling fine-grained restrictions
4. **Vahi-native** — Can extend with ASH and IPC rules
5. **Deny-first** — Secure default (explicit allow required)

### Negative

1. **Performance overhead** — Per-syscall policy checks add latency
2. **Memory overhead** — Each policy consumes kernel memory
3. **Complexity** — Policy composition and interaction with capabilities
4. **ABI surface** — Must maintain Linux-compatible syscall interface

### Risks

1. **ABI breakage** — Linux Landlock ABI may change
2. **Incomplete coverage** — Some syscalls may not be checked
3. **Bypass vectors** — File descriptors opened before restriction

## Alternatives Considered

### Alternative 1: Vahi-Native Sandboxing Only

**Rejected.** Linux programs using Landlock would not work. Limits compatibility.

### Alternative 2: seccomp-Only

**Rejected.** seccomp filters syscalls at the entry point, not resources. Landlock is resource-based, seccomp is syscall-based. Both are needed for comprehensive sandboxing.

### Alternative 3: No Sandboxing

**Rejected.** Security requirement. Every production OS needs sandboxing.

## Implementation Plan

1. **Phase 1:** Add `SandboxPolicy` type and check engine
2. **Phase 2:** Implement `landlandock_create_rule` and `landlandock_restrict_self`
3. **Phase 3:** Integrate with sys_open, sys_read, sys_write
4. **Phase 4:** Add network rules
5. **Phase 5:** Add Vahi-native rules (ASH, IPC)
6. **Phase 6:** Add seccomp for syscall filtering

## References

- Linux Landlock: `security/landlock/` (Linux 5.13+)
- Landlock ABI: `include/uapi/linux/landlock.h`
- `docs/roadmap-revised/07-security-architecture.md` — Security architecture
