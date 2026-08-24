# ADR-019: Kernel Object Model Capability/Rights Redesign

## Status

**DECISION REQUIRED** — This ADR proposes a major architectural change that requires team consensus.

## Context

The kernel object model (`KernelObject` trait, `HandleTable`, `SecurityDescriptor`, `Credentials`) has 7 critical defects identified in the architectural review:

1. **Dual FD table system** — `fd_table` (60 uses) and `handle_table` (13 uses) must stay synchronized
2. **Security checks only at bind time** — No use-time validation
3. **`access_mask` stored but never checked** — Dead security code
4. **KernelObject trait overloaded** — 20+ methods, violates Interface Segregation
5. **Credentials incomplete** — Missing supplementary groups, capability sets
6. **ACL checking incomplete** — No group matching, no deny-first semantics
7. **No right delegation** — Cannot restrict rights on dup

These defects block:
- POSIX compliance (setgroups, capability inheritance)
- Security hardening (use-time access checks)
- Capability-based access control
- Linux compatibility (proper credential model)

## Decision

**Migrate to structured rights model with credential snapshots.**

### New Types

```rust
/// Structured access rights — replaces u32 access_mask.
pub struct AccessRights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub append: bool,
    pub truncate: bool,
    pub ioctl: bool,
}

/// Complete credentials snapshot at handle creation time.
pub struct HandleCredentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
    pub cap_effective: u64,
    pub cap_permitted: u64,
    pub cap_inheritable: u64,
    pub supplementary_groups: Vec<u32>,
}

/// Enhanced handle entry.
pub struct HandleEntry {
    pub object: Arc<dyn KernelObject>,
    pub rights: AccessRights,
    pub credentials: HandleCredentials,
    pub flags: u64,
    pub offset: u64,
    pub audit_id: u64,
    pub create_time: u64,
}
```

### New HandleTable API

```rust
impl HandleTable {
    /// Insert with credential snapshot and structured rights.
    pub fn insert_with_rights(
        &mut self,
        object: Arc<dyn KernelObject>,
        desired_rights: AccessRights,
        credentials: HandleCredentials,
        flags: u64,
    ) -> Result<HandleValue, ()>;

    /// Duplicate with right restriction.
    pub fn dup_restricted(
        &mut self,
        old_handle: HandleValue,
        restriction: AccessRights,
    ) -> Result<HandleValue, ()>;

    /// Check access rights at use time.
    pub fn check_access(
        &self,
        handle: HandleValue,
        requested: AccessRights,
    ) -> Result<&HandleEntry, ()>;

    /// Filter by close-on-exec flag during fork.
    pub fn clone_for_fork(&self, close_on_exec: bool) -> Vec<Option<HandleEntry>>;
}
```

## Consequences

### Positive

1. **Use-time security checks** — Every I/O operation validates access rights
2. **Right delegation** — Can grant subset of rights to child processes
3. **Credential snapshot** — No repeated lock acquisition for security checks
4. **POSIX compliance** — Supports supplementary groups, capability inheritance
5. **Audit trail** — Credential snapshot enables security logging
6. **Type safety** — Compile-time guarantee of valid access operations

### Negative

1. **Migration cost** — Must migrate 60 `fd_table` uses to `handle_table`
2. **Memory overhead** — Credential snapshot adds ~100 bytes per handle
3. **Performance** — Use-time checks add overhead to I/O operations
4. **Complexity** — More types, more APIs, more documentation

### Risks

1. **Migration breaks existing code** — Must be done incrementally, one syscall at a time
2. **Performance regression** — Use-time checks may slow hot paths
3. **Incompatibility** — May break existing userspace that relies on current behavior

## Alternatives Considered

### Alternative 1: Keep u32 access_mask

**Rejected.** The u32 bitmask is insufficient for structured rights. Cannot represent "read + ioctl but not write" without custom encoding.

### Alternative 2: Bind-time only checks (status quo)

**Rejected.** Security vulnerability — once opened, any operation is permitted.

### Alternative 3: LSM hooks only

**Rejected.** LSM is for mandatory access control, not DAC. Still need use-time DAC checks.

## Implementation Plan

1. **Phase 1:** Add `AccessRights` and `HandleCredentials` types (no breaking changes)
2. **Phase 2:** Update `HandleTable::insert()` to populate new fields
3. **Phase 3:** Add use-time checks in sys_read/sys_write/sys_ioctl
4. **Phase 4:** Migrate syscalls from `fd_table` to `handle_table` (one at a time)
5. **Phase 5:** Remove `fd_table` and `FileDescriptor` enum
6. **Phase 6:** Add `dup_restricted()` for capability delegation

## References

- `docs/roadmap-revised/architecture-review/kernel-object-model.md` — Full architectural review
- Linux `include/linux/fs.h` — `struct file` with `f_mode` (FMODE_READ/FMODE_WRITE)
- Linux `include/uapi/asm-generic/fcntl.h` — `O_ACCMODE` mask
- Capability model: `include/uapi/linux/capability.h`
