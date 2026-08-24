#!/usr/bin/env python3
"""Minimal FAT16 ESP test - create a bootable ESP image with BOOTX64.EFI."""
import struct, os, math

SECTOR = 512
SPC = 8  # sectors per cluster = 4KB clusters
CLUSTER_BYTES = SPC * SECTOR

efi_path = r"C:\Windows\TEMP\limine-binary\BOOTX64.EFI"
efi_size = os.path.getsize(efi_path)
clusters_for_efi = math.ceil(efi_size / CLUSTER_BYTES)

fat_entries = clusters_for_efi + 200
fat_sectors = math.ceil(fat_entries * 2 / SECTOR)
root_sectors = 32
data_start = 1 + fat_sectors + root_sectors

img_size = 4 * 1024 * 1024
img = bytearray(img_size)
total_sectors = img_size // SECTOR

print(f"EFI: {efi_size} bytes, {clusters_for_efi} clusters")
print(f"FAT: {fat_sectors} sectors, data starts at sector {data_start}")

# Boot sector
bs = bytearray(SECTOR)
bs[0:3] = b"\xEB\x3C\x90"
bs[3:11] = b"MSDOS5.0"
struct.pack_into("<H", bs, 11, SECTOR)
bs[13] = SPC
struct.pack_into("<H", bs, 14, 1)
bs[16] = 1
struct.pack_into("<H", bs, 17, 512)
struct.pack_into("<H", bs, 19, 0)
bs[21] = 0xF8
struct.pack_into("<H", bs, 22, fat_sectors)
struct.pack_into("<H", bs, 24, 63)
struct.pack_into("<H", bs, 26, 255)
struct.pack_into("<I", bs, 28, 0)
struct.pack_into("<I", bs, 32, total_sectors)
bs[36] = 0x80
bs[38] = 0x29
bs[43:54] = b"EFI SYSPART"
bs[54:62] = b"FAT16   "
bs[510] = 0x55
bs[511] = 0xAA
img[0:SECTOR] = bs

# FAT
fat = bytearray(fat_sectors * SECTOR)
fat[0:2] = struct.pack("<H", 0xFFF8)
fat[2:4] = struct.pack("<H", 0xFFFF)
for i in range(clusters_for_efi):
    c = 2 + i
    val = c + 1 if i < clusters_for_efi - 1 else 0xFFFF
    struct.pack_into("<H", fat, c * 2, val)
img[SECTOR : SECTOR + len(fat)] = fat

# Root dir entry
root_off = (1 + fat_sectors) * SECTOR
entry = bytearray(32)
entry[0:8] = b"BOOTX64 "
entry[8:11] = b"EFI"
entry[11] = 0x20
struct.pack_into("<I", entry, 28, efi_size)
struct.pack_into("<H", entry, 26, 2)
img[root_off : root_off + 32] = entry

# File data
efi_data = open(efi_path, "rb").read()
for i in range(clusters_for_efi):
    c = 2 + i
    off = data_start * SECTOR + (c - 2) * CLUSTER_BYTES
    chunk = efi_data[i * CLUSTER_BYTES : (i + 1) * CLUSTER_BYTES]
    img[off : off + len(chunk)] = chunk

with open("test_esp.img", "wb") as f:
    f.write(img)
print(f"Created test_esp.img ({len(img)} bytes)")
