# Contributing to Vahi Kernel

## Parallel Development Rules

This kernel is developed by multiple AI agents working simultaneously on different modules. These rules prevent conflicts.

### 1. One Owner Per Module

Each module has ONE owner at a time. Check `docs/MODULES.md` for current ownership.

**Before starting work:**
1. Check `docs/MODULES.md` for module ownership
2. If the module has an owner, coordinate with them first
3. If no owner, claim it by adding your name to `docs/MODULES.md`

### 2. Interface Contract is Law

Each module has an interface contract in `docs/interfaces/`. These define:
- Public API (what other modules can use)
- Invariants (what must always be true)
- Testing requirements (what tests must exist)

**Rules:**
- You may add to the public API (new functions, new types)
- You may NOT remove or change existing public API without updating the interface doc
- You may NOT violate invariants without updating the invariant doc
- New public API must have corresponding tests

### 3. Cross-Module Changes

If your change touches code in multiple modules:
1. Identify all affected modules
2. Check ownership of each module
3. Get approval from all owners
4. Update interface docs for all affected modules
5. Run the full test suite

### 4. Lock Ordering

The following lock ordering is GLOBAL and MUST be respected:

```
PROCESS_TABLE → per-process locks → REFCOUNTS → BUDDY_ALLOCATOR → VFS → NETWORK
```

**Violation = deadlock.** This is not enforced by the compiler. You must verify it manually.

### 5. IRQ Context Rules

Code called from timer interrupt / IRQ handlers must:
- NOT allocate heap memory
- NOT take blocking locks (use `try_lock()`)
- NOT call `schedule()`
- NOT call `format!()` (allocates)

### 6. Commit Convention

```
<type>(<scope>): <description>

Types: feat, fix, docs, refactor, test, chore
Scopes: task, memory, syscalls, vfs, drivers, net, boot, sync, arch

Examples:
feat(task): add process lifecycle tests
fix(memory): resolve CoW deadlock in page fault handler
docs(syscalls): update interface contract for exec
test(vfs): add TarFS read/write tests
```

### 7. Verification Before Merge

Before merging any change:
1. `cargo build --target x86_64-unknown-none` — compiles
2. `cargo build --target x86_64-unknown-none --features self_test` — tests compile
3. QEMU boot test — boots to login prompt
4. Selftests — all pass (134/134)
5. Interface docs — updated if public API changed
6. Invariant docs — updated if invariants changed

### 8. What NOT to Touch

Unless you are the owner, do NOT modify:
- `kernel/src/sync/mod.rs` (IrqSafeMutex)
- `kernel/src/arch/` (architecture-specific code)
- `kernel/src/boot/` (boot state machine)
- `kernel/src/main.rs` (entry point)
- `kernel/src/panic_handler.rs` (panic handling)

These are Tier 0-1 modules that everything depends on. Changes here require project-wide coordination.

---

## Quick Reference

| Task | Where to Look |
|------|---------------|
| Fix a bug in the scheduler | `kernel/src/task/scheduler/` — owner: TBD |
| Add a new syscall | `kernel/src/syscalls/dispatch.rs` — owner: TBD |
| Fix a page fault | `kernel/src/memory/paging.rs` — owner: TBD |
| Add a new test | `kernel/src/tests/` — owner: TBD |
| Update documentation | `docs/` — owner: TBD |
| Fix a driver | `kernel/src/drivers/` — owner: TBD |

---

## Module Ownership Template

When you claim a module, add this to `docs/MODULES.md`:

```markdown
| `module` | `kernel/src/module/` | YourName | N | ~XXXX | Stable/Partial/Experimental |
```

And update the interface doc in `docs/interfaces/module.md` with your contact info.
