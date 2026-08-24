#!/usr/bin/env python3
"""
Create a Limine-bootable UEFI disk image.

Uses pyfatfs for FAT filesystem creation and limine.exe for BIOS boot.

Usage:
    python make_limine_bootable.py [--output PATH] [--limine-dir DIR]
"""

import os
import sys
import struct
import subprocess
import tempfile
import shutil

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
ROOT_DIR = os.path.dirname(SCRIPT_DIR)

KERNEL_PATH = os.path.join(ROOT_DIR, "kernel", "target", "x86_64-unknown-none", "debug", "vahi_kernel")
INITRD_PATH = os.path.join(ROOT_DIR, "kernel", "initrd.tar")
LIMINE_CONF = os.path.join(ROOT_DIR, "kernel", "limine.conf")
DEFAULT_OUTPUT = os.path.join(ROOT_DIR, "bootimage-vahi_kernel.bin")
LIMINE_DIR = "C:/Windows/TEMP/limine-binary"


def create_disk_image(output_path, limine_dir, size_mb=64):
    """Create a Limine-bootable GPT disk image."""
    
    SECTOR = 512
    total_sectors = size_mb * 1024 * 1024 // SECTOR
    
    # Partition layout
    esp_start = 2048  # 1MB
    esp_sectors = 2048  # 1MB
    esp_end = esp_start + esp_sectors - 1
    
    data_start = esp_end + 1
    data_sectors = total_sectors - data_start - 33  # Leave room for backup GPT
    data_end = data_start + data_sectors - 1
    
    # Create raw disk
    disk = bytearray(total_sectors * SECTOR)
    
    # MBR protective entry
    mbr = bytearray(SECTOR)
    mbr[446] = 0x00; mbr[447] = 0x00; mbr[448] = 0x02; mbr[449] = 0x00
    mbr[450] = 0xEE  # GPT protective
    mbr[451] = 0xFF; mbr[452] = 0xFF; mbr[453] = 0xFF
    struct.pack_into('<I', mbr, 454, 1)
    struct.pack_into('<I', mbr, 458, total_sectors - 1)
    mbr[510] = 0x55; mbr[511] = 0xAA
    disk[0:SECTOR] = mbr
    
    # GPT header at LBA 1
    gpt = bytearray(SECTOR)
    gpt[0:8] = b'EFI PART'
    struct.pack_into('<I', gpt, 8, 0x00010000)
    struct.pack_into('<I', gpt, 12, 92)
    struct.pack_into('<Q', gpt, 24, 1)
    struct.pack_into('<Q', gpt, 32, total_sectors - 1)
    struct.pack_into('<Q', gpt, 40, 2048)
    struct.pack_into('<Q', gpt, 48, total_sectors - 34)
    struct.pack_into('<Q', gpt, 56, 0x123456789ABCDEF0)  # Disk GUID
    struct.pack_into('<Q', gpt, 72, 2)
    struct.pack_into('<I', gpt, 80, 128)
    struct.pack_into('<I', gpt, 84, 128)
    disk[SECTOR:2*SECTOR] = gpt
    
    # Backup GPT
    disk[(total_sectors-1)*SECTOR:total_sectors*SECTOR] = gpt
    
    # Partition entries at LBA 2
    # ESP partition
    esp_type = bytes([0x28,0x73,0x2A,0xC1,0x1F,0xF8,0xD2,0x11,0xBA,0x4B,0x00,0xA0,0xC9,0x3E,0xC9,0x3B])
    esp_guid = bytes([0xAA,0xBB,0xCC,0xDD,0x11,0x22,0x33,0x44,0x55,0x66,0x77,0x88,0x99,0xAA,0xBB,0xCC])
    
    esp_entry = bytearray(128)
    esp_entry[0:16] = esp_type
    esp_entry[16:32] = esp_guid
    struct.pack_into('<Q', esp_entry, 32, esp_start)
    struct.pack_into('<Q', esp_entry, 40, esp_end)
    disk[2*SECTOR:2*SECTOR+128] = esp_entry
    
    # Data partition (FAT16)
    data_type = bytes([0xA2,0xA0,0xD0,0xEB,0xE5,0xB9,0x33,0x44,0x87,0xC0,0x68,0xB6,0xB7,0x26,0x99,0xC7])
    data_guid = bytes([0x11,0x22,0x33,0x44,0x55,0x66,0x77,0x88,0x99,0xAA,0xBB,0xCC,0xDD,0xEE,0xFF,0x00])
    
    data_entry = bytearray(128)
    data_entry[0:16] = data_type
    data_entry[16:32] = data_guid
    struct.pack_into('<Q', data_entry, 32, data_start)
    struct.pack_into('<Q', data_entry, 40, data_end)
    disk[2*SECTOR+128:2*SECTOR+256] = data_entry
    
    # Create ESP (FAT16) with BOOTX64.EFI
    print("  Creating ESP...")
    esp_data = create_fat16_partition(
        esp_sectors * SECTOR,
        {"BOOTX64.EFI": os.path.join(limine_dir, "BOOTX64.EFI")}
    )
    disk[esp_start*SECTOR:esp_start*SECTOR+len(esp_data)] = esp_data
    
    # Create data partition (FAT16) with kernel, initrd, limine.conf
    print("  Creating data partition...")
    data_files = {
        "LIMINE.CONF": LIMINE_CONF,
        "BOOT/VAHI_KERNEL": KERNEL_PATH,
        "BOOT/INITRD.TAR": INITRD_PATH,
        "BOOT/BOOTX64.EFI": os.path.join(limine_dir, "BOOTX64.EFI"),
    }
    data_part = create_fat16_partition(data_sectors * SECTOR, data_files)
    disk[data_start*SECTOR:data_start*SECTOR+len(data_part)] = data_part
    
    # Write image
    with open(output_path, 'wb') as f:
        f.write(disk)
    
    # Install Limine BIOS bootloader
    print("  Installing Limine BIOS...")
    limine_exe = os.path.join(limine_dir, "limine-tool-windows-x86", "limine.exe")
    if os.path.exists(limine_exe):
        result = subprocess.run(
            [limine_exe, "bios-install", output_path, "--force"],
            capture_output=True, text=True, timeout=30
        )
        if result.returncode == 0:
            print("  Limine BIOS installed OK")
        else:
            print(f"  Limine BIOS install warning: {result.stderr.strip()[:100]}")
    
    print(f"  Created: {output_path} ({os.path.getsize(output_path):,} bytes)")


