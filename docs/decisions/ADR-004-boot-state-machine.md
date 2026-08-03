# ADR-004: Formal Boot State Machine for PID 1 Launch

## Status
Accepted

## Date
2026-08-01

## Context
The kernel boot flow (hardware init → scheduler → userspace launch) is complex. Before the boot state machine was introduced, boot failures produced unclear error output with no structured trace. The boot sequence had phases that used the error-prone approach of sequential functions with manual error propagation, making it hard to:
- Recover from non-fatal errors during boot
- Produce meaningful diagnostic output on failure
- Validate that phases execute in the correct order
- Add or remove boot phases without breaking the sequence

## Decision
Implement a formal boot state machine in `kernel/src/boot/` with:

1. An enum `BootState` with validated transitions:
   ```
   InitKernel → LocateInit → ParseElf → CreateAddressSpace → MapStack → CreatePid1 → SetupConsole → EnterUserspace → Running
   ```
2. A `BootContext` carrying trace events, tried init paths, and ELF data
3. A `BootSession` for transient launch objects (entry point, stack pointer)
4. A `BootLogger` for structured boot output
5. Transition validation in `state.rs:valid_next()` — illegal transitions cause a panic with full trace dump

## Alternatives Considered

### Sequential functions with early returns
- Pros: Simple, minimal code
- Cons: No trace on failure, no transition validation, poor diagnostics
- Rejected: Debugging boot failures was too difficult

### Builder pattern
- Pros: Type-safe configuration
- Cons: Awkward for a linear sequence, more boilerplate
- Rejected: State machine more naturally models the constrained transitions

### Global state + goto-like error handling
- Pros: Fast to write
- Cons: Unstructured, hard to reason about
- Rejected: Not maintainable

## Consequences
- Boot failures produce a detailed trace (all entered/exited states, errors, attempted init paths)
- Illegal transitions are caught at runtime with a panic
- Adding a new phase requires adding an enum variant + transition validation + handler function
- The boot trace is stored globally for access by the panic handler
- The machine is fully synchronous — no async during boot