#!/usr/bin/env python3
"""
Build a Limine-bootable ISO image for QEMU testing.

Uses El Torito + ISO9660, which UEFI and BIOS both support natively.
No FAT32 or GPT needed.
"""
import struct, os, sys, hashlib, argparse, time

SECTOR_SIZE = 2048  # ISO9660 sector size

def create_iso(files: dict, output_path: str, limine_bios_cd: str, limine_uefi_cd: str):
    """
    Create an ISO9660 image with El Torito boot records for both BIOS and UEFI.
    
    files: {iso_path: bytes_data}
    """
    # Calculate total size needed
    # Layout: system area (32KB) + boot catalog + file data + directory records
    # We'll build in two passes: first determine sizes, then write
    
    sector_num = 16  # Start after system area (16 * 2048 = 32KB)
    
    # El Torito boot catalog (1 sector)
    boot_catalog_lba = sector_num
    sector_num += 1
    
    # Boot image (BIOS) - padded to sector boundary
    bios_img_size = os.path.getsize(limine_bios_cd) if os.path.exists(limine_bios_cd) else 0
    bios_img_sectors = (bios_img_size + SECTOR_SIZE - 1) // SECTOR_SIZE if bios_img_size > 0 else 0
    bios_img_lba = sector_num
    sector_num += bios_img_sectors
    
    # UEFI boot image (the limine-uefi-cd.bin is the MBR post that goes after the BIOS image)
    # Actually for Limine, the UEFI boot image is just the El Torito entry pointing to EFI/BOOT/BOOTX64.EFI
    
    # Collect all files
    all_files = dict(files)
    
    # Create directory records
    # ISO9660 uses system area + volume descriptors + directory + data
    # Simplified: we build a Rock Ridge / El Torito hybrid
    
    # First pass: allocate file sectors
    file_sectors = {}
    for path, data in sorted(all_files.items()):
        file_sectors[path] = (sector_num, len(data))
        sector_num += (len(data) + SECTOR_SIZE - 1) // SECTOR_SIZE
    
    # Directory record area
    dir_data = bytearray()
    
    # Root directory entry
    def make_dir_record(name, loc_lba, size, flags=0, is_dir=False):
        name_bytes = name.upper().encode('ascii')
        if is_dir:
            name_bytes += b';1'  # ISO9660 version number
        else:
            name_bytes += b';1'
        
        rec_len = 34 + len(name_bytes)
        if rec_len % 2:
            rec_len += 1  # Pad to even
        rec = bytearray(rec_len)
        rec[0] = rec_len
        rec[25] = len(name_bytes)  # File name length
        rec[28:32] = struct.pack('<I', loc_lba)
        rec[32:36] = struct.pack('<I', size)
        rec[34] = len(name_bytes)
        if is_dir:
            rec[26] = 0x02  # Directory
            rec[27] = 0  # Interleave
        else:
            rec[26] = 0  # File
        rec[34:34 + len(name_bytes)] = name_bytes
        return bytes(rec)
    
    root_lba = sector_num
    
    # Root directory
    root_entries = bytearray()
    root_entries += make_dir_record('.', root_lba, 0, is_dir=True)  # Will patch size
    root_entries += make_dir_record('..', root_lba, 0, is_dir=True)
    
    # Add EFI/BOOT directory
    efi_boot_entries = bytearray()
    for path, (loc, size) in sorted(file_sectors.items()):
        name = path.split('/')[-1]
        efi_boot_entries += make_dir_record(name, loc, size)
    
    efi_boot_dir_size = len(efi_boot_entries)
    efi_boot_entries += make_dir_record('.', 0, 0, is_dir=True)  # placeholder
    efi_boot_entries += make_dir_record('..', root_lba, 0, is_dir=True)
    # Actually need to compute efi_boot LBA first
    
    # Simpler approach: all files in root directory
    root_dir_entries = bytearray()
    root_dir_entries += make_dir_record('.', 0, 0, is_dir=True)
    root_dir_entries += make_dir_record('..', 0, 0, is_dir=True)
    for path, (loc, size) in sorted(file_sectors.items()):
        root_dir_entries += make_dir_record(path, loc, size)
    
    root_dir_size = len(root_dir_entries)
    sector_num += (root_dir_size + SECTOR_SIZE - 1) // SECTOR_SIZE
    
    # Patch root directory size
    struct.pack_into('<I', root_dir_entries, 10, root_dir_size)
    # Patch root self-reference LBA
    struct.pack_into('<I', root_dir_entries, 2, root_lba)
    
    # Now build the full ISO
    total_sectors = sector_num + 16  # Extra padding
    iso = bytearray(total_sectors * SECTOR_SIZE)
    
    # Write El Torito Boot Catalog (sector 17 = LBA 16)
    # Validation Entry
    ve = bytearray(32)
    ve[0] = 0x01  # Header ID
    ve[1] = 0x00  # Platform: x86
    ve[2:4] = struct.pack('<H', 0x0000)  # Reserved
    ve[4:24] = b'Vahi OS Builder       '  # ID string (20 bytes)
    ve[24:28] = struct.pack('<I', 0x00000000)  # Checksum
    ve[28] = 0x55  # Key byte 1
    ve[29] = 0xAA  # Key byte 2
    # Compute checksum
    cksum = 0
    for i in range(16):
        cksum += struct.unpack_from('<H', ve, i * 2)[0]
    cksum = (0x10000 - (cksum & 0xFFFF)) & 0xFFFF
    struct.pack_into('<H', ve, 24, cksum)
    
    iso[boot_catalog_lba * SECTOR_SIZE:boot_catalog_lba * SECTOR_SIZE + 32] = ve
    
    # Initial/Default Entry (for BIOS boot)
    ie = bytearray(32)
    ie[0] = 0x88  # Bootable
    ie[1] = 0x00  # Media: no emulation
    ie[2:4] = struct.pack('<H', bios_img_lba & 0xFFFF)  # Load segment
    ie[4] = 0x00  # System type
    ie[5] = 0x00  # Reserved
    ie[6:8] = struct.pack('<H', (bios_img_lba >> 16) & 0xFFFF)  # Load segment (high)
    ie[8:12] = struct.pack('<I', bios_img_sectors if bios_img_size > 0 else 0)
    ie[12:16] = struct.pack('<I', bios_img_lba)
    
    iso[boot_catalog_lba * SECTOR_SIZE + 32:boot_catalog_lba * SECTOR_SIZE + 64] = ie
    
    # Section Header Entry (for EFI)
    she = bytearray(32)
    she[0] = 0x91  # Section header, more entries follow
    she[1] = 0xEF  # Platform: EFI
    she[2:4] = struct.pack('<H', 1)  # Number of entries in this section
    she[4:28] = b'EFI Boot Section      '
    
    iso[boot_catalog_lba * SECTOR_SIZE + 64:boot_catalog_lba * SECTOR_SIZE + 96] = she
    
    # EFI boot entry (pointing to EFI/BOOT/BOOTX64.EFI)
    # Find the BOOTX64.EFI file location
    bootx64_path = 'EFI/BOOT/BOOTX64.EFI'
    if bootx64_path in file_sectors:
        bootx64_lba, bootx64_size = file_sectors[bootx64_path]
        ee = bytearray(32)
        ee[0] = 0x88  # Bootable
        ee[1] = 0x00  # Media: no emulation
        ee[2:4] = struct.pack('<H', 0)  # Load segment
        ee[4] = 0xEF  # System type: EFI
        ee[6:8] = struct.pack('<H', 0)
        ee[8:12] = struct.pack('<I', 1)  # 1 sector
        ee[12:16] = struct.pack('<I', bootx64_lba)
        
        iso[boot_catalog_lba * SECTOR_SIZE + 96:boot_catalog_lba * SECTOR_SIZE + 128] = ee
    
    # Write file data
    for path, (loc, size) in file_sectors.items():
        data = files[path]
        offset = loc * SECTOR_SIZE
        iso[offset:offset + len(data)] = data
    
    # Write root directory
    root_offset = root_lba * SECTOR_SIZE
    iso[root_offset:root_offset + len(root_dir_entries)] = root_dir_entries
    
    # Primary Volume Descriptor (at LBA 16)
    pvd = bytearray(SECTOR_SIZE)
    pvd[0] = 0x01  # Type: boot record
    pvd[1:6] = b'CD001'
    pvd[6] = 0x01  # Version
    pvd[7] = 0x00  # Unused
    pvd[8:40] = b'VAHI KERNEL' + b'\x00' * 28  # Volume label
    pvd[40:72] = b'\x00' * 32  # Unused
    pvd[72:80] = struct.pack('<I', total_sectors)  # Volume space
    pvd[73] = 0x01  # Volume set size (little byte)
    pvd[80:88] = b'\x00' * 8
    pvd[88] = 0x01  # Path table size (LE)
    pvd[92] = 0x01  # LE path table loc
    pvd[96:100] = struct.pack('<I', 0)
    pvd[100] = 0x01  # Root dir record (34 bytes)
    pvd[102] = 0x00
    pvd[104:108] = struct.pack('<I', root_lba)
    pvd[108:112] = struct.pack('<I', root_dir_size)
    pvd[115:128] = b'VAHI OS' + b'\x00' * 6  # Publisher
    pvd[139:184] = b'VAHI KERNEL BUILDER' + b'\x00' * 27  # Data preparer
    pvd[319] = 0x59  # Directory record version
    
    # Write PV descriptor
    pvd_lba = 16
    iso[pvd_lba * SECTOR_SIZE:pvd_lba * SECTOR_SIZE + SECTOR_SIZE] = pvd
    
    # Write Terminator at LBA 17
    term_lba = 17
    iso[term_lba * SECTOR_SIZE] = 0xFF
    
    with open(output_path, 'wb') as f:
        f.write(iso)
    
    print(f"ISO: {len(iso)} bytes ({len(iso)/1024/1024:.1f} MiB) -> {output_path}")
    return len(iso)


