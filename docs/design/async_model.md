# Async/Await Execution Model Design

SkyOS uses Rust's async/await for all kernel operations, from syscall handling to driver I/O.

## Why Async?

Traditional kernel threading models use preemptive multitasking with blocking I/O. This has several drawbacks:
- Thread stacks consume significant memory (typically 4-16 KiB each)
- Context switches are expensive (~1-10 microseconds)
- Synchronization is complex and error-prone

The async model addresses these issues by:
- Using stackless coroutines (state machines) instead of full thread stacks
- Performing context switches only at explicit yield points
- Eliminating most locking through single-threaded execution within tasks

## The Executor

Each CPU core runs an independent async executor. The executor polls tasks in a loop, advancing each task's state machine until it returns `Poll::Pending`. When a task blocks, the executor parks it and polls other tasks.

```rust
impl Executor {
    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        pin_mut!(future);
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut Context::from_waker(&waker)) {
                return result;
            }
            // Park if no progress
        }
    }
}
```

## Non-blocking Drivers

Drivers use async operations for I/O. A disk read, for example, submits the request, returns `Pending`, and is woken when the DMA transfer completes:

```rust
async fn read_block(&self, block: u64) -> Result<Block> {
    self.submit_request(block).await?;
    let result = self.wait_for_completion().await?;
    Ok(result)
}
```

## Interrupt to Async Bridge

Interrupt handlers convert hardware events into async wakeups. When an interrupt fires, the handler places a notification in the relevant driver's event queue and wakes the driver's task via its waker.
