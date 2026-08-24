#!/usr/bin/env python3
"""
Create a Limine-bootable UEFI disk image for the Vahi kernel.

Usage:
    python make_limine_image.py [--limine-dir PATH] [--output PATH]

Requires:
    - limine binaries (BOOTX64.EFI, BOOTIA32.EFI, BIOSSYS, etc.)
    - Kernel ELF at kernel/target/x86_64-unknown-none/debug/vahi_kernel
    - initrd at kernel/initrd.tar
    - limine.conf at kernel/limine.conf

The script creates a FAT16 disk image with:
    /boot/BOOTX64.EFI    (Limine UEFI bootloader)
    /boot/vahi_kernel    (kernel ELF)
    /boot/initrd.tar     (initial ramdisk)
    /limine.conf         (boot configuration)
"""

import struct
import os
import sys
import shutil
import subprocess
import tempfile
import math

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
ROOT_DIR = os.path.dirname(SCRIPT_DIR)

# Default paths
KERNEL_PATH = os.path.join(ROOT_DIR, "kernel", "target", "x86_64-unknown-none", "debug", "vahi_kernel")
INITRD_PATH = os.path.join(ROOT_DIR, "kernel", "initrd.tar")
LIMINE_CONF = os.path.join(ROOT_DIR, "kernel", "limine.conf")
DEFAULT_OUTPUT = os.path.join(ROOT_DIR, "bootimage-vahi_kernel.bin")

# Disk geometry
SECTOR_SIZE = 512
CLUSTER_SIZE = 4096  # 8 sectors per cluster
RESERVED_SECTORS = 2048  # Space for Limine stage1/stage2
NUM_FATS = 1
ROOT_DIR_ENTRIES = 512
FAT_TYPE = 16  # FAT16


