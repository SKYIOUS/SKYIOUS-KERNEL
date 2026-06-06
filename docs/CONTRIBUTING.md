# Contributing to SkyOS

Thank you for your interest in contributing! This document outlines the contribution process and coding standards.

## 1. Code of Conduct

This project adheres to a standard code of conduct. Please be respectful and professional in all interactions.

## 2. Commit Message Format

We follow the Conventional Commits specification. Each commit message should be in the format:

```
<type>(<scope>): <short description>
```

-   **Types:** `feat`, `fix`, `refactor`, `docs`, `test`, `arch`, `driver`, `chore`
-   **Scope:** `mem`, `sched`, `vfs`, `net`, `syscall`, `shell`, `gui`, `build`

**Examples:**

-   `feat(mem): implement buddy frame allocator`
-   `fix(sched): fix context switch RSP alignment bug`
-   `docs(syscall): add doc comments to sys_open and sys_read`

## 3. Pull Request Process

1.  Fork the repository.
2.  Create a new branch for your feature or bug fix.
3.  Write clean, documented code that follows the project's coding style.
4.  Ensure your changes build successfully.
5.  Submit a pull request with a clear description of your changes.

## 4. Coding Style

-   Follow standard Rust conventions (`rustfmt`).
-   Add `///` doc comments to all public functions, structs, and enums.
-   Add `//!` module-level doc comments to every new file.
-   Use `spin::Mutex` for all kernel data structures that require locking.
-   Avoid `unwrap()` and `expect()` in kernel code that can fail gracefully. Return a `Result` instead.
-   Justify every `unsafe` block with a `// SAFETY:` comment explaining why the operation is sound.
