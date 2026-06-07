# Debugging Techniques

SkyOS supports multiple debugging methods for kernel development.

## Serial Output

The kernel logs diagnostic information through a serial port (COM1 at I/O port `0x3F8`). Use QEMU's `-serial stdio` to view output. Log levels include `ERROR`, `WARN`, `INFO`, `DEBUG`, and `TRACE`, controlled by the `log_level` kernel parameter.

```rust
// In kernel code
log_info!("Memory manager initialized: {} frames free", free_frames);
log_debug!("Page table at {:?}", page_table_addr);
```

## GDB Debugging

Connect GDB to QEMU for full breakpoint and step debugging:

```gdb
(gdb) target remote :1234
(gdb) add-symbol-file target/release/skyos 0x200000
(gdb) break src/kernel/main.rs:42
(gdb) continue
```

## Kernel Panic Handler

When a kernel panic occurs, the panic handler prints:
- The panic message and location
- A stack backtrace (if frame pointers are enabled)
- CPU register state
- Current task information

## QEMU Logging

QEMU can log guest interactions:
```bash
qemu-system-x86_64 -d int,cpu_reset -D qemu.log
```

## KASAN

The kernel includes a software-based address sanitizer (KASAN) for detecting use-after-free and out-of-bounds accesses. Enable it with `--features kasan` during builds.
