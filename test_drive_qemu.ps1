# Build kernel, create test disk, boot in QEMU with IDE drive.
Write-Host "[*] Building kernel..."
python build_disk.py

if ($LASTEXITCODE -ne 0) {
    Write-Host "[!] Build failed. Aborting."
    exit 1
}

Write-Host "[*] Creating test disk image (32MB)..."
& .\make_disk_image.ps1 -Size "32M" -OutFile "test_disk.img"

Write-Host "[*] Booting QEMU with IDE disk..."
qemu-system-x86_64 `
    -bios OVMF.fd `
    -drive format=raw,file=skyos_uefi.img,if=ide,index=0 `
    -drive format=raw,file=test_disk.img,if=ide,index=1 `
    -m 512M -smp 2 `
    -serial stdio
