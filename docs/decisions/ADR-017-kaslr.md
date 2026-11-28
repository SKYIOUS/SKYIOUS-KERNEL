# ADR-017: KASLR (Kernel Address Space Layout Randomization)

## Status
Accepted (partially implemented — relocations do not work on release builds)

**Proposed** — No team decision required; straightforward security feature.

## Context

KASLR randomizes the kernel's memory layout at boot time, making it harder for attackers to predict kernel addresses. This is a standard security feature in all modern operating systems.

Current state:
- KASLR randomization of page tables implemented (kernel maps at random slide)
- **RELOCATIONS NEVER APPLIED** on release builds — `apply_kaslr_relocations` fails ELF magic check at `kaslr_reloc.rs:338` on every build (issues.md A-6)
- Kernel boots only because all old addresses remain aliased (obfuscation only)
- PIC build path (commit `310b2b4`) places `R_X86_64_RELATIVE` entries in alloc `.rela` section and applies them — **but this is not the default release build**
- RDRAND is available for early entropy

## Decision

**Implement basic KASLR using RDRAND for early entropy + relocations.**

### Design

```text
Boot:
  1. Read RDRAND for random offset (64-bit)
  2. Mask to page-aligned boundary (offset & 0xFFF000)
  3. Add random offset to kernel base address
  4. Update page tables for new address
  5. Apply relocations (RELATIVE) to fix absolute pointers
  6. Jump to randomized entry point
```

### Acceptance Criteria

- [x] Kernel base address randomized each boot (page table mapping)
- [x] No crashes from randomization
- [x] Graceful fallback if RDRAND unavailable (fixed address)
- [x] Page tables updated correctly
- [ ] **Relocations applied on release builds** — **BLOCKED** by A-6 (lld emits non-alloc `.rela` sections; PIC route works but not default)

## Consequences

### Positive

- Prevents predictable-address attacks (on PIC builds)
- Standard security feature
- Zero runtime cost after boot

### Negative

- Slightly longer boot time (RDRAND + page table update + relocations)
- May complicate debugging (addresses change each boot)
- **Current reality: KASLR is obfuscation only on release builds** — relocations never applied (A-6 OPEN)

## Known Issues (from issues.md A-6)

1. `--emit-relocs` on root `.cargo/config.toml` produces `R_X86_64_64` (not `RELATIVE`); lld emits as non-alloc sections
2. PIC route (`relocation-model=static` removed) produces `R_X86_64_RELATIVE` in alloc `.rela` inside `PT_LOAD` — works but only on feature-gated builds
3. `[KASLR-VERIFY] match:` printer still prints `NO` (samples RIP 24 bytes into prologue)
4. SMP≥2 boots with relocations untested; audit profiles untested

## Alternatives Considered

### Alternative 1: No KASLR

**Rejected.** Security requirement. Every modern OS has KASLR.

### Alternative 2: ASLR (full randomization)

**Deferred.** ASLR randomizes user-space too. Start with kernel-only KASLR.

### Alternative 3: Gate KASLR off until relocations work on release builds

**Considered.** Current approach: report honestly at boot (`rela entries=0: refusing to slide`), don't claim success. See A0.5 in PLAN.md.
