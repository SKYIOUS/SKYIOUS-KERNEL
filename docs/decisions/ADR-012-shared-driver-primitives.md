# ADR-012: Shared Driver Primitives

## Status
Proposed

## Date
2026-08-20

## Context
Every DMA-capable driver reimplements the same three helpers inline: `virt_to_phys(VirtAddr)` (with unwrap on failure), `virt_to_phys_dma` (with the identity-mapped bounce path), and the ring-buffer/queue boilerplate (virtio's virtq, xhci's TRB ring, ahci's command list, nvme's SQ/CQ). The review counted 30+ call sites across `drivers/usb/xhci.rs`, `drivers/usb/uhci.rs`, `drivers/storage/{ahci,nvme,virtio_block}.rs`, `drivers/net/{virtio,e1000}.rs`, `drivers/audio/hda.rs`, `drivers/gpu/virtio_gpu.rs`, `syscalls/mod.rs`, `task/process.rs`.

## Decision
Consolidate into `hal/dma.rs` (existing file, already has helpers):

```rust
// hal/dma.rs
pub fn phys_of(va: VirtAddr) -> PhysAddr            // panics on unmapped; debug assert in dev
pub fn dma_addr(va: VirtAddr) -> u64               // virt_to_phys_dma wrapper (bounce path)
pub struct DmaRing<T> { ... }                      // one producer/consumer ring, per-device instantiation
```

Rules:
- `phys_of` replaces the per-driver `virt_to_phys(...).unwrap()` pattern (one failure contract instead of 8 ad-hoc ones)
- `DmaRing<T>` replaces virtq/TRB/command-list ring boilerplate only where the ring semantics match (SPSC, fixed size); ahci's NCQ list is NOT a ring — keep its structure
- Drivers keep their own register/spec logic; this only extracts the address-mapping and ring mechanics

## Alternatives Considered

### Leave each driver self-contained
- Pros: zero churn, drivers stay spec-local
- Cons: 30 call sites of copy-paste unwrap; the failure contract differs per driver (some `.unwrap()`, some `.expect("...")`, some `.ok_or(())?`) — a wedge in one driver no longer panics elsewhere, inconsistent
- Rejected: dedup is the review's candidate #6, mechanically safe (same ops, same semantics)

### Full DMA abstraction (Buffer trait, pools, scatter-gather)
- Pros: what Linux does
- Cons: a trait with N implementations before we know the N+1th shape; drivers here are small enough that a trait hides spec detail
- Rejected: YAGNI until a driver needs scatter-gather or IOMMU

## Consequences
- One place defines "unmapped VA → what happens" (dev: debug assert; prod: panic with symbol context)
- Ring instantiation becomes `DmaRing::new(n, capacity)`, deleting ~200 lines of near-identical ring math
- virtio/xhci/nvme keep their spec logic; only mechanics move
- `virt_to_phys`/`virt_to_phys_dma` in `memory/` stay as the low-level implementations; `hal/dma` is the sanctioned front door