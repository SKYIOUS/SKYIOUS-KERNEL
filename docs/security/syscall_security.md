# Syscall Permission Model

Every system call is subject to permission checks before execution.

## Capability-Based Access Control

SkyOS uses a capability system rather than traditional UID/GID checks for fine-grained access control.

```rust
#[derive(Clone, Copy)]
pub struct Capability(u64);

impl Capability {
    pub const NONE: Capability = Capability(0);
    pub const SYS_TIME: Capability = Capability(1 << 0);
    pub const SYS_ADMIN: Capability = Capability(1 << 1);
    pub const NET_RAW: Capability = Capability(1 << 2);
    pub const NET_ADMIN: Capability = Capability(1 << 3);
    pub const DEV_ACCESS: Capability = Capability(1 << 4);
    pub const DRIVER_LOAD: Capability = Capability(1 << 5);
}
```

## Syscall Permission Checks

Each syscall specifies required capabilities:

```rust
fn sys_clock_settime(ctx: &mut SyscallContext) -> Result<usize, SysError> {
    if !ctx.task.capabilities.contains(Capability::SYS_TIME) {
        return Err(SysError::EPERM);
    }
    // Perform operation
    Ok(0)
}
```

## Default Capabilities

New processes inherit capabilities from their parent. The init process starts with all capabilities. Unprivileged processes have minimal capabilities:
- Basic I/O (read, write, open, close)
- Memory management (mmap, brk)
- Process management (fork, exit, getpid)
- Signal handling (sigaction)
- Time reading (clock_gettime, not settime)

## Privileged Operations

These operations require specific capabilities:
- `settime` requires `SYS_TIME`
- `mount` requires `SYS_ADMIN`
- `raw socket` requires `NET_RAW`
- `load driver` requires `DRIVER_LOAD`
- `access device memory` requires `DEV_ACCESS`

## UID/GID Compatibility

For POSIX compatibility, the kernel also supports traditional UID/GID checks. The effective UID must be 0 (root) or match the target UID for operations like `setuid()` and `kill()`. This layer is implemented on top of the capability system.
