# run_qemu.ps1 — Boot the Vahi kernel in QEMU.
#
# Usage:
#   .\scripts\run_qemu.ps1                  # UEFI boot with OVMF
#   .\scripts\run_qemu.ps1 -Bios            # Legacy BIOS boot
#   .\scripts\run_qemu.ps1 -Display         # Show display (VGA)
#   .\scripts\run_qemu.ps1 -Debug           # Enable GDB stub
#   .\scripts\run_qemu.ps1 -Custom "C:\path\to\image.bin"
param(
    [switch]$Bios,
    [switch]$Display,
    [switch]$Debug,
    [string]$Custom
)

$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot
$Image = if ($Custom) { $Custom } else { Join-Path $RootDir "bootimage-vahi_kernel.bin" }

if (-not (Test-Path $Image)) {
    Write-Host "ERROR: Disk image not found at $Image" -ForegroundColor Red
    Write-Host "Run .\scripts\build.ps1 first."
    exit 1
}

$QemuArgs = @(
    "-m", "512M"
    "-serial", "stdio"
    "-drive", "file=$Image,format=raw"
)

if (-not $Bios) {
    $Ovmf = Join-Path $RootDir "OVMF.fd"
    if (Test-Path $Ovmf) {
        $QemuArgs += @("-drive", "if=pflash,format=raw,readonly=on,file=$Ovmf")
        Write-Host "Booting in UEFI mode..." -ForegroundColor Cyan
    } else {
        Write-Host "WARNING: OVMF.fd not found, falling back to BIOS mode." -ForegroundColor Yellow
        $Bios = $true
    }
} else {
    Write-Host "Booting in BIOS mode..." -ForegroundColor Cyan
}

if (-not $Display) {
    $QemuArgs += @("-display", "none")
}

if ($Debug) {
    $QemuArgs += @("-s", "-S")
    Write-Host "GDB stub on localhost:1234" -ForegroundColor Yellow
}

Write-Host "qemu-system-x86_64 $($QemuArgs -join ' ')" -ForegroundColor DarkGray
& qemu-system-x86_64 @QemuArgs
