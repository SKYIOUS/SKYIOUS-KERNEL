# Adding a New Syscall

This guide walks through adding a new system call to SkyOS.

## Step 1: Define the Syscall Number

Add a new entry in `src/syscalls/syscall_table.rs`:

```rust
pub const SYS_MY_NEW_CALL: usize = 42; // Choose an unused number
```

## Step 2: Implement the Handler

Create a handler function in `src/syscalls/my_call.rs`:

```rust
pub fn sys_my_new_call(ctx: &mut SyscallContext) -> Result<usize, SysError> {
    let arg1 = ctx.rdi as u64;
    let arg2 = ctx.rsi as u64;
    // Implementation logic
    Ok(0)
}
```

## Step 3: Register in Dispatch Table

Add the entry to the dispatch match in `syscall_entry()`:

```rust
SYS_MY_NEW_CALL => sys_my_new_call(context),
```

## Step 4: Add Userspace Wrapper

In `userspace/libc/src/syscalls.rs`:

```rust
pub fn my_new_call(arg1: u64, arg2: u64) -> Result<usize, Errno> {
    unsafe {
        let ret = syscall2(SYS_MY_NEW_CALL, arg1, arg2);
        if ret < 0 { Err(Errno::from(-ret)) } else { Ok(ret as usize) }
    }
}
```

## Step 5: Add Documentation

Update the syscall documentation in `docs/syscalls/` with the new entry, including its purpose, arguments, return values, and error conditions.
