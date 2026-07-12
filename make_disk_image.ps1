param(
    [string]$Size = "32M",
    [string]$OutFile = "test_disk.img"
)

# Create an empty disk image for QEMU to test PATA/IDE or AHCI block drivers.
# qemu-system-x86_64 will expose this as a raw disk (usually /dev/sda or similar).
# The kernel's PCI enumeration + storage driver probe should detect it.

Write-Host "[*] Creating $Size disk image: $OutFile"
# Create a sparse (all-zero) raw image using qemu-img if available, else fallback
if (Get-Command "qemu-img" -ErrorAction SilentlyContinue) {
    & qemu-img create -f raw "$OutFile" "$Size"
} else {
    # Fallback: create a file filled with zeros
    $bytes = switch -Regex ($Size) {
        '^(\d+)M$' { [int]$Matches[1] * 1MB }
        '^(\d+)G$' { [int]$Matches[1] * 1GB }
        default { 32MB }
    }
    $stream = [System.IO.File]::OpenWrite($OutFile)
    $stream.SetLength($bytes)
    $stream.Close()
    Write-Host "[*] Created $OutFile ($bytes bytes)"
}

# Write a simple MBR partition table with one partition
Write-Host "[*] Writing MBR partition table..."
$mbr = [byte[]]::new(512)

# Boot signature
$mbr[510] = 0x55
$mbr[511] = 0xAA

# Partition entry at offset 446
# Partition type 0x0C = FAT32 LBA
$mbr[446]   = 0x00    # status (0 = non-bootable)
$mbr[447]   = 0x00    # CHS start head
$mbr[448]   = 0x01    # CHS start sector
$mbr[449]   = 0x00    # CHS start cylinder
$mbr[450]   = 0x0C    # partition type (FAT32 LBA)
$mbr[451]   = 0x00    # CHS end head (fake)
$mbr[452]   = 0x02    # CHS end sector (fake)
$mbr[453]   = 0x00    # CHS end cylinder (fake)
# LBA start = 1 (sector 0 = MBR)
$mbr[454]   = 0x01; $mbr[455] = 0x00; $mbr[456] = 0x00; $mbr[457] = 0x00
# LBA size = total sectors - 1
$totalSec = [int]($bytes / 512) - 1
$mbr[458]   = $totalSec -band 0xFF
$mbr[459]   = ($totalSec -shr 8) -band 0xFF
$mbr[460]   = ($totalSec -shr 16) -band 0xFF
$mbr[461]   = ($totalSec -shr 24) -band 0xFF

$stream = [System.IO.File]::OpenWrite($OutFile)
$stream.Write($mbr, 0, 512)
$stream.Close()

Write-Host "[*] Done! Use with QEMU:"
Write-Host "    qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=$OutFile -m 512M"
