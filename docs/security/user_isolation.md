# Userspace Isolation and Process Separation

SkyOS ensures that userspace processes cannot interfere with each other or the kernel.

## Address Space Isolation

Each process has an independent address space controlled by a separate page table. The kernel enforces:
- No process can access another process's memory
- No process can access kernel memory (except via syscalls)
- The kernel cannot accidentally access userspace memory (SMAP)

```rust
pub struct Process {
    id: ProcessId,
    page_table: PageTable,
    memory_mappings: Vec<MemoryRegion>,
    // ...
}

impl Process {
    pub fn switch_to(&self) {
        unsafe {
            // Load this process's page table
            self.page_table.load();
        }
    }
}
```

## File Descriptor Isolation

File descriptors are process-local. One process cannot access another process's file descriptors. When a file descriptor is shared (via fork or UNIX domain sockets), both processes have independent references to the same kernel object.

## Signal Isolation

Processes can only send signals to:
- Themselves
- Processes in the same process group
- Processes owned by the same user (or root)

Signals cannot be sent across user boundaries without appropriate capabilities.

## IPC Security

IPC mechanisms are subject to access controls:
- **Pipes**: Only accessible by processes that hold the file descriptor
- **Shared memory**: Protected by file permissions on the shm object
- **Message queues**: Protected by file permissions
- **UNIX sockets**: Protected by filesystem permissions

## Resource Limits

Each process has configurable resource limits enforced by the kernel:

| Limit | Description |
|-------|-------------|
| RLIMIT_AS | Maximum address space size |
| RLIMIT_DATA | Maximum data segment size |
| RLIMIT_STACK | Maximum stack size |
| RLIMIT_NOFILE | Maximum file descriptor count |
| RLIMIT_NPROC | Maximum number of processes |
| RLIMIT_MEMLOCK | Maximum locked memory |
| RLIMIT_CPU | Maximum CPU time |

## Process State Transitions

Processes transition between states:
- **Running**: Executing on a CPU
- **Runnable**: Ready to execute, waiting for scheduler
- **Blocked**: Waiting for I/O, IPC, or timer
- **Zombie**: Terminated, waiting for parent to collect exit status
- **Dead**: Resources freed

The kernel validates every state transition to ensure security invariants.
