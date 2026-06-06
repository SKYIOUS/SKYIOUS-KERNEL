# Building and Running SkyOS

This document provides instructions for building the Skyious kernel and running it in QEMU and VirtualBox.

## 1. Prerequisites

-   Rust (nightly toolchain)
-   `rustup component add rust-src`
-   `cargo install bootimage` (or ensure it's available)
-   QEMU (for running)
-   VirtualBox (for running)

## 2. Build Process

The build process is managed by the `build_disk.py` Python script in the root of the repository.

To build the kernel and create bootable disk images, run:

```bash
python build_disk.py
```

This script performs the following steps:

1.  **Cleans old images:** Removes any previous `skyos_uefi.img` or `skyos.vdi` files.
2.  **Builds the kernel:** Compiles the kernel in the `kernel/` directory.
3.  **Runs the image builder:** Executes the `builder` crate, which uses the `bootloader` library to create a UEFI-bootable disk image (`skyos-uefi.img`) in the `target/` directory.
4.  **Copies the image:** Moves `skyos-uefi.img` to the project root.
5.  **Converts to VDI:** Uses `VBoxManage` to convert the raw UEFI image into a VirtualBox Disk Image (`skyos.vdi`).

## 3. Running SkyOS

### 3.1 QEMU (Recommended for Development)

QEMU is faster for quick iteration and debugging.

**Command:**

```bash
qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=skyos_uefi.img -m 512M -smp 2
```

**Notes:**

-   You need a copy of `OVMF.fd` for UEFI support. You can download it from various sources online.
-   `-smp 2` starts the kernel with 2 CPU cores, allowing you to test the SMP scheduler.

### 3.2 VirtualBox

1.  **Create a New VM:**
    -   **Type:** `Other`
    -   **Version:** `Other/Unknown (64-bit)`
2.  **Memory:** Assign at least 512 MB of RAM.
3.  **Hard Disk:**
    -   Select "Use an existing virtual hard disk file".
    -   Choose the `skyos.vdi` file from the project root.
4.  **Enable EFI:**
    -   Go to **Settings > System > Motherboard**.
    -   Check the box for **Enable EFI (special OSes only)**.
5.  **Start the VM.**
