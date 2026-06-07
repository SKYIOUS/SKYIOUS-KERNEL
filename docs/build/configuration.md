# Feature Flags and Build Configuration

SkyOS uses Cargo features for build-time configuration.

## Feature Flags

```toml
[features]
default = ["acpi", "pci", "ps2"]
acpi = ["dep:aml"]
pci = []
ps2 = []
virtio = []
e1000 = []
smp = []
kasan = []
profiling = []
log_error = []
log_warn = ["log_error"]
log_info = ["log_warn"]
log_debug = ["log_info"]
log_trace = ["log_debug"]
```

## Enabling Features

```bash
# Build with specific features
cargo build --features "virtio,e1000,smp"

# Build with all features
cargo build --features "log_trace,profiling,kasan"
```

## Kernel Parameters

The kernel accepts parameters via the boot command line:

| Parameter | Description | Default |
|-----------|-------------|---------|
| `log_level` | Log filter (error/warn/info/debug/trace) | `info` |
| `init` | Path to init program | `/sbin/init` |
| `root` | Root filesystem device | `/dev/sda1` |
| `mem` | Maximum memory (e.g., `mem=512M`) | All detected |
| `norandmaps` | Disable ASLR | enabled |
| `smp` | Number of CPUs to use | All available |

## Build Profiles

Debug builds include:
- Runtime assertions and safety checks
- KASAN (kernel address sanitizer)
- Full debug symbols
- Unoptimized code for faster compilation

Release builds include:
- LTO (Link-Time Optimization)
- Code generation optimizations (-O2)
- Strip debug symbols (optional)
- Profile-guided optimization (optional)
