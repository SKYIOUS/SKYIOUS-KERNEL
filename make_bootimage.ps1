# PowerShell script to create bootimage-vahi_kernel.bin
$ErrorActionPreference = "Stop"

$rootDir = $PSScriptRoot
Set-Location $rootDir

if (!(Test-Path "kernel")) {
    Write-Host "ERROR: Could not find 'kernel' directory at $rootDir" -ForegroundColor Red
    exit 1
}

Write-Host "--- SARGA OS Bootimage Builder ---" -ForegroundColor Cyan

# 0. Build userspace first (init, sargash, etc.)
Write-Host "Step 0: Building userspace..." -ForegroundColor Gray
& "$rootDir\build_userspace.ps1"
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Userspace build failed!" -ForegroundColor Red
    exit 1
}

# 1. Build the kernel
Write-Host "Step 1: Building kernel..." -ForegroundColor Gray
cd kernel
cargo build --target x86_64-unknown-none
cd ..

# 2. Run the image builder
Write-Host "Step 2: Running image builder..." -ForegroundColor Gray
cargo run --manifest-path builder/Cargo.toml

# 3. Check output
$outputBinary = "target/x86_64-vahi/debug/bootimage-vahi_kernel.bin"
if (Test-Path $outputBinary) {
    Write-Host "SUCCESS: Created $outputBinary" -ForegroundColor Green
    
    Copy-Item $outputBinary "$rootDir/bootimage-vahi_kernel.bin" -Force
    Write-Host "Copied to: $rootDir/bootimage-vahi_kernel.bin" -ForegroundColor Cyan
} else {
    Write-Host "ERROR: Could not find output at $outputBinary" -ForegroundColor Red
}
