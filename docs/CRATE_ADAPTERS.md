# Inter-Crate Adapter Patterns

How Vahi kernel crates communicate across module boundaries using trait-based
dependency inversion.

## Problem

The kernel has three circular dependencies:

```text
task ↔ memory     Process owns AddressSpace; memory needs CURRENT_PROCESS
memory ↔ interrupts  Page fault handler needs memory; IRQ dispatches to memory
task ↔ vfs        Process owns FileDescriptor; VFS needs process info
```

These cycles prevent extracting modules into standalone crates.

## Solution: Trait-Based Dependency Inversion

Each cycle is broken by defining a trait in `vahi-types`. The dependent module
calls through the trait instead of a direct `crate::` import. The kernel's boot
code wires the real implementations at startup.

### Cycle 1: task ↔ memory

```text
Before: memory/paging.rs calls crate::task::process::CURRENT_PROCESS
After:  memory/paging.rs calls vahi_types::ProcessProvider::current_pid()
```

**Trait:** `ProcessProvider`

```rust
pub trait ProcessProvider: Send + Sync {
    fn current_pid(&self) -> Pid;
    fn process_exists(&self, pid: Pid) -> bool;
    fn is_init(&self) -> bool;
    fn current_credentials(&self) -> Credentials;
    fn vmas(&self, pid: Pid) -> Option<&[Vma]>;
    fn is_user_address(&self, addr: VirtAddr) -> bool;
}
```

**Wiring (kernel boot):**
```rust
static PROVIDER: KernelProcessProvider = KernelProcessProvider;
vahi_types::register_process_provider(&PROVIDER);
```

### Cycle 2: memory ↔ interrupts

```text
Before: interrupts/page_fault.rs calls crate::memory::paging::handle_page_fault()
After:  interrupts/page_fault.rs calls vahi_types::PageFaultHandler::handle()
```

**Trait:** `PageFaultHandler`

```rust
pub trait PageFaultHandler: Send + Sync {
    fn handle(&self, fault_addr: VirtAddr, error_code: u64) -> bool;
    fn needs_cow(&self, addr: VirtAddr) -> bool;
}
```

**Wiring:**
```rust
static HANDLER: KernelPageFaultHandler = KernelPageFaultHandler;
vahi_types::register_page_fault_handler(&HANDLER);
```

### Cycle 3: task ↔ vfs

```text
Before: task/process.rs directly uses VFS types
After:  task/process.rs uses vahi_types::FileOps trait
```

**Trait:** `FileOps`

```rust
pub trait FileOps: Send + Sync {
    fn read(&self, buf: &mut [u8], offset: u64) -> Result<usize, i32>;
    fn write(&self, buf: &[u8], offset: u64) -> Result<usize, i32>;
    fn seek(&self, offset: u64, whence: u32) -> Result<u64, i32>;
    fn close(&self) -> Result<(), i32>;
    fn stat(&self) -> Result<FileStat, i32>;
    fn mmap(&self, offset: u64, len: usize, prot: u32, flags: u32) -> Result<VirtAddr, i32>;
}
```

### Additional Adapters

| Trait | Breaks | Registered By |
|-------|--------|--------------|
| `ArchOps` | boot ↔ arch | kernel arch module |
| `TimerSource` | interrupts ↔ task | kernel timer |
| `Driver` | drivers ↔ task | kernel drivers |
| `InterruptController` | drivers ↔ interrupts | kernel apic |
| `BlockDevice` | drivers ↔ vfs | kernel drivers |
| `NicDevice` | drivers ↔ net | kernel drivers |
| `SocketOps` | net ↔ task | kernel net |
| `AcpiProvider` | apic ↔ acpi | kernel acpi |
| `MemoryProvider` | apic ↔ memory | kernel memory |
| `SerialWriter` | apic ↔ serial | kernel serial |

## Registration Flow

```text
1. kernel_main()
2.   arch::init()           → register_arch_ops()
3.   memory::init()         → register_process_provider(), register_page_fault_handler()
4.   timer::init()          → register_timer_source()
5.   apic::init(acpi, mem)  → uses AcpiProvider, MemoryProvider
6.   task::init()           → creates init process
7.   drivers::init()        → registers block/net devices
8.   vfs::init()            → mounts root filesystem
```

## Pattern Rules

1. **Traits live in `vahi-types`** — never in the crate that implements them
2. **Registration is one-shot** — `spin::Once` ensures single initialization
3. **No initialization order dependencies** between extracted crates
4. **Each crate only depends on `vahi-types`** — never on another extracted crate
5. **Kernel wires everything** — extracted crates are pure libraries
