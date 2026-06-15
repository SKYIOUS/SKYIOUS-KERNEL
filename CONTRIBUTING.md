# Contributing to the Vahi Kernel

Thank you for your interest in contributing to the **Vahi kernel**, the core of **SARGA OS**. All contributions are welcome — kernel code, drivers, documentation, testing, bug reports, and ideas.

This project is governed by the **SKYIOUS Software License (SSL)**. By contributing, you agree that your contributions will be licensed under the same terms.

---

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Ways to Contribute](#ways-to-contribute)
- [Getting Started](#getting-started)
- [Development Workflow](#development-workflow)
- [Architecture Overview](#architecture-overview)
- [Build System](#build-system)
- [Commit Messages](#commit-messages)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Reporting Issues](#reporting-issues)
- [Feature Requests](#feature-requests)
- [Community](#community)
- [License](#license)

---

## Code of Conduct

We are committed to providing a welcoming, inclusive, and harassment-free experience for everyone, regardless of:

- Age, body size, disability, ethnicity, gender identity and expression
- Level of experience, nationality, personal appearance, race, religion
- Sexual identity and orientation, or any other dimension of diversity

### Expected Behavior

- Be respectful and considerate in all interactions
- Use welcoming and inclusive language
- Accept constructive criticism gracefully
- Focus on what is best for the community and the project
- Show empathy towards other community members

### Unacceptable Behavior

- Harassment, intimidation, or discrimination in any form
- Trolling, insulting/derogatory comments, and personal or political attacks
- Publishing others' private information without explicit permission
- Any other conduct which could reasonably be considered inappropriate

### Enforcement

Project maintainers are responsible for clarifying and enforcing these standards. Violations may be reported to the project team and will be addressed promptly. Consequences may range from a warning to temporary or permanent ban from the project.

---

## Ways to Contribute

You don't need to be a kernel expert to contribute. Here are many ways to help:

| Area | How to Contribute |
|------|-------------------|
| **Report bugs** | Open an issue with clear steps to reproduce |
| **Suggest features** | Open an issue describing the feature |
| **Write drivers** | Add support for new hardware (NICs, storage, audio, USB) |
| **Add filesystems** | Implement new filesystem drivers |
| **Improve architecture** | Port to aarch64, RISC-V, or other architectures |
| **Write syscalls** | Add new system calls for userspace |
| **Fix bugs** | Pick an open issue and submit a pull request |
| **Write tests** | Add unit tests, integration tests, or edge-case coverage |
| **Improve documentation** | Enhance docs, doc comments, guides, and the architecture docs |
| **Performance tuning** | Optimize hot paths, reduce memory usage |
| **Security auditing** | Review code for vulnerabilities, improve safety |
| **Build tooling** | Improve build scripts, CI, developer experience |
| **Code review** | Review open pull requests |
| **Answer questions** | Help others in issues and discussions |
| **Spread the word** | Star the repo, write about the project, tell friends |

---

## Getting Started

### Prerequisites

- **Rust** (nightly channel)
- **Git**
- **QEMU** (for testing)
- Basic knowledge of operating systems concepts
- Familiarity with Rust (or willingness to learn)

### Setup

```bash
# Clone the repository
git clone https://github.com/your-username/SKYIOUS-KERNEL.git
cd SKYIOUS-KERNEL

# Ensure you have nightly Rust
rustup default nightly

# Install required components
rustup component add rust-src
rustup component add llvm-tools-preview

# Add the x86_64 target
rustup target add x86_64-unknown-none

# Build the kernel
cd kernel
cargo build
```

### Finding Your First Contribution

- Look for issues labeled `good first issue` or `help wanted`
- Check the `docs/future/` directory for the roadmap and phase plans
- Browse open issues for work items
- Look at existing drivers for patterns if you want to add a new one
- Consider porting a simple driver from Linux to see how the driver model works

---

## Development Workflow

### Branching

- `master` — the main development branch, always buildable
- For changes, create a new branch from `master`:
  ```bash
  git checkout -b fix/descriptive-name
  ```

### Development Loop

```bash
# 1. Make your changes in kernel/src/

# 2. Build the kernel (quick)
cd kernel && cargo build

# 3. Build with features
cargo build --features self_test
cargo build --features "smp,net,ai_rule"

# 4. Build bootimage
cd ../builder && cargo run -- ../kernel/target/x86_64-unknown-none/debug/vahi_kernel

# 5. Run in QEMU
../run_test_nographic.ps1    # Serial-only test
../run_qemu_display.ps1      # Full display
```

### Building Userspace

```powershell
# Build all userspace crates and create initrd
.\build_userspace.ps1

# Then rebuild kernel + bootimage (userspace is embedded in kernel)
cd kernel && cargo build
cd ../builder && cargo run -- ../kernel/target/x86_64-unknown-none/debug/vahi_kernel
```

### Keeping Your Fork Updated

```bash
git remote add upstream https://github.com/SKYIOUS/SKYIOUS-KERNEL.git
git fetch upstream
git rebase upstream/master
```

---

## Architecture Overview

For a detailed architecture walkthrough, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Here is a high-level summary:

### Kernel Modules

```
kernel/src/
├── main.rs          Entry point, boot flow, panic handler
├── arch/            Architecture abstraction (x86_64, aarch64)
├── memory/          Physical + virtual memory management
├── task/            Processes, threads, scheduler, async executor
├── syscalls/        System call dispatch, signal handling
├── vfs/             Virtual filesystem layer, filesystem drivers
├── drivers/         Hardware drivers (storage, net, audio, USB, GPU)
├── net/             TCP/IP stack (smoltcp integration)
├── gui/             Compositor, window manager, terminal emulator
├── ebpf/            eBPF virtual machine and verifier
├── emulation.rs     Linux syscall emulation
├── elf_dyn.rs       Dynamic ELF loader
├── acpi.rs          ACPI table parsing
├── pci.rs           PCI bus enumeration
└── security.rs      LSM framework
```

### Syscall ABI

The syscall ABI is **frozen for v1.0**. See [docs/SYSCALL_ABI.md](docs/SYSCALL_ABI.md) for the complete specification.

### Key Design Decisions

- Monolithic kernel for performance and simplicity
- Rust ownership model for memory safety (no garbage collector)
- Preemptive priority-based scheduler with cooperative async tasks
- Higher-half kernel (x86_64: `0xFFFFFFFF80000000`)
- UEFI boot via `bootloader` crate
- Architecture abstraction via `Arch` trait for portability

---

## Build System

### Kernel Build

```bash
cd kernel
cargo build                     # Debug build
cargo build --release           # Release build with LTO + opt-level=z
cargo build --features self_test  # With self-test framework
cargo build --features "smp,net,ai_rule"  # Default features
```

### Cargo Features

| Feature | Default | Description |
|---------|---------|-------------|
| `smp` | Yes | SMP support (multi-core) |
| `net` | Yes | Network stack (smoltcp) |
| `ai_rule` | Yes | VahiAI subsystem |
| `ai_llm` | No | LLM integration |
| `self_test` | No | Built-in self-test framework |

### Builder

The `builder/` crate creates a UEFI bootable disk image from the compiled kernel:

```bash
cd builder
cargo run -- ../kernel/target/x86_64-unknown-none/debug/vahi_kernel
```

### Target Specifications

| Target | Arch | Description |
|--------|------|-------------|
| `x86_64-unknown-none` | x86_64 | Built-in rust-src target for kernel |
| `aarch64-unknown-none.json` (in kernel/) | aarch64 | Custom target for ARM64 kernel |
| `x86_64-skyos.json` (in userspace/) | x86_64 | Custom target for userspace |

### Linker Scripts

| Script | Arch | Kernel Base |
|--------|------|-------------|
| `kernel/linker.ld` | x86_64 | `0xFFFFFFFF80000000` (higher-half) |
| `kernel/aarch64-linker.ld` | aarch64 | `0x40080000` (physical) |

---

## Commit Messages

We follow the **Conventional Commits** specification. Each commit message must be in the format:

```
<type>(<scope>): <short description>

<body (optional)>

<footer (optional)>
```

### Types

| Type | Usage |
|------|-------|
| `feat` | A new feature |
| `fix` | A bug fix |
| `docs` | Documentation only changes |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `test` | Adding or modifying tests |
| `chore` | Build process, CI, tooling changes |
| `style` | Formatting, whitespace (no code change) |
| `perf` | A performance improvement |
| `driver` | A driver addition or modification |
| `arch` | Architecture-specific changes (x86_64, aarch64) |
| `syscall` | Syscall additions or modifications |

### Scopes

| Scope | Area |
|-------|------|
| `mem` | Memory management |
| `sched` | Scheduler, processes, threads |
| `vfs` | Filesystem, VFS layer |
| `net` | Network stack, drivers |
| `syscall` | Syscall dispatch, ABI |
| `drv` | Hardware drivers |
| `gui` | Compositor, window manager |
| `acpi` | ACPI, power management |
| `pci` | PCI enumeration |
| `ebpf` | eBPF VM, verifier |
| `arch` | Architecture abstraction |
| `build` | Build system, linker, config |
| `ci` | CI pipeline |
| `docs` | Documentation |

### Examples

```
feat(sched): implement priority-based round-robin scheduler with 8 levels

fix(vfs): handle dangling symlinks in path resolution

driver(storage): add NVMe SSD driver with PRP DMA

arch(aarch64): implement context switch for ARM64

docs(syscall): document SYS_CLONE calling convention

test(mem): add buddy allocator stress test for order-0 through order-10
```

---

## Pull Request Process

1. **Before starting**, check if an issue exists for your change. If not, open one to discuss. This avoids wasted effort.

2. **Fork** the repository and create a feature branch from `master`.

3. **Write your code** following the coding standards below.

4. **Build and test** your changes locally:
   ```bash
   cd kernel && cargo build
   cargo build --features self_test
   ```

5. **Commit** your changes with a descriptive commit message.

6. **Push** to your fork and submit a pull request against `master`.

7. **Describe your changes** in the PR. Include:
   - What the change does and why it's needed
   - How it was tested
   - Any breaking changes or migration steps
   - Architecture impact (does it affect x86_64, aarch64, or both?)

8. **Respond to review feedback** and make requested changes.

9. **After approval**, a maintainer will merge your PR.

### PR Checklist

- [ ] Code compiles without warnings (`cargo build`)
- [ ] No new `deny(warnings)` violations
- [ ] Follows coding style (`cargo fmt`)
- [ ] Includes `///` doc comments for public items
- [ ] All `unsafe` blocks have `// SAFETY:` justification
- [ ] Avoids `unwrap()` in kernel code paths
- [ ] Includes tests where applicable
- [ ] Commit messages follow Conventional Commits
- [ ] PR description clearly explains the change

---

## Coding Standards

### Rust Conventions

- Follow standard Rust conventions (`rustfmt` with default settings)
- Use Rust 2021 edition
- `#![deny(warnings)]` is enabled — all warnings are errors
- No `std` — kernel is `#![no_std]` with `extern crate alloc;`

### Documentation

- Add `///` doc comments to all public functions, structs, enums, and traits
- Add `//!` module-level doc comments to every new file
- Document error conditions and panics
- Use doc examples where appropriate

### Safety (Critical)

- Justify every `unsafe` block with a `// SAFETY:` comment explaining why the operation is sound
- Example:
  ```rust
  // SAFETY: The pointer is valid because we checked bounds
  // and the page is mapped. The caller guarantees exclusive
  // access to this memory region.
  core::ptr::write_volatile(addr, val);
  ```
- Prefer safe abstractions over raw pointer manipulation
- Validate all user-supplied pointers and lengths in syscalls
- No panicking paths in interrupt handlers

### Naming

- `snake_case` for functions, methods, variables, modules
- `UpperCamelCase` for types, enums, traits
- `SCREAMING_SNAKE_CASE` for constants and statics
- Descriptive names over short abbreviations
- Prefix kernel-internal functions with `k_` where ambiguity exists

### Error Handling

- Return `Result<T, E>` instead of panicking or using `unwrap()`
- Define meaningful error types
- Propagate errors up to the syscall boundary, then convert to errno values
- Use `?` operator for error propagation

### Locking

- Use `spin::Mutex` for kernel data structures (no OS locks available)
- Keep critical sections short
- Document lock ordering to prevent deadlocks
- Avoid holding locks across context switches

### Interrupt Safety

- Interrupt handlers must not block or acquire locks held by non-interrupt code
- Use lock-free or interrupt-safe mechanisms where needed

---

## Testing

### Unit Tests (Self-Test Framework)

The kernel has a built-in self-test framework, gated by the `self_test` feature:

```bash
cd kernel
cargo build --features self_test
```

Tests live in `kernel/src/tests/` or inline in module files:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_free() {
        let addr = allocate_block().expect("allocation failed");
        free_block(addr);
        // Verify block is returned to free list
    }
}
```

### Current Test Coverage

- **SkyFS**: format, mount, create/write/read files, unlink, directory operations
- **eBPF**: verifier safety checks (LDX_R10 protection, CALL validation), VM execution
- **Memory**: buddy allocator basic allocation and deallocation

### Integration Tests (QEMU)

Integration tests are in the `tests/` directory:

```powershell
.\tests\test_boot.ps1       # Boot and verify login prompt appears
.\tests\test_login.ps1      # Login as root and verify shell prompt
.\tests\test_panic.ps1      # Verify kernel panic displays correctly
```

Each test runs QEMU with `-nographic -serial stdio`, monitors serial output, and returns PASS/FAIL.

### Adding Tests

When adding a new feature, add tests for:

1. **Happy path** — the feature works under normal conditions
2. **Error handling** — the feature handles invalid input gracefully
3. **Edge cases** — boundary conditions, zero-length, maximum sizes
4. **Safety** — unsafe blocks are exercised and verified

### CI Pipeline

GitHub Actions runs on every push and pull request:

```yaml
Jobs:
  - build-kernel:   Compile for x86_64 and aarch64
  - build-userspace: Compile userspace workspace
  - selftest:       Compile with self_test feature
```

---

## Documentation

Comprehensive documentation is at [`docs/`](docs/). Please contribute to:

- **Doc comments** on all public API in the kernel
- **Architecture docs** — update `docs/ARCHITECTURE.md` when adding major subsystems
- **Syscall docs** — document new syscalls in `docs/syscalls/`
- **Driver docs** — add driver documentation in `docs/drivers/`
- **`docs/future/`** — update the roadmap as features are implemented
- **`docs/guide/`** — add guides for common tasks

### Documentation Template for New Features

When adding a significant new feature, include:

```
- What the feature does
- Why it was designed this way (alternatives considered)
- How to use it (API surface)
- Limitations and future improvements
```

---

## Reporting Issues

When reporting a bug, please include:

- **Clear title** describing the issue
- **Steps to reproduce** — what actions lead to the bug
- **Expected behavior** — what should happen
- **Actual behavior** — what actually happens (including panic messages, register dumps)
- **Environment**:
  - QEMU command line or hardware config
  - Kernel commit hash
  - Build profile (debug/release)
  - Features enabled
- **Logs** — serial output, especially panic messages

```markdown
Title: Page fault in sys_open when path exceeds 256 bytes

Steps to reproduce:
1. Call sys_open with a path of 300 bytes
2. Path buffer is on the stack

Expected: Returns ENAMETOOLONG
Actual: Kernel page fault at 0xFFFF_8000_1234_5678

Environment:
- QEMU 9.0, `-cpu max`, 2 cores
- Commit abcdef1234
- Debug build with default features

Serial output:
[0] PAGE FAULT at 0xFFFF_8000_1234_5678
[0] Error code: PF_USER | PF_WRITE
[0] RSP: 0xFFFF_C000_0000_4000
```

---

## Feature Requests

Feature requests are welcome. Please include:

- **What** — describe the feature clearly
- **Why** — what problem does it solve, what use case does it enable
- **Implementation ideas** — any thoughts on approach (optional)
- **Prior art** — similar features in Linux, BSD, or other kernels

### Current Priority Areas

- aarch64 stabilization and MMU/GIC/UEFI boot
- RISC-V architecture port
- WiFi driver support
- Real hardware testing (NVMe, AHCI, USB, HDA on bare metal)
- Performance optimization and profiling
- Comprehensive test suite expansion
- Userspace toolchain improvements (self-hosting compiler)

---

## Community

- **Issues**: Use GitHub issues for bugs and feature requests
- **Pull Requests**: For code contributions
- **Discussions**: Use GitHub discussions for questions and ideas

We strive to:
- Review PRs within 7 days
- Respond to issues within 3 days
- Be respectful and constructive in all feedback
- Welcome first-time contributors with mentorship

---

## License

By contributing to the Vahi kernel, you agree that your contributions will be licensed under the **SKYIOUS Software License (SSL) v1.0**. See the [LICENSE](LICENSE) file for details.

This project and its contributors operate under the principle that **code contributions are irrevocable** — once submitted and accepted, they become part of the project under the project's license.
