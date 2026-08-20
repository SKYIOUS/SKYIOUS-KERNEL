# SKYIOUS Kernel — Architecture Review & Simplification Plan

Status: Approved (user: all 12 candidates, delete outright, QEMU local verification OK)
Date: 2026-08-20
Grounding: OSDev Wiki Expanded Main Page (subsystem checklist: kernel models, task models,
memory management, syscalls, scheduling, synchronization, IPC, filesystems).
ADRs 001 (monolithic) and 008 (feature flags) are respected — not re-litigated.

## Phases

1. Grilling + docs (grill-with-docs) — one question at a time per candidate; decisions
   crystallize into ADRs (`docs/decisions/`) and CONTEXT.md glossary.
2. Thermo-nuclear review — strict audit of live hot paths, high-conviction findings only.
3. Execution — incremental, one change per commit, verified after each (see below).
4. Documentation reconciliation — stale docs (ash-spec, kernel-future-plan, filesystem-design).

## Candidates (from exploration, file:line evidence)

| # | Candidate | Strength | Plan |
|---|-----------|----------|------|
| 1 | Dead-weight deletion: DELETE vahiai/, korlang/, kext/, rtsched.rs, memory/pressure.rs, memory/virt.rs, ebpf/ash*.rs, tests/hypervisor_tests.rs, threading_demo, shell.rs launcher, repo-root logs. KEEP + wire-up (plan-2): ash/, hypervisor/, verified/, compositor/+gpu, ebpf/jit.rs, shell/commands fold. Remove dead features ai_rule/ai_llm/objects_v2. | Strong | plan-1 + plan-2 |
| 2 | Decompose syscalls/mod.rs (7301 lines): with_fd()/resolve_user_path() helpers, split per-domain | Strong | plan-2 |
| 3 | Dual frame allocators (phys.rs vs buddy.rs) + 5 overlapping memory trackers | Worth exploring | plan-4 |
| 4 | Dual scheduler run-queue (stride heap + legacy priority queues) + 4x wake/block scans | Worth exploring | plan-4 |
| 5 | Two tick counters (interrupts::get_ticks vs hal::timer::get_ticks) | Strong | plan-3 |
| 6 | Driver duplication: DmaBuf x4, VirtIO queue x3, NicDevice match x3, SocketObject x2 | Strong | plan-3 |
| 7 | HAL CpuContext/Timer shallow on x86 (re-enters task::thread; global mutex per halt) | Worth exploring | plan-3 |
| 8 | Two boot paths (main.rs:614 legacy vs boot/state.rs machine; Running/Failed unreachable) | Strong | plan-3 |
| 9 | Two shells (dead shell/commands/* vs live gui/terminal.rs subset) | Strong | plan-1 |
| 10 | emulation.rs:260 silent success (unknown Linux syscall -> SYS_SYNC -> 0) | Strong | plan-3 |
| 11 | PCI discovery if-else chain, MSI only for E1000, 4M-spin polls, hda_println!, acpi_prt stub | Worth exploring | plan-3 |
| 12 | skyfs #![allow(dead_code)] module-wide, dead page_cache writeback, Process 40 lock-per-field, leaked TSSes | Worth exploring | plan-4 |

Corrections from verification:
- SchedLock (task/lock.rs) is NOT dead — VFS global uses it (vfs/mod.rs:390). Fix stale comment, keep.
- tests/pata_read_test.rs is live (registered under self_test) — keep.

## Top recommendation

Candidate 1 first: deletes whole categories of complexity with near-zero behavioral risk
(most targets never compile or never execute), shrinks build + CI matrix. Then 2, then 3.

## Verification (after every step, mirrors .github/workflows/ci.yml)

```
cargo build                                                                   # default features
cargo build --no-default-features --features <updated-all-list>               # all-features CI combo
cargo check --features self_test
cargo clippy -- -D warnings                                                   # CI gate; deny(warnings) in main.rs
QEMU selftest: TAP 13, "N passed, 0 failed", smp 1 + 2 (run_test_nographic.ps1 / run.ps1, OVMF.fd)
```

CI workflow feature lists must be updated as features are deleted.

## Deliverables

- docs/architecture/implementation-plan-1-deletion-campaign.md (next)
- HTML report at %TEMP%/architecture-review-<ts>.html
- ADRs 009-012+ in docs/decisions/
- CONTEXT.md (created lazily as terms crystallize)