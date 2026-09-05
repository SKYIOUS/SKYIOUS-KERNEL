# Vahi Kernel — End-to-End Reliability Mission

## Mission Objective

Transform Vahi Kernel from a feature-rich QEMU-demonstrated kernel into a **coherent, testable, reliable, and defensible operating-system kernel baseline**.

The goal is **not** Linux feature parity, maximum syscall count, maximum driver count, or adding new experimental subsystems.

The goal is to make the existing kernel trustworthy.

The kernel must progress from:

> “The kernel boots, starts userspace, and demonstrates many subsystems.”

to:

> “The kernel has a clearly defined supported surface, its critical execution paths are continuously validated, failures are reproducible, core isolation and memory invariants are tested, and the architecture is strong enough to safely continue development.”

The mission is complete only when the repository itself provides convincing evidence that this statement is true.

---

## Current Reality

The current kernel is approximately:

- 283 Rust source files
- 64K+ lines of Rust
- UEFI boot capable
- Able to reach userspace/init
- Able to fork and exec
- CoW fork implemented
- Demand paging implemented
- Scheduler implemented
- Basic filesystem support implemented
- Basic networking implemented
- Multiple drivers implemented
- Experimental eBPF support
- Experimental security subsystems
- Experimental compositor/userspace-related functionality

However, substantial portions of the feature surface remain partial, stubbed, insufficiently tested, or unverified on real hardware.

The kernel currently has significant technical debt around:

- global mutable state
- locking architecture
- large modules
- unsafe-code auditing
- error typing
- dead/experimental code
- syscall completeness
- filesystem durability
- SMP/concurrency validation
- hardware validation
- security-boundary testing

The current mission must address the **root reliability problems**, not merely make the feature list larger.

---

# Primary End State

Achieve an end-to-end validated Vahi execution path:

UEFI
→ kernel initialization
→ memory initialization
→ interrupt initialization
→ scheduler initialization
→ storage/filesystem initialization
→ init/PID 1
→ userspace ELF loading
→ process creation
→ fork/CoW
→ exec
→ syscall interaction
→ IPC/process synchronization
→ filesystem I/O
→ networking where supported
→ service lifecycle
→ login/userspace shell
→ controlled shutdown/reboot path

Every supported stage must have deterministic tests or validation evidence.

Where functionality is intentionally unsupported, it must be explicitly classified as unsupported rather than appearing functional.

---

# Mission Principles

## 1. Correctness before breadth

Do not add features merely to increase subsystem counts.

A smaller set of correctly implemented primitives is preferable to a large collection of partial implementations.

Do not implement new experimental facilities unless the agent determines that they are directly necessary to complete or validate the core mission.

---

## 2. Evidence over claims

Every major capability must have evidence.

“Implemented” is not sufficient.

Prefer:

- automated tests
- QEMU boot tests
- syscall-level tests
- stress tests
- fault-path tests
- concurrency tests
- deterministic regression tests
- reproducible failure cases
- real-hardware validation where feasible

Do not mark functionality complete merely because it compiles or works once.

---

## 3. Preserve working functionality

Do not perform broad architectural rewrites without establishing regression coverage first.

Before changing critical infrastructure, identify the currently working behavior and create tests that protect it.

Existing CoW, fork, exec, boot, scheduler, filesystem, driver, and networking functionality must not regress silently.

---

## 4. No fake completeness

Do not convert stubs into apparently functional implementations without actually implementing them.

Do not hide incomplete functionality behind:

- unconditional success returns
- meaningless `Result<T, ()>`
- disabled warnings
- fake feature flags
- placeholder implementations
- tests that only exercise the happy path

If a subsystem is incomplete, either finish it to the required mission scope or explicitly classify it as experimental/unsupported.

---

# Required Mission Outcomes

## A. Establish a trustworthy validation system

Create a reliable automated validation pipeline covering the kernel's critical execution paths.

The agent must determine the appropriate test architecture.

