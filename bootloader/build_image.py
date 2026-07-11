"""Build a bootable FAT16 disk image with stage1, stage2, and kernel.bin."""
import argparse, math, struct, sys

SECTOR_SIZE   = 512
# Must match values in stage1.asm
SPC           = 4          # sectors per cluster
RESV          = 65         # reserved sectors (boot + stage2)
FAT_CNT       = 2
ROOT_MAX      = 512
MEDIA         = 0xF8
FAT16_SIZE    = 16         # sectors per FAT
TOTAL_SECTORS = 32768      # 16 MB
TOTAL_CLUSTERS = (TOTAL_SECTORS - RESV - FAT_CNT * FAT16_SIZE - (ROOT_MAX * 32 + SECTOR_SIZE - 1) // SECTOR_SIZE) // SPC

def build_bootsector(stage1_bin: bytes, bpb: dict) -> bytes:
    """Patch BPB values into stage1 binary at the correct FAT offsets."""
    buf = bytearray(stage1_bin)
    # BPB fields (little-endian)
    struct.pack_into("<H", buf, 11, bpb["bytes_per_sector"])   # bpb_bps
    buf[13] = bpb["sectors_per_cluster"]                      # bpb_spc
    struct.pack_into("<H", buf, 14, bpb["reserved_sectors"])  # bpb_resv
    buf[16] = bpb["fat_count"]                                # bpf_fatcnt
    struct.pack_into("<H", buf, 17, bpb["root_max_entries"])  # bpb_rootmax
    struct.pack_into("<H", buf, 19, bpb["total_sectors_16"])  # bpb_tot16
    buf[21] = bpb["media_descriptor"]                         # bpb_media
    struct.pack_into("<H", buf, 22, bpb["fat_size_16"])       # bpb_fat16sz
    struct.pack_into("<H", buf, 24, bpb["sectors_per_track"]) # bpb_spt
    struct.pack_into("<H", buf, 26, bpb["head_count"])        # bpb_heads
    struct.pack_into("<I", buf, 28, bpb["hidden_sectors"])    # bpb_hidden
    struct.pack_into("<I", buf, 32, bpb["total_sectors_32"])  # bpb_tot32
    return bytes(buf)

def fat16_checksum(bpb: dict) -> int:
    """Volume serial number as simple sum of BPB bytes."""
    # Not a real checksum; just a placeholder volume serial
    return 0x20250711

def make_fat16_image(stage1: bytes, stage2: bytes, kernel_bin: bytes) -> bytes:
    total = TOTAL_SECTORS * SECTOR_SIZE
    img = bytearray(total)

    bpb = {
        "bytes_per_sector":   SECTOR_SIZE,
        "sectors_per_cluster": SPC,
        "reserved_sectors":   RESV,
        "fat_count":          FAT_CNT,
        "root_max_entries":   ROOT_MAX,
        "total_sectors_16":   0,               # use 32-bit field
        "media_descriptor":   MEDIA,
        "fat_size_16":        FAT16_SIZE,
        "sectors_per_track":  63,
        "head_count":         16,
        "hidden_sectors":     0,
        "total_sectors_32":   TOTAL_SECTORS,
    }

    # 1. Stage1 bootsector (sector 0)
    assert len(stage1) <= SECTOR_SIZE, "stage1 too large"
    boot = build_bootsector(stage1, bpb)
    img[0:len(boot)] = boot

    # 2. Stage2 (sectors 1 .. RESV-1)
    stage2_sectors = math.ceil(len(stage2) / SECTOR_SIZE)
    assert stage2_sectors < RESV, f"stage2 too large ({len(stage2)} B = {stage2_sectors} sectors, need < {RESV})"
    img[SECTOR_SIZE:SECTOR_SIZE+len(stage2)] = stage2

    # 3. FAT16 filesystem starting at sector RESV
    fat_offset   = RESV * SECTOR_SIZE
    fat2_offset  = (RESV + FAT16_SIZE) * SECTOR_SIZE
    root_lba     = RESV + FAT_CNT * FAT16_SIZE
    root_offset  = root_lba * SECTOR_SIZE
    root_sectors = (ROOT_MAX * 32 + SECTOR_SIZE - 1) // SECTOR_SIZE
    data_offset  = (root_lba + root_sectors) * SECTOR_SIZE

    # 3a. FAT1 — mark clusters 0,1 as reserved
    fat1  = bytearray(FAT16_SIZE * SECTOR_SIZE)
    fat1[0] = MEDIA            # cluster 0: media descriptor byte
    fat1[1] = 0xFF             # cluster 0 (cont)
    fat1[2] = 0xFF; fat1[3] = 0xFF  # cluster 1: EOC marker
    # Everything else = 0 (free cluster)

    # 3b. FAT2 — identical to FAT1
    fat2 = bytearray(fat1)

    # 3c. Root directory — empty
    root_dir = bytearray(root_sectors * SECTOR_SIZE)

    # 3d. Write kernel.bin into data area
    # Find free clusters, allocate as needed
    kernel_size = len(kernel_bin)
    clusters_needed = (kernel_size + SPC * SECTOR_SIZE - 1) // (SPC * SECTOR_SIZE)
    if clusters_needed > TOTAL_CLUSTERS:
        raise RuntimeError(f"kernel too large: need {clusters_needed} clusters, max {TOTAL_CLUSTERS}")

    # Simple contiguous allocator
    start_cluster = 2
    cluster = start_cluster
    for i in range(clusters_needed):
        off = 2 + i * 2
        if i == clusters_needed - 1:
            val = 0xFFF8  # EOC
        else:
            val = cluster + 1
        struct.pack_into("<H", fat1, off, val)
        struct.pack_into("<H", fat2, off, val)

    # Write kernel data
    write_offset = data_offset
    remaining = kernel_size
    for c in range(start_cluster, start_cluster + clusters_needed):
        chunk = SPC * SECTOR_SIZE
        if chunk > remaining:
            chunk = remaining
        img[write_offset:write_offset+chunk] = kernel_bin[:chunk]
        kernel_bin = kernel_bin[chunk:]
        write_offset += SPC * SECTOR_SIZE
        remaining -= chunk

    # Create directory entry for KERNEL.BIN
    # FAT name: KERNEL  BIN (11 bytes, uppercase, space-padded)
    fat_filename = bytearray(b"KERNEL  BIN")
    entry = bytearray(32)
    entry[0:11] = fat_filename
    entry[11] = 0x20  # archive attribute
    struct.pack_into("<H", entry, 26, start_cluster)  # cluster_lo
    struct.pack_into("<I", entry, 28, kernel_size)     # file_size

    # Insert into root directory
    root_dir[0:32] = entry

    # 4. Assemble image
    img[fat_offset:fat_offset+len(fat1)] = fat1
    img[fat2_offset:fat2_offset+len(fat2)] = fat2
    img[root_offset:root_offset+len(root_dir)] = root_dir

    return bytes(img)

def main():
    parser = argparse.ArgumentParser(description="Build FAT16 bootable disk image")
    parser.add_argument("--stage1", required=True, help="stage1.bin")
    parser.add_argument("--stage2", required=True, help="stage2.bin")
    parser.add_argument("--kernel", required=True, help="kernel.bin")
    parser.add_argument("--output", required=True, help="output image file")
    args = parser.parse_args()

    stage1 = open(args.stage1, "rb").read()
    stage2 = open(args.stage2, "rb").read()
    kernel = open(args.kernel, "rb").read()

    img = make_fat16_image(stage1, stage2, kernel)

    with open(args.output, "wb") as f:
        f.write(img)

    sys.stderr.write(f"Wrote {len(img)} bytes to {args.output}\n")
    sys.stderr.write(f"  stage1: {len(stage1)} B, stage2: {len(stage2)} B, kernel: {len(kernel)} B\n")

if __name__ == "__main__":
    main()
