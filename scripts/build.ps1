# build.ps1 — Build the Vahi kernel and create a bootable disk image.
#
# Usage:
#   .\scripts\build.ps1              # Release build + disk image
#   .\scripts\build.ps1 -Debug       # Debug build + disk image
#   .\scripts\build.ps1 -KernelOnly  # Build kernel only
#   .\scripts\build.ps1 -ImageOnly   # Create disk image only
param(
    [switch]$Debug,
    [switch]$KernelOnly,
    [switch]$ImageOnly
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$KernelDir = Join-Path $RootDir "kernel"
$BuilderDir = Join-Path $RootDir "builder"
$Profile = if ($Debug) { "debug" } else { "release" }
$Output = Join-Path $RootDir "bootimage-vahi_kernel.bin"

# ── Step 1: Build kernel ──────────────────────────────────────────────
if (-not $ImageOnly) {
    Write-Host "=== Building kernel ($Profile) ===" -ForegroundColor Cyan
    Push-Location $KernelDir
    try {
        if ($Debug) {
            cargo build --target x86_64-unknown-none
        } else {
            cargo build --release --target x86_64-unknown-none
        }
        if ($LASTEXITCODE -ne 0) { throw "Kernel build failed" }
    } finally { Pop-Location }
    Write-Host "Kernel built: kernel/target/x86_64-unknown-none/$Profile/vahi_kernel"
}

# ── Step 2: Create bootable disk image ────────────────────────────────
if (-not $KernelOnly) {
    Write-Host ""
    Write-Host "=== Creating bootable disk image ===" -ForegroundColor Cyan
    $KernelBin = Join-Path $KernelDir "target\x86_64-unknown-none\$Profile\vahi_kernel"
    python "$BuilderDir\build_limine_image.py" --kernel $KernelBin --output $Output
    if ($LASTEXITCODE -ne 0) { throw "Image creation failed" }
}

Write-Host ""
Write-Host "=== Build complete ===" -ForegroundColor Green
Write-Host "Image: $Output"
Write-Host ""
Write-Host "Run with:"
Write-Host "  .\scripts\run_qemu.ps1"
