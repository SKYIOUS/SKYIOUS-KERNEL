# Vahi Kernel — Agent Guide

## Build & Run

```powershell
python build_disk.py                          # full build (kernel → UEFI image → VDI)
.\make_bootimage.ps1                          # kernel + UEFI image only
qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=skyos_uefi.img -m 512M -smp 2
```

- Requires **nightly** Rust with `rust-src` + `llvm-tools-preview` components
- Target: `x86_64-unknown-none` with `-C target-feature=-mmx,-sse,+soft-float`
- Custom linker script: `kernel/linker.ld` (higher-half at `0xFFFFFFFF80000000`)
- Heap at `0xFFFF_C000_0000_0000`, phys mem offset at `0xFFFF_8000_0000_0000`
- `panic = "abort"` in both dev and release profiles
- **No test harness** — `#![no_std]` + `#![no_main]`, no `#[test]`s found. Verification is manual/boot-time.

## Project Structure

| Path | Purpose |
|------|---------|
| `kernel/` | Kernel crate (`vahi_kernel`), entrypoint `kernel/src/main.rs` via `bootloader_api::entry_point!` |
| `builder/` | Standalone crate that creates UEFI disk image from kernel ELF via `bootloader::UefiBoot` |
| `build_disk.py` | Orchestrates build + image creation + VDI conversion (requires `VBoxManage`) |
| `docs/` | Architecture, memory map, scheduler, VFS, syscall ABI, driver model, changelog |

## Feature Flags (in `kernel/Cargo.toml`)

`smp` — SMP support, `net` — smoltcp networking, `ai_rule` — Vahiai rule engine, `ai_llm` — LLM support
Default: `["smp", "net", "ai_rule"]`

## Key Conventions

- `// SAFETY:` comment required on every `unsafe` block
- `#![deny(warnings)]` — all warnings are errors
- Avoid `unwrap()`/`expect()` in kernel code; return `Result` instead
- Kernel data structures use `spin::Mutex` for locking
- `extern crate alloc;` at crate root
- Syscall ABI: `syscall` instr, rax=number, args in rdi,rsi,rdx,r10,r8,r9, return in rax
- `#[feature(abi_x86_interrupt)]` required for interrupt handlers

## Architecture Notes

- Monolithic kernel, higher-half mapped at `0xFFFFFFFF80000000`
- Physical memory: buddy allocator (`memory/buddy.rs`), slab for small kernel objects (`memory/slab.rs`)
- Scheduler: per-CPU, preemptive, priority-based (8 levels) round-robin, driven by LAPIC timer (10ms)
  - Each CPU has its own `PerCpuScheduler` with ready queues + `current_thread`
  - Global shared queues for sleep/block/futex states
  - New threads go to a global `pending_queue`; any idle CPU steals from it
  - Thread functions: `schedule()`, `try_schedule()`, `spawn()`, `spawn_thread()`, `block_on_pipe()`, `wake_pipe()`
  - `PerCpuData` (accessed via `gs:0x0`) is self-referential: offset 0 = `self_ptr` (pointer to struct itself)
- Async executor (`task/executor.rs`) runs in a dedicated kernel thread with `crossbeam-queue`
- VFS is trait-based (`VfsNode` trait); supported: Tmpfs, Ext2 (ro), FAT32, Pipe
- Boot: UEFI (OVMF) → `bootloader` crate v0.11 → `kernel_main(BootInfo)`
