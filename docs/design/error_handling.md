# Kernel Error Handling Strategy

SkyOS uses a structured approach to error handling that leverages Rust's type system.

## Error Types

The kernel defines several error types for different subsystems:

```rust
#[derive(Debug)]
pub enum Error {
    NotFound,
    PermissionDenied,
    AlreadyExists,
    InvalidInput,
    OutOfMemory,
    Io(IoError),
    Syscall(SysError),
    Vfs(VfsError),
    Driver(DriverError),
}
```

## Error Propagation

Functions return `Result<T, Error>` throughout the kernel. The `?` operator propagates errors upward, with conversion between error types via `From` implementations.

## Errno Mapping

Kernel errors are mapped to POSIX errno values at the syscall boundary:

```rust
impl From<Error> for SysError {
    fn from(err: Error) -> SysError {
        match err {
            Error::NotFound => SysError::ENOENT,
            Error::PermissionDenied => SysError::EACCES,
            Error::AlreadyExists => SysError::EEXIST,
            Error::InvalidInput => SysError::EINVAL,
            Error::OutOfMemory => SysError::ENOMEM,
            Error::Io(_) => SysError::EIO,
            _ => SysError::EIO,
        }
    }
}
```

## No Panic Policy

The kernel core never panics. All potential panic sources are eliminated:
- Array indexing uses `.get()` or checked ranges
- Division uses checked arithmetic
- Memory allocation failures are propagated as errors
- Unwrapping uses `.ok_or()` or `?`

## Debug Assertions

Debug builds include assertions for invariant violations. These are stripped in release builds and are not considered error handling—they exist to catch programming errors during development.

## Userspace Error Handling

Errors returned from syscalls are negative errno values. The libc translates these into `errno` variable setting and returns -1 from the wrapper function.
