# Kernel Object Model — Deep Architectural Review

## Executive Summary

The kernel object model (`KernelObject` trait, `HandleTable`, `SecurityDescriptor`, `Credentials`) is the right architectural direction but has **7 critical defects** and **5 structural issues** that must be resolved before it can serve as the foundation for security, capability-based access control, and Linux compatibility.

**Overall Grade: C+** — Good bones, incomplete implementation, dual-table debt.

---

## Critical Findings

### Finding 1: Dual FD Table System (CRITICAL)

**Severity:** P0 — Architectural debt that will block every future feature.

The codebase maintains **two completely separate file descriptor systems**:

| System | Type | Uses | Security |
|--------|------|------|----------|
| `fd_table` | `Vec<Option<FileDescriptor>>` | **60 call sites** | Only O_ACCMODE check |
| `handle_table` | `HandleTable` (KernelObject-based) | **13 call sites** | Bind-time access_check |

These must stay synchronized — both tables must agree on handle numbers. The comment in `syscalls/helpers.rs:198` reveals the fragility:

> *"the two tables must agree on what an fd number means. A forked child has an empty handle table but inherits stdio fds 0-2 in fd_table"*

**Impact:**
- Every new syscall must maintain two tables
- fork() must synchronize both tables
- dup/dup2 must coordinate both tables
- close() must coordinate both tables
- Security checks are inconsistent (fd_table has none, handle_table has bind-time only)

**Recommendation:** Migrate all syscalls to `handle_table` and remove `fd_table`. The `FileDescriptor` enum should become a set of `KernelObject` implementations.

### Finding 2: Security Checks Only at Bind Time (CRITICAL)

**Severity:** P0 — Security vulnerability.

`access_check()` is called **only** in two places:
1. `HandleTable::insert()` — at open time
2. `syscalls/helpers.rs:190` — at open time

**Never called during I/O operations.** Once a handle is opened, any operation is permitted regardless of the original `access_mask`.

**Attack scenario:**
1. Open file with O_RDONLY (access_mask = ACCESS_READ)
2. Handle stored in handle_table with access_mask = 4 (READ)
3. sys_write() proceeds — no check that access_mask includes ACCESS_WRITE
4. Data written to read-only file

**Impact:** Complete bypass of DAC security model after first open.

**Recommendation:** Add use-time access checking in sys_read/sys_write/sys_ioctl. Store access_mask in HandleEntry and check at each operation.

### Finding 3: `access_mask` Stored But Never Checked at Runtime (CRITICAL)

**Severity:** P0 — Dead security code.

`HandleEntry.access_mask` is set during `insert()` but **never read** by any I/O operation. The only access-mode check is in `sys_read` which checks `fd_flags` (the legacy `O_ACCMODE` bits):

```rust
// sys_read checks fd_flags, not handle_table access_mask
if (fd as usize) < fdfl.len() && (fdfl[fd as usize] & 3) == 1 {
    return errno::Errno::EBADF as u64;
}
```

This means:
- `access_mask` in HandleEntry is completely dead at runtime
- The security model is enforced only by the legacy `fd_flags` system
- The HandleTable security integration is a lie

### Finding 4: KernelObject Trait is Overloaded (HIGH)

**Severity:** P1 — Interface Segregation Principle violation.

The `KernelObject` trait has **20+ methods** covering:

- File-like: `read`, `write`, `ioctl`, `stat`, `truncate`, `poll_readable`, `poll_writable`
- Socket-like: `socket_bind`, `socket_connect`, `socket_listen`, `socket_accept`, `socket_peer_name`, `socket_local_name`
- Lifecycle: `on_handle_create`, `on_handle_close`, `on_close`
- Metadata: `type_name`, `query_name`, `set_name`

Every implementor must provide 20+ default methods. A Socket implements `read/write/truncate` with `Err(())`. A File implements `socket_bind/socket_listen` with `Err(())`.

**Impact:**
- Compile-time: every type pays for 20+ vtable entries
- Runtime: method dispatch overhead for irrelevant operations
- Readability: unclear what operations a type actually supports
- Safety: no compile-time guarantee that a Socket can't be `read()`

**Recommendation:** Split into focused traits:
- `KernelObject` — minimal: `header()`, `type_id()`, `type_name()`
- `FileLike` — `read`, `write`, `ioctl`, `stat`, `truncate`
- `Pollable` — `poll_readable`, `poll_writable`
- `SocketLike` — `socket_bind`, `socket_connect`, etc.
- `Closeable` — `on_close`, `on_handle_close`

