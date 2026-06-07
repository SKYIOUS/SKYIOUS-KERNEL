# Coding Standards

This document defines the Rust coding standards and conventions for the SkyOS kernel. All contributions must adhere to these guidelines.

## Formatting

Code must be formatted with `rustfmt` using the project's `rustfmt.toml` configuration. The CI pipeline enforces formatting, and pull requests that deviate will be rejected. Use `cargo fmt` before committing to ensure compliance. Configuration for line width, indentation, and import grouping is defined in the repository root.

## Naming Conventions

Follow standard Rust naming conventions: `snake_case` for functions, methods, variables, and module names; `UpperCamelCase` for types, enums, and traits; `SCREAMING_SNAKE_CASE` for constants and statics. Use descriptive names that convey intent. Avoid abbreviations unless they are universally understood (e.g., `vm`, `cpu`, `io`). Prefer full words like `address` over `addr`, and `allocation` over `alloc` in public API surfaces.

## Unsafe Code Rules

Unsafe Rust is necessary in kernel development but must be used sparingly and rigorously justified. Every `unsafe` block must be preceded by a `// SAFETY:` comment that explains why the preconditions for safety are upheld, referencing the relevant invariants or documentation. Functions marked `unsafe` must document their safety preconditions in a `# Safety` section in the doc comment. Wrap unsafe operations in safe abstractions whenever possible, and mark internal helper functions as `unsafe` only when they truly require it.

## Imports and Module Structure

Group imports in the following order, separated by blank lines: standard library (`core::`, `alloc::`), external crates, internal kernel modules (`crate::`). Use `use` statements at the module level; avoid deep nesting of `use` inside functions unless necessary to resolve ambiguity. Each module file should begin with a `//!` doc comment describing its purpose.

## Error Handling

Prefer returning `Result<T, E>` over panicking. Kernel code should avoid `unwrap()` and `expect()` except in contexts where failure is truly impossible (e.g., infallible allocation in init code). Use `?` operator for propagation. Define custom error types using enums rather than integer error codes. Document error variants and their meanings.

## Locking and Concurrency

Use `spin::Mutex` for kernel data structures that require locking in interrupt-disabled contexts. Use `IrqSafeLock` (or equivalent) for data shared between normal execution and interrupt handlers. Acquire locks for the shortest duration possible, and document what each lock protects. Avoid lock ordering issues by establishing a global lock hierarchy and documenting it in comments.

## Documentation

All public functions, structs, enums, and traits must have `///` doc comments explaining their purpose, parameters, return values, and any panic or error conditions. Module-level `//!` comments should summarize the module's role in the kernel. Inline comments should explain non-obvious logic, not restate the code.
