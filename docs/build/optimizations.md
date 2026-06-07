# Build Optimization Flags and LTO

SkyOS uses several optimization techniques for release builds.

## Link-Time Optimization (LTO)

LTO is enabled in release builds for the kernel:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
```

"Fat" LTO allows cross-language LTO between Rust and C/C++ code.

## Code Generation

```toml
[profile.release]
opt-level = 3
debug = false
panic = "abort"
```

- **opt-level = 3**: Maximum optimization for speed
- **panic = "abort"**: No unwinding code, reduces binary size

## Target-Specific Optimizations

```toml
[target.x86_64-skyos-unknown]
rustflags = [
    "-C", "target-cpu=x86-64-v3",  # Modern x86_64 features
    "-C", "link-arg=-Tlinker.ld",  # Custom linker script
    "-C", "force-frame-pointers=no",  # Omit frame pointers for perf
]
```

## Binary Size Reduction

```bash
# Strip debug symbols
cargo build --release
strip -s target/x86_64-skyos/release/skyos

# Use LTO for cross-crate inlining
# Already enabled in profile.release

# Remove unused code
rustflags = ["-C", "link-arg=--gc-sections"]
```

## Profile-Guided Optimization (PGO)

For maximum performance, PGO can be used:

```bash
# Step 1: Build with instrumentation
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release

# Step 2: Run representative workloads in QEMU
cargo run --release

# Step 3: Rebuild using profiling data
RUSTFLAGS="-Cprofile-use=/tmp/pgo-data" cargo build --release
```

## Compile-Time Optimization

For faster compilation during development:

```toml
[profile.dev]
opt-level = 0
debug = true
codegen-units = 256   # More parallel codegen
incremental = true    # Incremental compilation
```
