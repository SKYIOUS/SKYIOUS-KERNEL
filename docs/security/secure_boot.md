# Secure Boot

SkyOS supports secure boot to ensure that only trusted code executes during the boot process. This document describes the implementation and usage of secure boot features.

## UEFI Secure Boot

SkyOS supports UEFI Secure Boot on x86-64 systems. The kernel's EFI boot stub is signed with a key enrolled in the system's UEFI firmware. During boot, the firmware verifies the boot stub's signature against the enrolled keys before allowing execution. If verification fails, the system halts with a firmware error. This prevents unauthorized or tampered kernel images from being loaded.

## Kernel Image Signing

The SkyOS build system produces a signed EFI executable as part of the release process. Signing is performed using a project-specific key stored in a hardware security module during CI builds. For self-built images, developers may enroll their own keys in the firmware or use Machine Owner Key (MOK) management with Shim. The build script `scripts/sign-efi.py` handles the signing process and accepts a path to a PEM-encoded certificate and private key.

## Measured Boot

In addition to signature verification, SkyOS supports measured boot using the TPM 2.0 (Trusted Platform Module). During boot, the kernel measures each stage of the boot process into TPM PCRs (Platform Configuration Registers). These measurements can be used for remote attestation and to seal secrets to specific boot configurations. The TPM driver measures the boot stub, kernel image, kernel command line, and initramfs before handing control to the kernel proper.

## Secure Boot Chain

The full secure boot chain consists of:

1. UEFI firmware verifies the boot stub signature.
2. The boot stub measures itself and the kernel image into TPM PCRs.
3. The kernel verifies the initramfs signature before unpacking it.
4. Kernel modules loaded at runtime must be signed; unsigned modules are rejected.
5. Userspace integrity monitoring (planned) will extend the chain to runtime process verification.

## Enabling Secure Boot

To enable secure boot, ensure UEFI Secure Boot is turned on in the firmware settings. Install the SkyOS certificate using MOK management (`mokutil --import skyos.der`) and reboot. The boot stub will be verified automatically. For measured boot, a TPM 2.0 device must be present and enabled in the firmware. Use `dmesg | grep tpm` to verify TPM detection during boot.
