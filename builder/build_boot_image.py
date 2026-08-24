#!/usr/bin/env python3
"""
Build a Limine-bootable GPT disk image with FAT16 ESP.
Uses the `limine` crate's protocol (limine-bootloader v12.x).
"""
import struct, os, sys, math, argparse, binascii, uuid

SECTOR_SIZE = 512

def align_up(v, a):
    return ((v + a - 1) // a) * a

def gpt_crc32(data):
    return binascii.crc32(data) & 0xFFFFFFFF

def efi_guid(s):
    p = s.split('-')
    return (struct.pack('<IHH', int(p[0], 16), int(p[1], 16), int(p[2], 16))
            + bytes.fromhex(p[3]) + bytes.fromhex(p[4]))

EFI_SYSTEM_GUID = "C12A7328-F81F-11D2-BA4B-00A0C93EC93B"


class FAT16Image:
    """Minimal FAT16 image builder with proper directory nesting."""

    def __init__(self, total_sectors):
        self.image = bytearray(total_sectors * SECTOR_SIZE)
        self.spc = 4                        # sectors per cluster
        self.cluster_size = self.spc * SECTOR_SIZE
        self.reserved = 1
        self.num_fats = 2
        self.root_entries = 512
        self.root_sectors = (self.root_entries * 32 + SECTOR_SIZE - 1) // SECTOR_SIZE

        # Determine FAT size: FAT entries must cover all data clusters
        for fs in range(1, 256):
            data_area = total_sectors - self.reserved - self.num_fats * fs - self.root_sectors
            num_clusters = data_area // self.spc
            if num_clusters < 1:
                continue
            # Each FAT16 entry is 2 bytes; 2 reserved entries at start
            needed = (num_clusters + 2) * 2
            if fs * SECTOR_SIZE >= needed:
                self.fat_size = fs
                break
        else:
            self.fat_size = 32

        self.data_start = self.reserved + self.num_fats * self.fat_size + self.root_sectors
        self.total_clusters = (total_sectors - self.data_start) // self.spc
        self.fat = [0] * (self.total_clusters + 2)
        self.fat[0] = 0xFFF8   # media type
        self.fat[1] = 0xFFFF   # end-of-chain
        self.next_free = 2
        self.dir_id = 0        # unique IDs for . and .. entries

    def alloc_cluster(self):
        c = self.next_free
        self.next_free += 1
        return c

    def alloc_chain(self, size):
        """Allocate a cluster chain for `size` bytes, return first cluster."""
        n = max(1, (size + self.cluster_size - 1) // self.cluster_size)
        first = self.alloc_cluster()
        prev = first
        for _ in range(n - 1):
            c = self.alloc_cluster()
            self.fat[prev] = c
            prev = c
        self.fat[prev] = 0xFFFF
        return first

    def cluster_to_sector(self, c):
        return self.data_start + (c - 2) * self.spc

    def write_chain(self, first, data):
        """Write `data` into the cluster chain starting at `first`."""
        c = first
        off = 0
        while off < len(data) and c >= 2:
            sector = self.cluster_to_sector(c)
            byte_off = sector * SECTOR_SIZE
            chunk = data[off:off + self.cluster_size]
            self.image[byte_off:byte_off + len(chunk)] = chunk
            off += self.cluster_size
            if self.fat[c] == 0xFFFF:
                break
            c = self.fat[c]

    def make83(self, name):
        """Convert a filename to 8.3 format."""
        if '.' in name:
            base, ext = name.rsplit('.', 1)
        else:
            base, ext = name, ''
        b = base.upper().encode('ascii').ljust(8, b' ')[:8]
        e = ext.upper().encode('ascii').ljust(3, b' ')[:3]
        return b + e

    def make_dir_entry(self, name83, attr, cluster, size):
        """Create a 32-byte FAT16 directory entry."""
        e = bytearray(32)
        e[0:11] = name83
        e[11] = attr
        e[20:22] = struct.pack('<H', (cluster >> 16) & 0xFFFF)
        e[26:28] = struct.pack('<H', cluster & 0xFFFF)
        e[28:32] = struct.pack('<I', size)
        return bytes(e)

    # ─── Tree API ───────────────────────────────────────────────

    def __init_tree(self):
        return {'__type': 'dir', '__children': {}}

    def add_file(self, tree, parent, name, data):
        """Add a file to the tree under `parent` path."""
        # Navigate to parent
        node = tree
        for part in parent.strip('/').split('/'):
            if part:
                node = node['__children'][part]
        node['__children'][name] = {
            '__type': 'file',
            '__data': data,
            '__children': {},
        }

    def add_dir(self, tree, parent, name):
        """Add a directory to the tree under `parent` path."""
        node = tree
        for part in parent.strip('/').split('/'):
            if part:
                node = node['__children'][part]
        key = name
        if key not in node['__children']:
            node['__children'][key] = {
                '__type': 'dir',
                '__children': {},
            }

    # ─── Allocation passes ──────────────────────────────────────

    def alloc_files(self, node, file_map):
        """Allocate clusters for all files in the tree."""
        for name, child in node['__children'].items():
            if child['__type'] == 'file':
                data = child['__data']
                child['__cluster'] = self.alloc_chain(len(data))
                self.write_chain(child['__cluster'], data)
                file_map[id(child)] = child['__cluster']
            else:
                self.alloc_files(child, file_map)

    def alloc_dirs(self, node, dir_map):
        """Allocate clusters for all directories (not root)."""
        for name, child in node['__children'].items():
            if child['__type'] == 'dir':
                # Allocate one cluster for this directory's entries
                child['__cluster'] = self.alloc_chain(self.cluster_size)
                dir_map[id(child)] = child['__cluster']
                self.alloc_dirs(child, dir_map)

    def write_dir(self, node, dir_map):
        """Write directory entries for all subdirectories."""
        for name, child in node['__children'].items():
            if child['__type'] == 'dir':
                entries = bytearray()
                my_cluster = child['__cluster']

                # . entry (points to itself)
                entries += self.make_dir_entry(
                    b'.          ', 0x10, my_cluster, 0)

                # .. entry (points to parent)
                parent_cluster = node.get('__cluster', 0)
                entries += self.make_dir_entry(
                    b'..         ', 0x10, parent_cluster, 0)

                # Child entries
                for cname, cchild in sorted(child['__children'].items()):
                    if cchild['__type'] == 'dir':
                        entries += self.make_dir_entry(
                            self.make83(cname), 0x10,
                            cchild['__cluster'], 0)
                    else:
                        entries += self.make_dir_entry(
                            self.make83(cname), 0x20,
                            cchild.get('__cluster', 0),
                            len(cchild['__data']))

                # Pad to cluster size
                while len(entries) < self.cluster_size:
                    entries += b'\x00' * 32

                self.write_chain(my_cluster, bytes(entries))
                # Recurse
                self.write_dir(child, dir_map)

    # ─── BPB / FAT writes ───────────────────────────────────────

    def write_bpb(self):
        bs = bytearray(SECTOR_SIZE)
        bs[0:3] = b'\xEB\x3C\x90'            # jump
        bs[3:11] = b'MSWIN4.1'                # OEM ID
        struct.pack_into('<H', bs, 11, 512)   # bytes per sector
        bs[13] = self.spc                      # sectors per cluster
        struct.pack_into('<H', bs, 14, self.reserved)
        bs[16] = self.num_fats
        struct.pack_into('<H', bs, 17, self.root_entries)
        struct.pack_into('<H', bs, 19, 0)     # total sectors (16-bit, 0 → use 32-bit)
        bs[21] = 0xF8                          # media descriptor
        struct.pack_into('<H', bs, 22, self.fat_size)
        struct.pack_into('<H', bs, 24, 32)    # sectors per track
        struct.pack_into('<H', bs, 26, 64)    # heads
        struct.pack_into('<I', bs, 28, 0)     # hidden sectors
        struct.pack_into('<I', bs, 32, 0)     # total sectors (32-bit, 0 → computed)
        bs[36] = 0x80                          # drive number
        bs[38] = 0x29                          # extended boot signature
        struct.pack_into('<I', bs, 39, 0x12345678)  # volume serial
        bs[43:54] = b'VAHI OS    '             # volume label
        bs[54:62] = b'FAT16   '                # filesystem type
        struct.pack_into('<H', bs, 510, 0xAA55)
        self.image[0:SECTOR_SIZE] = bs

    def write_fats(self):
        for fat_num in range(self.num_fats):
            off = (self.reserved + fat_num * self.fat_size) * SECTOR_SIZE
            for i in range(min(self.total_clusters + 2, len(self.fat))):
                struct.pack_into('<H', self.image, off + i * 2, self.fat[i] & 0xFFFF)

    def write_root_dir(self, root_node):
        """Write the FAT16 root directory (fixed location)."""
        root_start = self.reserved + self.num_fats * self.fat_size
        byte_off = root_start * SECTOR_SIZE
        entries = bytearray()

        # . entry for root
        entries += self.make_dir_entry(b'.          ', 0x10, 0, 0)

        for name, child in sorted(root_node['__children'].items()):
            if child['__type'] == 'dir':
                entries += self.make_dir_entry(
                    self.make83(name), 0x10, child['__cluster'], 0)
            else:
                entries += self.make_dir_entry(
                    self.make83(name), 0x20,
                    child.get('__cluster', 0),
                    len(child['__data']))

        self.image[byte_off:byte_off + len(entries)] = entries


def build_image(kernel_path, output_path, esp_mb=64):
    esp_sectors = (esp_mb * 1024 * 1024) // SECTOR_SIZE

    with open(kernel_path, 'rb') as f:
        kernel = f.read()
    print(f"Kernel: {len(kernel)} bytes")

    initrd = None
    initrd_path = 'initrd.tar'
    if os.path.exists(initrd_path):
        with open(initrd_path, 'rb') as f:
            initrd = f.read()
        print(f"Initrd: {len(initrd)} bytes")

    # Limine v12 config: entry name starts with /
    conf = b"serial: yes\nTIMEOUT=0\n\n/SkyOS\n    PROTOCOL=limine\n    KERNEL_PATH=boot:///vahi_kernel\n"
    if initrd:
        conf += b"    MODULE_PATH=boot:///initrd.tar\n    MODULE_CMDLINE=initrd\n"

    # Find Limine binaries
    limine_dir = None
    for candidate in [
        os.path.join(os.environ.get('TEMP', '/tmp'), 'limine-binary'),
        '/tmp/limine-binary',
    ]:
        if os.path.exists(os.path.join(candidate, 'BOOTX64.EFI')):
            limine_dir = candidate
            break
    if not limine_dir:
        print("ERROR: Limine bootloader not found. Run the Limine setup first.")
        sys.exit(1)

    with open(os.path.join(limine_dir, 'BOOTX64.EFI'), 'rb') as f:
        bootx64 = f.read()
    with open(os.path.join(limine_dir, 'BOOTIA32.EFI'), 'rb') as f:
        bootia32 = f.read()

    # ── Build FAT16 ESP ──
    print("Building FAT16 ESP...")
    fat = FAT16Image(esp_sectors)

    # Build file tree
    tree = {'__type': 'dir', '__children': {}, '__cluster': 0}  # root cluster = 0 (fixed)
    fat.add_dir(tree, '/', 'EFI')
    fat.add_dir(tree, '/EFI', 'BOOT')
    fat.add_file(tree, '/EFI/BOOT', 'BOOTX64.EFI', bootx64)
    fat.add_file(tree, '/EFI/BOOT', 'BOOTIA32.EFI', bootia32)
    # startup.nsh: UEFI shell auto-boots Limine (use absolute device path)
    startup_nsh = b"FS0:\\EFI\\BOOT\\BOOTX64.EFI\n"
    fat.add_file(tree, '/', 'startup.nsh', startup_nsh)
    fat.add_file(tree, '/', 'limine.conf', conf)
    fat.add_file(tree, '/', 'vahi_kernel', kernel)
    if initrd:
        fat.add_file(tree, '/', 'initrd.tar', initrd)

    # Allocate and write
    file_map = {}
    fat.alloc_files(tree, file_map)
    fat.alloc_dirs(tree, {})
    fat.write_dir(tree, {})
    fat.write_root_dir(tree)
    fat.write_bpb()
    fat.write_fats()

    esp_data = bytes(fat.image)

    # ── GPT Layout ──
    esp_start = 34    # first usable LBA
    esp_end = esp_start + esp_sectors - 1
    backup_entries_start = esp_end + 34   # 32 sectors for backup entries
    total_sectors = backup_entries_start + 33  # 32 entries + 1 backup header

    disk = bytearray(total_sectors * SECTOR_SIZE)

    # Copy ESP data
    off = esp_start * SECTOR_SIZE
    disk[off:off + len(esp_data)] = esp_data

    # Protective MBR
    mbr = bytearray(SECTOR_SIZE)
    mbr[446 + 4] = 0xEE          # GPT protective
    struct.pack_into('<I', mbr, 446 + 8, 1)
    struct.pack_into('<I', mbr, 446 + 12, total_sectors - 1)
    struct.pack_into('<H', mbr, 510, 0xAA55)
    disk[0:SECTOR_SIZE] = mbr

    # Partition entries (primary at LBA 2, backup at backup_entries_start)
    entries = bytearray(128 * 128)  # 128 entries * 128 bytes each

    esp_entry = bytearray(128)
    esp_entry[0:16] = efi_guid(EFI_SYSTEM_GUID)
    esp_entry[16:32] = uuid.uuid4().bytes
    struct.pack_into('<Q', esp_entry, 32, esp_start)
    struct.pack_into('<Q', esp_entry, 40, esp_end)
    struct.pack_into('<Q', esp_entry, 48, 0)   # attributes (0 = no special flags)
    name = 'EFI System'.encode('utf-16-le')
    esp_entry[56:56 + len(name)] = name
    entries[0:128] = esp_entry

    disk[2 * SECTOR_SIZE:34 * SECTOR_SIZE] = entries
    disk[backup_entries_start * SECTOR_SIZE:(backup_entries_start + 32) * SECTOR_SIZE] = entries

    entries_crc = gpt_crc32(bytes(entries))

    # Primary GPT header (at LBA 1)
    ph = bytearray(SECTOR_SIZE)
    ph[0:8] = b'EFI PART'
    struct.pack_into('<I', ph, 8, 0x00010000)       # revision 1.0
    struct.pack_into('<I', ph, 12, 92)               # header size
    struct.pack_into('<I', ph, 16, 0)                # CRC32 (set last)
    struct.pack_into('<Q', ph, 24, 1)                # my_lba
    struct.pack_into('<Q', ph, 32, total_sectors - 1) # alternate_lba
    struct.pack_into('<Q', ph, 40, esp_start)        # first_usable_lba
    struct.pack_into('<Q', ph, 48, backup_entries_start - 1) # last_usable_lba
    ph[56:72] = efi_guid(EFI_SYSTEM_GUID)            # disk GUID
    struct.pack_into('<Q', ph, 72, 2)                # partition entries LBA
    struct.pack_into('<I', ph, 80, 128)              # num partition entries
    struct.pack_into('<I', ph, 84, 128)              # partition entry size
    struct.pack_into('<I', ph, 88, entries_crc)      # partition entries CRC32
    struct.pack_into('<I', ph, 16, gpt_crc32(bytes(ph[:92])))
    disk[SECTOR_SIZE:2 * SECTOR_SIZE] = ph

    # Backup GPT header (at last LBA)
    bh = bytearray(ph)
    struct.pack_into('<Q', bh, 24, total_sectors - 1) # my_lba
    struct.pack_into('<Q', bh, 32, 1)                 # alternate_lba
    struct.pack_into('<Q', bh, 72, backup_entries_start)
    struct.pack_into('<I', bh, 16, 0)
    struct.pack_into('<I', bh, 16, gpt_crc32(bytes(bh[:92])))
    disk[total_sectors * SECTOR_SIZE - SECTOR_SIZE:total_sectors * SECTOR_SIZE] = bh

    with open(output_path, 'wb') as f:
        f.write(disk)
    print(f"Wrote {output_path} ({len(disk)} bytes, {len(disk) / 1024 / 1024:.1f} MiB)")
    print(f"  ESP: LBA {esp_start}-{esp_end} ({esp_sectors} sectors, FAT16)")


def main():
    p = argparse.ArgumentParser(description="Build a Limine-bootable GPT disk image")
    p.add_argument('--kernel', default='kernel/target/x86_64-unknown-none/release/vahi_kernel')
    p.add_argument('--output', default='bootimage-vahi_kernel.bin')
    p.add_argument('--esp-size', type=int, default=64,
                   help='ESP size in MB (default 64)')
    args = p.parse_args()

    if not os.path.exists(args.kernel):
        alt = 'kernel/target/x86_64-unknown-none/debug/vahi_kernel'
        if os.path.exists(alt):
            args.kernel = alt
        else:
            print("Kernel not found. Build with: cargo build --release")
            sys.exit(1)

    build_image(args.kernel, args.output, args.esp_size)


if __name__ == '__main__':
    main()
