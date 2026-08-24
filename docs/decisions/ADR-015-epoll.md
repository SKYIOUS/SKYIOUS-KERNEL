# ADR-015: epoll Implementation

## Status
Proposed

## Context
The kernel currently supports `select()` and `poll()` for event notification, but both are O(n) — they scan all file descriptors on every call. For servers handling thousands of connections, this is unacceptable:
- nginx with 10K connections: select scans 10K FDs per request
- redis with 10K connections: poll scans 10K FDs per request
- Both scale as O(n) — performance degrades linearly with connection count

Linux introduced `epoll` in 2.0 (2001) to solve this:
- `epoll_create` — create an epoll instance
- `epoll_ctl` — add/modify/remove FDs from interest list
- `epoll_wait` — wait for events (O(1) notification via callback)

## Decision
Implement epoll as a file descriptor with:
- Interest list (Red-Black tree for O(log n) add/remove)
- Ready list (linked list for O(1) event retrieval)
- Callback registration on target FDs (event-driven notification)
- Support for edge-triggered and level-triggered modes

## Consequences
- Performance: O(1) event notification vs O(n) for select/poll
- Memory: Per-epoll instance overhead (~1KB + 16 bytes per monitored FD)
- Complexity: New file type, callback integration with VFS/fd layer
- Compatibility: Required by nginx, redis, Node.js, and most modern servers

## Alternatives Considered
1. **kqueue** — BSD-style, more general but less common in Linux ecosystem
2. **io_uring** — more general but heavier, already planned separately
3. **Keep select/poll** — works but doesn't scale

## References
- Linux epoll: `fs/eventpoll.c`
- FreeBSD kqueue: `sys/event/kqueue.c`
- Windows IOCP: `ntoskrnl/io/iocomp.c`
