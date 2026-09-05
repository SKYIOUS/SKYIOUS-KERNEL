# Module Interface: `task`

**Path:** `kernel/src/task/`
**Owner:** TBD
**Tier:** 2 (depends on memory, sync, arch)

---

## Public API

### Process (`task/process.rs`)

```rust
// ─── Process Creation ────────────────────────────────────────────

/// Create a new process with the given PID, parent, and address space.
/// The process starts with empty fd table, default credentials, and Native emulation.
pub fn Process::new(pid: u64, parent_id: Option<u64>, address_space: AddressSpace) -> Process

/// Get the next unique PID (atomic counter, starts at 100).
pub fn Process::next_id() -> u64

/// Register a process in the global PROCESS_TABLE.
/// Must be called after process creation to make it findable.
pub fn Process::register(proc: Arc<Process>)

/// Load an ELF binary into a new address space.
/// Returns a fully initialized Process with entry point, stack, and mappings.
pub fn Process::load_elf(data: &[u8], address_space: AddressSpace) -> Result<Process, ElfError>

// ─── Process Access ──────────────────────────────────────────────

/// Global current process pointer. Use try_lock() in IRQ context.
pub static CURRENT_PROCESS: IrqSafeMutex<Option<Arc<Process>>>

/// Global process table. Use lock() for reads, try_lock() in IRQ context.
pub static PROCESS_TABLE: IrqSafeMutex<BTreeMap<u64, Arc<Process>>>

// ─── Process State ───────────────────────────────────────────────

/// File descriptor table per process.
pub struct ProcessFiles {
    pub fd_table: Vec<Option<FileDescriptor>>,
    pub fd_flags: Vec<u64>,
    pub dir_fds: Vec<u64>,
}

/// Credentials (UID/GID) per process.
pub struct Credentials {
    pub uid: u32, pub gid: u32,
    pub euid: u32, pub egid: u32,
    pub suid: u32, pub sgid: u32,
}

/// Emulation mode per process.
pub enum EmulationMode { Native, Linux, Windows }
```

### Thread (`task/thread.rs`)

```rust
/// Create a clone of the current thread for fork/clone.
pub fn Thread::clone_thread(
    process: Arc<Process>,
    regs_ptr: *mut u64,
    child_stack: u64,
) -> Option<Thread>

/// Switch from current thread to next thread.
/// Saves current RSP, restores next RSP, swaps FPU state.
pub fn switch_thread(
    old_rsp: *mut u64,
    new_rsp: u64,
    new_fs_base: u64,
    old_fpu: *mut FpuArea,
    new_fpu: *const FpuArea,
)
```

### Scheduler (`task/scheduler/`)

```rust
/// Get the per-CPU scheduler for the given CPU.
pub fn cpu_sched(cpu: usize) -> Option<&'static IrqSafeMutex<PerCpuScheduler>>

/// Get the current CPU's scheduler.
pub fn this_cpu_sched() -> &'static IrqSafeMutex<PerCpuScheduler>

/// Try to schedule the next thread. Non-blocking (uses try_lock).
pub fn try_schedule()

/// Block the current thread and switch to the next runnable thread.
pub fn schedule()

/// Add a thread to the global pending queue.
pub fn spawn_thread(thread: Thread)
```

---

## Invariants

1. **PID uniqueness:** Every process has a unique PID. PIDs start at 100. PID 1 is reserved for init.
2. **Parent-child consistency:** Every process (except PID 1) has a parent. Every parent's children list contains only valid PIDs.
3. **CURRENT_PROCESS is always set:** After boot, CURRENT_PROCESS is always Some. It is only None during early boot.
4. **Schedule safety:** `schedule()` never returns if there are other runnable threads. It only returns when the current thread is the only runnable thread.
5. **try_lock in IRQ context:** All scheduler operations in IRQ context (tick, timer) use try_lock(). Never use lock() in IRQ context.

---

## Testing Requirements

| Test | What It Validates | Priority |
|------|-------------------|----------|
| `process:create` | Process creation, PID, credentials | ✅ Exists |
| `process:fd_inherit` | fd table cloning | ✅ Exists |
| `process:emulation_mode` | Emulation mode get/set | ✅ Exists |
| `process:register_lookup` | Process table register/lookup | ✅ Exists |
| `process:try_lock` | CURRENT_PROCESS.try_lock() | ✅ Exists |
| `scheduler:tick` | Timer tick processes correctly | ✅ Exists |
| `scheduler:sleep_queue` | Sleep/wake cycle | ✅ Exists |
| **NEW: process:fork_exec_cycle** | Fork → exec → exit → wait cycle | ❌ Needed |
| **NEW: process:rapid_fork_exit** | Stress test fork/exit | ❌ Needed |
| **NEW: scheduler:starvation** | No thread starves indefinitely | ❌ Needed |

---

## Common Pitfalls

1. **Nested IrqSafeMutex:** Never hold CURRENT_PROCESS.lock() while calling another function that locks CURRENT_PROCESS. Use try_lock() for optional operations.
2. **IRQ context:** Never allocate heap memory, take blocking locks, or call schedule() from IRQ context.
3. **Stale references:** After fork, the child's Arc<Process> is independent. Dropping the parent does not affect the child.
4. **File descriptor inheritance:** Clone fd table at the START of exec, before any heavy work. The clone must use a single lock scope.
