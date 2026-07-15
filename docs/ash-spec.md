# ASH — Application-Specific Safe Handlers

## Overview

ASH (Application-Specific Safe Handlers) is a mechanism in the Vahi Kernel
that allows `libsarga` to download safe eBPF bytecode that runs in the NIC
interrupt context, enabling sub-microsecond packet response without scheduling
userspace.

## Lifecycle

```
Load → Verify → Install → Execute (IRQ) → Results Collection
```

1. **Load**: Userspace calls `bpf(BPF_PROG_LOAD, ...)` to load an eBPF program.
   The program undergoes standard eBPF verification (structural + tnum abstract
   interpretation).

2. **Verify**: The program is additionally checked for IRQ safety:
   - ≤ 512 instructions (ASH limit)
   - No unsafe helper calls (only helpers 1–3: map_lookup, get_pid, get_ticks)
   - tnum verification passes (bounded loops, stack bounds, no div-by-zero)

3. **Install**: Userspace calls `bpf(BPF_PROG_ATTACH, fd, BPF_ATTACH_ASH,
   "protocol:port")` to install the handler. The target string specifies
   protocol and port filtering (e.g. `"6:80"` for TCP port 80, `"1:0"` for all
   ICMP). The program bytecode is copied into an IRQ-safe handler table.

4. **Execute (IRQ)**: On each NIC interrupt, before the main network stack
   processes the packet, every matching ASH handler runs on a pre-allocated
   per-CPU stack. No heap allocation occurs.

5. **Results Collection**: The handler's return value (R0) determines the
   action the NIC driver should take.

## Register ABI

| Register | Input/Output | Description |
|----------|-------------|-------------|
| R1       | Input       | Packet data pointer |
| R2       | Input       | Packet length |
| R3       | Input       | Protocol number (1=ICMP, 6=TCP, 17=UDP) |
| R4       | Input       | Destination port |
| R5       | Output      | Destination IP for reply (DropWithReply) |
| R6       | Output      | Destination port for reply (DropWithReply) |
| R7       | Output      | Reply data length (DropWithReply) |
| R0       | Output      | Action code (0=Pass, 1=Drop, 2=DropWithReply) |

## Action Codes

| Value | Constant        | Description |
|-------|-----------------|-------------|
| 0     | `ASH_PASS`      | Pass packet to normal network stack |
| 1     | `ASH_DROP`      | Drop packet silently |
| 2     | `ASH_DROP_REPLY`| Drop and initiate reply (read R5–R7) |

## Allowed Helpers

ASH programs may only call helpers that are safe in IRQ context
(no blocking, no allocation, no console I/O):

| Helper ID | Function              | Reason |
|-----------|-----------------------|--------|
| 1         | `map_lookup_elem`     | Pre-allocated maps, lock-free read |
| 2         | `get_current_pid`     | Returns cached process ID |
| 3         | `get_ticks`           | Returns monotonically increasing tick counter |

Helper 4 (`debug_print`) and any future helpers that acquire locks or
allocate memory are **forbidden** in ASH context.

## Safety Guarantees

| Guarantee | Mechanism |
|-----------|-----------|
| No heap allocation in IRQ | Pre-allocated per-CPU stack (`AshPerCpu`), no `alloc` in execution path |
| Bounded loops | tnum abstract interpretation proves termination (Phase 4 verifier) |
| Stack bounds | tnum verification checks all memory accesses are within `[0, STACK_SIZE)` |
| No division by zero | tnum verifier rejects programs where divisor could be zero |
| Bounded instruction count | `is_ash_safe()` enforces ≤ 512 instructions |
| No unsafe helpers | `is_ash_safe()` restricts helper calls to IDs 1–3 |
| Register bounds | Structural verifier enforces dst_reg/src_reg ≤ 10, no write to R10 |
| Interrupt-safe | Called with interrupts disabled; no blocking operations |

## Data Structures

```rust
pub struct AshHandler {
    pub prog_id: u64,
    pub insns: Vec<EbpfInsn>,
    pub can_initiate: bool,
    pub protocol: u8,
    pub dst_port: u16,
}

pub struct AshPerCpu {
    pub stack: [u8; STACK_SIZE],  // 512 bytes
    pub regs: EbpfRegs,
}
```

## Syscall Integration

`BPF_PROG_ATTACH` with `attach_type = 4` (`BPF_ATTACH_ASH`):

```
target format: "<protocol>:<port>"
  "6:80"    → TCP port 80
  "1:0"     → All ICMP
  "0:0"     → All protocols and ports
```

`BPF_PROG_DETACH` with `attach_type = 4` removes the handler.

## File Locations

| File | Purpose |
|------|---------|
| `kernel/src/ebpf/ash.rs` | ASH execution engine |
| `kernel/src/ebpf/ash_tests.rs` | Self-tests for ASH |
| `kernel/src/ebpf/verifier.rs` | `is_ash_safe()` — IRQ safety verifier |
| `kernel/src/interrupts.rs` | ASH invocation in NIC interrupt handler |
| `docs/ash-spec.md` | This document |

## Reply Transmission (Future)

The `ash_send_udp()` function constructs a minimal UDP/IP packet and writes
directly to the NIC's TX ring. This requires:
- Pre-allocated TX descriptors per CPU
- IP header checksum calculation (no allocation)
- UDP checksum offload or manual calculation
- DMA-compatible buffer management

Currently `ash_send_udp()` is a stub returning `false`. Full TX-path
integration is tracked for a future milestone.
