$rootDir = $PSScriptRoot
$bios = Join-Path $rootDir "OVMF.fd"
$img = Join-Path $rootDir "bootimage-vahi_kernel.bin"

if (!(Test-Path $img)) {
    Write-Host "Boot image not found at $img" -ForegroundColor Red
    Write-Host "Run .\make_bootimage.ps1 first" -ForegroundColor Yellow
    exit 1
}

Write-Host "--- Booting SKYIOUS Kernel (nographic mode) ---" -ForegroundColor Cyan
qemu-system-x86_64 -bios $bios -drive format=raw,file=$img -m 512M -smp 2 -nographic
