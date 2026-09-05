# vahi-boot

Boot sequence and initialization — state machine, early memory, ACPI discovery.

## Boot State Machine

```text
Firmware → Loader → Memory → Heap → Scheduler → SMP → Drivers → Userspace
   0         1        2       3        4         5       6          7
```

Each state is strictly monotonic — never goes backward.

## Initialization Order

| Step | State | What | Depends On |
|------|-------|------|------------|
| 1 | Firmware | CPU mode, GDT | Nothing |
| 2 | Loader | Limine protocol handshake | Firmware |
| 3 | Memory | Frame allocator, page tables | Loader |
| 4 | Heap | Global heap allocator | Memory |
| 5 | Scheduler | Process table, init process | Heap |
| 6 | SMP | AP startup via SIPI | Scheduler |
| 7 | Drivers | PCI enumeration, driver probe | SMP |
| 8 | Userspace | exec /init | Drivers |

## Modules

| Module | Contents |
|--------|----------|
| `state` | Transition result types, validation |
| `logger` | Early boot serial logger |

## Invariants

- Boot state transitions are **monotonic**
- Memory init must complete before heap init
- Frame allocator initialized before any allocation
- Scheduler initialized before SMP
