"""
Create a test ext2 disk image (raw) with known files for QEMU testing.

The image contains:
  /hello.txt          "Hello from ext2 test disk!\n"
  /test_write.txt     "This file was pre-created.\n"
  /bin/               directory
  /home/              directory
  /home/user/         directory

Usage:
    python scripts/make_test_ext2.py [output.img] [size-mb]
"""

import os
import sys
import struct

BLOCK_SIZE = 1024
SECTOR_SIZE = 512
EXT2_MAGIC = 0xEF53
EXT2_INODE_SIZE = 128


def le16(buf, pos, val):
    struct.pack_into("<H", buf, pos, val & 0xFFFF)


def le32(buf, pos, val):
    struct.pack_into("<I", buf, pos, val & 0xFFFFFFFF)


def round4(n):
    """Round n up to the next multiple of 4."""
    return (n + 3) // 4 * 4


class Ext2Builder:
    """Build a minimal ext2 filesystem image in memory."""

    def __init__(self, total_blocks: int, inodes_per_group: int = 1024):
        self.total_blocks = total_blocks
        self.inodes_per_group = inodes_per_group
        self.inodes_per_block = BLOCK_SIZE // EXT2_INODE_SIZE
        self.inode_blocks = (inodes_per_group + self.inodes_per_block - 1) // self.inodes_per_block
        self.data_start = 5 + self.inode_blocks
        self.used_blocks = self.data_start  # metadata blocks
        self.used_inodes = 0
        self.block_bitmap = bytearray(BLOCK_SIZE)
        self.inode_bitmap = bytearray(BLOCK_SIZE)
        self.inode_table = bytearray(self.inode_blocks * BLOCK_SIZE)
        self.block_data = {}  # block_num -> bytes
        self.next_inode = 1
        self.next_block = self.data_start

    def mark_block(self, b: int):
        if b >= self.used_blocks:
            self.used_blocks = b + 1
        self.block_bitmap[b // 8] |= 1 << (b % 8)

    def mark_inode(self, inum: int):
        if inum >= self.used_inodes:
            self.used_inodes = inum + 1
        self.inode_bitmap[inum // 8] |= 1 << (inum % 8)

    def alloc_inode(self) -> int:
        inum = self.next_inode
        self.next_inode += 1
        return inum

    def alloc_block(self) -> int:
        blk = self.next_block
        self.next_block += 1
        return blk

    def write_inode(self, inum: int, mode: int, uid: int, gid: int,
                    size: int, links: int, blocks: list):
        idx = inum - 1
        off = idx * EXT2_INODE_SIZE
        buf = bytearray(EXT2_INODE_SIZE)
        le16(buf, 0, mode)
        le16(buf, 2, uid)
        le32(buf, 4, size)
        le16(buf, 24, gid)
        le16(buf, 26, links)
        le32(buf, 28, (len(blocks) * BLOCK_SIZE) // 512)
        for i, blk in enumerate(blocks):
            if i < 15:
                le32(buf, 40 + i * 4, blk)
        self.inode_table[off:off + EXT2_INODE_SIZE] = buf
        self.mark_inode(inum)

    def write_data_block(self, blk: int, data: bytes):
        self.mark_block(blk)
        padded = data.ljust(BLOCK_SIZE, b'\0')[:BLOCK_SIZE]
        self.block_data[blk] = padded

    def add_dir(self, parent_inum: int, name: str, inum: int,
                parent_parent_inum: int = None) -> int:
        """Create a directory. Returns inode number."""
        if parent_parent_inum is None:
            parent_parent_inum = parent_inum

        # Create entries
        entries = [
            (inum, '.', 2, 12),
            (parent_inum, '..', 2, 12),
        ]

        blk = self.alloc_block()
        buf = bytearray(BLOCK_SIZE)
        off = 0
        for i, (ino, ename, ftype, reclen) in enumerate(entries):
            is_last = (i == len(entries) - 1)
            actual = reclen if not is_last else (BLOCK_SIZE - off)
            le32(buf, off, ino)
            le16(buf, off + 4, actual)
            buf[off + 6] = len(ename)
            buf[off + 7] = ftype
            buf[off + 8:off + 8 + len(ename)] = ename.encode()
            off += actual

        self.write_data_block(blk, bytes(buf))
        self.write_inode(inum, 0x41FF, 0, 0, BLOCK_SIZE, 2, [blk])
        return inum

    def add_file(self, parent_inum: int, name: str, content: bytes, inum: int):
        blk = self.alloc_block()
        self.write_data_block(blk, content)
        self.write_inode(inum, 0x81FF, 0, 0, len(content), 1, [blk])

    def add_to_dir(self, parent_inum: int, name: str, child_inum: int, file_type: int):
        """Add an entry to an existing directory by rewriting its block."""
        # Find the directory's data block from its inode
        idx = parent_inum - 1
        off = idx * EXT2_INODE_SIZE
        inode_view = self.inode_table[off:off + EXT2_INODE_SIZE]
        blk = struct.unpack_from("<I", inode_view, 40)[0]

        data = bytearray(self.block_data.get(blk, b'\0' * BLOCK_SIZE))
        # Find the last entry (largest rec_len)
        pos = 0
        while pos < BLOCK_SIZE:
            reclen = struct.unpack_from("<H", data, pos + 4)[0]
            if reclen == 0 or pos + reclen >= BLOCK_SIZE:
                break
            # Check if this is the last entry (its rec_len reaches end)
            if pos + reclen >= BLOCK_SIZE:
                break
            pos += reclen

        # Shrink the current "last" entry to its actual size
        if pos > 0:
            prev_reclen = struct.unpack_from("<H", data, pos - 4 + 4)[0]
            prev_namelen = data[pos - 4 + 6]
            actual = round4(8 + prev_namelen)
            struct.pack_into("<H", data, pos - 4 + 4, actual)
            pos = (pos - 4) + actual

        # Add new entry at current position
        namelen = len(name)
        entry_size = round4(8 + namelen)
        remaining = BLOCK_SIZE - pos
        actual_reclen = max(entry_size, remaining)

        le32(data, pos, child_inum)
        le16(data, pos + 4, actual_reclen)
        data[pos + 6] = namelen
        data[pos + 7] = file_type
        data[pos + 8:pos + 8 + namelen] = name.encode()

        self.block_data[blk] = bytes(data)

    def write(self, path: str):
        total_sectors = (self.total_blocks * BLOCK_SIZE) // SECTOR_SIZE
        image = bytearray(total_sectors * SECTOR_SIZE)

        # Superblock at block 1 (byte offset 1024)
        sb = bytearray(BLOCK_SIZE)
        le32(sb, 0, self.inodes_per_group)
        le32(sb, 4, self.total_blocks)
        le32(sb, 8, 0)
        le32(sb, 12, self.total_blocks - self.used_blocks)
        le32(sb, 16, self.inodes_per_group - self.used_inodes)
        le32(sb, 20, 1)  # s_first_data_block = 1
        le32(sb, 24, 0)  # s_log_block_size = 0
        le32(sb, 28, 0)
        le32(sb, 32, self.total_blocks)
        le32(sb, 36, self.total_blocks)
        le32(sb, 40, self.inodes_per_group)
        le16(sb, 56, EXT2_MAGIC)
        le16(sb, 58, 1)  # clean
        le32(sb, 76, 1)  # rev_level = 1
        le32(sb, 84, 11)  # s_first_ino
        le16(sb, 88, EXT2_INODE_SIZE)
        image[BLOCK_SIZE * 1:BLOCK_SIZE * 2] = sb

        # Block group descriptor at block 2
        gd = bytearray(BLOCK_SIZE)
        le32(gd, 0, 3)  # block bitmap
        le32(gd, 4, 4)  # inode bitmap
        le32(gd, 8, 5)  # inode table
        le16(gd, 12, (self.total_blocks - self.used_blocks) & 0xFFFF)
        le16(gd, 14, (self.inodes_per_group - self.used_inodes) & 0xFFFF)
        le16(gd, 16, 4)  # bg_used_dirs_count
        image[BLOCK_SIZE * 2:BLOCK_SIZE * 3] = gd

        # Bitmaps
        image[BLOCK_SIZE * 3:BLOCK_SIZE * 4] = self.block_bitmap
        image[BLOCK_SIZE * 4:BLOCK_SIZE * 5] = self.inode_bitmap

        # Inode table
        image[BLOCK_SIZE * 5:BLOCK_SIZE * (5 + self.inode_blocks)] = self.inode_table

        # Data blocks
        for blk, data in self.block_data.items():
            image[blk * BLOCK_SIZE:(blk + 1) * BLOCK_SIZE] = data

        with open(path, 'wb') as f:
            f.write(image)


def main():
    output = sys.argv[1] if len(sys.argv) > 1 else "test_ext2.img"
    size_mb = int(sys.argv[2]) if len(sys.argv) > 2 else 32
    total_blocks = (size_mb * 1024 * 1024) // BLOCK_SIZE

    print(f"[*] Creating {size_mb} MB ext2 image: {output}")
    fs = Ext2Builder(total_blocks)

    # Inode 1 = bad blocks (reserved), skip
    fs.mark_inode(0)  # inode 0 doesn't exist but mark bitmap bit 0
    fs.mark_inode(1)  # reserved
    fs.next_inode = 2  # root dir

    # Mark metadata blocks in bitmap
    for b in range(fs.data_start):
        fs.mark_block(b)

    # Root directory (inode 2)
    root_inum = 2
    fs.add_dir(root_inum, '/', root_inum, root_inum)  # parent is self for ".."

    # Files and dirs
    hello_inum = fs.alloc_inode()
    fs.add_file(root_inum, 'hello.txt', b'Hello from ext2 test disk!\n', hello_inum)
    fs.add_to_dir(root_inum, 'hello.txt', hello_inum, 1)

    testw_inum = fs.alloc_inode()
    fs.add_file(root_inum, 'test_write.txt', b'This file was pre-created.\n', testw_inum)
    fs.add_to_dir(root_inum, 'test_write.txt', testw_inum, 1)

    bin_inum = fs.alloc_inode()
    fs.add_dir(root_inum, 'bin', bin_inum, root_inum)
    fs.add_to_dir(root_inum, 'bin', bin_inum, 2)

    home_inum = fs.alloc_inode()
    fs.add_dir(root_inum, 'home', home_inum, root_inum)
    fs.add_to_dir(root_inum, 'home', home_inum, 2)

    user_inum = fs.alloc_inode()
    fs.add_dir(home_inum, 'user', user_inum, home_inum)
    fs.add_to_dir(home_inum, 'user', user_inum, 2)

    fs.write(output)

    print(f"[*] Done. {total_blocks} blocks, {fs.used_inodes} inodes used")
    print()
    print("Contents:")
    print("  /hello.txt         - 'Hello from ext2 test disk!'")
    print("  /test_write.txt    - 'This file was pre-created.'")
    print("  /bin/              - directory")
    print("  /home/             - directory")
    print("  /home/user/        - directory")
    print()
    print("Boot with QEMU:")
    print(f"  qemu-system-x86_64 -bios OVMF.fd -drive format=raw,file=skyos_uefi.img,if=ide,index=0 -drive format=raw,file={output},if=ide,index=1 -m 512M -smp 2 -serial stdio")


if __name__ == '__main__':
    main()
