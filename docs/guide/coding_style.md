# Rust Coding Conventions

SkyOS follows Rust coding conventions with additional project-specific rules.

## Naming Conventions

- Types: `PascalCase` (`MemoryManager`, `PageTable`)
- Functions: `snake_case` (`map_page`, `allocate_frame`)
- Variables: `snake_case` (`free_frames`, `current_task`)
- Constants: `SCREAMING_SNAKE_CASE` (`PAGE_SIZE`, `KERNEL_HEAP_BASE`)
- Statics: `SCREAMING_SNAKE_CASE` (`GLOBAL_ALLOCATOR`, `CPU_LOCAL`)
- Type parameters: single uppercase letter (`T`, `E`)

## Formatting

- Use `rustfmt` with default settings
- Line width: 100 characters
- Indentation: 4 spaces (no tabs)
- Imports grouped: `core`/`alloc`, external crates, internal modules

```rust
use core::sync::atomic::{AtomicBool, Ordering};
use alloc::sync::Arc;
use crate::memory::paging::PageTable;
```

## Safety

- Mark `unsafe` blocks with a safety comment explaining the invariants
- Prefer safe abstractions over direct `unsafe` when possible
- Document soundness guarantees for all `unsafe` functions

## Error Handling

- Use `Result<T, Error>` for fallible operations
- Define error types as enums with `#[non_exhaustive]`
- Use `anyhow`-style context for error propagation

## No Panic Policy

Kernel code should never panic. Use `.ok()`, `Option`, and `Result` instead of `.unwrap()`, `.expect()`, or array indexing that could panic. Critical assertions use `assert!` only in debug builds.