def create_fat16_image(output_path, files_dict, total_size_mb=64):
    """
    Create a FAT16 disk image with the given files.
    
    files_dict: {destination_path: source_path} e.g. {"/boot/vahi_kernel": "/path/to/vahi_kernel"}
    """
    total_size = total_size_mb * 1024 * 1024
    total_sectors = total_size // SECTOR_SIZE
    sectors_per_cluster = CLUSTER_SIZE // SECTOR_SIZE
    total_clusters = (total_size - RESERVED_SECTORS * SECTOR_SIZE) // CLUSTER_SIZE
    
    # FAT16 can handle up to 65525 clusters
    if total_clusters > 65525:
        total_clusters = 65525
        total_size = RESERVED_SECTORS * SECTOR_SIZE + total_clusters * CLUSTER_SIZE
        total_sectors = total_size // SECTOR_SIZE
    
    # Calculate FAT size
    fat_entries = total_clusters + 2  # +2 for reserved entries
    fat_size_bytes = fat_entries * 2  # 2 bytes per entry for FAT16
    fat_size_sectors = math.ceil(fat_size_bytes / SECTOR_SIZE)
    
    # Root directory size
    root_dir_size = ROOT_DIR_ENTRIES * 32  # 32 bytes per entry
    root_dir_sectors = math.ceil(root_dir_size / SECTOR_SIZE)
    
    print(f"  Disk: {total_size_mb}MB, {total_sectors} sectors, {total_clusters} clusters")
    print(f"  FAT: {fat_size_sectors} sectors, {fat_entries} entries")
    
    # Initialize disk image
    image = bytearray(total_size)
    
    # --- Boot Sector (VBR) ---
    boot = bytearray(SECTOR_SIZE)
    boot[0:3] = b'\xEB\x58\x90'  # JMP SHORT + NOP
    boot[3:11] = b'MSWIN4.1'  # OEM name
    struct.pack_into('<H', boot, 11, SECTOR_SIZE)  # Bytes per sector
    boot[13] = sectors_per_cluster  # Sectors per cluster
    struct.pack_into('<H', boot, 14, RESERVED_SECTORS)  # Reserved sectors
    boot[16] = NUM_FATS  # Number of FATs
    struct.pack_into('<H', boot, 17, ROOT_DIR_ENTRIES)  # Root dir entries
    struct.pack_into('<H', boot, 19, total_sectors)  # Total sectors (16-bit)
    boot[21] = 0xF8  # Media type (hard disk)
    struct.pack_into('<H', boot, 22, fat_size_sectors)  # FAT size (sectors)
    struct.pack_into('<H', boot, 24, 63)  # Sectors per track
    struct.pack_into('<H', boot, 26, 255)  # Number of heads
    struct.pack_into('<I', boot, 28, 0)  # Hidden sectors
    struct.pack_into('<I', boot, 32, total_sectors if total_sectors < 0x10000 else 0)  # Total sectors (32-bit)
    boot[36] = 0x80  # Drive number
    boot[38] = 0x29  # Extended boot signature
    boot[39:43] = b'\x01\x02\x03\x04'  # Volume serial number
    boot[43:54] = b'Vahi OS    '  # Volume label
    boot[54:62] = b'FAT16   '  # Filesystem type
    
    image[0:SECTOR_SIZE] = boot
    
    # --- FAT Table ---
    fat_start = RESERVED_SECTORS
    fat = bytearray(fat_size_sectors * SECTOR_SIZE)
    fat[0:2] = struct.pack('<H', 0xFFF8)  # FAT16 media byte
    fat[2:4] = struct.pack('<H', 0xFFFF)  # End of chain
    
    # Allocate clusters for files
    file_clusters = {}
    next_cluster = 2  # First available cluster
    
    for dest_path, source_path in sorted(files_dict.items()):
        file_size = os.path.getsize(source_path)
        num_clusters = math.ceil(file_size / CLUSTER_SIZE)
        clusters = []
        
        for i in range(num_clusters):
            clusters.append(next_cluster)
            if i < num_clusters - 1:
                fat_offset = next_cluster * 2
                struct.pack_into('<H', fat, fat_offset, next_cluster + 1)
            next_cluster += 1
        
        # Mark last cluster
        fat_offset = clusters[-1] * 2
        struct.pack_into('<H', fat, fat_offset, 0xFFFF)
        
        file_clusters[dest_path] = {
            'size': file_size,
            'clusters': clusters,
            'source': source_path,
        }
    
    print(f"  Allocated {next_cluster - 2} clusters for {len(files_dict)} files")
    
    image[fat_start * SECTOR_SIZE:fat_start * SECTOR_SIZE + len(fat)] = fat
    
    # --- Root Directory ---
    root_start = fat_start + fat_size_sectors
    root = bytearray(root_dir_sectors * SECTOR_SIZE)
    
    # Create root directory entries for /boot/ directory and limine.conf
    entry_offset = 0
    
    # Entry for /boot/ directory
    dir_entry = bytearray(32)
    dir_entry[0:11] = b'BOOT       '  # Short name (8.3 format)
    dir_entry[11] = 0x10  # Directory attribute
    struct.pack_into('<H', dir_entry, 26, 2)  # First cluster (2 = first data cluster)
    root[entry_offset:entry_offset + 32] = dir_entry
    entry_offset += 32
    
    # Entry for limine.conf in root
    conf_entry = bytearray(32)
    name = 'LIMINE  CONF'
    conf_entry[0:11] = name.encode('ascii')
    conf_entry[11] = 0x20  # Archive attribute
    conf_entry[28:32] = struct.pack('<I', file_clusters['/limine.conf']['size'])
    struct.pack_into('<H', conf_entry, 26, file_clusters['/limine.conf']['clusters'][0])
    root[entry_offset:entry_offset + 32] = conf_entry
    entry_offset += 32
    
    # Entries for /boot/ subdirectory
    boot_entries = {}
    for dest_path in sorted(file_clusters.keys()):
        if dest_path.startswith('/boot/'):
            fname = dest_path[6:]  # Remove /boot/ prefix
            # Convert to 8.3 format
            name_83 = fname.upper().replace('.', ' ').ljust(11)[:11]
            
            dir_entry = bytearray(32)
            dir_entry[0:11] = name_83.encode('ascii')
            dir_entry[11] = 0x20  # Archive attribute
            dir_entry[28:32] = struct.pack('<I', file_clusters[dest_path]['size'])
            struct.pack_into('<H', dir_entry, 26, file_clusters[dest_path]['clusters'][0])
            root[entry_offset:entry_offset + 32] = dir_entry
            entry_offset += 32
            boot_entries[dest_path] = dir_entry
    
    image[root_start * SECTOR_SIZE:root_start * SECTOR_SIZE + len(root)] = root
    
    # --- Write File Data ---
    data_start = root_start + root_dir_sectors
    
    for dest_path, info in file_clusters.items():
        with open(info['source'], 'rb') as f:
            file_data = f.read()
        
        for i, cluster in enumerate(info['clusters']):
            offset = data_start * SECTOR_SIZE + (cluster - 2) * CLUSTER_SIZE
            chunk_start = i * CLUSTER_SIZE
            chunk_end = min(chunk_start + CLUSTER_SIZE, len(file_data))
            image[offset:offset + (chunk_end - chunk_start)] = file_data[chunk_start:chunk_end]
    
    # --- Write Image ---
    with open(output_path, 'wb') as f:
        f.write(image)
    
    print(f"  Created: {output_path} ({len(image)} bytes)")
    return output_path


