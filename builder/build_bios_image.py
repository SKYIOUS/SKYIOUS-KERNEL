#!/usr/bin/env python3
"""
Build a BIOS-bootable disk image with Limine.
Uses Limine's BIOS bootloader (not UEFI).
"""
import struct, os, sys, argparse, subprocess, shutil

SECTOR_SIZE = 512


class Fat16Image:
    """Minimal FAT16 for Limine BIOS boot."""

    def __init__(self, total_sectors):
        self.image = bytearray(total_sectors * SECTOR_SIZE)
        self.spc = 4
        self.cluster_size = self.spc * SECTOR_SIZE
        self.reserved = 1
        self.num_fats = 2
        self.root_entries = 512
        self.root_sectors = (self.root_entries * 32 + SECTOR_SIZE - 1) // SECTOR_SIZE
        self.fat_size = 1  # Will be computed
        for fs in range(1, 256):
            data_area = total_sectors - self.reserved - self.num_fats * fs - self.root_sectors
            num_clusters = data_area // self.spc
            if num_clusters < 1:
                continue
            if fs * SECTOR_SIZE >= (num_clusters + 2) * 2:
                self.fat_size = fs
                break
        self.data_start = self.reserved + self.num_fats * self.fat_size + self.root_sectors
        self.total_clusters = (total_sectors - self.data_start) // self.spc
        self.fat = [0] * (self.total_clusters + 2)
        self.fat[0] = 0xFFF8
        self.fat[1] = 0xFFFF
        self.next_free = 2
        self.tree = {'__type': 'dir', '__children': {}, '__cluster': 0}

    def alloc_cluster(self):
        c = self.next_free
        self.next_free += 1
        return c

    def alloc_chain(self, size):
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
        c = first
        off = 0
        while off < len(data) and c >= 2:
            s = self.cluster_to_sector(c) * SECTOR_SIZE
            chunk = data[off:off + self.cluster_size]
            self.image[s:s + len(chunk)] = chunk
            off += self.cluster_size
            if self.fat[c] == 0xFFFF:
                break
            c = self.fat[c]

    def make83(self, name):
        if '.' in name:
            base, ext = name.rsplit('.', 1)
        else:
            base, ext = name, ''
        return base.upper().encode('ascii').ljust(8, b' ')[:8] + ext.upper().encode('ascii').ljust(3, b' ')[:3]

    @staticmethod
    def lfn_checksum(name83):
        """Compute the checksum for an 8.3 name used in LFN entries."""
        s = 0
        for b in name83:
            s = ((s >> 1) + ((s & 1) << 7) + b) & 0xFF
        return s

    def make_lfn_entries(self, long_name, name83):
        """Create LFN directory entries for a long filename."""
        # Encode long name as UTF-16LE, pad to multiple of 13
        utf16 = long_name.encode('utf-16-le') + b'\x00\x00'
        padded = utf16.ljust(((len(utf16) + 25) // 26) * 26, b'\x00')
        chunks = [padded[i:i+26] for i in range(0, len(padded), 26)]
        entries = []
        cksum = self.lfn_checksum(name83)
        for idx, chunk in enumerate(reversed(chunks)):
            seq = idx + 1
            if idx == len(chunks) - 1:
                seq |= 0x40  # Last entry marker
            e = bytearray(32)
            e[0] = seq
            e[11] = 0x0F  # LFN attribute
            e[12] = cksum
            # Name characters 1-5 (offset 1, 14 bytes)
            name1 = chunk[0:10]
            e[1:11] = name1.ljust(10, b'\x00')
            # Name characters 6-11 (offset 14, 12 bytes)
            name2 = chunk[10:22]
            e[14:26] = name2.ljust(12, b'\x00')
            # Name characters 12-13 (offset 28, 4 bytes)
            name3 = chunk[22:26]
            e[28:32] = name3.ljust(4, b'\x00')
            entries.append(bytes(e))
        return entries

    def make_dir_entry(self, name83, attr, cluster, size):
        e = bytearray(32)
        e[0:11] = name83
        e[11] = attr
        e[20:22] = struct.pack('<H', (cluster >> 16) & 0xFFFF)
        e[26:28] = struct.pack('<H', cluster & 0xFFFF)
        e[28:32] = struct.pack('<I', size)
        return bytes(e)

    def add_dir(self, parent, name):
        node = self.tree
        for part in parent.strip('/').split('/'):
            if part:
                node = node['__children'][part]
        if name not in node['__children']:
            node['__children'][name] = {'__type': 'dir', '__children': {}}

    def add_file(self, parent, name, data):
        node = self.tree
        for part in parent.strip('/').split('/'):
            if part:
                node = node['__children'][part]
        node['__children'][name] = {'__type': 'file', '__data': data, '__children': {}}

    def alloc_files(self, node):
        for name, child in node['__children'].items():
            if child['__type'] == 'file':
                child['__cluster'] = self.alloc_chain(len(child['__data']))
                self.write_chain(child['__cluster'], child['__data'])
            else:
                self.alloc_files(child)

    def alloc_dirs(self, node):
        for name, child in node['__children'].items():
            if child['__type'] == 'dir':
                child['__cluster'] = self.alloc_chain(self.cluster_size)
                self.alloc_dirs(child)

    def _emit_entry(self, entries, long_name, name83, attr, cluster, size):
        """Emit LFN + 8.3 entry. LFN entries precede the 8.3 entry."""
        if long_name != name83.decode('ascii').strip():
            for lfn_e in self.make_lfn_entries(long_name, name83):
                entries += lfn_e
        entries += self.make_dir_entry(name83, attr, cluster, size)

    def write_dir(self, node):
        for name, child in node['__children'].items():
            if child['__type'] == 'dir':
                entries = bytearray()
                my_cl = child['__cluster']
                entries += self.make_dir_entry(b'.          ', 0x10, my_cl, 0)
                parent_cl = node.get('__cluster', 0)
                entries += self.make_dir_entry(b'..         ', 0x10, parent_cl, 0)
                for cn, cc in sorted(child['__children'].items()):
                    name83 = self.make83(cn)
                    if cc['__type'] == 'dir':
                        self._emit_entry(entries, cn, name83, 0x10, cc['__cluster'], 0)
                    else:
                        self._emit_entry(entries, cn, name83, 0x20, cc.get('__cluster', 0), len(cc['__data']))
                while len(entries) < self.cluster_size:
                    entries += b'\x00' * 32
                self.write_chain(my_cl, bytes(entries))
                self.write_dir(child)

    def write_root_dir(self, root):
        off = (self.reserved + self.num_fats * self.fat_size) * SECTOR_SIZE
        entries = bytearray()
        entries += self.make_dir_entry(b'.          ', 0x10, 0, 0)
        for name, child in sorted(root['__children'].items()):
            name83 = self.make83(name)
            if child['__type'] == 'dir':
                self._emit_entry(entries, name, name83, 0x10, child['__cluster'], 0)
            else:
                self._emit_entry(entries, name, name83, 0x20, child.get('__cluster', 0), len(child['__data']))
        self.image[off:off + len(entries)] = entries

    def write_bpb(self):
        bs = bytearray(SECTOR_SIZE)
        bs[0:3] = b'\xEB\x3C\x90'
        bs[3:11] = b'MSWIN4.1'
        struct.pack_into('<H', bs, 11, 512)
        bs[13] = self.spc
        struct.pack_into('<H', bs, 14, self.reserved)
        bs[16] = self.num_fats
        struct.pack_into('<H', bs, 17, self.root_entries)
        struct.pack_into('<H', bs, 19, 0)
        bs[21] = 0xF8
        struct.pack_into('<H', bs, 22, self.fat_size)
        struct.pack_into('<H', bs, 24, 32)
        struct.pack_into('<H', bs, 26, 64)
        struct.pack_into('<I', bs, 28, 0)
        struct.pack_into('<I', bs, 32, 0)
        bs[36] = 0x80
        bs[38] = 0x29
        struct.pack_into('<I', bs, 39, 0x12345678)
        bs[43:54] = b'VAHI OS    '
        bs[54:62] = b'FAT16   '
        struct.pack_into('<H', bs, 510, 0xAA55)
        self.image[0:SECTOR_SIZE] = bs

    def write_fats(self):
        for fn in range(self.num_fats):
            off = (self.reserved + fn * self.fat_size) * SECTOR_SIZE
            for i in range(min(self.total_clusters + 2, len(self.fat))):
                struct.pack_into('<H', self.image, off + i * 2, self.fat[i] & 0xFFFF)

    def finalize(self):
        self.alloc_files(self.tree)
        self.alloc_dirs(self.tree)
        self.write_dir(self.tree)
        self.write_root_dir(self.tree)
        self.write_bpb()
        self.write_fats()
        return bytes(self.image)


def build_image(kernel_path, output_path, esp_mb=64):
    esp_sectors = (esp_mb * 1024 * 1024) // SECTOR_SIZE

    with open(kernel_path, 'rb') as f:
        kernel = f.read()
    print(f"Kernel: {len(kernel)} bytes")

    initrd = None
    if os.path.exists('initrd.tar'):
        with open('initrd.tar', 'rb') as f:
            initrd = f.read()
        print(f"Initrd: {len(initrd)} bytes")

    # Limine BIOS config
    conf = b"TIMEOUT=0\n\n/SkyOS\n    PROTOCOL=limine\n    KERNEL_PATH=boot:///vahi_kernel\n"
    if initrd:
        conf += b"    MODULE_PATH=boot:///initrd.tar\n    MODULE_CMDLINE=initrd\n"

    # Build FAT16 with Limine BIOS files + kernel
    print("Building FAT16...")
    fat = Fat16Image(esp_sectors)

    # Add Limine BIOS files
    # On Windows, /tmp maps to the Windows temp directory
    import tempfile
    limine_dir = os.path.join(tempfile.gettempdir(), 'limine-binary')
    # BIOS bootloader searches for 'limine-bios.sys' by LFN name
    # 8.3 name will be LIMINE-BSYS (truncated), LFN entry provides full name
    with open(os.path.join(limine_dir, 'limine-bios.sys'), 'rb') as f:
        fat.add_file('/', 'limine-bios.sys', f.read())
    with open(os.path.join(limine_dir, 'limine-bios-cd.bin'), 'rb') as f:
        fat.add_file('/', 'limine-cd.bin', f.read())

    fat.add_file('/', 'limine.conf', conf)
    fat.add_file('/', 'vahi_kernel', kernel)
    if initrd:
        fat.add_file('/', 'initrd.tar', initrd)

    esp_data = fat.finalize()

    # Create disk image: MBR + FAT16 partition
    # MBR: 1 sector, partition starts at sector 2048 (1MB alignment)
    mbr_start = 0
    part_start = 2048  # partition starts at LBA 2048
    part_sectors = esp_sectors

    total_sectors = part_start + part_sectors
    disk = bytearray(total_sectors * SECTOR_SIZE)

    # Write FAT16 at partition offset
    disk[part_start * SECTOR_SIZE:part_start * SECTOR_SIZE + len(esp_data)] = esp_data

    # Write MBR partition table entry
    mbr = bytearray(SECTOR_SIZE)
    # Partition 1: FAT32/LBA (type 0x0C) at LBA part_start
    mbr[446 + 4] = 0x0C  # Type: FAT32 LBA
    struct.pack_into('<I', mbr, 446 + 8, part_start)  # LBA start
    struct.pack_into('<I', mbr, 446 + 12, part_sectors)  # LBA count
    struct.pack_into('<H', mbr, 510, 0xAA55)  # Boot signature
    # Boot indicator: 0x80 = active
    mbr[446] = 0x80
    disk[0:SECTOR_SIZE] = mbr

    with open(output_path, 'wb') as f:
        f.write(disk)
    print(f"Wrote {output_path} ({len(disk)} bytes, {len(disk) / 1024 / 1024:.1f} MiB)")

    # Install Limine BIOS bootloader
    print("Installing Limine BIOS bootloader...")
    limine_exe = os.path.join(limine_dir, 'limine-tool-windows-x86', 'limine.exe')
    if os.path.exists(limine_exe):
        result = subprocess.run(
            [limine_exe, 'bios-install', output_path],
            capture_output=True, text=True, timeout=30
        )
        print(f"  stdout: {result.stdout}")
        if result.stderr:
            print(f"  stderr: {result.stderr}")
        print(f"  exit code: {result.returncode}")
    else:
        print(f"  WARNING: {limine_exe} not found, trying PATH...")
        result = subprocess.run(
            ['limine', 'bios-install', output_path],
            capture_output=True, text=True, timeout=30
        )
        print(f"  stdout: {result.stdout}")
        if result.stderr:
            print(f"  stderr: {result.stderr}")
        print(f"  exit code: {result.returncode}")


def main():
    p = argparse.ArgumentParser(description="Build a BIOS-bootable Limine disk image")
    p.add_argument('--kernel', default='target/x86_64-unknown-none/release/vahi_kernel')
    p.add_argument('--output', default='bootimage-bios.bin')
    p.add_argument('--esp-size', type=int, default=64)
    args = p.parse_args()

    if not os.path.exists(args.kernel):
        alt = 'kernel/target/x86_64-unknown-none/release/vahi_kernel'
        if os.path.exists(alt):
            args.kernel = alt
        else:
            print("Kernel not found. Build with: cargo build --release")
            sys.exit(1)

    build_image(args.kernel, args.output, args.esp_size)


if __name__ == '__main__':
    main()
