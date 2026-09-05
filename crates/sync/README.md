# vahi-sync

IRQ-safe spin mutex for `#![no_std]` kernels.

## What It Does

Disables interrupts while the lock is held, preventing the timer IRQ from
preempting a thread mid-critical-section and stranding the lock. Restores
interrupt state on drop.

## API

```rust
let data = IrqSafeMutex::new(MyData::new());

// Lock (disables interrupts)
{
    let mut guard = data.lock();
    guard.modify();
} // unlocks + restores interrupts

// Try-lock (for IRQ context)
if let Some(guard) = data.try_lock() {
    // Got the lock
}
```

## When to Use

- Protecting shared state accessed from both thread and IRQ context
- Short critical sections (do NOT hold across blocking operations)
- Anywhere `spin::Mutex` is used but IRQ safety is needed

## When NOT to Use

- Long critical sections (interrupts are disabled while held)
- Blocking operations (sleep, wait, yield) — deadlock risk
- Single-threaded code (use `spin::Mutex` instead)
- Already in IRQ context with a lock held (deadlock risk)

## Invariants

- Interrupts are disabled while the guard is held
- Original interrupt state is restored on guard drop
- `try_lock()` re-enables interrupts if they were enabled before the attempt
