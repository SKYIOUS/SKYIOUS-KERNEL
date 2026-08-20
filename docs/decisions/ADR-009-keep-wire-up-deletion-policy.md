# ADR-009: Keep and Wire Up — Deletion Policy for Half-Built Subsystems

## Status
Accepted

## Date
2026-08-20

## Context
The architecture review (12 candidates, 2026-08-20) found several subsystems that are either half-built, orphaned, or fake. Three options existed per candidate: keep + wire up, keep as-is, or delete outright. Prior ADRs (001 monolithic kernel, 008 feature flags) constrain the options.

## Decision
Three-way classification, decided by interview with the maintainer:

- **Group A — KEEP + WIRE UP (plan 2):** `ash/` (real design, 2 call-sites to hook), `hypervisor/` (real VMCS/vCPU design, needs init wiring), `verified/` (proof-based verification runner, needs main.rs wiring), `compositor/` (real GUI compositor, needs gui/ seam), `ebpf/jit.rs` (real JIT, needs a caller).
- **Group B — DELETE:** `vahiai/` (fake-AI intent ladder), `korlang/` (stub ABI with no compiler), `kext/` (does not compile — references nonexistent `pci::publish_nubs`).
- **Group C — DELETE (broken/orphaned):** `rtsched.rs`, `memory/pressure.rs`, `ebpf/ash*.rs`, `tests/hypervisor_tests.rs`, `memory/virt.rs` (later KEPT — it is live via `tests/new_features.rs`), `threading_demo`, repo-root logs.
- **KEEP + FOLD:** `shell/commands/*` into `gui/terminal.rs` (the live shell).

Git history preserves everything deleted (`git checkout <commit> -- path`).

## Alternatives Considered

### Delete everything unproven
- Pros: smallest kernel, no maintenance tax
- Cons: discards real designs (hypervisor VMCS, verified proofs, JIT) the maintainer wants to build on
- Rejected by maintainer: "if anything exists to be used in future, let it be there and prepare a plan to make to use it"

### Keep everything as-is
- Pros: zero churn
- Cons: dead code that does not compile under feature gates it claims to support (kext, compositor↔gpu), misleading docs
- Rejected

## Consequences
- Kernel shrinks: 226 files → ~200; syscalls god file untouched by this ADR (see ADR-010)
- Feature flags `ai_rule`, `ai_llm`, `objects_v2` removed (ADR-008 updated)
- `gpu` temporarily excluded from the CI all-features build until plan A4 (compositor↔gpu wiring) lands
- Plan 2 sequences: A6 (shell fold) > A1 (ash hooks) > A3 (verified runner) > A2 (hypervisor) > A4 (compositor) > A5 (ebpf/jit)