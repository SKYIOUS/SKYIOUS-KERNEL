# VDSO Rollout — Design Decisions

## ABI Versioning
**Scheme**: Monotonic integer (`v1`, `v2`, `v3`...), stored as `u32 vdso_abi_version` in `VdsoData` at offset `0x08C` (after `tsc_sync_seq`).

**Policy**:
- Increment **only** on incompatible layout/semantic changes (field removal, offset change, seqlock protocol change)
- Compatible additions (new fields at end, new clock IDs, new kernel_flags bits) do **not** bump version
- Userspace reads version at startup; if mismatch, falls back to syscall-only path and logs once
- Kernel exposes version via `CLOCK_SKYIOUS_KERNEL_FLAGS` high 16 bits (or new dedicated clock `CLOCK_SKYIOUS_VDSO_VERSION` = 105)

**Rationale**: Integer is simpler than semver for kernel/userspace ABI; matches Linux's `vdso_enabled`/`vdso_version` approach. No need for major/minor/patch when only breaking changes matter.

**Migration**: v1 = current layout. Future v2 adds fields at end (after `_reserved`), preserves all v1 offsets.

---

## COARSE Clock Implementation
**Decision**: Read `ticks` directly (10 ms resolution) — no separate cached coarse time.

**Implementation**:
- `CLOCK_MONOTONIC_COARSE` (6): `sec = ticks * 10 / 1000`, `nsec = (ticks * 10 % 1000) * 1_000_000`
- `CLOCK_REALTIME_COARSE` (5): Same as MONOTONIC_COARSE + cached RTC offset (read once at boot)
- No seqlock needed — single atomic `ticks` read is consistent on x86_64
- Exposed via `vdso_read_tsc_time()` fast path (clock_id 5/6) and syscall fallback

**Rationale**: 
- Linux's coarse clocks read `jiffies` (tick counter) directly — same model
- Eliminates extra cached state + seqlock for coarse path
- 10 ms resolution is sufficient for timeout calculations, polling loops
- TSC-based fast path already exists for precise clocks; coarse is for "good enough" speed

**Alternative rejected**: Separate `coarse_sec`/`coarse_nsec` fields with own seqlock — adds 16 bytes + writer overhead for marginal benefit (ticks is already atomic).

---

## Fallback Protocol
**Decision**: Immediate syscall on vDSO page fault/unmapped; log once per process.

**Userspace algorithm** (`vdso_libc.c`):
```c
int clock_gettime_vdso(clockid_t clk, struct timespec *ts) {
    const struct vdso_data *vdso = vdso_page_current();
    if (!vdso_is_mapped(vdso)) {          // page fault check via mincore or sigaction
        log_once("vDSO unavailable, using syscall");
        return sys_clock_gettime(clk, ts);
    }
    // ... existing seqlock read ...
}
```

**Kernel support**:
- `vdso_page_current()` returns `null` if page not mapped for current CPU
- Page fault handler (`page_fault.rs`) calls `vdso_map_cpu_page_into()` on demand
- If mapping fails (OOM, etc.), userspace gets `SIGSEGV` → sigaction handler → syscall fallback

**Rationale**: 
- Immediate syscall is correct; retry adds latency with no benefit (page won't appear spontaneously)
- Single log avoids spam; process lifetime is short enough
- Page fault mapping covers normal case (lazy mapping on first access)

---

## Security Audit Scope (Full — 3 Reviewers)

| Area | Threat | Mitigation |
|------|--------|------------|
| `tsc_base_origin` | Info leak: reveals boot time + TSC epoch | Zero on init? No — needed for CLOCK_SKYIOUS_BOOT_NS. Accept: boot time is public via `uptime` anyway. |
| Seqlock retries | Side channel: timing reveals writer activity | Accept: writers are timer IRQ (100 Hz) + context switch — predictable. No secret data. |
| `kernel_flags` | Exposes hardware inventory (NVMe, xHCI, GPU) | Accept: `lspci`/`lsusb` already expose this. No capabilities leaked. |
| Userspace pointer validation | Kernel writes to userspace vDSO page via HHDM | Page mapped `USER_ACCESSIBLE | PRESENT | NO_EXECUTE`; kernel writes via HHDM (kernel mapping), not user pointer. Safe. |
| `vdso_update_process_info` | Called from IRQ? `cgroup_id` is `Option` to skip in IRQ | Audit all call sites: only from `fork`/`execve`/`setns` (process context). OK. |
| TSC sync IPI | DoS via malicious IPI broadcast | `smp_broadcast_func` restricted to kernel; no userspace trigger. OK. |

**Audit deliverable**: Findings in `docs/architecture/issues.md` with severity (critical/high/medium/low), fix or accept rationale.

---

## CPU Hotplug Cleanup (Design Only — Deferred)

**Problem**: `VDSO_FRAME_REFS` permanently pins frames; no cleanup when CPU offlines.

**Design**:
- Add `vdso_cpu_offline(cpu_id)` called from SMP hotplug path
- Decrement frame ref via `frame_info::decrement()`; clear `VDSO_PAGES[cpu_id]`
- Userspace: on `getcpu` returning offlined CPU, re-read via syscall
- `vdso_abi_version` bump not required (compatible removal)

**Deferred because**: CPU hotplug not implemented in scheduler; no test surface.

---

## Clock ID Registry (from `vdso-clocks.md`)

| Clock ID | Name | vDSO Fast Path | Source |
|----------|------|----------------|--------|
| 0 | CLOCK_REALTIME | Yes (RTC + TSC interpolation) | `vdso_read_tsc_time(0)` |
| 1 | CLOCK_MONOTONIC | Yes (TSC) | `vdso_read_tsc_time(1)` |
| 2 | CLOCK_PROCESS_CPUTIME_ID | Syscall only | — |
| 3 | CLOCK_THREAD_CPUTIME_ID | Syscall only | — |
| 5 | CLOCK_REALTIME_COARSE | **New** (ticks) | `vdso_read_tsc_time(5)` |
| 6 | CLOCK_MONOTONIC_COARSE | **New** (ticks) | `vdso_read_tsc_time(6)` |
| 100 | CLOCK_SKYIOUS_THREAD_CPUTIME | Yes (thread_cputime_ns) | Atomic load |
| 101 | CLOCK_SKYIOUS_SCHED_DEADLINE | Syscall only | — |
| 102 | CLOCK_SKYIOUS_BOOT_NS | Yes (tsc_base_origin) | `vdso_read_tsc_time(102)` |
| 103 | CLOCK_SKYIOUS_KERNEL_FLAGS | Yes (kernel_flags) | Atomic load |
| 104 | CLOCK_SKYIOUS_PROCESS_INFO | Yes (cgroup/ns/caps) | Atomic loads |
| 105 | CLOCK_SKYIOUS_VDSO_VERSION | **New** (vdso_abi_version) | Atomic load |

---

## Initramfs Structure
```
/init                 → vDSO-enabled init (statically linked with vdso_libc)
/bin/sash             → vDSO-enabled shell
/lib/vdso_libc.a      → static archive (for future dynamic userspace)
/etc/vdso.conf        → optional: disable_vdso=0, log_fallback=1
```

**Builder step** (in `build_limine_image.py`):
1. Compile `userspace/libc/vdso_libc.c` → `vdso_libc.a`
2. Compile `init.c` + `sash.c` linking `vdso_libc.a`
3. `tar -cf initrd.tar init sash vdso_libc.a vdso.conf`
4. Embed in FAT partition; Limine config: `MODULE_PATH=boot:///initrd.tar MODULE_CMDLINE=initrd`