At minimum, validation should cover:

- boot
- init/PID 1
- userspace entry
- ELF loading
- process creation
- fork
- CoW
- page faults
- exec
- exit/wait
- file descriptors
- pipes/IPC
- memory allocation
- scheduler behavior
- synchronization
- filesystem operations
- networking where enabled
- driver initialization
- syscall boundary behavior

Tests must distinguish:

- unit correctness
- kernel-internal behavior
- syscall/API behavior
- end-to-end boot behavior
- stress behavior
- failure behavior

The validation system must be usable repeatedly and must produce machine-readable success/failure results.

---

# B. Harden the process lifecycle

The process lifecycle is a critical trust boundary.

Validate the complete lifecycle:

create
→ run
→ fork
→ CoW
→ exec
→ signal/synchronization
→ exit
→ wait/reap

Test:

- multiple processes
- rapid fork/exit cycles
- fork followed by memory writes
- parent/child synchronization
- descriptor inheritance
- exec replacement
- invalid ELF
- invalid userspace pointers
- failed exec
- process termination
- orphan/reaping behavior
- concurrent process activity

The implementation must not merely pass a simple demonstration.

The mission should identify and eliminate lifecycle races, deadlocks, stale references, and resource leaks discovered during validation.

---

# C. Prove memory-management correctness

Treat memory management as one of the highest-priority areas.

Validate:

- physical frame allocation
- virtual memory mapping
- unmapping
- page faults
- demand paging
- CoW
- reference counting
- TLB invalidation
- address-space isolation
- userspace/kernel separation
- allocation failure paths

Particular attention must be given to:

- CoW page faults
- concurrent fork/page-fault activity
- lock acquisition inside fault paths
- invalid mappings
- double-free/use-after-free scenarios
- memory exhaustion
- corrupted page-table state

Any suspected correctness issue must be reproduced with a deterministic test before being considered resolved.

---

# D. Audit the syscall boundary

The syscall layer must be treated as an untrusted-input boundary.

Determine which currently advertised syscalls are actually functional.

Classify them honestly as:

- fully functional
- functional with limitations
- experimental
- stub
- unsupported

Critical syscalls should receive userspace-level tests rather than only internal kernel tests.

Test:

- invalid pointers
- invalid lengths
- integer overflow
- invalid file descriptors
- permission failures
- concurrent access
- resource exhaustion
- malformed arguments
- process termination during operations
- boundary values

Security-sensitive syscalls must not merely return successful values without enforcing their intended semantics.

---

# E. Make synchronization and locking defensible

Audit the kernel's global locking architecture.

Do not blindly rewrite every mutex.

First identify:

- global locks
- lock ordering
- IRQ-context locking
- nested locking
- locks held during page faults
- locks held during scheduling
- locks held during I/O
- interrupt-disabled critical sections
- potential lock inversions
- unnecessary global serialization

Construct a clear locking model.

Eliminate demonstrated deadlocks and dangerous lock dependencies.

Where global state is unnecessarily serialized, introduce better ownership, per-CPU state, finer-grained locking, or other appropriate mechanisms—but only where justified by evidence.

The objective is not “zero mutexes.”

The objective is:

> no known unsafe locking architecture on critical execution paths.

---

# F. Validate SMP and concurrency

The kernel claims SMP capability.

Therefore SMP must be treated as a real requirement rather than a compile-time feature.

Validate at minimum:

- SMP boot
- multiple runnable CPUs
- scheduler activity on multiple CPUs
- concurrent process creation
- concurrent memory faults
- concurrent filesystem access
- synchronization primitives
- interrupt interaction
- cross-CPU wakeups
- process migration if supported
- shared resource contention

If a claimed SMP feature is not actually supported, classify it honestly and constrain the supported configuration.

Do not claim scalable SMP merely because multiple CPUs initialize.

---

# G. Filesystem reliability

Select the filesystem(s) that are actually intended to form the supported Vahi baseline.