### Finding 5: Credentials are Incomplete (HIGH)

**Severity:** P1 — POSIX compliance gap.

Current `Credentials` struct:
```rust
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
    pub cap_effective: u64,
}
```

Missing fields required for POSIX/Linux compatibility:
- `supplementary_groups: Vec<u32>` — for group-based access checks
- `cap_permitted: u64` — permitted capabilities (bounding set)
- `cap_inheritable: u64` — inheritable capabilities
- `cap_ambient: u64` — ambient capabilities (Linux 4.3)
- `securebits: u32` — SUGID/SUGID_FIFO behavior
- `no_new_privs: bool` — prevent privilege escalation on exec

**Impact:**
- `setgroups()` syscall cannot work
- Capability model is incomplete (only effective, no permitted/inheritable)
- SUID/SGID cannot be correctly implemented
- `execve()` cannot correctly drop capabilities

### Finding 6: SecurityDescriptor ACL Checking is Incomplete (HIGH)

**Severity:** P1 — Security bypass.

Current ACL check:
```rust
for ace in &sec.acl {
    if ace.uid == cred.euid || ace.uid == cred.uid {
        if ace.access_mask & desired == desired {
            return ace.ace_type == AceType::Allow;
        }
    }
}
```

Problems:
1. **No group matching** — ACLs only check uid, not gid or supplementary groups
2. **No deny-first semantics** — Deny ACEs are checked in the same loop as Allow
3. **No ACL inheritance** — ACLs don't propagate through directory hierarchies
4. **No POSIX ACL semantics** — Linux ACLs have user/group/other/mask/extended

**Impact:** ACL-based security is non-functional for group-based access control.

### Finding 7: No Right Delegation or Transfer (MEDIUM)

**Severity:** P2 — Missing capability model feature.

`HandleTable::dup()` copies the entire `access_mask`:
```rust
let new_entry = HandleEntry {
    object: entry.object.clone(),
    access_mask: entry.access_mask,  // full copy
    flags: entry.flags,
    ...
};
```

No way to:
- Restrict rights on dup (like `fcntl(F_DUPFD_CLOEXEC)`)
- Delegate a subset of rights
- Create a read-only clone of a read-write handle

**Impact:** Cannot implement capability-based access control where a process grants limited access to a child.

---

## Structural Issues

### Issue 1: HandleTable Uses Linear Scan

`HandleTable::insert()` does a linear scan to find a free slot:
```rust
for (i, slot) in self.table.iter_mut().enumerate() {
    if slot.is_none() { ... }
}
```

With 1000+ open FDs, this is O(n) per open. Linux uses a radix tree for O(1) amortized.

### Issue 2: HandleEntry Clones Arc on Every Dup

`dup()` clones the `Arc<dyn KernelObject>` — this is correct but the clone is expensive for hot paths. Consider reference-counted handles.

### Issue 3: No Close-on-Exec Semantics

`clone_table()` for fork doesn't filter by `O_CLOEXEC`:
```rust
pub fn clone_table(&self) -> Vec<Option<HandleEntry>> {
    // ponytail: simple clone, no close-on-exec filtering needed yet
    self.table.clone()
}
```

This means all handles are inherited across fork+exec, violating POSIX semantics.

### Issue 4: SecurityDescriptor is Mutex-Locked Per-Object

Every `access_check` acquires `object.header().security.lock()`. For hot-path operations (read/write), this adds lock contention. Consider:
- Caching credentials at open time
- Using RCU for read-side access checks
- Using per-handle security snapshot

### Issue 5: No Audit Trail

`audit_id` is assigned but never read. No syscall logging, no security event recording. For a production kernel, this is a significant gap.

---

## Proposed Capability/Rights Redesign

### Design Principles

1. **Bind-time + Use-time checks** — Open checks permissions, I/O checks access mode
2. **Structured rights** — Not just u32 bitmask, but typed capability sets
3. **Right restriction on dup** — Can grant subset of rights to child
4. **Credential snapshot at open** — Avoid repeated lock acquisition
5. **POSIX-compatible** — Must work with Linux ABI

### New Types