def main():
    parser = argparse.ArgumentParser(description="Build Limine-bootable ISO")
    parser.add_argument('--kernel', default='kernel/target/x86_64-unknown-none/release/vahi_kernel')
    parser.add_argument('--output', default='bootimage.iso')
    parser.add_argument('--limine-dir', default=None)
    args = parser.parse_args()
    
    # Find Limine dir
    limine_dir = args.limine_dir
    if not limine_dir:
        for candidate in [os.path.join(os.environ.get('TEMP', '/tmp'), 'limine-binary')]:
            if os.path.exists(os.path.join(candidate, 'BOOTX64.EFI')):
                limine_dir = candidate
                break
    if not limine_dir:
        print("ERROR: Limine dir not found"); sys.exit(1)
    
    # Read kernel
    kernel_path = args.kernel
    if not os.path.exists(kernel_path):
        alt = 'kernel/target/x86_64-unknown-none/debug/vahi_kernel'
        if os.path.exists(alt): kernel_path = alt
        else: print(f"ERROR: kernel not found"); sys.exit(1)
    
    with open(kernel_path, 'rb') as f: kernel_data = f.read()
    print(f"Kernel: {len(kernel_data)} bytes")
    
    initrd_data = None
    if os.path.exists('initrd.tar'):
        with open('initrd.tar', 'rb') as f: initrd_data = f.read()
        print(f"Initrd: {len(initrd_data)} bytes")
    
    # Limine config (v12 format: entries use '/', options use 'key: value')
    limine_conf = (
        b"TIMEOUT: 0\n"
        b"SERIAL: yes\n"
        b"/SkyOS\n"
        b"    PROTOCOL: limine\n"
        b"    KERNEL_PATH: boot():/vahi_kernel\n"
    )
    if initrd_data:
        limine_conf += b"    MODULE_PATH: boot():/initrd.tar\n    MODULE_CMDLINE: initrd\n"
    
    # Read Limine binaries
    with open(os.path.join(limine_dir, 'BOOTX64.EFI'), 'rb') as f: bootx64 = f.read()
    bios_cd_path = os.path.join(limine_dir, 'limine-bios-cd.bin')
    uefi_cd_path = os.path.join(limine_dir, 'limine-uefi-cd.bin')
    bios_sys_path = os.path.join(limine_dir, 'limine-bios.sys')
    
    files = {
        'EFI/BOOT/BOOTX64.EFI': bootx64,
        'EFI/BOOT/limine.conf': limine_conf,
        'limine.conf': limine_conf,  # Also at root for BIOS
        'vahi_kernel': kernel_data,
    }
    if initrd_data:
        files['initrd.tar'] = initrd_data
    if os.path.exists(bios_sys_path):
        with open(bios_sys_path, 'rb') as f: files['limine-bios.sys'] = f.read()
    
    create_iso(files, args.output, bios_cd_path, uefi_cd_path)


if __name__ == '__main__':
    main()
