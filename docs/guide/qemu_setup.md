# QEMU Setup and Debugging

SkyOS uses QEMU as the primary emulation and debugging platform.

## Basic QEMU Configuration

Create a `qemu.sh` script in the project root:

```bash
#!/bin/bash
qemu-system-x86_64 \
    -machine q35 \
    -cpu max \
    -m 512M \
    -serial stdio \
    -drive format=raw,file=target/x86_64-skyos/release/bootimage-skyos.bin \
    -device virtio-net,netdev=net0 \
    -netdev user,id=net0,hostfwd=tcp::8080-:80
```

## GDB Debugging

For debugging with GDB, start QEMU with the `-s` flag:

```bash
qemu-system-x86_64 \
    -s -S \
    -machine q35 \
    -cpu max \
    -m 512M \
    -serial stdio \
    -drive format=raw,file=bootimage.bin
```

Then in another terminal:

```bash
gdb target/x86_64-skyos/release/skyos
(gdb) target remote :1234
(gdb) break kernel_main
(gdb) continue
```

## Networking

For network testing, QEMU's user-mode networking provides NAT and port forwarding. Use `-nic user,hostfwd=tcp::8080-:80` to forward host port 8080 to guest port 80.

## QEMU Monitor

Press `Ctrl+Alt+2` in the QEMU window to access the monitor. Useful commands:
- `info registers` - CPU register dump
- `info cpus` - CPU state
- `system_reset` - Reset the emulated system
- `quit` - Exit QEMU
