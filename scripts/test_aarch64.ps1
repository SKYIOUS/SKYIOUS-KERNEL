# QEMU aarch64 boot test
# Prerequisites:
#   cargo build --target aarch64-unknown-none (in kernel/)
#   builder runs with VAHI_ARCH=aarch64
#
# Usage:
#   .\scripts\test_aarch64.ps1

$ErrorActionPreference = "Stop"
$rootDir = Split-Path -Parent $PSScriptRoot

Write-Host "=== Vahi Kernel aarch64 QEMU Test ===" -ForegroundColor Cyan

# 1. Locate bootimage
$bootimage = Join-Path $rootDir "target/aarch64-vahi/debug/bootimage-vahi_kernel.bin"
if (!(Test-Path $bootimage)) {
    Write-Host "Bootimage not found at $bootimage" -ForegroundColor Yellow
    Write-Host "Build the kernel first:" -ForegroundColor Gray
    Write-Host "  cd kernel; cargo build --target aarch64-unknown-none" -ForegroundColor Gray
    Write-Host "  cd .. ; cargo run --manifest-path builder/Cargo.toml" -ForegroundColor Gray
    Write-Host "    (with `$env:VAHI_ARCH='aarch64')" -ForegroundColor Gray
    exit 1
}

Write-Host "Bootimage: $bootimage" -ForegroundColor Green

# 2. Run in QEMU aarch64
Write-Host "Starting QEMU (aarch64, virt, cortex-a72)..." -ForegroundColor Cyan
Write-Host "  Serial output below (Ctrl+C to exit):" -ForegroundColor Gray
Write-Host ""

qemu-system-aarch64 `
    -machine virt `
    -cpu cortex-a72 `
    -bios QEMU_EFI.fd `
    -drive format=raw,file=$bootimage `
    -m 512M `
    -smp 2 `
    -serial stdio `
    -nographic

Write-Host ""
Write-Host "QEMU exited." -ForegroundColor Cyan
