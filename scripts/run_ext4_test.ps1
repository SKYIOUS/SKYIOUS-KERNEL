#!/usr/bin/env pwsh
# ext4 acceptance test: build kernel with ext4, create ext4 disk image,
# boot QEMU, verify ext4 root mount prints to serial.
# Requires: WSL with mkfs.ext4, qemu-system-x86_64, OVMF.fd

$ErrorActionPreference = "Stop"
$rootDir = Split-Path -Parent $PSScriptRoot
Set-Location $rootDir

# 1. Build kernel with ext4 enabled (default)
Write-Host "=== Step 1: Build kernel ===" -ForegroundColor Cyan
Set-Location kernel
cargo build --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { throw "Kernel build failed" }
Set-Location $rootDir

# 2. Create UEFI bootimage
Write-Host "=== Step 2: Build bootimage ===" -ForegroundColor Cyan
cargo run --manifest-path builder/Cargo.toml
if ($LASTEXITCODE -ne 0) { throw "Bootimage build failed" }

# 3. Create ext4 disk image via WSL
Write-Host "=== Step 3: Create ext4 test disk ===" -ForegroundColor Cyan
$imgPath = "$rootDir\ext4_test.img"
if (Test-Path $imgPath) { Remove-Item $imgPath }

# Run the ext4 image creation in WSL
wsl bash -c "cd /mnt/c/Users/nanda/Desktop/Github/SKYIOUS\ KERNEL && bash scripts/mk_ext4_test_img.sh /tmp/ext4_test.img 32" 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host "WARN: WSL ext4 image creation failed, trying native fallback..." -ForegroundColor Yellow
    # Fallback: create a raw disk image with dd (zeros) so QEMU still boots
    $stream = [System.IO.File]::Create($imgPath)
    $stream.SetLength(32MB)
    $stream.Close()
    Write-Host "Created empty 32MB disk image (no ext4 fs)" -ForegroundColor Yellow
} else {
    # Copy from WSL tmp
    wsl cp /tmp/ext4_test.img "/mnt/c/Users/nanda/Desktop/Github/SKYIOUS KERNEL/ext4_test.img" 2>&1
}

# 4. Boot QEMU with ext4 disk
Write-Host "=== Step 4: Boot QEMU ===" -ForegroundColor Cyan
$biosPath = "$rootDir\OVMF.fd"
$bootImg = "$rootDir\vahi_uefi.img"
$qemuLog = "$rootDir\qemu_ext4_test.log"

# Verify files exist
if (!(Test-Path $biosPath)) { throw "Missing OVMF.fd" }
if (!(Test-Path $bootImg)) { throw "Missing bootimage" }

Write-Host "Booting QEMU with ext4 test disk (serial log: $qemuLog)" -ForegroundColor Gray

$qemuArgs = @(
    "-bios", $biosPath
    "-drive", "format=raw,file=$bootImg"
    "-drive", "format=raw,file=$imgPath"
    "-m", "512M"
    "-smp", "2"
    "-serial", "file:$qemuLog"
    "-display", "none"
    "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04"
    "-no-reboot"
    "-cpu", "max"
)

$proc = Start-Process -FilePath "qemu-system-x86_64" -ArgumentList $qemuArgs -NoNewWindow -Wait -PassThru

# 5. Check serial output for ext4 mount message
Write-Host "=== Step 5: Verify ext4 mount ===" -ForegroundColor Cyan
if (Test-Path $qemuLog) {
    $log = Get-Content $qemuLog -Raw
    if ($log -match "Mounted Ext4") {
        Write-Host "PASS: ext4 mount confirmed in serial output" -ForegroundColor Green
    } else {
        Write-Host "FAIL: ext4 mount not found in serial output" -ForegroundColor Red
        Write-Host "--- Last 50 lines of serial log ---" -ForegroundColor Gray
        Get-Content $qemuLog -Tail 50
    }
    if ($log -match "VFS: Root filesystem from") {
        Write-Host "PASS: root filesystem mounted" -ForegroundColor Green
    }
} else {
    Write-Host "FAIL: serial log not found" -ForegroundColor Red
}

Write-Host "=== Done ===" -ForegroundColor Cyan
