# Process and Thread Model

SkyOS implements a lightweight task model where processes and threads share the same underlying task structure.

## Task Structure

Every execution context is represented by a `Task` struct containing:

```rust
pub struct Task {
    id: TaskId,
    state: TaskState,
    context: TaskContext,
    memory: MemorySpace,
    scheduler_info: SchedulerInfo,
    parent: Option<TaskId>,
    children: Vec<TaskId>,
    ipc_ports: Vec<IpcPort>,
}
```

Tasks are reference-counted through `Arc<Task>` and live in the global task registry.

## Processes vs Threads

The distinction between processes and threads is minimal:
- **Processes** have their own address space (`MemorySpace`)
- **Threads** share their parent's address space
- Both are scheduled independently by the async executor

## Process Lifecycle

1. **Creation**: `fork()` or `exec()` creates a new task. `fork()` copies the parent address space (copy-on-write); `exec()` replaces it.
2. **Scheduling**: Tasks are placed in the run queue and await their timeslice.
3. **Blocking**: Tasks block on timers, I/O, IPC messages, or synchronization primitives.
4. **Termination**: `exit()` transitions the task to `Zombie` state until the parent calls `wait()`.

## Thread Local Storage

Each task has a TLS area pointed to by the `FS` segment base register. The kernel allocates TLS during task creation and switches it on context switches.
