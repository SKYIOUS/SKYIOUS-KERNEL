#!/usr/bin/env python3
"""
Build a Limine-bootable GPT disk image from scratch.

Creates a GPT-partitioned raw disk with:
  - Partition 1: EFI System Partition (FAT32) containing:
    * EFI/BOOT/BOOTX64.EFI  (Limine UEFI bootloader)
    * limine-bios.sys        (Limine BIOS bootloader)
    * limine.conf            (boot configuration)
    * vahi_kernel            (kernel ELF)
    * initrd.tar             (optional ramdisk)
  - Partition 2: BIOS Boot Partition (for legacy BIOS bootstrap)

Usage:
    py builder/build_limine_image.py [--bios] [--kernel PATH] [--initrd PATH] [--output PATH]

No external dependencies — pure Python stdlib (struct, os, hashlib, uuid, binascii).

## Build Paths

1. **Rust builder (recommended)**: `--rust-builder` — uses the custom Vahi UEFI
   bootloader. This is the production-tested path. The Rust builder is
   compiled from `builder/src/main.rs`.

2. **Limine (alternative)**: `--limine-dir` or auto-download — uses the Limine
   bootloader with binaries auto-downloaded from GitHub releases.
   NOTE: The `limine` Rust crate v0.6.5 (protocol revision 6) works with the
   v12.7.0 bootloader. The old "init hangs in a futex loop" issue was root-
   caused to a kernel mmap bug (fixed 2026-09-03), not the boot protocol.
"""

import struct
import os
import sys
import hashlib
import argparse
import math
import uuid
import binascii
import zipfile
import tempfile
import shutil
import urllib.request
import urllib.error

# ── Constants ────────────────────────────────────────────────────────────────

SECTOR_SIZE = 512
SECTORS_PER_CLUSTER = 8  # 4 KiB clusters
CLUSTER_SIZE = SECTOR_SIZE * SECTORS_PER_CLUSTER  # 4096
RESERVED_SECTORS = 32
NUM_FATS = 2
ROOT_DIR_CLUSTER = 2

EFI_SYSTEM_PARTITION_GUID = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"
BIOS_BOOT_PARTITION_GUID = "21686148-6449-6E6F-744E-656564454649"

# Limine binary release URL — update this when upgrading Limine.
LIMINE_VERSION = "12.7.0"
LIMINE_BINARY_URL = f"https://github.com/limine-bootloader/limine/releases/download/v{LIMINE_VERSION}/limine-binary.zip"


