# PowerShell script to create bootimage-vahi_kernel.bin
# Usage:
#   .\make_bootimage.ps1                # x86_64 (default)
#   $env:VAHI_ARCH='aarch64'; .\make_bootimage.ps1  # aarch64

$ErrorActionPreference = "Stop"
$rootDir = $PSScriptRoot
Set-Location $rootDir

if (!(Test-Path "kernel")) {
    Write-Host "ERROR: Could not find 'kernel' directory at $rootDir" -ForegroundColor Red
    exit 1
}

$arch = if ($env:VAHI_ARCH) { $env:VAHI_ARCH } else { "x86_64" }
$target = if ($arch -eq "aarch64") { "aarch64-unknown-none" } else { "x86_64-unknown-none" }

Write-Host "--- SARGA OS Bootimage Builder (arch=$arch) ---" -ForegroundColor Cyan

# 0. Build userspace first (init, sargash, etc.)
Write-Host "Step 0: Building userspace..." -ForegroundColor Gray
& "$rootDir\build_userspace.ps1"
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Userspace build failed!" -ForegroundColor Red
    exit 1
}

# 1. Build the kernel
Write-Host "Step 1: Building kernel (target=$target)..." -ForegroundColor Gray
Set-Location kernel
if ($arch -eq "aarch64") {
    cargo build --target aarch64-unknown-none
} else {
    cargo build --target x86_64-unknown-none
}
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Kernel build failed!" -ForegroundColor Red
    exit 1
}
Set-Location $rootDir

# 2. Run the image builder
Write-Host "Step 2: Running image builder (arch=$arch)..." -ForegroundColor Gray
$env:VAHI_ARCH = $arch
cargo run --manifest-path builder/Cargo.toml

# 3. Check output
$outDirName = if ($arch -eq "aarch64") { "aarch64-vahi" } else { "x86_64-vahi" }
$outputBinary = "target/${outDirName}/debug/bootimage-vahi_kernel.bin"
if (Test-Path $outputBinary) {
    Write-Host "SUCCESS: Created $outputBinary" -ForegroundColor Green
    Copy-Item $outputBinary "$rootDir/bootimage-vahi_kernel.bin" -Force
    Write-Host "Copied to: $rootDir/bootimage-vahi_kernel.bin" -ForegroundColor Cyan
} else {
    Write-Host "ERROR: Could not find output at $outputBinary" -ForegroundColor Red
}