def create_fat16_partition(size_bytes, files):
    """
    Create a FAT16 filesystem partition image.
    
    files: dict of {dest_path: source_path} where dest_path uses / as separator.
    Directories are created automatically.
    """
    SECTOR = 512
    sectors = size_bytes // SECTOR
    spc = 4  # sectors per cluster = 2KB
    
    # Calculate layout dynamically
    max_clusters = sectors // spc  # Maximum clusters for this partition
    fat_entries = min(max_clusters + 2, 65524)  # FAT16 max + 2 reserved
    fat_sectors = (fat_entries * 2 + SECTOR - 1) // SECTOR
    root_sectors = 4  # 512 entries * 32 bytes = 16KB = 32 sectors... no, 512*32/512 = 32 sectors
    # Actually root dir = 512 entries * 32 bytes = 16384 bytes = 32 sectors
    root_sectors = 32
    data_start_sector = 1 + fat_sectors + root_sectors
    
    fs = bytearray(sectors * SECTOR)
    
    # FAT
    fat = bytearray(fat_sectors * SECTOR)
    fat[0:2] = struct.pack('<H', 0xFFF8)  # Media byte
    fat[2:4] = struct.pack('<H', 0xFFFF)  # End of chain
    
    # Boot sector
    bs = bytearray(SECTOR)
    bs[0:3] = b'\xEB\x3C\x90'
    bs[3:11] = b'MSDOS5.0'
    struct.pack_into('<H', bs, 11, SECTOR)
    bs[13] = spc
    struct.pack_into('<H', bs, 14, 1)  # Reserved sectors
    bs[16] = 1  # FATs
    struct.pack_into('<H', bs, 17, 512)  # Root entries
    struct.pack_into('<H', bs, 19, 0 if sectors > 65535 else sectors)  # 16-bit total sectors (0 = use 32-bit)
    bs[21] = 0xF8
    struct.pack_into('<H', bs, 22, fat_sectors)
    struct.pack_into('<H', bs, 24, 63)
    struct.pack_into('<H', bs, 26, 255)
    struct.pack_into('<I', bs, 28, 0)
    struct.pack_into('<I', bs, 32, sectors)
    bs[36] = 0x80
    bs[38] = 0x29
    bs[43:54] = b'Vahi OS    '
    bs[54:62] = b'FAT16   '
    bs[510:512] = b'\x55\xAA'
    fs[0:SECTOR] = bs
    fs[SECTOR:SECTOR+len(fat)] = fat
    
    # Track allocations
    next_cluster = [2]
    dir_entries = {}  # path -> first_cluster
    file_data_map = {}  # path -> (data, first_cluster)
    
    def alloc_cluster():
        c = next_cluster[0]
        next_cluster[0] += 1
        return c
    
    def name_83(name):
        """Convert to 8.3 format."""
        base, ext = os.path.splitext(name)
        return (base.upper()[:8] + '   ')[:8].encode('ascii') + (ext.upper()[1:4] + '  ')[:3].encode('ascii')
    
    def add_root_entry(name_83_bytes, first_cluster, size=0, is_dir=False):
        """Add entry to root directory."""
        entry = bytearray(32)
        entry[0:11] = name_83_bytes
        entry[11] = 0x10 if is_dir else 0x20
        struct.pack_into('<H', entry, 26, first_cluster)
        if not is_dir:
            struct.pack_into('<I', entry, 28, size)
        
        root_off = (1 + fat_sectors) * SECTOR
        for i in range(512):
            if fs[root_off + i*32] == 0:
                fs[root_off + i*32:root_off + (i+1)*32] = entry
                return
    
    def add_subdir_entry(subdir_cluster, parent_cluster, name_83_bytes):
        """Add entry to a subdirectory."""
        off = data_start_sector * SECTOR + (parent_cluster - 2) * spc * SECTOR
        for i in range(16):
            if fs[off + i*32] == 0:
                entry = bytearray(32)
                entry[0:11] = name_83_bytes
                entry[11] = 0x10
                struct.pack_into('<H', entry, 26, subdir_cluster)
                fs[off + i*32:off + (i+1)*32] = entry
                return
    
    def write_file_data(first_cluster, data):
        """Write file data to clusters."""
        num_clusters = (len(data) + spc * SECTOR - 1) // (spc * SECTOR)
        clusters = []
        for _ in range(num_clusters):
            c = alloc_cluster()
            clusters.append(c)
        
        # Chain FAT
        for i in range(len(clusters) - 1):
            off = clusters[i] * 2
            fat[off] = clusters[i+1] & 0xFF
            fat[off+1] = (clusters[i+1] >> 8) & 0xFF
        off = clusters[-1] * 2
        fat[off] = 0xFF
        fat[off+1] = 0xFF
        
        # Write data
        for i, c in enumerate(clusters):
            file_off = data_start_sector * SECTOR + (c - 2) * spc * SECTOR
            chunk = data[i*spc*SECTOR:(i+1)*spc*SECTOR]
            fs[file_off:file_off+len(chunk)] = chunk
        
        return clusters[0]
    
    # Process files
    for dest, src in sorted(files.items()):
        data = open(src, 'rb').read()
        parts = dest.split('/')
        
        if len(parts) == 1:
            # File in root
            first = write_file_data(0, data)
            add_root_entry(name_83(parts[0]), first, len(data))
        elif len(parts) == 2:
            # File in subdirectory
            dirname = parts[0]
            fname = parts[1]
            
            if dirname not in dir_entries:
                # Create directory
                dir_cluster = alloc_cluster()
                fat[dir_cluster*2] = 0xFF
                fat[dir_cluster*2+1] = 0xFF
                
                # . and .. entries
                dot_off = data_start_sector * SECTOR + (dir_cluster - 2) * spc * SECTOR
                dot = bytearray(32)
                dot[0:11] = b'.          '
                dot[11] = 0x10
                struct.pack_into('<H', dot, 26, dir_cluster)
                fs[dot_off:dot_off+32] = dot
                
                dotdot = bytearray(32)
                dotdot[0:11] = b'..         '
                dotdot[11] = 0x10
                fs[dot_off+32:dot_off+64] = dotdot
                
                add_root_entry(name_83(dirname), dir_cluster, is_dir=True)
                dir_entries[dirname] = dir_cluster
            
            first = write_file_data(0, data)
            add_subdir_entry(dir_entries[dirname], dir_entries[dirname], name_83(fname))
            
            # Also add to root for /BOOT/VAHI_KERNEL etc.
            # Actually, we need the parent to know about this file
            # The subdirectory entry is already added above
    
    # Write FAT
    fs[SECTOR:SECTOR+len(fat)] = fat
    
    return fs


def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--limine-dir", default=LIMINE_DIR)
    parser.add_argument("--output", default=DEFAULT_OUTPUT)
    parser.add_argument("--size-mb", type=int, default=64)
    args = parser.parse_args()
    
    print("=== Limine Boot Image Builder ===")
    
    for path, desc in [(KERNEL_PATH, "Kernel"), (INITRD_PATH, "initrd"), (LIMINE_CONF, "limine.conf")]:
        if not os.path.exists(path):
            print(f"ERROR: {desc} not found: {path}")
            sys.exit(1)
    
    if not os.path.exists(os.path.join(args.limine_dir, "BOOTX64.EFI")):
        print(f"ERROR: BOOTX64.EFI not found in {args.limine_dir}")
        sys.exit(1)
    
    create_disk_image(args.output, args.limine_dir, args.size_mb)
    
    print(f"\n  Boot with:")
    print(f"    qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file={args.output} -m 512M -smp 2 -nographic")


if __name__ == "__main__":
    main()
