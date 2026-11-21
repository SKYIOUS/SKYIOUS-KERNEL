# VDSO Rollout — Execution State

## Baseline
- **Revision**: `94dce40da7ffe3e6715db1480d04b83cad2106f0`
- **Branch**: `main`
- **Environment**: x86_64 kernel, QEMU (TCG), Windows host (cross-compiler unavailable; CI uses Ubuntu with musl-gcc)
- **Kernel config**: `--features self_test,net,smp,ext4,iommu` + `uhci,ash,hypervisor,verification,gpu` for CI all-features

## Active Truth Index (authority order)
1. **This state.md** — execution contract, phases, gate evidence, recovery
2. `todo.md` — itemized work list (sole owner of item status/waits)
3. `design.md` — ABI versioning, fallback protocol, hotplug cleanup design
4. **Repository specs** (immutable during execution unless ADR):
   - `kernel/src/vdso.rs` — kernel impl (x86_64 complete)
   - `kernel/src/tests/vdso_tests.rs` — 19 functional + 6 benchmark selftests
   - `docs/interfaces/vdso.md` — ABI spec (needs version field)
   - `docs/architecture/vdso-design.md` — design rationale
   - `docs/guides/vdso-userspace.md` — integration guide (needs migration)
   - `docs/reference/vdso-clocks.md` — clock ID registry
   - `userspace/libc/vdso_libc.c/h` — userspace helper (needs fallback, build)
   - `builder/build_limine_image.py` — image builder (needs initrd step)
5. **AGENTS.md** — operating contract (non-negotiables, kernel rules)

## Phases & Gate Evidence

| Phase | Description | Gate Evidence (conjunctive) |
|-------|-------------|------------------------------|
| **P0: Design freeze** | Record ABI versioning, fallback, COARSE clock decisions in `design.md` | `design.md` complete; no open questions |
| **P1: Kernel ABI version + COARSE** | Add `vdso_abi_version` to `VdsoData`; expose `CLOCK_MONOTONIC_COARSE` (6) / `CLOCK_REALTIME_COARSE` (5) via `ticks` | Kernel builds; `vdso:clock_gettime` + new `vdso:clock_getres_coarse` selftests pass |
| **P2: vdso_libc fallback + build** | Add immediate-syscall fallback in `vdso_libc.c`; integrate into userspace build (Makefile/meson) | `vdso_libc_test` builds statically; runs in QEMU; fallback path exercised |
| **P3: init/sash integration** | Link `init` and `sash` against `vdso_libc` as primary ABI; verify fast paths | `init` boots in QEMU; `clock_gettime`/`getpid`/`getcpu` via vDSO match syscalls |
| **P4: Initramfs generation** | Builder step creates `initrd.tar` with vDSO-enabled `init`/`sash`; Limine config updated | `bootimage-vahi_kernel.bin` boots to userspace prompt via initrd |
| **P5: CI/CD pipeline** | GitHub Actions: build → QEMU boot (self_test) → run all `vdso:*` + `vdso_bench:*` → clippy | Workflow YAML passes on PR + nightly; artifacts uploaded |
| **P6: Security audit** | Full audit: tsc_base_origin leak, seqlock side channels, kernel_flags exposure, pointer validation | 3 independent reviewers; findings tracked in `docs/architecture/issues.md`; all high-severity fixed |
| **P7: Documentation** | ABI version policy in `vdso.md`; man page `vdso.7`; migration guide in `vdso-userspace.md` | All docs render; migration guide covers syscall→vDSO transition |
| **P8: Integration validation** | End-to-end: kernel + userspace + initrd + CI green | All gates above pass; no unresolved high-severity findings |

## Review & Validation
- **Reviewers**: 3 independent at P3 (init boots), P6 (security), P8 (integration)
- **Re-review**: After each fix round (max 3 rounds), then defer unresolved
- **Validators**: `cargo build --target x86_64-unknown-none`, `cargo clippy --target x86_64-unknown-none -- -D warnings`, QEMU boot with `--features self_test`, selftest runner (`run_qemu_tests.py`)

## Dependencies & Recovery
- **External**: GitHub Actions runner with KVM (fallback: TCG, benchmarks warn-only), musl-gcc (fallback: gcc -static)
- **Internal**: Kernel self-tests must pass before userspace integration; builder must produce bootable image before CI
- **Recovery**: On phase failure, checkpoint state.md, record evidence, move failed item + dependents behind independent work, recheck at phase boundary
- **Quota/Session**: Before QEMU runs, verify checkpoint + non-blocking wait; on exhaustion, record next action + unmet gates

## Next Action
P2/P3/P4 blocked on cross-compiler (musl-gcc) unavailable on Windows host. CI pipeline (P5) created to run on GitHub Actions with Ubuntu runners. P6 security audit findings documented in issues.md. P7 documentation complete except render verification. Continue with CI validation and cross-compiler setup.

## Initialization TODO (to be removed after completion)
- [x] Create `.goal-task/vdso-rollout/` directory
- [x] Populate `state.md` with baseline, phases, gates
- [x] Populate `design.md` with ABI versioning, fallback, COARSE decisions
- [x] Populate `todo.md` with itemized work list
- [x] Verify QEMU + musl-gcc/gcc availability (QEMU ✓, musl-gcc ✗ on Windows; CI uses Ubuntu)
- [x] Record baseline revision in state.md at execution start