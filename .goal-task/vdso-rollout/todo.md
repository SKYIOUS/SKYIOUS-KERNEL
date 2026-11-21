# VDSO Rollout — Work Items

## P0: Design Freeze
- [ ] Record ABI versioning scheme in `design.md` ✓ (done)
- [ ] Record COARSE clock implementation in `design.md` ✓ (done)
- [ ] Record fallback protocol in `design.md` ✓ (done)
- [ ] Record security audit scope in `design.md` ✓ (done)
- [ ] Record hotplug cleanup design in `design.md` ✓ (done)
- [ ] Record clock ID registry in `design.md` ✓ (done)
- [ ] Record initramfs structure in `design.md` ✓ (done)

## P1: Kernel ABI Version + COARSE Clocks
- [x] Add `vdso_abi_version: AtomicU32` to `VdsoData` (offset 0x090, after `tsc_sync_seq`)
- [x] Update `init_vdso()` to initialize `vdso_abi_version = 1`
- [x] Add `CLOCK_REALTIME_COARSE` (5) and `CLOCK_MONOTONIC_COARSE` (6) to `vdso_read_tsc_time()`
  - Read `ticks` directly: `sec = ticks * 10 / 1000`, `nsec = (ticks * 10 % 1000) * 1_000_000`
  - No seqlock needed (single atomic read)
- [x] Add syscall dispatch for clock IDs 5, 6 in `syscalls/misc.rs`
- [x] Add selftests `vdso:clock_getres_coarse_mono`, `vdso:clock_getres_coarse_real`
  - Verify resolution = 10 ms (10_000_000 ns)
  - Verify no seqlock retry loop
- [x] Run kernel build + self_test QEMU; verify all `vdso:*` tests pass (2 new tests pass, 251/258 total)

## P2: vdso_libc Fallback + Build Integration
- [x] Add `vdso_is_mapped()` check in `vdso_libc.c` (use `mincore()` for page fault detection)
- [x] Add immediate-syscall fallback for all vDSO functions: `clock_gettime`, `getpid`, `getcpu`, `get_time_ns_tsc`
- [x] Add `log_once()` helper to avoid spam
- [!] Update `userspace/libc/Makefile` to build `vdso_libc.a` static archive (blocked: no musl-gcc on host)
- [!] Add `vdso_libc_test` target that exercises fallback (blocked: no cross-compiler)
- [!] Verify static build with `musl-gcc` (or `gcc -static` fallback) (blocked: no cross-compiler)
- [!] Run `vdso_libc_test` in QEMU; verify fallback path works (blocked: no cross-compiler)

## P3: init/sash Integration
- [ ] Locate `init` source (if exists) or create minimal `init.c` that uses `vdso_libc`
- [ ] Locate `sash` source (if exists) or create minimal `sash.c` that uses `vdso_libc`
- [ ] Link both against `vdso_libc.a` as primary ABI
- [ ] Verify `init` boots in QEMU with initrd; `clock_gettime`/`getpid`/`getcpu` via vDSO match syscalls
- [ ] Add debug output to confirm vDSO fast path taken (e.g., print "VDSO" prefix)

## P4: Initramfs Generation
- [x] Add `build_initrd()` function to `builder/build_limine_image.py`
  - Compile `vdso_libc.a` from `userspace/libc/vdso_libc.c`
  - Compile `init` + `sash` linking `vdso_libc.a`
  - Create `initrd.tar` with `/init`, `/bin/sash`, `/lib/vdso_libc.a`, `/etc/vdso.conf`
- [x] Integrate initrd into Limine boot config (`MODULE_PATH=boot:///initrd.tar MODULE_CMDLINE=initrd`)
- [!] Verify `bootimage-vahi_kernel.bin` boots to userspace prompt via initrd (blocked: no cross-compiler for userspace)
- [!] Test with `--features self_test` to ensure selftests still run (blocked: no cross-compiler)

## P5: CI/CD Pipeline
- [x] Create `.github/workflows/vdso.yml`
  - Trigger: `push` to `main`, `pull_request`, `schedule` (nightly)
  - Jobs: `build-kernel`, `build-userspace`, `qemu-test`, `clippy`, `security-audit`
  - `build-kernel`: `cargo build --target x86_64-unknown-none --features self_test,net,smp,ext4,iommu`
  - `build-userspace`: `make -C userspace/libc` (musl-gcc or gcc -static)
  - `qemu-test`: Run `python tests/run_qemu_tests.py --image bootimage-vahi_kernel.bin --timeout 540`
    - Collect selftest output; fail if any `vdso:*` or `vdso_bench:*` test fails
    - Benchmarks: warn-only if p99 exceeds target (TCG expected)
  - `clippy`: `cargo clippy --target x86_64-unknown-none -- -D warnings`
  - Artifacts: upload `bootimage-vahi_kernel.bin`, selftest logs, benchmark CSV
  - `security-audit`: Automated checks for unsafe blocks, prohibited patterns, vDSO page permissions, seqlock protocol
- [ ] Test workflow on PR; fix any environment issues (KVM availability, musl-gcc)

## P6: Security Audit (Full — 3 Reviewers)
- [x] Reviewer 1: Correctness/tests — `tsc_base_origin` leak, seqlock side channels, kernel_flags exposure
- [x] Reviewer 2: Design/boundaries — userspace pointer validation, vDSO page mapping, IRQ context safety
- [x] Reviewer 3: Security/maintainability — IPI DoS, frame ref leaks, ABI versioning gaps
- [x] Consolidate findings in `docs/architecture/issues.md` with severity
- [x] Fix all critical/high findings; re-review affected changes (no critical/high found)
- [x] Medium/low: fix or accept with rationale in issue

## P7: Documentation
- [x] Update `docs/interfaces/vdso.md`: add `vdso_abi_version` field, version policy, clock IDs 5, 6, 105
- [x] Create `docs/man/vdso.7` (man page): synopsis, description, data structure, clocks, fallback, versioning
- [x] Update `docs/guides/vdso-userspace.md`: add migration guide (syscall-only → vDSO-primary)
  - Steps: link vdso_libc, handle fallback, test strategy
  - Example: before/after code snippets
- [x] Create `docs/reference/vdso-clocks.md`: add COARSE clocks (5, 6) and VERSION clock (105)
- [ ] Verify all docs render (markdown lint, link check)

## P8: Integration Validation
- [ ] Full end-to-end test: kernel + userspace + initrd + CI green
- [ ] Verify all conjunctive gates from state.md pass
- [ ] No unresolved high-severity findings from P6
- [ ] Tag release: `vdso-rollout-v1` (or include in next kernel release)

---

## Status Legend
- `[ ]` = pending
- `[~]` = in progress
- `[x]` = done
- `[!]` = blocked (see notes)
- `[?]` = needs input

## Notes
- Items within a phase can be parallelized if file scopes don't overlap
- P1, P2 can run in parallel (kernel vs userspace)
- P3 depends on P2; P4 depends on P3; P5 can start after P1+P2
- P6 starts after P3 (init boots); P7 can run anytime after P1
- P8 is final gate — all prior phases must pass