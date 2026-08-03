# ADR-008: Cargo Feature Flags for Kernel Configuration

## Status
Accepted

## Date
2026-08-01

## Context
The kernel has optional subsystems (networking, SMP, AI, GPU, etc.) that not all builds need. We needed a mechanism to conditionally compile components without a separate configuration system.

Requirements:
- No runtime configuration overhead for disabled features
- Compile-time elimination of unused code
- Simple to enable/disable for development
- Different default sets for development vs release

## Decision
Use Cargo feature flags (compile-time) for all optional subsystems. No runtime config file or Kconfig-style system.

```toml
[features]
default = ["smp", "net", "ai_rule", "ext4"]
verification = []
ai_rule = []
ai_llm = []
smp = []
net = []
uhci = []
ext4 = []
self_test = []
ash = []
gpu = []
hypervisor = []
objects_v2 = []
```

Each feature gate is checked with `#[cfg(feature = "...")]` in source code, applied at the module level or individual item level.

## Alternatives Considered

### Kconfig-style (Linux kernel config)
- Pros: Familiar to kernel developers, supports dependencies and prompts
- Cons: Separate build system, not integrated with Cargo
- Rejected: Overkill for this project's complexity level

### Runtime feature detection
- Pros: Single binary for all hardware
- Cons: Code size bloat, runtime checks on every code path
- Rejected: Eliminating dead code at compile time is better for a kernel

### Conditional compilation with build.rs
- Pros: More flexible than feature flags
- Cons: More complex, out-of-band from Cargo.toml
- Rejected: Feature flags are sufficient for our subsystem-level gating

## Consequences
- `default = ["smp", "net", "ai_rule", "ext4"]` for general use
- `#[cfg(feature = "smp")]` prevents SMP code from being compiled when disabled
- Features are additive — no feature interaction issues
- Build combinations are tested in CI with `--no-default-features` and feature combinations
- Zero runtime overhead for disabled features