def align_up(value, alignment):
    return ((value + alignment - 1) // alignment) * alignment


def gpt_crc32(data: bytes) -> int:
    return binascii.crc32(data) & 0xFFFFFFFF


def efi_guid_str_to_bytes(guid_str: str) -> bytes:
    """Convert 'XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX' to EFI GUID bytes (little-endian Data1-3, big-endian Data4)."""
    parts = guid_str.split('-')
    a = int(parts[0], 16)
    b = int(parts[1], 16)
    c = int(parts[2], 16)
    d = bytes.fromhex(parts[3])  # big-endian
    e = bytes.fromhex(parts[4])  # big-endian
    return struct.pack('<IHH', a, b, c) + d + e


# ── Limine Auto-Download ─────────────────────────────────────────────────────

def download_limine_binaries(dest_dir: str):
    """Download and extract Limine bootloader binaries from GitHub releases.

    Args:
        dest_dir: Directory to extract binaries into (will be created).
    """
    print(f"Downloading Limine v{LIMINE_VERSION} binaries...")
    print(f"  URL: {LIMINE_BINARY_URL}")

    os.makedirs(dest_dir, exist_ok=True)

    # Download to a temp file
    tmp_zip = os.path.join(dest_dir, "_limine_download.zip")
    try:
        req = urllib.request.Request(
            LIMINE_BINARY_URL,
            headers={"User-Agent": "vahi-kernel-builder/1.0"}
        )
        with urllib.request.urlopen(req, timeout=120) as resp:
            total = resp.headers.get("Content-Length")
            if total:
                print(f"  Downloading {int(total) / 1024:.0f} KB...")
            else:
                print("  Downloading...")
            with open(tmp_zip, "wb") as f:
                while True:
                    chunk = resp.read(65536)
                    if not chunk:
                        break
                    f.write(chunk)
    except (urllib.error.URLError, OSError) as e:
        # Clean up partial download
        if os.path.exists(tmp_zip):
            os.remove(tmp_zip)
        print(f"ERROR: Failed to download Limine binaries: {e}")
        print(f"  URL: {LIMINE_BINARY_URL}")
        print(f"  You can also manually place Limine binaries in: {dest_dir}")
        print(f"  Required files: BOOTX64.EFI, limine-bios.sys")
        sys.exit(1)

    # Extract — the zip contains a top-level "limine-binary/" directory
    try:
        with zipfile.ZipFile(tmp_zip, "r") as zf:
            for member in zf.namelist():
                # Strip the top-level "limine-binary/" prefix
                parts = member.split("/", 1)
                if len(parts) < 2 or not parts[1]:
                    continue  # skip top-level directory entries
                target_name = parts[1]
                # Skip directory entries (end with /)
                if target_name.endswith("/"):
                    continue
                target_path = os.path.join(dest_dir, target_name)

                # Create subdirectories if needed
                target_dir = os.path.dirname(target_path)
                if target_dir and not os.path.exists(target_dir):
                    os.makedirs(target_dir, exist_ok=True)

                # Extract file
                with zf.open(member) as src, open(target_path, "wb") as dst:
                    shutil.copyfileobj(src, dst)

                if target_name in ("BOOTX64.EFI", "limine-bios.sys", "limine.exe"):
                    print(f"  Extracted: {target_name}")
    finally:
        # Clean up zip
        if os.path.exists(tmp_zip):
            os.remove(tmp_zip)

    # Verify required files exist
    required = ["BOOTX64.EFI", "limine-bios.sys"]
    for fname in required:
        fpath = os.path.join(dest_dir, fname)
        if not os.path.exists(fpath):
            print(f"ERROR: Required file not found after download: {fname}")
            print(f"  Expected at: {fpath}")
            sys.exit(1)

    print(f"  Limine binaries installed to: {dest_dir}")


def find_limine_dir(explicit_path=None) -> str:
    """Locate or download Limine bootloader binaries.

    Search order:
      1. Explicit --limine-dir path
      2. $TEMP/limine-binary/
      3. Auto-download from GitHub releases
    """
    if explicit_path:
        if os.path.exists(os.path.join(explicit_path, "BOOTX64.EFI")):
            return explicit_path
        print(f"ERROR: Limine UEFI binary not found at {explicit_path}/BOOTX64.EFI")
        sys.exit(1)

    # Try common cached locations
    temp_root = os.environ.get("TEMP", os.environ.get("TMPDIR", "/tmp"))
    candidates = [
        os.path.join(temp_root, "limine-binary"),
        os.path.join(temp_root, "limine-binary", "limine-binary"),
    ]
    for candidate in candidates:
        if os.path.exists(os.path.join(candidate, "BOOTX64.EFI")):
            print(f"Using cached Limine binaries at: {candidate}")
            return candidate

    # Auto-download
    dest = os.path.join(temp_root, "limine-binary")
    download_limine_binaries(dest)
    return dest


# ── FAT32 Writer ─────────────────────────────────────────────────────────────

class FAT32Image:
    """Minimal FAT32 filesystem writer with proper directory entry tracking."""

    def __init__(self, total_sectors: int, hidden_sectors: int = 0):
        self.total_sectors = total_sectors
        self.cluster_size = CLUSTER_SIZE
        self.spc = SECTORS_PER_CLUSTER
        self.hidden_sectors = hidden_sectors

        # Calculate FAT size
        data_sectors = total_sectors - RESERVED_SECTORS
        total_clusters = data_sectors // self.spc
        fat_entries = total_clusters + 2
        self.fat_size_sectors = align_up(fat_entries * 4, SECTOR_SIZE) // SECTOR_SIZE

        self.data_start_sector = RESERVED_SECTORS + NUM_FATS * self.fat_size_sectors
        actual_data_sectors = total_sectors - self.data_start_sector
        self.total_clusters = actual_data_sectors // self.spc
        assert self.total_clusters >= 65525, f"FAT32 needs >= 65525 clusters, got {self.total_clusters}"

        self.image = bytearray(total_sectors * SECTOR_SIZE)

        # FAT state
        self.fat = [0] * (self.total_clusters + 2)
        self.fat[0] = 0x0FFFFFF8  # media type
        self.fat[1] = 0x0FFFFFFF  # end-of-chain
        self.fat[ROOT_DIR_CLUSTER] = 0x0FFFFFFF  # root directory end-of-chain

        # ROOT_DIR_CLUSTER (2) is reserved for the root directory
        self.next_free_cluster = ROOT_DIR_CLUSTER + 1

        # dir_contents[cluster_number] = list of 32-byte entries
        self.dir_contents = {ROOT_DIR_CLUSTER: []}
        # path -> (first_cluster, size, is_dir)
        self.entries = {}

    def _alloc_clusters(self, count: int) -> list:
        clusters = []
        while len(clusters) < count:
            if self.next_free_cluster >= self.total_clusters + 2:
                raise RuntimeError("FAT32: out of clusters")
            clusters.append(self.next_free_cluster)
            self.next_free_cluster += 1
        for i in range(len(clusters) - 1):
            self.fat[clusters[i]] = clusters[i + 1]
        self.fat[clusters[-1]] = 0x0FFFFFFF
        return clusters

    def _cluster_to_sector(self, cluster: int) -> int:
        return self.data_start_sector + (cluster - 2) * self.spc

    def _write_clusters_data(self, clusters: list, data: bytes):
        for i, cluster in enumerate(clusters):
            offset = i * self.cluster_size
            chunk = data[offset:offset + self.cluster_size]
            sector = self._cluster_to_sector(cluster)
            start = sector * SECTOR_SIZE
            self.image[start:start + len(chunk)] = chunk

    def _make_83_name(self, name: str) -> bytes:
        """Convert a filename to 8.3 format (11 bytes)."""
        if '.' in name:
            base, ext = name.rsplit('.', 1)
        else:
            base, ext = name, ''
        base_8 = base.upper().encode('ascii').ljust(8, b'\x20')[:8]
        ext_3 = ext.upper().encode('ascii').ljust(3, b'\x20')[:3]
        return base_8 + ext_3

    def _make_dir_entry(self, name: str, attr: int, cluster: int, size: int) -> bytes:
        entry = bytearray(32)
        entry[0:11] = self._make_83_name(name)
        entry[11] = attr
        entry[20:22] = struct.pack('<H', (cluster >> 16) & 0xFFFF)
        entry[26:28] = struct.pack('<H', cluster & 0xFFFF)
        entry[28:32] = struct.pack('<I', size)
        return bytes(entry)

    def _make_lfn_entries(self, long_name: str) -> list:
        """Create LFN entries for a long filename (preceding the 8.3 entry)."""
        # Encode to UTF-16LE and pad to 13-char boundary
        utf16 = long_name.encode('utf-16-le')
        # Pad to multiple of 26 bytes (13 chars × 2 bytes)
        padded = utf16 + b'\x00' * ((26 - len(utf16) % 26) % 26)
        num_entries = len(padded) // 26

        entries = []
        for seq in range(num_entries, 0, -1):
            chunk = padded[(seq - 1) * 26:seq * 26]
            # Split into 13 UTF-16 characters
            chars = [struct.unpack_from('<H', chunk, i * 2)[0] for i in range(13)]

            e = bytearray(32)
            e[0] = seq if seq < num_entries else (seq | 0x40)  # 0x40 on last
            e[11] = 0x0F  # LFN attribute
            e[12] = 0     # type
            e[13] = 0     # checksum (filled below)

            # Chars 0-4 at offset 1
            for i in range(5):
                struct.pack_into('<H', e, 1 + i * 2, chars[i])
            # Chars 5-9 at offset 14
            for i in range(6):
                struct.pack_into('<H', e, 14 + i * 2, chars[5 + i])
            # Chars 10-12 at offset 28
            for i in range(2):
                struct.pack_into('<H', e, 28 + i * 2, chars[11 + i])

            entries.append(bytes(e))

        return entries

    def _needs_lfn(self, name: str) -> bool:
        """Check if a name needs LFN entries."""
        if '.' in name:
            base, ext = name.rsplit('.', 1)
        else:
            base, ext = name, ''
        if len(base) > 8 or len(ext) > 3:
            return True
        # Lowercase also needs LFN (8.3 is uppercase only)
        if name != name.upper():
            return True
        return False

    def _build_name_entries(self, name: str, attr: int, cluster: int, size: int) -> list:
        """Build the full set of directory entries (LFN + 8.3) for a file/dir."""
        result = []

        if self._needs_lfn(name):
            lfn_entries = self._make_lfn_entries(name)
            # Compute checksum from 8.3 name
            name_83 = self._make_83_name(name)
            checksum = 0
            for b in name_83:
                checksum = ((checksum >> 1) + b + (checksum & 1) * 128) & 0xFF
            # Set checksum in all LFN entries
            fixed_lfn = []
            for e in lfn_entries:
                ea = bytearray(e)
                ea[13] = checksum
                fixed_lfn.append(bytes(ea))
            result.extend(fixed_lfn)

        result.append(self._make_dir_entry(name, attr, cluster, size))
        return result

    def add_file_with_path(self, virtual_path: str, data: bytes):
        """
        Add a file at a virtual path like 'EFI/BOOT/BOOTX64.EFI'.
        Creates intermediate directories as needed.
        """
        parts = virtual_path.replace('\\', '/').split('/')
        current_cluster = ROOT_DIR_CLUSTER

        # Navigate/create directories
        for i, part in enumerate(parts[:-1]):
            dir_path = '/'.join(parts[:i + 1])
            if dir_path in self.entries:
                current_cluster = self.entries[dir_path][0]
            else:
                # Allocate cluster for new directory
                new_cluster = self._alloc_clusters(1)[0]
                # Initialize with . and .. entries
                dot = self._make_dir_entry('.', 0x10, new_cluster, 0)
                dotdot = self._make_dir_entry('..', 0x10, current_cluster, 0)
                self.dir_contents[new_cluster] = [dot, dotdot]
                self.entries[dir_path] = (new_cluster, 0, True)

                # Add entry to parent directory
                dir_entries = self._build_name_entries(part, 0x10, new_cluster, 0)
                self.dir_contents[current_cluster].extend(dir_entries)

                current_cluster = new_cluster

        # Write the file data
        clusters_needed = max(1, align_up(len(data), CLUSTER_SIZE) // CLUSTER_SIZE)
        allocated = self._alloc_clusters(clusters_needed)
        self._write_clusters_data(allocated, data)

        # Add entry to parent directory
        file_entries = self._build_name_entries(parts[-1], 0x20, allocated[0], len(data))
        self.dir_contents[current_cluster].extend(file_entries)

        self.entries[virtual_path] = (allocated[0], len(data), False)
        return allocated[0]

    def finalize(self):
        """Write all directory contents, FAT, boot sector, and FSInfo to image."""
        # ── Write directory contents ──
        for cluster, entries in self.dir_contents.items():
            data = b''
            for e in entries:
                data += e
            # Pad to cluster boundary
            padded_len = align_up(len(data), CLUSTER_SIZE)
            data += b'\x00' * (padded_len - len(data))
            self._write_clusters_data([cluster], data)

        # ── Boot Sector (Sector 0) ──
        boot = bytearray(SECTOR_SIZE)
        boot[0:3] = b'\xEB\x58\x90'
        boot[3:11] = b'MSWIN4.1'
        struct.pack_into('<H', boot, 11, SECTOR_SIZE)
        boot[13] = self.spc
        struct.pack_into('<H', boot, 14, RESERVED_SECTORS)
        boot[16] = NUM_FATS
        struct.pack_into('<H', boot, 17, 0)  # root entries (0 for FAT32)
        struct.pack_into('<H', boot, 19, 0)  # total sectors 16
        boot[21] = 0xF8  # media
        struct.pack_into('<H', boot, 22, 0)  # FAT size 16
        struct.pack_into('<H', boot, 24, 0)  # sectors per track
        struct.pack_into('<H', boot, 26, 0)  # number of heads
        struct.pack_into('<I', boot, 28, self.hidden_sectors)  # hidden sectors (partition start LBA)
        struct.pack_into('<I', boot, 32, self.total_sectors)  # total sectors 32

        # FAT32-specific
        struct.pack_into('<I', boot, 36, self.fat_size_sectors)
        struct.pack_into('<H', boot, 40, 0)  # extended flags
        struct.pack_into('<H', boot, 42, 0)  # FAT32 version
        struct.pack_into('<I', boot, 44, ROOT_DIR_CLUSTER)
        struct.pack_into('<H', boot, 48, 1)  # FSInfo sector
        struct.pack_into('<H', boot, 50, 6)  # backup boot sector
        boot[64] = 0x80  # drive number
        boot[66] = 0x29  # extended boot sig
        struct.pack_into('<I', boot, 67, 0x12345678)
        boot[71:82] = b'SKYIOUS    '
        boot[82:90] = b'FAT32   '
        struct.pack_into('<H', boot, 510, 0xAA55)

        self.image[0:SECTOR_SIZE] = boot

        # ── FSInfo (Sector 1) ──
        fsinfo = bytearray(SECTOR_SIZE)
        fsinfo[0:4] = b'\x52\x52\x61\x41'          # lead signature 0x41615252
        struct.pack_into('<I', fsinfo, 484, 0x61417272) # struct signature 0x61417272
        struct.pack_into('<I', fsinfo, 488, 0xFFFFFFFF) # free count (unknown)
        struct.pack_into('<I', fsinfo, 492, 0xFFFFFFFF) # next free (unknown)
        self.image[SECTOR_SIZE:2 * SECTOR_SIZE] = fsinfo

        # ── Backup boot sector (sector 6) + backup FSInfo (sector 7) ──
        self.image[6 * SECTOR_SIZE:7 * SECTOR_SIZE] = self.image[0:SECTOR_SIZE]
        self.image[7 * SECTOR_SIZE:8 * SECTOR_SIZE] = self.image[SECTOR_SIZE:2 * SECTOR_SIZE]

        # ── Write FAT copies ──
        fat_bytes = bytearray()
        for i in range(min(self.total_clusters + 2, len(self.fat))):
            fat_bytes += struct.pack('<I', self.fat[i] & 0x0FFFFFFF)
        fat_size_bytes = self.fat_size_sectors * SECTOR_SIZE
        if len(fat_bytes) < fat_size_bytes:
            fat_bytes += b'\x00' * (fat_size_bytes - len(fat_bytes))

        for fat_num in range(NUM_FATS):
            offset = (RESERVED_SECTORS + fat_num * self.fat_size_sectors) * SECTOR_SIZE
            self.image[offset:offset + fat_size_bytes] = fat_bytes

        return bytes(self.image)


# ── GPT Writer ───────────────────────────────────────────────────────────────

class GPTDiskImage:
    def __init__(self, esp_size_bytes: int, bios_size_bytes: int = 1 * 1024 * 1024):
        self.sector_size = SECTOR_SIZE
        self.alignment = 2048 * SECTOR_SIZE
        self.esp_size = align_up(esp_size_bytes, self.alignment)
        self.bios_size = align_up(bios_size_bytes, self.alignment)

        self.total_sectors = (
            1  # protective MBR
            + self.bios_size // SECTOR_SIZE
            + 2048  # alignment gap
            + self.esp_size // SECTOR_SIZE
            + 2048  # trailing alignment
            + 33  # GPT header + entries at end
        )

        self.disk = bytearray(self.total_sectors * SECTOR_SIZE)

        # Partition layout — start at LBA 64 to leave room for
        # GPT header (LBA 1), entries (LBAs 2-33), and the MBR gap
        # that Limine bios-install uses (needs LBAs 1-62 free).
        PARTITION_START_LBA = 64
        if self.bios_size > 0:
            self.bios_start_lba = PARTITION_START_LBA
            self.bios_end_lba = self.bios_start_lba + self.bios_size // SECTOR_SIZE - 1
            esp_base = self.bios_end_lba + 1
        else:
            esp_base = PARTITION_START_LBA
        self.esp_start_lba = align_up(esp_base * SECTOR_SIZE, self.alignment) // SECTOR_SIZE
        self.esp_end_lba = self.esp_start_lba + self.esp_size // SECTOR_SIZE - 1

    def write_protective_mbr(self):
        mbr = bytearray(SECTOR_SIZE)
        mbr[446 + 4] = 0xEE  # type: GPT protective
        struct.pack_into('<I', mbr, 446 + 8, 1)
        struct.pack_into('<I', mbr, 446 + 12, self.total_sectors - 1)
        struct.pack_into('<H', mbr, 510, 0xAA55)
        self.disk[0:SECTOR_SIZE] = mbr

    def write_gpt(self):
        GPT_ENTRY_SIZE = 128
        entries_lba = self.total_sectors - 33
        header_lba = self.total_sectors - 1
        num_entry_sectors = (128 * GPT_ENTRY_SIZE) // SECTOR_SIZE  # 32 sectors

        entries = bytearray(128 * GPT_ENTRY_SIZE)

        def make_entry(type_guid, name_str, start, end, attrs=0):
            e = bytearray(GPT_ENTRY_SIZE)
            e[0:16] = efi_guid_str_to_bytes(type_guid)
            e[16:32] = uuid.uuid4().bytes
            struct.pack_into('<Q', e, 32, start)
            struct.pack_into('<Q', e, 40, end)
            struct.pack_into('<Q', e, 48, attrs)
            name_utf16 = name_str.encode('utf-16-le')[:72]
            e[56:56 + len(name_utf16)] = name_utf16
            return e

        # ESP: attribute bit 0 = required for boot (EFI_SPEC_PART_ATTR_EFI_SYSTEM_PARTITION)
        ESP_ATTR = 0x1
        entry_idx = 0
        if self.bios_size > 0:
            entries[0:GPT_ENTRY_SIZE] = make_entry(BIOS_BOOT_PARTITION_GUID, 'BIOS Boot',
                                                    self.bios_start_lba, self.bios_end_lba)
            entry_idx = 1
        esp_offset = entry_idx * GPT_ENTRY_SIZE
        entries[esp_offset:esp_offset + GPT_ENTRY_SIZE] = make_entry(EFI_SYSTEM_PARTITION_GUID, 'EFI System',
                                                                     self.esp_start_lba, self.esp_end_lba, ESP_ATTR)

        # Write entries to primary location (LBA 2)
        self.disk[2 * SECTOR_SIZE:(2 + num_entry_sectors) * SECTOR_SIZE] = entries
        # Write entries to backup location (end of disk)
        self.disk[entries_lba * SECTOR_SIZE:(entries_lba + num_entry_sectors) * SECTOR_SIZE] = entries

        # ── Primary GPT header (LBA 1) ──
        primary_header = bytearray(SECTOR_SIZE)
        primary_header[0:8] = b'EFI PART'
        struct.pack_into('<I', primary_header, 8, 0x00010000)
        struct.pack_into('<I', primary_header, 12, 92)
        struct.pack_into('<Q', primary_header, 24, 1)           # my_lba = 1
        struct.pack_into('<Q', primary_header, 32, header_lba)  # alt_lba = backup
        struct.pack_into('<Q', primary_header, 40, 34)          # first_usable_lba
        struct.pack_into('<Q', primary_header, 48, entries_lba - 1) # last_usable_lba
        primary_header[56:72] = efi_guid_str_to_bytes(EFI_SYSTEM_PARTITION_GUID)
        struct.pack_into('<Q', primary_header, 72, 2)           # partition entries LBA
        struct.pack_into('<I', primary_header, 80, 128)         # num entries
        struct.pack_into('<I', primary_header, 84, GPT_ENTRY_SIZE) # entry size
        struct.pack_into('<I', primary_header, 16, 0)  # header CRC → 0
        struct.pack_into('<I', primary_header, 88, 0)  # entries CRC → 0
        struct.pack_into('<I', primary_header, 88, gpt_crc32(bytes(entries)))
        struct.pack_into('<I', primary_header, 16, gpt_crc32(bytes(primary_header[:92])))
        self.disk[1 * SECTOR_SIZE:2 * SECTOR_SIZE] = primary_header

        # ── Backup GPT header (last LBA) ──
        backup_header = bytearray(primary_header)
        struct.pack_into('<Q', backup_header, 24, header_lba)  # my_lba = backup
        struct.pack_into('<Q', backup_header, 32, 1)           # alt_lba = primary
        struct.pack_into('<Q', backup_header, 40, 34)          # first_usable_lba
        struct.pack_into('<Q', backup_header, 48, entries_lba - 1) # last_usable_lba
        backup_header[56:72] = efi_guid_str_to_bytes(EFI_SYSTEM_PARTITION_GUID)
        struct.pack_into('<Q', backup_header, 72, entries_lba)  # entries at end
        struct.pack_into('<I', backup_header, 16, 0)
        struct.pack_into('<I', backup_header, 88, 0)
        struct.pack_into('<I', backup_header, 88, gpt_crc32(bytes(entries)))
        struct.pack_into('<I', backup_header, 16, gpt_crc32(bytes(backup_header[:92])))
        self.disk[header_lba * SECTOR_SIZE:(header_lba + 1) * SECTOR_SIZE] = backup_header

    def build_fat32_esp(self, files: dict) -> bytes:
        fat = FAT32Image(self.esp_size // SECTOR_SIZE, hidden_sectors=self.esp_start_lba)
        for path, data in sorted(files.items()):
            fat.add_file_with_path(path, data)
        return fat.finalize()

    def assemble(self, fat32_data: bytes):
        self.write_protective_mbr()
        esp_offset = self.esp_start_lba * SECTOR_SIZE
        self.disk[esp_offset:esp_offset + len(fat32_data)] = fat32_data
        self.write_gpt()

    def write(self, path: str):
        with open(path, 'wb') as f:
            f.write(self.disk)
        print(f"Wrote {len(self.disk)} bytes ({len(self.disk) / 1024 / 1024:.1f} MiB) to {path}")


# ── Rust Builder Path ───────────────────────────────────────────────────────

def _run_rust_builder(args):
    """Build using the Rust builder (production build path).

    The Rust builder uses the custom Vahi UEFI bootloader which is the
    production-tested boot path. The Limine path below is a verified
    alternative; both boot to userspace.
    """
    import subprocess
    import shutil

    builder_dir = os.path.join(os.path.dirname(__file__), '..')
    builder_dir = os.path.abspath(builder_dir)
    builder_exe = os.path.join(builder_dir, 'builder', 'target', 'debug', 'builder.exe')
    if not os.path.exists(builder_exe):
        builder_exe = os.path.join(builder_dir, 'builder', 'target', 'debug', 'builder')

    if not os.path.exists(builder_exe):
        print("Rust builder not found. Building it first...")
        ret = subprocess.run(
            ['cargo', 'build'],
            cwd=os.path.join(builder_dir, 'builder'),
            capture_output=True
        )
        if ret.returncode != 0:
            print(f"ERROR: Failed to build Rust builder: {ret.stderr.decode()}")
            sys.exit(1)
        if os.path.exists(builder_exe):
            print("Rust builder built successfully.")
        else:
            # Check alternative path
            builder_exe = os.path.join(builder_dir, 'builder', 'target', 'debug', 'builder')

    if not os.path.exists(builder_exe):
        print(f"ERROR: Rust builder not found at {builder_exe}")
        sys.exit(1)

    print(f"Using Rust builder: {builder_exe}")
    ret = subprocess.run([builder_exe], cwd=os.path.join(builder_dir, 'builder'))
    if ret.returncode != 0:
        print(f"ERROR: Rust builder failed with code {ret.returncode}")
        sys.exit(1)

    # The Rust builder outputs to kernel/target/.../bootimage-vahi_kernel.bin
    # Copy to the requested output path
    src = os.path.join(builder_dir, 'kernel', 'target', 'x86_64-unknown-none', 'debug', 'bootimage-vahi_kernel.bin')
    if not os.path.exists(src):
        # Try other triples
        for triple in ['x86_64-vahi', 'x86_64-unknown-none']:
            src = os.path.join(builder_dir, 'kernel', 'target', triple, 'debug', 'bootimage-vahi_kernel.bin')
            if os.path.exists(src):
                break

    if os.path.exists(src):
        shutil.copy2(src, args.output)
        print(f"Done! Image: {os.path.abspath(args.output)}")
        print(f"\nTo boot (UEFI):")
        print(f'  qemu-system-x86_64 -drive if=pflash,format=raw,file=OVMF.fd -drive format=raw,file={args.output} -serial stdio -m 512')
    else:
        print(f"WARNING: Rust builder output not found. Check builder output directory.")


# ── Main ─────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Build a bootable GPT disk image",
        epilog="Use --rust-builder to build via the Rust builder (production path), "
               "or --limine-dir to build with Limine binaries (alternative)."
    )
    parser.add_argument('--kernel', default='kernel/target/x86_64-unknown-none/debug/vahi_kernel',
                        help='Path to kernel ELF')
    parser.add_argument('--initrd', default='initrd.tar', help='Path to initrd (optional)')
    parser.add_argument('--output', default='bootimage-vahi_kernel.bin', help='Output image path')
    parser.add_argument('--esp-size', default='300M', help='ESP size (default 300M)')
    parser.add_argument('--no-bios', action='store_true', help='Skip BIOS boot partition')
    parser.add_argument('--limine-dir', default=None, help='Path to Limine binary directory')
    parser.add_argument('--rust-builder', action='store_true',
                        help='Use the Rust builder (production path, requires builder/ to be compiled)')
    args = parser.parse_args()

    # ── Rust builder path (production) ──
    if args.rust_builder:
        _run_rust_builder(args)
        return

    # Parse size
    esp_size_str = args.esp_size.upper()
    if esp_size_str.endswith('M'):
        esp_size = int(esp_size_str[:-1]) * 1024 * 1024
    elif esp_size_str.endswith('K'):
        esp_size = int(esp_size_str[:-1]) * 1024
    else:
        esp_size = int(esp_size_str)

    # Find kernel
    kernel_path = args.kernel
    if not os.path.exists(kernel_path):
        alt = os.path.join('kernel', 'target', 'x86_64-unknown-none', 'debug', 'vahi_kernel')
        if os.path.exists(alt):
            kernel_path = alt
        else:
            print(f"ERROR: Kernel ELF not found at {kernel_path} or {alt}")
            sys.exit(1)

    with open(kernel_path, 'rb') as f:
        kernel_data = f.read()
    print(f"Kernel: {len(kernel_data)} bytes from {kernel_path}")

    initrd_data = None
    if os.path.exists(args.initrd):
        with open(args.initrd, 'rb') as f:
            initrd_data = f.read()
        print(f"Initrd: {len(initrd_data)} bytes from {args.initrd}")
    else:
        print(f"WARNING: No initrd at {args.initrd}")

    # Build limine.conf (v12+ format: entries use '/', options use 'key: value')
    limine_conf = (
        b"TIMEOUT: 0\n"
        b"SERIAL: yes\n"
        b"/SkyOS\n"
        b"    PROTOCOL: limine\n"
        b"    KERNEL_PATH: boot():/vahi_kernel\n"
    )
    if initrd_data:
        limine_conf += b"    MODULE_PATH: boot():/initrd.tar\n    MODULE_CMDLINE: initrd\n"

    # Find Limine binaries (auto-downloads if not cached)
    limine_dir = find_limine_dir(args.limine_dir)

    files = {}

    # EFI/BOOT/BOOTX64.EFI
    uefi_path = os.path.join(limine_dir, 'BOOTX64.EFI')
    if os.path.exists(uefi_path):
        with open(uefi_path, 'rb') as f:
            files['EFI/BOOT/BOOTX64.EFI'] = f.read()
        print(f"EFI bootloader: {len(files['EFI/BOOT/BOOTX64.EFI'])} bytes")
    else:
        print(f"ERROR: Limine UEFI binary not found at {uefi_path}")
        sys.exit(1)

    # limine-bios.sys
    bios_sys_path = os.path.join(limine_dir, 'limine-bios.sys')
    if os.path.exists(bios_sys_path):
        with open(bios_sys_path, 'rb') as f:
            files['limine-bios.sys'] = f.read()
        print(f"BIOS bootloader: {len(files['limine-bios.sys'])} bytes")

    files['limine.conf'] = limine_conf
    files['vahi_kernel'] = kernel_data
    if initrd_data:
        files['initrd.tar'] = initrd_data

    # Create disk
    bios_size = 0 if args.no_bios else 1 * 1024 * 1024
    disk = GPTDiskImage(esp_size, bios_size)

    print("Building FAT32 ESP...")
    fat32_data = disk.build_fat32_esp(files)
    print(f"FAT32 image: {len(fat32_data)} bytes ({len(fat32_data) / 1024 / 1024:.1f} MiB)")

    print("Assembling GPT disk image...")
    disk.assemble(fat32_data)
    disk.write(args.output)

    # Install BIOS bootloader
    if not args.no_bios:
        limine_tool = os.path.join(limine_dir, 'limine-tool-windows-x86', 'limine.exe')
        if os.path.exists(limine_tool):
            print("\nInstalling BIOS bootloader...")
            abs_output = os.path.abspath(args.output)
            win_path = abs_output.replace('/', '\\')
            ret = os.system(f'"{limine_tool}" bios-install --force "{win_path}"')
            if ret == 0:
                print("BIOS bootloader installed!")
            else:
                print(f"WARNING: BIOS install returned {ret}")

    print(f"\nDone! Image: {os.path.abspath(args.output)}")
    print(f"\nTo boot (UEFI):")
    print(f'  qemu-system-x86_64 -drive if=pflash,format=raw,file=OVMF.fd -drive format=raw,file={args.output} -serial stdio -m 512')
    print(f"\nTo boot (BIOS):")
    print(f'  qemu-system-x86_64 -drive format=raw,file={args.output} -serial stdio -m 512')
    print(f"\nNOTE: The Limine path boots to userspace with init forks verified.")
    print(f"      For the production build, use: python builder/build_limine_image.py --rust-builder")


if __name__ == '__main__':
    main()