def download_limine(target_dir):
    """Download Limine pre-built binaries."""
    # Try v8.2.2 first, fall back to v7
    urls = [
        ("https://github.com/limine-bootloader/limine/releases/download/v8.2.2/limine-8.2.2.zip", "limine-8.2.2.zip"),
        ("https://github.com/limine-bootloader/limine/releases/download/v7.2.2/limine-7.2.2.zip", "limine-7.2.2.zip"),
    ]
    
    for url, filename in urls:
        zip_path = os.path.join(target_dir, filename)
        print(f"  Downloading {url}...")
        result = subprocess.run(
            ["curl", "-sL", "-o", zip_path, url],
            capture_output=True, text=True
        )
        if result.returncode == 0 and os.path.getsize(zip_path) > 1000:
            # Extract
            if filename.endswith('.zip'):
                subprocess.run(["unzip", "-o", zip_path, "-d", target_dir], capture_output=True)
            elif filename.endswith('.tar.gz'):
                subprocess.run(["tar", "-xzf", zip_path, "-C", target_dir], capture_output=True)
            
            # Find the extracted directory
            for item in os.listdir(target_dir):
                if item.startswith('limine-') and os.path.isdir(os.path.join(target_dir, item)):
                    return os.path.join(target_dir, item)
            
            return target_dir
    
    return None


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Create Limine-bootable disk image")
    parser.add_argument("--limine-dir", help="Path to Limine binaries directory")
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="Output disk image path")
    parser.add_argument("--size", type=int, default=64, help="Disk size in MB")
    args = parser.parse_args()
    
    # Verify inputs exist
    for path, desc in [(KERNEL_PATH, "Kernel ELF"), (INITRD_PATH, "initrd"), (LIMINE_CONF, "limine.conf")]:
        if not os.path.exists(path):
            print(f"ERROR: {desc} not found at {path}")
            sys.exit(1)
    
    print("=== Limine Boot Image Builder ===")
    
    # Get Limine binaries
    limine_dir = args.limine_dir
    if not limine_dir:
        # Check common locations
        for candidate in [
            os.path.join(ROOT_DIR, "limine"),
            os.path.join(ROOT_DIR, "builder", "limine"),
            os.path.join(tempfile.gettempdir(), "limine"),
        ]:
            if os.path.isdir(candidate) and any(f.startswith('BOOT') for f in os.listdir(candidate)):
                limine_dir = candidate
                break
        
        if not limine_dir:
            print("  Limine binaries not found. Downloading...")
            limine_dir = download_limine(tempfile.gettempdir())
            if not limine_dir:
                print("ERROR: Could not obtain Limine binaries")
                print("  Download manually from https://github.com/limine-bootloader/limine/releases")
                print("  and pass --limine-dir PATH")
                sys.exit(1)
    
    print(f"  Limine dir: {limine_dir}")
    
    # Find Limine UEFI binary
    uefi_path = None
    for candidate in [
        os.path.join(limine_dir, "BOOTX64.EFI"),
        os.path.join(limine_dir, "uefi", "BOOTX64.EFI"),
        os.path.join(limine_dir, "boot", "BOOTX64.EFI"),
    ]:
        if os.path.exists(candidate):
            uefi_path = candidate
            break
    
    if not uefi_path:
        # List what's in the directory for debugging
        print(f"  Contents of {limine_dir}:")
        for f in sorted(os.listdir(limine_dir)):
            print(f"    {f}")
        print("ERROR: BOOTX64.EFI not found in Limine directory")
        sys.exit(1)
    
    print(f"  UEFI bootloader: {uefi_path}")
    
    # Create file mapping
    files = {
        "/limine.conf": LIMINE_CONF,
        "/boot/vahi_kernel": KERNEL_PATH,
        "/boot/initrd.tar": INITRD_PATH,
        "/boot/BOOTX64.EFI": uefi_path,
    }
    
    # Add BIOS boot files if available
    bios_path = None
    for candidate in [
        os.path.join(limine_dir, "BIOS", "boot.bin"),
        os.path.join(limine_dir, "bios", "boot.bin"),
        os.path.join(limine_dir, "boot", "boot.bin"),
    ]:
        if os.path.exists(candidate):
            bios_path = candidate
            break
    
    if bios_path:
        files["/boot/boot.bin"] = bios_path
    
    print(f"\n  Files to include:")
    for dest, src in sorted(files.items()):
        size = os.path.getsize(src)
        print(f"    {dest} ({size:,} bytes)")
    
    # Create disk image
    print(f"\n  Creating {args.size}MB FAT16 disk image...")
    create_fat16_image(args.output, files, args.size)
    
    print(f"\n  Done! Boot with:")
    print(f"    qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file={args.output} -m 512M -smp 2 -nographic")
    print()


if __name__ == "__main__":
    main()
