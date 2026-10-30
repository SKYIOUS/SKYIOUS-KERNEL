# Vahi Kernel — Improvement Backlog

Generated 2026-10-26 from codebase survey (413 .rs files, ~115k lines, 19 crates, 3.5 GB target/)

---

## Quick Wins (<1 hour each, do this week)

### Q1: Delete dead kernel stub files
- [ ] `kernel/src/drivers.rs` (1 line)
- [ ] `kernel/src/memory.rs` (1 line)
- [ ] `kernel/src/net.rs` (1 line)
- [ ] `kernel/src/vfs.rs` (1 line)
- [ ] `kernel/src/limine.rs` (3 lines)
- [ ] `kernel/src/limine_marker.rs` (1 line)
- Update `kernel/src/lib.rs` to re-export from crates directly

### Q2: Release profile optimization
- [ ] Add `codegen-units = 16` to `[profile.release]`
- [ ] Add `lto = "thin"` to `[profile.release]`
- Expected: 30-40% faster release builds (local: 28s → ~18s)

### Q3: Fold `crates/gdt` into kernel
- [ ] Move `crates/gdt/src/lib.rs` → `kernel/src/gdt.rs` (merge with existing 184 lines)
- [ ] Delete `crates/gdt/` (Cargo.toml, README.md, lib.rs)
- [ ] Remove from workspace members in root `Cargo.toml`
- [ ] Update `kernel/Cargo.toml` deps

### Q4: Add `scripts/dev.sh` (daily driver)
- [ ] fmt → policy → build → clippy → host-test (no QEMU)
- [ ] Mirrors `verify.ps1 -SkipQemu` but as portable shell script

### Q5: Nightly audit job → weekly
- [ ] Edit `.github/workflows/nightly.yml`: change `dependency-audit` cron from daily to weekly (`0 6 * * 0`)

---

## Medium Wins (1-2 days each)

### M1: Crate consolidation (19 → 11 crates)
| Merge | Into | Est. effort |
|-------|------|-------------|
| `sync` (519 lines) | `hal` or `task` | 30 min |
| `crypto` (492 lines) | `hal` or `kernel/crypto.rs` | 30 min |
| `gdt` (77 lines) | Done in Q3 | — |
| `limine` (141 lines) | `builder/` as build dep | 1 hour |
| `apic` (1071 lines) | `crates/hal/src/x86_64/apic.rs` | 1 hour |
| `acpi` (234 lines) | `crates/hal/src/acpi.rs` | 30 min |
| `pci` (289 lines) | `crates/drivers/src/pci.rs` | 30 min |
| `verified` (1215 lines) | `kernel/src/verified/` | 2 hours |

Each merge: move src, update `Cargo.toml` deps, remove crate from workspace, fix imports, run CI.

### M2: Split oversized files (>1000 lines, AGENTS.md rule)
| File | Lines | Target modules |
|------|-------|----------------|
| `kernel/src/syscalls/dispatch.rs` | 1752 | `dispatch/{process,fs,net,ipc,vm,misc}.rs` |
| `kernel/src/syscalls/process_lifecycle.rs` | 1390 | `process_lifecycle/{exec,exit,wait}.rs` |
| `kernel/src/task/process.rs` | 1339 | `process/{structs,creation,lifecycle}.rs` |
| `kernel/src/iommu.rs` | 1226 | `iommu/{x86,arm,dmar}.rs` |
| `kernel/src/kaslr_reloc.rs` | 993 | Keep (at threshold) or extract to `kaslr/` |
| `kernel/src/tests/mqueue_contract.rs` | 986 | Move to `crates/ipc/tests/` |
| `crates/vfs/src/defs.rs` | 933 | `vfs/traits.rs` + `vfs/defs.rs` |
| `crates/drivers/src/usb/xhci/mod.rs` | 899 | `xhci/{ring,port,device}.rs` |
| `kernel/src/task/thread.rs` | 896 | `thread/{structs,scheduler}.rs` |
| `crates/vfs/src/skyfs/mod.rs` | 893 | Already has submodules — promote to crate? |

### M3: Delete `kernel/vendor/hashbrown/`
- [ ] Check if `hashbrown` patch in `Cargo.toml` still needed
- [ ] If not, remove vendor (4 files, 18k lines) and use crates.io version
- [ ] Update `kernel/Cargo.toml` to depend on `hashbrown` from crates.io

---

## Structural (week+)

### S1: Unify test runner
- [ ] `tests/run_qemu_tests.py`: add `--serial-mode stdio|file`, `--junit`, `--json`
- [ ] `kernel/src/selftest.rs`: add `--json` mode for structured CI parsing
- [ ] GitHub Actions: upload JUnit, render test results natively

### S2: Self-hosted ARM runner
- [ ] Provision ARM64 runner for `cross-arch` job (currently emulated on x86_64, slow)
- [ ] Or drop `cross-arch` until real hardware available

### S3: Documentation consolidation
- [ ] Merge `docs/roadmap-revised/` (17 files) into `docs/architecture/` + `PLAN.md`
- [ ] Create `docs/decisions/README.md` index for 27 ADRs
- [ ] Trim `docs/tested-working.md` to current status only

### S4: Security hardening
- [ ] Add `cargo clippy --profile audit -- -D warnings` to CI
- [ ] Add `cargo deny` for license/advisory checks

### S5: Architecture cleanup
- [ ] Reorganize `kernel/src/syscalls/` (27 files, 19k lines) into subsystem modules
- [ ] Move `kernel/src/tests/` (8.8k lines) to `crates/*/tests/` or `tests/integration/`
- [ ] Add `cargo test -p vahi-verified` to CI

---

## Tracking

| Phase | Items | Target |
|-------|-------|--------|
| Quick Wins | Q1-Q5 | This session |
| Medium | M1-M3 | Next 2-3 sessions |
| Structural | S1-S5 | Dedicated PRs |

---

## Done (this session)
- [x] Host tests + clippy in `verify.ps1`
- [x] CI on feature branches
- [x] Root `jobs = 1` removed
- [x] 3 `cargo clean` removed
- [x] 10 solo-feature builds dropped
- [x] Composite actions + pin enforced
- [x] `make image` paths fixed
- [x] Single kernel build via artifacts (build-kernel + needs:)