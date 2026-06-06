# Korlang ABI Contract

This document defines the interface between Korlang programs and the Skyious kernel.

## 1. Runtime Environment

- **Format:** Korlang programs are compiled to standard 64-bit ELF files.
- **Entry Point:** The entry point is defined in the ELF header.
- **Stack:** The kernel provides a 32KB stack for each process.

## 2. Calling Convention

Korlang uses the standard SkyOS Syscall ABI (see `SYSCALL_ABI.md`) for all interactions with the kernel.

## 3. Specialized Syscalls

In addition to standard POSIX syscalls, SkyOS provides a dedicated Korlang syscall:

- **`SYS_KORLANG` (Number 201):**
  - **Purpose:** Provide specialized runtime support for the Korlang interpreter/JIT.
  - **Arguments:**
    - `rdi`: Sub-command ID
    - `rsi`: Argument 1
    - `rdx`: Argument 2
  - **Sub-commands:**
    - `0`: Panic (Logs a message and exits the process).

## 4. `libsky` (Korlang Standard Library)

Korlang programs should use the `libsky` library, which wraps these syscalls into idiomatic Korlang functions (e.g., `print`, `spawn`, `open`).