Do not attempt to make every existing filesystem production-quality simultaneously.

For the chosen supported filesystem:

Validate:

- create
- read
- write
- append
- truncate
- rename
- unlink
- directory operations
- metadata
- concurrent access
- allocation failure
- full filesystem
- malformed data
- recovery behavior
- repeated mount/unmount where supported

If SkyFS remains part of the supported baseline, specifically validate its journal and crash-recovery model.

A journaling filesystem must have a demonstrated answer to:

> “What happens if the system dies at every important point during a write?”

If that cannot currently be demonstrated, the filesystem must remain experimental.

---

# H. Driver reality check

Inventory every claimed driver.

For each driver determine:

- actually implemented?
- initialized?
- interrupt-driven?
- DMA-safe?
- error handling present?
- reset/recovery present?
- tested in QEMU?
- tested on hardware?
- used by the kernel?
- tested under failure?

Separate drivers into:

1. Supported
2. Experimental
3. Detection-only
4. Stub
5. Dead/unused

Do not inflate the driver count.

The supported driver set should be small enough to test properly.

---

# I. Security baseline

Do not attempt to reproduce the entire Linux security ecosystem.

Instead, establish a small but real Vahi security baseline.

The baseline must include demonstrable:

- kernel/userspace isolation
- userspace pointer validation
- privilege-boundary enforcement
- memory permission enforcement
- SMEP/SMAP where applicable
- stack protection where applicable
- address-space isolation
- resource ownership rules
- capability/permission semantics if advertised

Experimental security facilities such as seccomp, Landlock, CFI, or similar mechanisms must not be represented as production security features unless their actual enforcement is implemented and tested.

Security tests must intentionally attempt invalid operations and verify that the kernel rejects them safely.

---

# J. Unsafe-code audit

Perform a structured audit of unsafe code on critical paths.

Prioritize:

1. memory management
2. page tables
3. process/address-space management
4. interrupt handling
5. scheduler/context switching
6. DMA
7. drivers
8. userspace pointer access
9. filesystem internals

Every critical unsafe block should have a meaningful safety invariant.

Do not mechanically add comments such as:

“SAFETY: this is safe.”

The explanation must state why the required invariants hold.

---

# K. Error-handling cleanup

Identify weak error patterns, especially meaningless error types.

Prioritize replacement of:

`Result<T, ()>`

on critical paths where callers need to distinguish failure causes.

Do not perform a repository-wide mechanical rewrite if it creates unnecessary churn.

Improve error types where they materially improve:

- debugging
- recovery
- syscall semantics
- driver recovery
- filesystem handling
- memory-management diagnostics
- userspace-visible errors

---

# L. Codebase structural cleanup

Reduce architectural friction discovered during the mission.

Pay particular attention to oversized modules such as process/lifecycle and other files exceeding the repository's intended size guidelines.

Split modules based on responsibility, not arbitrary line counts.

The resulting architecture should make it easier to answer:

- where process state lives
- where lifecycle transitions occur
- where address spaces are managed
- where syscalls are dispatched
- where scheduling decisions occur
- where filesystem operations enter the kernel
- where hardware interaction occurs

Avoid refactoring purely for aesthetics.

Every structural change should improve ownership, testability, or correctness.

---

# M. Experimental-feature containment

Audit experimental subsystems such as:

- io_uring
- seccomp
- Landlock
- CFI
- eBPF extensions
- compositor
- hypervisor
- ASH/sandbox functionality
- other incomplete facilities discovered during repository inspection

The agent must determine whether each belongs in:

- supported baseline
- experimental feature
- isolated development area
- disabled/default-off feature
- removal

Do not allow incomplete functionality to masquerade as finished functionality.

---

# N. Real-hardware validation

After QEMU validation becomes reliable, move toward physical x86_64 hardware validation.

The objective is not broad hardware compatibility.

The objective is to prove that Vahi is not accidentally dependent on QEMU behavior.

Test the supported baseline on appropriate real hardware and investigate failures involving:

