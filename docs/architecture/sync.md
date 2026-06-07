# Synchronization Primitives

SkyOS provides a full set of synchronization primitives built on atomic operations and the scheduler.

## Spinlocks

Spinlocks are the lowest-level synchronization primitive, used in interrupt handlers and extremely short critical sections. They disable interrupts on the local CPU while held to prevent deadlocks.

```rust
pub struct Spinlock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}
```

Spinlocks are marked with `SpinlockIrqSave` to track interrupt state on acquisition and restore it on release.

## Mutexes

For longer critical sections, the kernel uses sleeping mutexes. When a thread cannot acquire a mutex, it is removed from the run queue and placed in a wait queue. The mutex owner's priority is temporarily boosted to prevent priority inversion.

## Conditional Variables

Condition variables allow threads to wait for a condition to become true. They are used with mutexes:

```rust
let mut data = mutex.lock();
while !condition(&data) {
    condvar.wait(&mut data);
}
```

## Futexes

Futexes (fast userspace mutexes) provide a hybrid approach: the fast path is entirely in userspace using atomic compare-and-swap, and only contention triggers a syscall. This makes them the preferred synchronization mechanism for performance-sensitive userspace code.

## Barrier

Barriers synchronize a group of threads at a point in execution. The kernel barrier implementation uses a generation counter to handle reuse safely.
