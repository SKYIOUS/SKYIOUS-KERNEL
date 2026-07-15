$rootDir = $PSScriptRoot
$bios = Join-Path $rootDir "OVMF.fd"
$img = Join-Path $rootDir "bootimage-vahi_kernel.bin"

if (!(Test-Path $img)) {
    Write-Host "Boot image not found. Building first..." -ForegroundColor Yellow
    & "$rootDir\make_bootimage.ps1"
}

Write-Host "--- Booting SKYIOUS Kernel in QEMU ---" -ForegroundColor Cyan
qemu-system-x86_64 -bios $bios -drive format=raw,file=$img -m 512M -smp 2 -cpu max -serial stdio
