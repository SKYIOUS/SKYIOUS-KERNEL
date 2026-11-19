# ADR Index — Architectural Decisions

## Existing ADRs

| ADR | Title | Status |
|-----|-------|--------|
| ADR-001 | Monolithic kernel | Accepted |
| ADR-002 | hashbrown migration | Accepted |
| ADR-003 | Rust nightly no_std | Accepted |
| ADR-004 | Boot state machine | Accepted |
| ADR-005 | Stride scheduling | Accepted |
| ADR-006 | Capability security | Accepted |
| ADR-007 | SMP support | Accepted |
| ADR-008 | Feature flags | Accepted |
| ADR-009 | Keep/wire-up/deletion policy | Accepted |
| ADR-010 | Syscalls decomposition | Accepted |
| ADR-011 | Single clock | Accepted |
| ADR-012 | Shared driver primitives | Accepted |

## New ADRs (Proposed)

| ADR | Title | Status | Decision Required |
|-----|-------|--------|-------------------|

| ADR-015 | epoll | Accepted | Implemented in `syscalls/epoll.rs` |
| ADR-016 | Scheduler architecture (stride → EEVDF) | Accepted | Keep stride, migrate when needed; RT classes added |
| ADR-017 | KASLR | Accepted | Implemented in `main.rs` |
| ADR-018 | SkyFS journaling | Accepted | Fixed — replays data to target blocks |
| ADR-019 | Kernel object model redesign | Accepted | 16 per-object rights in `objects/security.rs` |
| ADR-020 | Landlock / sandboxing | Accepted | Linux-compatible ABI with Vahi extensions |
| ADR-021 | VM object model | Deferred | Current VMA implementation works; low priority |
| ADR-022 | WaitableObject abstraction | Deferred | IrqSafeMutex works for now; low priority |
| ADR-023 | Vahi IPC model | Accepted | Structured messages + zero-copy + port IPC |
| ADR-024 | Container scope | Accepted | Isolation primitives only, no Docker compat |
| ADR-025 | Virtualization scope | Accepted | Integrated subsystem, already in kernel |

## Decisions Made

### 1. Scheduler Architecture (ADR-016) ✅

**Decision:** Keep stride, migrate to EEVDF when needed. RT classes (SCHED_FIFO, SCHED_RR) added.

### 2. Kernel Object Model (ADR-019) ✅

**Decision:** Structured rights — 16 per-object rights with grant/drop/compose/fork.

### 3. Landlock Approach (ADR-020) ✅

**Decision:** Linux-compatible ABI with Vahi extensions.

### 4. Container Scope (ADR-024) ✅

**Decision:** Isolation primitives only — namespaces + cgroups, no Docker compat.

### 5. Virtualization Scope (ADR-025) ✅

**Decision:** Integrated subsystem — VMX/SVM already in kernel, not a separate program.

## ADR Template

```markdown
# ADR-XXX: Title

## Status
Proposed | Accepted | Deprecated | Superseded

## Context
What is the issue that we're seeing that motivates this decision?

## Decision
What is the change that we're proposing and/or doing?

## Consequences
What becomes easier or more difficult to do because of this change?

## Alternatives Considered
What other options were considered?
```