- timing
- interrupts
- APIC
- PCI
- DMA
- storage
- framebuffer
- memory maps
- CPU topology
- device initialization

Document the exact hardware/configuration used.

---

# O. Documentation must describe reality

Update project documentation only after implementation and validation establish the actual state.

Documentation must clearly distinguish:

- supported
- tested
- experimental
- partial
- unsupported
- planned

Remove misleading feature claims.

Do not use Linux feature counts as a proxy for kernel quality.

The project's credibility should come from reproducible engineering evidence.

---

# Definition of Done

The mission is complete only when all of the following are true:

### Boot

- Vahi boots reliably in the supported QEMU configuration.
- The complete boot-to-userspace path is automated.
- Boot failures are reproducible.
- Serial/TAP output is machine-readable.

### Userspace

- PID 1 reliably starts.
- ELF execution is validated.
- fork/exec/exit/wait are validated.
- CoW behavior is stress-tested.
- Process isolation is tested.

### Memory

- Allocation and reclamation are tested.
- Page faults are tested.
- CoW faults are tested.
- Address-space isolation is tested.
- Failure paths are tested.
- No known critical memory-management race remains.

### Syscalls

- Supported syscalls are explicitly classified.
- Critical syscalls have userspace-level tests.
- Invalid arguments and pointers are tested.
- Stub syscalls are not advertised as implemented.

### Scheduler

- SMP boot is tested if SMP remains supported.
- Concurrent execution is tested.
- Wake/sleep/block paths are tested.
- No known critical scheduler deadlock remains.

### Filesystem

- The selected supported filesystem has end-to-end tests.
- Concurrent operations are tested.
- Failure behavior is tested.
- Crash/recovery semantics are demonstrated or the filesystem is explicitly marked experimental.

### Drivers

- Supported drivers are explicitly identified.
- Unsupported/experimental drivers are clearly classified.
- At least one real-hardware validation path exists.

### Security

- Kernel/userspace isolation is tested.
- Invalid userspace memory access is tested.
- Permission boundaries are tested.
- Security features are not claimed unless enforcement exists.

### Code quality

- Critical unsafe code has documented invariants.
- Critical deadlocks/races found by the mission are fixed.
- Important error paths have meaningful errors.
- Experimental/dead code is contained or removed.
- Major architectural ownership boundaries are clear.

### Regression

- The complete validation suite can be run repeatedly.
- Existing functionality is protected against regression.
- Failures produce actionable diagnostics.
- The final repository state is reproducible from a clean build.

---

# Agent Autonomy

The mission deliberately does **not** prescribe the implementation sequence.

The executing agent must:

1. Inspect the entire repository and existing documentation.
2. Determine the actual current architecture.
3. Validate the audit's claims against source code and tests.
4. Identify dependencies between problems.
5. Generate its own implementation plan.
6. Prioritize foundational correctness over feature breadth.
7. Execute the work incrementally.
8. Run appropriate tests after each meaningful change.
9. Re-evaluate the architecture as implementation progresses.
10. Avoid unnecessary rewrites.
11. Stop treating compilation as proof of correctness.
12. Produce evidence for every major completion claim.

The agent is expected to reject incorrect assumptions in the existing documentation or audit when repository evidence contradicts them.

Do not blindly follow the audit.

Do not blindly preserve existing architecture.

Use evidence.

---

# Final Mission Deliverable

At the end of the mission, produce a final engineering report containing:

- actual supported feature set
- actual tested feature set
- remaining experimental features
- remaining known limitations
- tests added
- tests executed
- failures discovered and fixed
- architectural changes
- security findings
- concurrency findings
- filesystem findings
- hardware-validation results
- remaining technical debt
- explicit production-readiness assessment

The final assessment must be honest.

Do not declare the kernel “production-ready” merely because all tests pass.

The mission succeeds when Vahi becomes **measurably more reliable, more understandable, more testable, and more trustworthy than it was at mission start**.