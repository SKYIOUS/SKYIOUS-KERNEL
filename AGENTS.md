# AGENTS.md — Vahi Kernel (operating contract)

Rules here bind every agent and human change. Violations = the change is wrong
even if tests pass.

## Docs map (authority order)
| Doc | Role |
|---|---|
| `AGENTS.md` | Operating contract (this file) |
| `CONTEXT.md` | Working agreement, git policy, verification commands |
| `docs/PLAN.md` | Single source of truth for what's next |
| `docs/decisions/` | ADRs. New decision → new ADR; conflicts reopen via ADR only |
| `docs/architecture/` | Architecture reviews, strategic roadmap |
| `docs/tested-working.md` | Verified status of subsystems — update when status changes |
| `CONTRIBUTING.md` | Contributor onboarding and PR process |

**Doc discipline:** a doc that contradicts the code is a bug. Any change that
invalidates a statement in these docs updates them in the same commit.

## Non-negotiables
1. **Evidence before claims.** Run `cargo build` (or `cargo build --release`)
   when touching kernel code. Run `cargo clippy -- -D warnings` when possible.
   Cite output stating the exact commands and what they covered — "tests pass"
   without scope is not evidence. `-D warnings` is deliberate: every warning
   (including rustc's) is treated as an error in new code. Never report success
   from intent. An unverified claim is a false claim.
2. **Scope discipline.** Touch only what the task names. No drive-by refactors,
   no comment churn, no reformatting of untouched lines. Exception: deleting
   what your own change made redundant is in scope (Operating protocol §3) —
   anything else outside the named task is drift.
3. **Root cause, not symptom.** Before changing a function's behavior or
   contract, find every caller. Fix at the shared choke point once — not
   per-caller guards.
4. **Files normally stay under 1000 lines.** Crossing means extract a
   submodule — unless extraction would worsen cohesion, which requires a
   documented reason naming why (commit message or ADR).
5. **No new dependency** without checking `kernel/Cargo.toml` first. Kernel
   is `#![no_std]` with `alloc` — no `std` crate, no `println!`, no `Vec`
   without `alloc::vec::Vec`. Dependencies must be `no_std`-compatible.
6. **Decisions get recorded.** Non-obvious choice → ADR in `docs/decisions/`.
   No second vocabulary.
7. **Comments carry why, not what.** Deliberate ceilings get a
   `ponytail:` marker naming the ceiling and upgrade path.
8. **Security boundaries are explicit.** Userspace-controlled data must never
   reach kernel state except through validated syscall parameters. All
   user pointers must go through `copy_from_user`/`copy_to_user`. No
   `unsafe` without a SAFETY comment explaining the invariant.

## Architecture summary
Rust, `#![no_std]`, edition 2021, nightly. Monolithic kernel (ADR-001).
- **x86_64**: Limine bootloader, UEFI, KASLR, SYSCALL/SYSRET, SMP via SIPI
- **aarch64**: EL1/EL0, GICv2, PSCI CPU_ON
- **riscv64**: RV64GC, SBI, PLIC
- **Memory**: Buddy allocator, slab, page tables (COW via isolate VM), swap, frame tracking, stack allocator
- **Scheduler**: Stride heap + SCHED_FIFO/SCHED_RR + CPU affinity + work stealing
- **Filesystems**: SkyFS (journaling), ext2 (R+W), ext4 (R/O), FAT32, TarFS, ramfs/tmpfs, devfs, ctlfs, FUSE bridge
- **Networking**: smoltcp TCP/UDP, DHCP, DNS, Unix sockets, zero-copy, RSS, TSO, TCP Reno congestion control
- **Security**: SMAP/SMEP/UMIP, KASLR, CFI, seccomp BPF, Landlock, capabilities, audit
- **Hypervisor**: VMX/SVM, EPT/NPT, vCPU, 10+ VM syscalls
- **eBPF**: Full VM, verifier, JIT (x86_64), 4 helpers, maps
- **ASH**: Verifier, interpreter, manager, hooks (net, syscall), JIT via ebpf/jit.rs, W^X exec-memory allocator
- **GUI**: Compositor at 30 FPS, window manager, terminal, splash, notifications, clipboard
- **Drivers**: NVMe, E1000, xHCI, PS/2, VirtIO, HDA audio, VirtIO-GPU, AHCI, PATA
- **IPC**: Pipes, sockets, eventfd, message queues, shared memory
- **Async I/O**: io_uring, epoll, timerfd, signalfd
- **Crypto**: SHA-256, HMAC, PBKDF2, entropy (RDRAND+TSC)
- **Objects**: KernelObject trait, handle table, security, namespace

Module map:
```
kernel/src/
  arch/{arch_x86_64,arch_aarch64,arch_riscv64}.rs  # Architecture-specific code
  apic/{mod,ioapic,lapic,msi,errata}.rs              # APIC subsystem
  ash/{mod,manager,runtime,syscalls,verifier,hooks}  # ASH scripting engine
  boot/{mod,init,logger,state,tasks}.rs              # Boot state machine
  compositor/{mod,blend,blur,flush,scene,shadow,vsync}.rs  # GUI compositor
  crypto/{mod,entropy,sha256}.rs                     # Crypto primitives
  debug/{mod,symbols}.rs                             # Debug utilities
  drivers/{mod,storage,net,usb,graphics,audio,input}.rs  # Device drivers
  ebpf/{mod,vm,verifier,jit,maps,helpers,tnum}.rs   # eBPF subsystem
  gui/{mod,drawing,filemanager,input,menu,mouse,paint,shell,splash,surface,terminal,wallpaper,widgets,window,windows}.rs  # GUI stack
  hal/{mod,cpu,x86_64,aarch64,riscv64,dma,irq,mutex,platform,exec_mem}.rs  # HAL
  hypervisor/{mod,vmx,svm,ept,memory,vcpu,sched,hypercalls,boot,devices}.rs  # Hypervisor
  interrupts/{mod,irq,exceptions,page_fault,diag}.rs  # Interrupt handling
  ipc/mod.rs                                         # IPC primitives
  memory/mod.rs                                      # Memory management (buddy, slab, paging, swap)
  net/mod.rs                                         # Network stack (smoltcp)
  objects/{mod,handle,namespace,security,syscalls,process_object,thread_object,window_object,fs_integration,gui_integration,net_integration}.rs  # Object manager
  pci/mod.rs                                         # PCI subsystem
  shell/{mod,commands}                               # Kernel shell
  sync/{mod,cfi,rcu}.rs                              # Sync primitives (IrqSafeMutex, CFI, RCU)
  syscalls/{mod,dispatch,numbers,net_*,process_*,fs_*,ipc_*,...}.rs  # Syscall implementations
  task/{mod,process,thread,oom,executor,keyboard,lock,scheduler/}  # Task management
  tests/{mod,apic,benchmarks,contract,ebpf,ext2_fs,fuzzer,futex,harness,init,memory,new_features,panic_path,pata_read,process_lifecycle,scheduler,skyfs,stress,sync,vfs}.rs  # Test suite
  verified/{mod,concurrency,journal,scheduler,runner}.rs  # Verified proofs
  vfs/mod.rs                                         # VFS layer
crates/                                              # Extracted crates (18 total)
  sync/       vahi-sync       — IrqSafeMutex (90 lines)
  memory/     vahi-memory     — buddy, slab, paging, swap, frame_info
  crypto/     vahi-crypto     — SHA-256, HMAC, PBKDF2, entropy (308 lines)
  hal/        vahi-hal        — DMA, IRQ controller, platform, timer
  limine/     vahi-limine     — Limine boot protocol (140 lines)
  apic/       vahi-apic       — Local APIC, I/O APIC, MSI (1072 lines)
  types/      vahi-types      — Shared types, Errno, credentials, VMA
  arch/       vahi-arch       — x86_64, aarch64, HAL
  gdt/        vahi-gdt        — GDT/IDT/TSS management
  interrupts/ vahi-interrupts — IRQ, page fault, exceptions
  task/       vahi-task       — process, thread, scheduler, OOM
  syscalls/   vahi-syscalls   — syscall numbers, dispatch, ptrace/seccomp/namespaces
  vfs/        vahi-vfs        — VFS layer, ext2/ext4/SkyFS/FAT32/devfs/FUSE
  net/        vahi-net        — TCP Reno, UDP, DHCP, DNS, Unix sockets
  drivers/    vahi-drivers    — NVMe, E1000, VirtIO, PS/2, xHCI, HDA, GPU
  acpi/       vahi-acpi       — ACPI tables, MADT, PRT
  objects/    vahi-objects    — KernelObject trait, handle table, security
  ipc/        vahi-ipc        — IPC primitives
  pci/        vahi-pci        — PCI subsystem
```

## Operating protocol (how every task runs)
1. **Skills before action.** Invoke matching skills BEFORE responding,
   exploring, or asking clarifying questions — when available in the
   environment. No matching skill exists → proceed under this contract.
2. **Exhaustive effort is the default, not a mode.**
   **Implementation-changing tasks** end with **prove → prune → re-check**;
   investigation/review/doc-only tasks end with **prove → re-check**:
   - **Prove:** evidence attached (command output, test names, file:line refs).
   - **Prune:** same-change deletion of exactly what your work made redundant —
     code, docs, deps — even in files the task didn't name. Leaving dead
     weight behind is an incomplete task.
   - **Re-check:** fresh read of the final diff; adversarial pass over risky
     logic; every touched doc re-checked against reality.
3. **Subagents over solo guessing.** Dispatch them instead of hand-waving:
   - Unfamiliar area → explore agent before planning anything.
   - Non-trivial diff → fresh-context reviewer before claiming done.
   - Independent subtasks run as parallel dispatches with disjoint file
     scopes stated upfront; overlapping scopes serialize.
4. **The loop never stops.** Every implementation-changing task runs
   PLAN → DO → CHECK in continuous cycles. Stopping between loops is how
   half-done work ships.

## Kernel-specific rules
1. **IRQ context safety.** Code called from timer interrupt / IRQ handlers
   must not allocate heap memory, must not take blocking locks, must use
   `try_lock()` not `lock()`. Violation = deadlock. Check `tick.rs` callers.
2. **No `std` imports.** The kernel is `#![no_std]`. Use `alloc::vec::Vec`,
   `alloc::string::String`, `alloc::format!`. Never `use std::`.
3. **`unsafe` requires SAFETY comment.** Every `unsafe` block must explain
   what invariant makes it safe. No bare `unsafe` without documentation.
4. **Architecture guards.** x86_64-specific code needs `#[cfg(target_arch = "x86_64")]`.
   aarch64-specific code needs `#[cfg(target_arch = "aarch64")]`. Code that
   runs on both must not reference architecture-specific types without guards.
5. **Lock ordering.** When multiple locks are taken, always acquire in the
   same order. Known order: PROCESS_TABLE → per-process locks → SOCKETS.
   Never reverse.
6. **Serial output for debugging.** Use `crate::serial_write()` or
   `crate::println!()` for debug output. Never `print!()` or `println!()`
   from `std`. Output goes to QEMU `-serial stdio`.
7. **Syscall numbers.** Defined in `syscalls/numbers.rs`. New syscall →
   add number constant → add dispatch entry → add handler wrapper → implement.
   Linux x86_64 numbers are preferred.
8. **Feature flags.** Default: `smp,net,ext4,iommu`. CI all-features adds
   `uhci,ash,hypervisor,verification,gpu`. New feature flags need ADR.
9. **Panic handler.** Uses `serial_write` only (no heap allocation in panic).
   Must dump registers, backtrace, process info, then halt. Never loop
   forever without diagnostic output.
10. **FPU state.** x86_64: `FpuArea` saved/restored in context switch via
    XSAVE/XRSTOR. aarch64: Q registers saved via STP/LDP. Both are
    `#[cfg]`-guarded. Never reference FPU types without architecture guard.

## Test-first mandate
1. **Selftests** (`--features self_test`): run in `kernel_main` after
   scheduler init. Add selftests for new subsystems.
2. **QEMU boot gate**: `qemu-system-x86_64 -serial stdio` must boot to
   `TAP version 13` + all selftests passing.
3. Bug fixes begin with a **reproduction case** that fails on old code and
   passes on new. "Fixed" without a repro is not fixed.
4. Trust boundaries (syscall input, userspace pointers, network packets)
   get **negative-path tests**, not just happy paths.
5. Trivial one-liners are exempt — YAGNI applies to tests too.

## Anti-slop policy (hard bans)
1. **No narration comments.** Comments exist only for non-obvious why.
   `// increment counter` and friends are deleted on sight.
2. **No speculative abstractions.** No single-implementation traits, factories
   for one product, config knobs for constants. Deletion beats addition.
3. **No placeholders.** No stubs "for later"; no TODO/FIXME without a matching
   PLAN.md or docs/architecture/issues.md row naming who/when.
4. **Never invent APIs, flags, or dependencies.** Verify against real source
   or authoritative upstream documentation before first use.
5. **Errors are handled, never swallowed.** No silent `.ok()` on data paths,
   no catch-log-continue where state matters.
6. **No scope drift.** Renames, reformats, comment edits outside the named
   task are reverted regardless of quality.
7. **No slop prose.** No emoji, no filler ("In summary…"), no sycophancy in
   commits, docs, PRs, or responses. State facts once.
8. **3am rule.** If you couldn't defend the line during a production incident
   (kernel panic, data corruption, security hole), don't ship it.

## Commands
- **Build**: `cd kernel && cargo build --release --target x86_64-unknown-none`
- **Build (debug)**: `cd kernel && cargo build --target x86_64-unknown-none`
- **Build image**: `python builder/build_limine_image.py --kernel kernel/target/x86_64-unknown-none/release/vahi_kernel --output bootimage-vahi_kernel.bin`
- **Make targets**: `make boot` (build + image), `make run` (build + QEMU), `make clean`
- **QEMU**: `qemu-system-x86_64 -drive file=bootimage-vahi_kernel.bin -m 512M -serial stdio`
- **QEMU (UEFI)**: `qemu-system-x86_64 -drive if=pflash,format=raw,file=OVMF.fd -drive file=bootimage-vahi_kernel.bin -m 512M -serial stdio`
- **Selftests**: build with `--features self_test`, boot in QEMU, check serial output
- **Clippy**: `cd kernel && cargo clippy --target x86_64-unknown-none -- -D warnings`
- **Commits**: conventional prefixes (`feat:`/`fix:`/`docs:`/`refactor:`/`chore:`),
  atomic per concern, subject ≤72 chars; docs invalidated by a change ship
  in the same commit.

## Workspace structure
- `Cargo.toml` (root) — workspace members: kernel, builder, 18 crates
- `kernel/` — the kernel (its own Cargo.toml, .cargo/config.toml, linker.ld)
- `crates/` — 18 extracted crates (sync, memory, crypto, hal, limine, apic, types, arch, gdt, interrupts, task, syscalls, vfs, net, drivers, acpi, objects, ipc, pci)
- `builder/` — Python scripts for Limine-bootable GPT disk images
- `scripts/` — build.sh, run_qemu.sh (and PowerShell equivalents)
- `tests/` — QEMU test harness, boot stress, selftest gate
- `docs/` — PLAN.md, ADRs, architecture reviews, interface contracts
- `examples/` — example userspace programs

## Session log (2026-09-21)
Completed this session:
- Full AGENTS.md inspection and update against actual codebase state
- Identified 18 extracted crates, 216 kernel src files, 344 total Rust files (82,266 LOC)
- Identified new subsystems: hypervisor (16 files), ebpf (7), gui (15), compositor (8), crypto (3), objects (12), ash (8)
- Updated architecture summary, module map, commands, workspace structure
- Updated feature flags: default = smp,net,ext4,iommu; added gpu, iommu, ash, hypervisor, verification

## Conventions & gotchas
- **Allocator**: `linked_list_allocator` crate, heap at fixed address. `alloc_error_handler` triggers OOM killer.
- **Sync primitives**: `IrqSafeMutex` (disables IF), `spin::Mutex` (busy-wait). Use `try_lock()` in IRQ context.
- **Global statics**: `lazy_static!` with `Mutex` for PROCESS_TABLE, SOCKETS, NETWORK_INTERFACE. New globals need justification.
- **Process IDs**: Start at 100 for user PIDs. PID 1 is forced for init.
- **Elf loading**: `xmas_elf` crate. Static binaries only initially; dynamic linking via `elf_dyn.rs`.
- **Linker scripts**: `kernel/linker.ld` (x86_64), `kernel/aarch64-linker.ld`. Higher-half at `0xFFFFFFFF80000000`.
- **Boot**: Limine protocol. Boot state machine in `boot/state.rs` with 8 states.
- **QEMU serial**: All `serial_write` output goes to `-serial stdio`. Check serial logs for boot progress.
- **Feature guards**: `#[cfg(feature = "net")]` for networking, `#[cfg(feature = "smp")]` for SMP. Check before using gated APIs.
- **RUSTFLAGS**: Never set RUSTFLAGS env on kernel build — overrides `.cargo/config.toml` rustflags and drops `-Tlinker.ld`.
- **Build order**: Build kernel first, then bootimage. The Rust builder (`builder/build_boot_image.py`) and Limine builder (`builder/build_limine_image.py`) produce different images.
- **SMP under TCG**: Use `-cpu qemu64,-smep` for SMP-2+ boots.

## Removed (do not reintroduce)
- `vahiai` crate (deleted — fake AI subsystem)
- `korlang` crate (deleted — DSL VM with no real consumers)
- `kext` crate (deleted — kernel extension framework unused)
- `rtsched` (deleted — real-time scheduler replaced by FIFO/RR in stride)
- `pressure` (deleted — memory pressure module replaced by OOM killer)
- Fake/broken hypervisor tests (deleted — not testing real VMX/SVM)
- `SCHED_QUEUES` / `PerCpuRunQueue` (deleted — dead code, never populated)
- `spawn_userspace_app` (deleted — duplicated exec path, never called)
- `APP_PATH_TO_LAUNCH` (deleted — dead static, only used by deleted function)
