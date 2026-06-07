# UEFI Boot Image Creation

SkyOS boots on UEFI systems using a custom bootloader.

## Boot Process

1. **UEFI firmware** loads the bootloader (`skyos.efi`) from the EFI system partition
2. **Bootloader** initializes UEFI services, sets up graphics output, and loads the kernel ELF
3. **Kernel** is parsed and loaded into memory by the bootloader
4. **Exit boot services** - UEFI boot services are terminated
5. **Kernel entry** - Execution transitions to the kernel

## Boot Image Structure

The boot image is a FAT32 partition with an embedded UEFI application:

```
bootimage-skyos.bin
├── EFI/
│   └── BOOT/
│       └── BOOTX64.EFI    # UEFI bootloader
├── kernel.elf             # The kernel binary
└── initrd.img             # Initial ramdisk
```

## Creating the Boot Image

The boot image is created by the `bootimage` tool:

```bash
# Build and create boot image in one step
cargo bootimage

# Or build components separately
cargo build --release
cargo bootimage --kernel target/x86_64-skyos/release/skyos
```

## Writing to Physical Media

```bash
# Write to USB drive (Linux)
sudo dd if=bootimage-skyos.bin of=/dev/sdX bs=1M status=progress

# Write to USB drive (macOS)
sudo dd if=bootimage-skyos.bin of=/dev/disk2 bs=1m
```

## ISO Creation

For optical media or virtual machines:

```bash
xorriso -as mkisofs -b bootimage-skyos.bin \
    -no-emul-boot -boot-load-size 4 \
    -o skyos.iso bootimage-skyos.bin
```
