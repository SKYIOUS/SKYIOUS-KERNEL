# vahi-arch

Architecture-specific code — CPU detection, register manipulation, MSRs.

## Modules

| Module | Contents |
|--------|----------|
| `cpu` | rdtsc, rdmsr/wrmsr, invlpg, flush_tlb, hlt |

## Architecture Dispatch

```rust
#[cfg(target_arch = "x86_64")]  → x86_64_impl
#[cfg(target_arch = "aarch64")] → aarch64_impl
```

All architecture-specific code behind `#[cfg]` guards.

## CPU Features Detected

| Feature | CPUID Leaf |
|---------|-----------|
| x2APIC | CPUID.01H:ECX[21] |
| XSAVE | CPUID.01H:ECX[26] |
| FSGSBASE | CPUID.07H:EBX[0] |
| SMEP | CPUID.07H:EBX[7] |
| SMAP | CPUID.07H:EBX[20] |
| RDRAND | CPUID.01H:ECX[30] |

## Safety

- MSR reads/writes are marked `unsafe` with `# Safety` docs
- `invlpg` must only be called after PTE modification
- `flush_tlb` reloads CR3 — use only when needed (expensive)
