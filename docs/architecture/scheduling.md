# Scheduler Design

The SkyOS scheduler is an async-first cooperative multitasking system built around a custom executor.

## Async Executor

The core of the scheduler is a work-stealing async executor inspired by `tokio`. Each CPU core maintains its own run queue, and tasks can be stolen by idle cores for load balancing.

```rust
pub struct Executor {
    local_queue: Crossbeams::ArrayQueue<Task>,
    steal_count: AtomicU64,
    waker_cache: HashMap<TaskId, Waker>,
}
```

## Cooperative Multitasking

Tasks yield control explicitly by returning `Poll::Pending` from their async functions. The executor polls tasks in a round-robin fashion within each priority level. A task that runs for too long without yielding is preempted by a timer interrupt.

## Priority Levels

Three priority levels exist:
1. **Real-time** (priority 0-31): Reserved for interrupt handlers and time-critical drivers
2. **Interactive** (priority 32-127): GUI applications and interactive services
3. **Background** (priority 128-255): Batch processing and non-interactive tasks

Each level uses a separate run queue with proportional-share scheduling across levels.

## Context Switching

Context switches are performed by saving and restoring callee-saved registers (RBP, RBX, R12-R15) along with the instruction pointer and stack pointer. The switch completes in approximately 100-200 nanoseconds on modern hardware.

## Waker Mechanism

When a task blocks on I/O or IPC, it registers a waker with the relevant subsystem. The waker is triggered when the awaited event completes, re-queuing the task in the executor's run queue.
