# vahi-gdt

GDT/IDT/TSS management — global descriptor table, per-CPU TSS, selectors.

## Key Types

| Type | Purpose |
|------|---------|
| `Selectors` | Kernel/user code/data/TSS segment selectors |
| `GdtMemoryProvider` | Trait for stack allocation |
| `SmpProvider` | Trait for CPU identification |

## Per-CPU State

```text
CPU 0: GDT₀ + TSS₀ (BSP)
CPU 1: GDT₁ + TSS₁ (AP, stack allocated during SIPI)
CPU N: GDTₙ + TSSₙ (AP)
```

Each per-CPU TSS contains:
- IST[0] for double fault stack
- RSP0 for privilege stack (Ring 3 → Ring 0 transition)

## Invariants

- GDT loaded before any segment register manipulation
- TSS RSP0 points to current thread's kernel stack top
- Per-CPU instances are allocated and leaked (never freed)
- Privilege stack updated on context switch
