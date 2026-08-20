# Implementation Plan 2 — Wire Up Group A (make kept code useful)

Policy: keep real designs and make them actually run. Each module below is kept behind
its feature gate; this plan lists the minimal wiring to make it useful, in dependency order.

## A1. ash/ — make registered handlers execute (feature: ash)

Status: register/verify/unregister syscalls work; `hook_net_receive` / `hook_syscall_entry`
are never called from the live paths (ash/hooks/net.rs:16, ash/hooks/syscall.rs:17).

Steps:
1. Call `crate::ash::hooks::net::hook_net_receive` from the net RX path (net/mod.rs poll or
   the E1000 IRQ RX handler) — needs the packet buffer + len + protocol + dest port that the
   handler ABI expects.
2. Call `crate::ash::hooks::syscall::hook_syscall_entry` from syscalls/mod.rs dispatch entry
   (before the main match) — needs syscall number + args.
3. Fix or delete ash/jit.rs: runtime.rs:29 transmutes a heap Vec to an fn pointer (NX
   violation on real hardware). Prefer deleting the JIT path and running the interpreter
   until a real executable-memory allocator exists.
4. Rewrite docs/ash-spec.md to describe the compiled ash/ ABI (not the dead ebpf/ash.rs).
5. Add a selftest: register a trivial handler, assert the hook fires on a synthetic event.

Verify: cargo build --features ash + QEMU selftest.

## A2. hypervisor/ — launch a guest end-to-end (feature: hypervisor)

Status: sys_vm_* syscalls exist (some stub), but create_guest never maps guest memory into
the EPT, no persistent VMCS/vCPU state, vcpu.run() rebuilds a fresh VmxHandler per call.

Steps:
1. Pick ONE VM model: keep `Hypervisor`+`GuestVm`+`Vcpu` (the syscall-facing model); delete
   the parallel `vmm.rs`/`guest.rs` model or make Guest a thin alias.
2. create_guest: allocate guest memory AND map it into the EPT (EPT is real — ept.rs).
3. vcpu: persist vmcs_phys across runs; remove the per-call VmxHandler::new().
4. Fill the stub bodies: sys_vm_load_kernel (return ENOSYS or implement ELF+EPT mapping),
   sys_vm_set_memory (map into EPT). Delete sys_vm_load_kernel hardcoded 0.
5. Re-add tests/hypervisor_tests.rs via tests/mod.rs (feature-gated) — was deleted in plan 1.
6. Document a "hello guest" userspace test program in examples/.

Verify: cargo build --features hypervisor + a selftest asserting EPT maps a guest page.

## A3. verified/ — connect models to real code (feature: verification)

Status: JournalStateMachine/scheduler contracts/concurrency models are standalone; the real
vfs/skyfs/journal.rs and task/scheduler.rs never consult them. runner.rs is wired at boot.

Steps:
1. Move proofs to docs/verified-proofs/ (lock_proof.md is about the real IrqSafeMutex —
   keep verbatim).
2. Wire the REAL scheduler to verified/scheduler.rs: call check_schedule_correctness in the
   scheduler's pick_next under verification feature (assert-only).
3. Wire vfs/skyfs/journal.rs to verified/journal.rs: assert journal invariants after each
   commit_transaction under verification feature.
4. Keep concurrency.rs as documentation until a concrete invariant needs it.

Verify: cargo build --features verification + selftest with the assert hooks on.

## A4. compositor/ — integrate HW compositing into gui/ (feature: gpu)

Status: HwCompositor (compositor/) never instantiated; gui/ renders via virtio_gpu::flip
directly (gui/mod.rs:832,853; gui/splash.rs:141).

Steps:
1. Decide the seam: gui owns the scene; compositor owns the blit. Introduce one call site —
   `compositor::compose(&scene)` invoked from gui's frame loop instead of raw flip() calls.
2. Keep the software path as the fallback when the gpu feature is off (two adapters exist:
   software flip, HW compose).
3. Add a selftest asserting compose(empty scene) leaves the backbuffer unchanged.

Verify: cargo build --features gpu + QEMU visual smoke test.

## A5. ebpf/jit.rs — give the JIT a caller (always compiled)

Status: only ALU64/JMP/EXIT compile; no call path uses it. ash/runtime.rs is the natural
consumer once A1 step 3 lands (interpret first, JIT when a safe exec-memory allocator exists).

Steps:
1. Land A1 first (interpreter path).
2. Add a kernel executable-memory allocator (W^X discipline: alloc RW, flip to RX, never RWX).
3. Route ash runtime through ebpf::jit when the program is JIT-able; fall back to the VM.
4. Extend jit.rs coverage instruction by instruction; each extension gets a selftest case.

Verify: cargo build + ebpf selftest suite (already 16 cases) + new JIT smoke cases.

## A6. shell/commands/* — fold the live terminal onto the shared table

Status: gui/terminal.rs:213 has a 13-command match; shell/commands/* has the rich set.

Steps:
1. Keep shell/commands/* as the canonical command implementations; change the command
   signature to a shared `CommandFn(&[&str]) -> Result<(), &str>`.
2. Re-point gui/terminal.rs execute_command to call the shared table; delete its inline match.
3. Commands that write the VGA framebuffer (themes) get a gui-aware output sink abstraction
   or are marked `[vga-only]` and skipped by the GUI terminal.
4. Add a selftest: dispatch "help" and assert the command table returns its help text.

Verify: cargo build + GUI terminal smoke test (type `help`, `ls`, `uptime`).

## Sequencing

A6 (fold) > A1 (ash) > A3 (verified) > A2 (hypervisor) > A4 (compositor) > A5 (JIT).
Each lands behind its feature gate so the default build is unaffected.