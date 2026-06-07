# Inter-process Communication

SkyOS provides multiple IPC mechanisms designed for different use cases, from lightweight message passing to shared memory.

## Message Passing

The primary IPC mechanism is channel-based message passing. Channels are unidirectional and bounded, with buffer sizes negotiated at creation time. Messages are dynamically sized up to 64 KiB.

```rust
pub struct Channel {
    buffer: RingBuffer<Message>,
    sender: Arc<Endpoint>,
    receiver: Arc<Endpoint>,
}
```

Messages are sent asynchronously: `send()` returns immediately if space is available, or returns `WouldBlock` if the buffer is full. The sender can wait on a capacity notification event.

## Shared Memory

For high-throughput communication, processes can share memory regions via `mmap()` with the `MAP_SHARED` flag. Shared regions are reference-counted and unmapped when all processes detach.

## Signals

Signals provide lightweight notification between processes. Each process has a signal mask and a signal handler table. The kernel delivers signals by modifying the target process's signal queue and waking it if it was blocked in `sigwait()`.

## Ports

UIPC (Userspace IPC) ports are asynchronous communication endpoints identified by a 64-bit port ID. Ports support:
- One-to-one and one-to-many communication
- Priority-tagged messages
- Timeouts on receive operations
