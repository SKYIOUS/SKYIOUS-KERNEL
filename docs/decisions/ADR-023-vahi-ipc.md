# ADR-023: Vahi IPC Model

## Status

**Proposed** — No team decision required; Vahi-native feature.

## Context

Vahi has Unix sockets for IPC. This ADR defines the Vahi-native IPC model that goes beyond Unix sockets.

Current state:
- Unix sockets work
- Shared memory (shm) works
- No unified IPC primitive
- No capability-based IPC
- No Vahi-specific IPC

## Decision

**Define Vahi IPC as a capability-based messaging primitive built on top of Unix sockets.**

### Architecture

```text
┌─────────────────────────────────────────────┐
│              Userspace                       │
│  vahi_send(endpoint, msg, caps)              │
│  vahi_recv(endpoint) -> (msg, caps)          │
└──────────────┬──────────────────────────────┘
               │ syscall
               ▼
┌─────────────────────────────────────────────┐
│           Vahi IPC Layer                     │
│  ┌─────────────────────────────────────┐    │
│  │        IPC Endpoint                 │    │
│  │  • Kernel object                    │    │
│  │  • Message queue                    │    │
│  │  • Capability slot                  │    │
│  └─────────────────────────────────────┘    │
│  ┌─────────────────────────────────────┐    │
│  │        Message Format               │    │
│  │  • Header (type, length, flags)     │    │
│  │  • Payload (bytes)                  │    │
│  │  • Capabilities (handle references) │    │
│  └─────────────────────────────────────┘    │
└─────────────────────────────────────────────┘
```

### New Types

```rust
/// An IPC endpoint (kernel object).
pub struct IpcEndpoint {
    pub header: ObjectHeader,
    pub message_queue: VecDeque<Message>,
    pub capabilities: Vec<CapabilitySlot>,
}

/// A message sent through IPC.
pub struct Message {
    pub header: MessageHeader,
    pub payload: Vec<u8>,
    pub capabilities: Vec<HandleValue>,
}

/// A capability slot in a message.
pub struct CapabilitySlot {
    pub handle: HandleValue,
    pub rights: AccessRights,
}

/// Message header.
pub struct MessageHeader {
    pub msg_type: u32,
    pub msg_len: u32,
    pub msg_flags: u32,
}
```

### New Syscalls

```rust
/// Create an IPC endpoint.
pub fn sys_vahi_endpoint_create(name: *const u8) -> u64;

/// Send a message through IPC.
pub fn sys_vahi_send(
    endpoint_fd: u64,
    msg_ptr: *const u8,
    msg_len: u64,
    caps_ptr: *const u64,
    caps_len: u64,
) -> u64;

/// Receive a message from IPC.
pub fn sys_vahi_recv(
    endpoint_fd: u64,
    msg_ptr: *mut u8,
    msg_len: u64,
    caps_ptr: *mut u64,
    caps_len: u64,
) -> u64;
```

### Acceptance Criteria

- [ ] IpcEndpoint type defined
- [ ] Message type defined
- [ ] CapabilitySlot type defined
- [ ] sys_vahi_endpoint_create works
- [ ] sys_vahi_send works
- [ ] sys_vahi_recv works
- [ ] Capabilities transferred correctly

## Consequences

### Positive

- Capability-based IPC (secure by default)
- Unified IPC primitive (beyond Unix sockets)
- Vahi-native differentiator

### Negative

- More types to maintain
- New syscall surface
- Userspace needs to adopt new API

## Alternatives Considered

### Alternative 1: Unix Sockets Only

**Rejected.** Unix sockets don't support capability transfer. Vahi needs capability-based IPC.

### Alternative 2: Linux-Style IPC (msg queues, semaphores)

**Rejected.** Linux IPC is legacy and not capability-based. Vahi needs a modern design.

### Alternative 3: microkernel-style IPC

**Partially adopted.** Vahi IPC is inspired by microkernel IPC but built on Unix socket semantics.