```rust
/// Structured access rights for kernel objects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AccessRights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub append: bool,
    pub truncate: bool,
    pub ioctl: bool,
}

impl AccessRights {
    pub const READ_ONLY: Self = Self { read: true, write: false, execute: false, append: false, truncate: false, ioctl: false };
    pub const READ_WRITE: Self = Self { read: true, write: true, execute: false, append: false, truncate: true, ioctl: true };
    pub const ALL: Self = Self { read: true, write: true, execute: true, append: true, truncate: true, ioctl: true };

    /// Can `self` perform the requested operation?
    pub fn allows(&self, requested: AccessRights) -> bool {
        (!requested.read || self.read) &&
        (!requested.write || self.write) &&
        (!requested.execute || self.execute) &&
        (!requested.append || self.append) &&
        (!requested.truncate || self.truncate) &&
        (!requested.ioctl || self.ioctl)
    }

    /// Restrict rights — result is intersection of self and restriction.
    pub fn restrict(&self, restriction: AccessRights) -> Self {
        Self {
            read: self.read && restriction.read,
            write: self.write && restriction.write,
            execute: self.execute && restriction.execute,
            append: self.append && restriction.append,
            truncate: self.truncate && restriction.truncate,
            ioctl: self.ioctl && restriction.ioctl,
        }
    }
}

/// Complete credentials snapshot at handle creation time.
#[derive(Clone, Debug)]
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

/// Enhanced handle entry with structured rights and credential snapshot.
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
    ) -> Result<HandleValue, ()> {
        let sec = object.header().security.lock();
        let creds = Credentials::from_handle_credentials(&credentials);
        if !security::access_check(&creds, &sec, &desired_rights) {
            return Err(());
        }
        // ... insert with rights and credential snapshot
    }

    /// Duplicate with right restriction.
    pub fn dup_restricted(
        &mut self,
        old_handle: HandleValue,
        restriction: AccessRights,
    ) -> Result<HandleValue, ()> {
        let entry = self.get(old_handle).ok_or(())?;
        let restricted_rights = entry.rights.restrict(restriction);
        // ... insert with restricted rights
    }

    /// Check access rights at use time.
    pub fn check_access(
        &self,
        handle: HandleValue,
        requested: AccessRights,
    ) -> Result<&HandleEntry, ()> {
        let entry = self.get(handle).ok_or(())?;
        if entry.rights.allows(requested) {
            Ok(entry)
        } else {
            Err(())
        }
    }

    /// Filter by close-on-exec flag during fork.
    pub fn clone_for_fork(&self, close_on_exec: bool) -> Vec<Option<HandleEntry>> {
        self.table.iter().map(|slot| {
            if close_on_exec && slot.as_ref().map_or(false, |e| e.flags & O_CLOEXEC != 0) {
                None
            } else {
                slot.clone()
            }
        }).collect()
    }
}
```

### Migration Path

1. **Phase 1:** Add `AccessRights` and `HandleCredentials` types alongside existing `access_mask: u32`
2. **Phase 2:** Update `HandleTable::insert()` to populate new fields
3. **Phase 3:** Add use-time checks in sys_read/sys_write/sys_ioctl
4. **Phase 4:** Migrate syscalls from `fd_table` to `handle_table` (one syscall at a time)
5. **Phase 5:** Remove `fd_table` and `FileDescriptor` enum
6. **Phase 6:** Add `dup_restricted()` for capability delegation

---

## Recommendations

### Immediate (P0)

1. **Add use-time access checks** — Check `access_mask` in sys_read/sys_write before proceeding
2. **Start fd_table migration** — Begin migrating one syscall (e.g., sys_read) to use handle_table exclusively
3. **Fix Credentials** — Add supplementary_groups, cap_permitted, cap_inheritable

### Short-term (P1)

4. **Split KernelObject trait** — Separate FileLike, SocketLike, Pollable traits
5. **Fix ACL group matching** — Check gid and supplementary groups in access_check
6. **Add close-on-exec filtering** — Filter in clone_table for fork+exec
7. **Add AccessRights type** — Replace u32 bitmask with structured rights

### Medium-term (P2)

8. **Complete fd_table migration** — Remove legacy fd_table entirely
9. **Add credential snapshot** — Store HandleCredentials at open time
10. **Add dup_restricted** — Enable right delegation to child processes
11. **Add HandleTable caching** — Use radix tree for O(1) handle lookup

### Long-term (P3)

12. **Add audit trail** — Log security-relevant operations
13. **Add capability bounding set** — Prevent capability escalation
14. **Add LSM hooks** — Integrate with Linux Security Module framework
