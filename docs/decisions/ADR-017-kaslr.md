# ADR-017: KASLR (Kernel Address Space Layout Randomization)

## Status

**Proposed** — No team decision required; straightforward security feature.

## Context

KASLR randomizes the kernel's memory layout at boot time, making it harder for attackers to predict kernel addresses. This is a standard security feature in all modern operating systems.

Current state:
- No KASLR implementation exists
- Kernel base address is fixed at `0xffffffff80000000`
- RDRAND is available for early entropy

## Decision

**Implement basic KASLR using RDRAND for early entropy.**

### Design

```text
Boot:
  1. Read RDRAND for random offset (64-bit)
  2. Mask to page-aligned boundary (offset & 0xFFF000)
  3. Add random offset to kernel base address
  4. Update page tables for new address
  5. Jump to randomized entry point
```

### Acceptance Criteria

- [ ] Kernel base address randomized each boot
- [ ] No crashes from randomization
- [ ] Graceful fallback if RDRAND unavailable (fixed address)
- [ ] Page tables updated correctly

## Consequences

### Positive

- Prevents predictable-address attacks
- Standard security feature
- Zero runtime cost

### Negative

- Slightly longer boot time (RDRAND + page table update)
- May complicate debugging (addresses change each boot)

## Alternatives Considered

### Alternative 1: No KASLR

**Rejected.** Security requirement. Every modern OS has KASLR.

### Alternative 2: ASLR (full randomization)

**Deferred.** ASLR randomizes user-space too. Start with kernel-only KASLR.
