# ADR-022: WaitableObject Abstraction

## Status

**Proposed** — No team decision required; follows from existing architecture.

## Context

Vahi has ad-hoc polling for different FD types (files, sockets, pipes). This ADR formalizes the WaitableObject abstraction that unifies polling/waiting.

Current state:
- `poll_readable()` / `poll_writable()` on KernelObject trait
- Ad-hoc per-FD-type polling in sys_poll
- No unified wait queue
- No edge-triggered support

## Decision

**Formalize the WaitableObject abstraction for unified polling/waiting.**

### Architecture

```text
WaitableObject (trait)
    ├── poll_readable() -> bool
    ├── poll_writable() -> bool
    ├── register_wait(waiter: Waiter)
    ├── unregister_wait(waiter: Waiter)
    └── notify_waiters()

Waiter
    ├── waker: Waker
    └── interests: InterestSet

WaitQueue
    ├── waiters: Vec<Waiter>
    ├── register(waiter: Waiter)
    ├── unregister(waiter: Waiter)
    └── notify()
```

### New Types

```rust
/// A kernel object that can be waited on.
pub trait WaitableObject: KernelObject {
    /// Check if the object is readable.
    fn poll_readable(&self) -> bool;
    /// Check if the object is writable.
    fn poll_writable(&self) -> bool;
    /// Register a waiter for events.
    fn register_wait(&self, waiter: Waiter);
    /// Unregister a waiter.
    fn unregister_wait(&self, waiter: Waiter);
    /// Notify waiters of events.
    fn notify_waiters(&self);
}

/// A waiter waiting for events on an object.
pub struct Waiter {
    pub waker: Waker,
    pub interests: InterestSet,
}

/// Set of events to wait for.
pub struct InterestSet {
    pub readable: bool,
    pub writable: bool,
    pub error: bool,
    pub hangup: bool,
}

/// A queue of waiters.
pub struct WaitQueue {
    pub waiters: Vec<Waiter>,
}
```

### Acceptance Criteria

- [ ] WaitableObject trait defined
- [ ] Waiter type defined
- [ ] WaitQueue type defined
- [ ] Files implement WaitableObject
- [ ] Sockets implement WaitableObject
- [ ] Pipes implement WaitableObject
- [ ] poll/select use WaitableObject

## Consequences

### Positive

- Unified polling/waiting interface
- Enables epoll/io_uring
- Clean abstraction for async I/O

### Negative

- More types to maintain
- Migration cost (existing code uses ad-hoc polling)

## Alternatives Considered

### Alternative 1: Keep Ad-Hoc Polling

**Rejected.** No formal abstraction makes it hard to add epoll/io_uring.

### Alternative 2: Linux-Style wait_queue_entry

**Adopted.** The Waiter type is similar to Linux's wait_queue_entry.
