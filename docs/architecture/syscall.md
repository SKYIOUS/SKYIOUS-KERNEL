# System Call Interface Design

SkyOS implements syscalls using the `syscall` instruction on x86_64, with the kernel maintaining a dispatch table indexed by syscall number.

## Calling Convention

Syscalls follow this convention:

| Register | Purpose |
|----------|---------|
| RAX      | Syscall number |
| RDI      | 1st argument |
| RSI      | 2nd argument |
| RDX      | 3rd argument |
| R10      | 4th argument |
| R8       | 5th argument |
| R9       | 6th argument |

Return values are placed in RAX. Negative values indicate errors (errno style).

## Syscall Entry

The `syscall` instruction switches to kernel mode with the kernel's GS-base and a dedicated syscall stack (IST). The entry handler saves userspace registers, validates arguments, and dispatches to the appropriate handler:

```rust
pub fn syscall_entry(context: &mut SyscallContext) {
    let number = context.rax;
    match number {
        0 => sys_read(context),
        1 => sys_write(context),
        2 => sys_open(context),
        // ...
        _ => sys_unknown(context),
    }
}
```

## Argument Validation

All userspace pointers passed to syscalls are validated before use:
- Pointers must fall within the process's valid memory regions
- Read/write permissions are checked against the page table
- String lengths are bounded to prevent overflow

## Error Handling

Syscalls return `Result<T, Error>` internally, which is converted to a signed integer. Negative errno values (e.g., `-EINVAL`) are returned to userspace on failure.
