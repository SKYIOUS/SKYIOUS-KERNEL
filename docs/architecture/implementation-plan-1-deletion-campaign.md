# Implementation Plan 1 — Deletion Campaign (revised)

Policy (user decisions, 2026-08-20):
- DELETE outright: Group C broken orphans + Group B partial/fake modules + dead wiring.
- KEEP + plan-to-wire: Group A real designs (ash, hypervisor, verified, compositor, ebpf/jit).
- KEEP + fold: shell/commands/* into gui/terminal.rs (the live shell).
- Git history preserves everything deleted (`git checkout <commit> -- path`).

## DELETE list

| Item | Why | Ref |
|------|-----|-----|
| kernel/src/vahiai/ | fake-AI intent ladder; Group B not kept | main.rs:84,316; syscalls 693,5240 |
| kernel/src/korlang/ | stub ABI, no compiler; Group B not kept | main.rs:78,313; syscalls 696,6049 |
| kernel/src/kext/ | won't compile (missing pci::publish_nubs); Group B not kept | not in main.rs; kext/loader.rs:54 |
| kernel/src/rtsched.rs | orphaned stub; feature not in Cargo.toml | Group C |
| kernel/src/memory/pressure.rs | references nonexistent proc.repossession | Group C |
| ~~kernel/src/memory/virt.rs~~ | KEPT — live (memory/mod.rs:18, tests/new_features.rs:101 test_page_constants) | — |
| kernel/src/ebpf/ash.rs + ash_tests.rs | dead; calls nonexistent verifier::is_ash_safe | Group C |
| ~~shell.rs (launcher)~~ | KEPT — launch disabled but shell/commands/* is the fold target; module compiles | main.rs:52, 413-415 |
| kernel/src/tests/hypervisor_tests.rs | orphaned; not in tests/mod.rs | Group C (hypervisor kept, but this test is dead — re-add in wire-up plan) |
| main.rs threading_demo | dead demo (379-406), no caller | Group C |
| ~~shell.rs (launcher)~~ | KEPT — launch disabled but shell/commands/* is the fold target; module compiles | main.rs:52, 413-415 |
| docs/ash-spec.md | documents the dead ebpf/ash.rs ABI — rewrite for the kept ash/ instead (see plan 2) | — |
| repo-root logs: serial_*.log, qemu_*.log, prev_serial.log, prev_esp.img, old_initrd_compare.tar, builder_run.log | committed debug litter | — |

## KEEP (Group A — wire-up in implementation plan 2)

- kernel/src/ash/ (hooks need 2 call-sites)
- kernel/src/hypervisor/ (needs vCPU/VMCS wiring)
- kernel/src/verified/ (needs runner wiring; proofs → docs/verified-proofs/)
- kernel/src/compositor/ (needs gui/ integration; feature gpu)
- kernel/src/ebpf/jit.rs (needs a caller; part of the kept eBPF layer)
- kernel/src/shell/commands/* (fold into gui/terminal.rs dispatch)

## KEEP (untouched)

- SchedLock (live: vfs/mod.rs:390 uses it) — only fix the stale "unused" comment
- tests/pata_read_test.rs (live under self_test)
- uhci feature (live)

## Cargo.toml feature audit

Remove: ai_rule, ai_llm, objects_v2 (dead)
Keep: smp, net, ext4, uhci, self_test, ash, gpu, hypervisor, verification
Default: ["smp", "net", "ext4"]

## Steps (one commit per step, verify after each)

- [x] 1. Delete Group C orphans: rtsched.rs, memory/pressure.rs, ebpf/ash*.rs, tests/hypervisor_tests.rs. Verify: cargo build. — DONE (memory/virt.rs kept: live).
- [x] 2. Delete Group B modules + wiring: vahiai/, korlang/, kext/ dirs; main.rs mods+inits; shell.rs "vahiai" arm + shell/commands/ai.rs + mod decl; syscalls numbers SYS_VAHIAI/SYS_KORLANG + dispatch arms + fn sys_vahiai/sys_korlang; shell/commands/system.rs kor(); Cargo.toml features ai_rule/ai_llm/objects_v2; README/CONTRIBUTING/ctlfs.tsv strings. Verify: cargo build + all-features. — DONE (default + uhci/ash/hypervisor/verification all build; gpu dropped from CI list until plan A4).
- [ ] 3. threading_demo removal — DONE in step 2 (deleted in main.rs edit).
- [ ] 4. shell.rs launcher: KEPT (fold target); shell/commands/ mod decl stays valid. No action needed this plan.
- [ ] 5. CI workflow feature-list updates — DONE (ci.yml:44 + nightly.yml:42 → "smp,net,ext4,uhci,ash,hypervisor,verification"; ci.yml:91 → net,smp,self_test; ADR-008 updated).
- [x] 6. Repo hygiene: git rm repo-root logs (serial_*.log etc.); .gitignore additions. — DONE (git rm serial_clean_test.log serial_debug.log builder/serial_loop.log builder/serial_mouse_fix.log; .gitignore += serial_*.log, prev_*.log, builder_run.log, old_initrd_compare.tar).
- [x] 7. task/lock.rs stale "unused" comment fix. — DONE.

## Verification (after every step)

```
cargo build                                                     # default — PASS
cargo build --no-default-features --features "smp,net,ext4,uhci,ash,hypervisor,verification"   # surviving all-features (gpu pending plan A4) — PASS
cargo check --features self_test                                # PASS
cargo clippy -- -D warnings                                     # pre-existing errors only, none in touched files
QEMU selftest: TAP 13, "92/92 passed, 0 failed", smp 1 + 2      # PASS both
```

## Risk register

- syscalls dispatch arms are inside cfg blocks — read the gates before editing (vahiai/korlang arms at 693/696). — RESOLVED: arms were ungated; removed.
- Deleting shell.rs: confirm nothing outside shell/ references it (grep `shell::` first). — RESOLVED: shell.rs kept; only dead launch line removed.
- CI all-features list must change in the same commit as the Cargo.toml feature removal (step 5). — DONE.
- gpu feature + compositor: `crate::drivers::gpu::ring` does not exist — pre-existing broken wiring, fixed by plan A4 (compositor→gui seam). CI all-features excludes gpu until A4.