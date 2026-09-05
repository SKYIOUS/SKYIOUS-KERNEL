#!/usr/bin/env python3
"""Build a Limine-bootable MBR+FAT32 disk image for UEFI boot.

Uses a simple MBR partition table instead of GPT to avoid GPT formatting issues.
The ESP partition starts at sector 2048 and is FAT32 LBA (type 0x0C).
"""
import struct, os, sys, math

SECTOR = 512
CLUSTER_SIZE = 4096
SPC = CLUSTER_SIZE // SECTOR  # 8
RESERVED = 32
NUM_FATS = 2
ROOT_CLUSTER = 2


def align_up(v, a):
    return ((v + a - 1) // a) * a


class FAT32:
    def __init__(self, sectors, hidden=0):
        self.sectors = sectors
        self.hidden = hidden
        self.image = bytearray(sectors * SECTOR)
        data_sectors = sectors - RESERVED
        total_clusters = data_sectors // SPC
        self.fat_size = align_up((total_clusters + 2) * 4, SECTOR) // SECTOR
        self.data_start = RESERVED + NUM_FATS * self.fat_size
        self.total_clusters = (sectors - self.data_start) // SPC
        self.fat = [0] * (self.total_clusters + 2)
        self.fat[0] = 0x0FFFFFF8
        self.fat[1] = 0x0FFFFFFF
        self.fat[ROOT_CLUSTER] = 0x0FFFFFFF  # root dir = single cluster
        self.next_free = ROOT_CLUSTER + 1
        self.dirs = {ROOT_CLUSTER: []}

    def alloc(self, count):
        clusters = []
        while len(clusters) < count:
            clusters.append(self.next_free)
            self.fat[self.next_free] = 0x0FFFFFFF  # single-cluster for now
            self.next_free += 1
        for i in range(len(clusters) - 1):
            self.fat[clusters[i]] = clusters[i + 1]
        self.fat[clusters[-1]] = 0x0FFFFFFF
        return clusters

    def _sector(self, cluster):
        return self.data_start + (cluster - 2) * SPC

    def write_cluster(self, cluster, data):
        s = self._sector(cluster)
        self.image[s * SECTOR:s * SECTOR + len(data)] = data

    def _83(self, name):
        if '.' in name:
            b, e = name.rsplit('.', 1)
        else:
            b, e = name, ''
        return b.upper().encode().ljust(8, b' ')[:8] + e.upper().encode().ljust(3, b' ')[:3]

    def _dir_entry(self, name, attr, cluster, size):
        e = bytearray(32)
        e[0:11] = self._83(name)
        e[11] = attr
        struct.pack_into('<H', e, 20, (cluster >> 16) & 0xFFFF)
        struct.pack_into('<H', e, 26, cluster & 0xFFFF)
        struct.pack_into('<I', e, 28, size)
        return bytes(e)

    def mkdir(self, parent, name):
        c = self.alloc(1)[0]
        dot = self._dir_entry('.', 0x10, c, 0)
        dotdot = self._dir_entry('..', 0x10, parent, 0)
        self.write_cluster(c, dot + dotdot + b'\x00' * (CLUSTER_SIZE - 64))
        self.dirs[parent].append(self._dir_entry(name, 0x10, c, 0))
        self.dirs[c] = []
        return c

    def add_file(self, parent, name, data):
        clusters = self.alloc(max(1, align_up(len(data), CLUSTER_SIZE) // CLUSTER_SIZE))
        for i, c in enumerate(clusters):
            chunk = data[i * CLUSTER_SIZE:(i + 1) * CLUSTER_SIZE]
            self.write_cluster(c, chunk.ljust(CLUSTER_SIZE, b'\x00'))
        self.dirs[parent].append(self._dir_entry(name, 0x20, clusters[0], len(data)))

    def finalize(self):
        # Write directory contents
        for c, entries in self.dirs.items():
            data = b''.join(entries).ljust(CLUSTER_SIZE, b'\x00')
            self.write_cluster(c, data)

        # Boot sector
        b = bytearray(SECTOR)
        b[0:3] = b'\xEB\x58\x90'
        b[3:11] = b'MSWIN4.1'
        struct.pack_into('<H', b, 11, SECTOR)
        b[13] = SPC
        struct.pack_into('<H', b, 14, RESERVED)
        b[16] = NUM_FATS
        b[21] = 0xF8
        struct.pack_into('<I', b, 28, self.hidden)  # hidden
        struct.pack_into('<I', b, 32, self.sectors)
        struct.pack_into('<I', b, 36, self.fat_size)
        struct.pack_into('<H', b, 40, 0)
        struct.pack_into('<H', b, 42, 0)
        struct.pack_into('<I', b, 44, ROOT_CLUSTER)
        struct.pack_into('<H', b, 48, 1)
        struct.pack_into('<H', b, 50, 6)
        b[64] = 0x80
        b[66] = 0x29
        struct.pack_into('<I', b, 67, 0xDEADBEEF)
        b[71:82] = b'SKYIOUS    '
        b[82:90] = b'FAT32   '
        struct.pack_into('<H', b, 510, 0xAA55)
        self.image[0:SECTOR] = b

        # FSInfo sector 1
        fi = bytearray(SECTOR)
        fi[0:4] = b'RRaA'
        struct.pack_into('<I', fi, 484, 0x61417272)
        struct.pack_into('<I', fi, 488, 0xFFFFFFFF)
        struct.pack_into('<I', fi, 492, 0xFFFFFFFF)
        self.image[SECTOR:2 * SECTOR] = fi

        # Backup boot + FSInfo
        self.image[6 * SECTOR:7 * SECTOR] = self.image[0:SECTOR]
        self.image[7 * SECTOR:8 * SECTOR] = self.image[SECTOR:2 * SECTOR]

        # FATs
        fat_bytes = bytearray()
        for i in range(self.total_clusters + 2):
            fat_bytes += struct.pack('<I', self.fat[i] & 0x0FFFFFFF)
        fat_bytes = fat_bytes.ljust(self.fat_size * SECTOR, b'\x00')
        for n in range(NUM_FATS):
            off = (RESERVED + n * self.fat_size) * SECTOR
            self.image[off:off + len(fat_bytes)] = fat_bytes

        return bytes(self.image)


def main():
    kernel_path = sys.argv[1] if len(sys.argv) > 1 else 'kernel/target/x86_64-unknown-none/release/vahi_kernel'
    limine_dir = sys.argv[2] if len(sys.argv) > 2 else '/tmp/limine-binary'
    output = sys.argv[3] if len(sys.argv) > 3 else 'bootimage-vahi_kernel.bin'

    with open(kernel_path, 'rb') as f:
        kernel_data = f.read()
    print(f'Kernel: {len(kernel_data)} bytes')

    with open(os.path.join(limine_dir, 'BOOTX64.EFI'), 'rb') as f:
        efi_data = f.read()
    print(f'EFI loader: {len(efi_data)} bytes')

    bios_sys = None
    bios_path = os.path.join(limine_dir, 'limine-bios.sys')
    if os.path.exists(bios_path):
        with open(bios_path, 'rb') as f:
            bios_sys = f.read()
        print(f'BIOS loader: {len(bios_sys)} bytes')

    limine_conf = b'TIMEOUT=0\n:SkyOS\n    PROTOCOL=limine\n    KERNEL_PATH=boot:///vahi_kernel\n'
    if os.path.exists('initrd.tar'):
        with open('initrd.tar', 'rb') as f:
            initrd_data = f.read()
        limine_conf += b'    MODULE_PATH=boot:///initrd.tar\n    MODULE_CMDLINE=initrd\n'
    else:
        initrd_data = None

    # Create FAT32: 32MB ESP
    esp_sectors = 32 * 1024 * 1024 // SECTOR  # 32MB
    fat = FAT32(esp_sectors, hidden=esp_start)

    # Build directory tree: /EFI/BOOT/BOOTX64.EFI
    efi = fat.mkdir(ROOT_CLUSTER, 'EFI')
    boot = fat.mkdir(efi, 'BOOT')
    fat.add_file(boot, 'BOOTX64.EFI', efi_data)

    # Add files to root
    fat.add_file(ROOT_CLUSTER, 'vahi_kernel', kernel_data)
    fat.add_file(ROOT_CLUSTER, 'limine.conf', limine_conf)
    if initrd_data:
        fat.add_file(ROOT_CLUSTER, 'initrd.tar', initrd_data)
    if bios_sys:
        fat.add_file(ROOT_CLUSTER, 'LIMINE-BSYS', bios_sys)

    fat32_data = fat.finalize()
    print(f'FAT32: {len(fat32_data)} bytes ({len(fat32_data) / 1024 / 1024:.1f} MiB)')

    # Build MBR disk: 32MB ESP + headers
    esp_start = 2048
    disk_sectors = esp_start + esp_sectors + 2048  # extra trailing space
    disk = bytearray(disk_sectors * SECTOR)

    # Write MBR
    mbr = bytearray(SECTOR)
    # Partition 1: FAT32 LBA (0x0C), starts at sector 2048
    mbr[446 + 0] = 0x00  # status
    mbr[446 + 4] = 0x0C  # type: FAT32 LBA
    struct.pack_into('<I', mbr, 446 + 8, esp_start)  # start LBA
    struct.pack_into('<I', mbr, 446 + 12, esp_sectors)  # num sectors
    struct.pack_into('<H', mbr, 510, 0xAA55)
    disk[0:SECTOR] = mbr

    # Write FAT32
    disk[esp_start * SECTOR:esp_start * SECTOR + len(fat32_data)] = fat32_data

    with open(output, 'wb') as f:
        f.write(disk)
    print(f'Wrote {len(disk)} bytes ({len(disk) / 1024 / 1024:.1f} MiB) to {output}')
    print(f'\nTo boot (UEFI):')
    print(f'  qemu-system-x86_64 -drive if=pflash,format=raw,readonly=on,file="/c/Program Files/qemu/share/edk2-x86_64-code.fd" -drive if=pflash,format=raw,file=OVMF_VARS.fd -drive format=raw,file={output} -serial stdio -m 512 -display none')


if __name__ == '__main__':
    main()
