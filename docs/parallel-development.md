# Parallel Development Guide for AI Agents

**Purpose:** Enable multiple AI agents to work on the Vahi Kernel simultaneously without conflicts.

---

## The Problem

Multiple AI agents working on the same codebase can:
1. Make conflicting changes to the same file
2. Violate lock ordering (causing deadlocks)
3. Break invariants that other agents depend on
4. Create merge conflicts that are expensive to resolve

## The Solution

**Module isolation + interface contracts + ownership.**

Each agent works on ONE module at a time. The module's interface contract defines what the agent can change and what must stay stable. Ownership prevents two agents from modifying the same module simultaneously.

---

## Agent Workflow

### Before Starting Work

```
1. Read docs/MODULES.md — find an unowned module
2. Read docs/interfaces/<module>.md IF PRESENT — understand the interface contract
   (interface docs currently exist for: drivers, memory, syscalls, task, vfs;
    for other modules, write a short interface note in your PR description)
3. Read docs/invariants/<module>.md IF PRESENT — understand what must never break
   (invariant docs currently exist for: memory, syscalls, task;
    for other modules, document any new invariants in your PR description)
4. Claim the module by adding your name to docs/MODULES.md
5. Read the existing code in the module
6. Run the build: cargo build --target x86_64-unknown-none
7. Run the tests: boot with --features self_test
```

### During Work

```
1. Make changes within the module's boundaries only
2. Do NOT modify files outside your module (unless you have permission)
3. Do NOT change the public API without updating docs/interfaces/<module>.md
4. Do NOT violate invariants without updating docs/invariants/<module>.md
5. Run build after each meaningful change
6. Run tests after each meaningful change
7. Commit with conventional format: feat(module): description
```

### Before Submitting

```
1. Verify build: cargo build --target x86_64-unknown-none
2. Verify tests: cargo build --target x86_64-unknown-none --features self_test
3. Boot test: QEMU boot to login prompt
4. Selftests: All current selftests pass (check the latest TAP output for the exact count)
5. Interface docs: Updated if public API changed (or PR description if no interface doc exists)
6. Invariant docs: Updated if invariants changed (or PR description if no invariant doc exists)
7. Release module ownership (remove your name from docs/MODULES.md)
```

---

## Module Boundaries

### What You CAN Change (within your module)

- Internal implementation details
- Private functions and types
- Performance optimizations
- Bug fixes
- New private helpers
- Test additions

### What You MUST NOT Change (without coordination)

- Public API (functions, types, traits exposed to other modules)
- Lock ordering (the global ordering in MODULES.md)
- Invariants documented in docs/invariants/<module>.md (where the file exists)
- Tier 0-1 modules (sync, arch, memory, boot, interrupts)
- Other modules' files

### What REQUIRES Coordination

- Changes to public API (update interface doc)
- Changes to invariants (update invariant doc)
- Cross-module changes (get approval from all affected owners)
- Lock ordering changes (project-wide review)

---

## Conflict Prevention

### File-Level Locking

Before editing a file, check if another agent owns it:

```
docs/MODULES.md → module ownership → file ownership
```

If another agent owns the module, coordinate before editing.

### Interface-Level Locking

Before changing a public function signature, check:

```
docs/interfaces/<module>.md → public API section  (only if the file exists)
```

If the function is used by other modules, those modules may break. For modules
without an interface doc, capture the public-API contract in the PR description
and announce the change on the module's channel.

### Invariant-Level Locking

Before changing behavior, check:

```
docs/invariants/<module>.md → invariants section  (only if the file exists)
```

If a documented invariant would be violated, the invariant must be updated in
the same commit. For modules without an invariant doc, any new behavior contract
introduced by your change becomes a new invariant and must be recorded in the
PR description.

---

## Emergency Protocols

### If a Build Breaks

```
1. STOP all work
2. Identify the breaking change (git bisect)
3. Fix the break or revert the change
4. Verify build passes
5. Resume work
```

### If a Deadlock is Found

```
1. STOP all work
2. Identify the lock ordering violation
3. Fix the ordering (follow MODULES.md lock ordering)
4. Add a test that verifies the ordering
5. Resume work
```

### If an Invariant is Violated

```
1. STOP all work on the affected module
2. Identify which invariant was violated
3. Determine if the invariant is still correct
4. If yes: fix the code to preserve the invariant
5. If no: update the invariant doc and coordinate with affected modules
6. Resume work
```

---

## Communication Protocol

### Between Agents

Use the following channels:

1. **`docs/MODULES.md`** — Module ownership claims
2. **`docs/interfaces/<module>.md`** — Interface change proposals (for the 5 modules that have one: drivers, memory, syscalls, task, vfs; otherwise use the PR description)
3. **`docs/invariants/<module>.md`** — Invariant change proposals (for the 3 modules that have one: memory, syscalls, task; otherwise use the PR description)
4. **Git commits** — Change descriptions
5. **PR descriptions** — Cross-module change coordination

### With Human Reviewers

All cross-module changes require human review before merge. Single-module changes can be self-merged if all tests pass.

---

## Example: Two Agents Working Simultaneously

**Agent A** works on `task` module (process lifecycle):
- Claims ownership of `kernel/src/task/`
- Reads `docs/interfaces/task.md`
- Adds process lifecycle tests
- Updates `docs/interfaces/task.md` with new test requirements
- Releases ownership

**Agent B** works on `memory` module (CoW):
- Claims ownership of `kernel/src/memory/`
- Reads `docs/interfaces/memory.md`
- Fixes CoW deadlock
- Updates `docs/invariants/memory.md` with new invariant
- Releases ownership

**No conflict:** Both agents work on different modules with different files. The only dependency is that `task` uses `memory`'s public API, which doesn't change.

---

## Quick Start Checklist

- [ ] Read `docs/MODULES.md` — understand the module structure
- [ ] Read `docs/interfaces/<your-module>.md` IF IT EXISTS — otherwise note the public API in your PR description
- [ ] Read `docs/invariants/<your-module>.md` IF IT EXISTS — otherwise note any new invariants in your PR description
- [ ] Claim the module in `docs/MODULES.md`
- [ ] Run build + tests to verify baseline
- [ ] Make changes within module boundaries
- [ ] Update interface/invariant docs if they exist; otherwise update the PR description
- [ ] Run build + tests to verify changes
- [ ] Release module ownership
