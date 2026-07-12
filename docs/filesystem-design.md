# Filesystem Design

## Architecture

```
Userspace (syscall interface)
    ↓ SYS_OPEN / SYS_READ / SYS_WRITE / SYS_STAT / SYS_GETDENTS64
VfsManager (vfs/mod.rs)
    ├── Mount table (path → FileSystem)
    ├── resolve_path(path) → Arc<dyn VfsNode>
    └── VfsNode trait
            ├── read / write / stat / children / find_child
            ├── create / mkdir / unlink / chmod / chown
            └── symlink / readlink

Implementations:
    ├── Ext2FileSystem  — vfs/ext2.rs  (753 lines)
    ├── FatFileSystem   — vfs/fat.rs   (280 lines, via fatfs crate)
    ├── TarfsMemory     — vfs/tarfs.rs (195 lines)
    ├── Tmpfs           — vfs/ramfs.rs (244 lines)
    ├── DevFs           — vfs/devfs.rs (364 lines)
    ├── CtlFs           — vfs/ctlfs.rs (229 lines)
    └── SkyFS           — vfs/skyfs/   (715+ lines, journaling)
```

## Supported Filesystems

| FS | Type | Read | Write | Create | Notes |
|----|------|------|-------|--------|-------|
| Ext2 | Disk | ✅ | ✅ | ✅ | Full R/W, indirect blocks, 1024-4096 byte blocks |
| Ext4 | Disk | ✅ | ❌ | ❌ | Read-only, extent tree, 64-bit, cfg-gated |
| FAT32 | Disk | ✅ | ✅ | ✅ | Via fatfs crate, R/W |
| TarFS | Initrd | ✅ | ❌ | ❌ | ustar format, block-device or memory-backed |
| Tmpfs | RAM | ✅ | ✅ | ✅ | In-memory, writable, at /tmp |
| DevFS | Virtual | ✅ | ✅ | ❌ | Device nodes at /dev |
| CtlFS | Virtual | ✅ | ❌ | ❌ | Plan9-style control files at /ctl |
| SkyFS | Disk | ✅ | ✅ | ✅ | Custom journaling FS with B-tree extents |

## Ext2 Driver (vfs/ext2.rs)

The ext2 driver implements the Second Extended Filesystem (ext2) with full read/write support.

### On-Disk Structures

```
Block 0:       Boot block (unused)
Block 1:       Superblock (1024 bytes at offset 1024)
Block 2:       Block Group Descriptors
Block 3:       Block bitmap
Block 4:       Inode bitmap
Blocks 5..N:   Inode table (128-byte inodes)
Blocks N+1..:  Data blocks
```

### Superblock Fields (struct Superblock)

- `s_magic` (u16 at byte 56) — must be `0xEF53`
- `s_log_block_size` — block size = 1024 << s_log_block_size
- `s_blocks_per_group`, `s_inodes_per_group` — group sizing
- `s_free_blocks_count`, `s_free_inodes_count` — free resource counts
- `s_rev_level` — 0=original, 1=dynamic (with variable inode size)

### Inode (struct Inode, 128 bytes)

- `i_mode` — file type + permission bits (0x81FF = regular, 777)
- `i_uid`, `i_gid` — owner/group
- `i_size` — file size in bytes
- `i_links_count` — hard link count
- `i_blocks` — count of 512-byte blocks used
- `i_block[15]` — direct (0-11), singly indirect (12), doubly indirect (13), triply indirect (14) block pointers

### Directory Entry (struct DirectoryEntry, 8 bytes + name)

- `inode` (u32) — inode number
- `rec_len` (u16) — record length (4-byte aligned)
- `name_len` (u8) — name length
- `file_type` (u8) — 1=regular, 2=dir

### Key Operations

**Mount**: Read superblock from sector 2, verify magic 0xEF53, compute block size and inode geometry.

**Read inode**: Locate block group → read group descriptor → compute sector offset in inode table → read 128-byte inode.

**Write inode**: Same as read, but modify and write back the sector containing the inode.

**Read file**: Collect direct + indirect block indices via `read_all_block_indices()`, read each data block, concatenate up to `i_size`.

**Write file**: Allocate blocks via `allocate_block()` (scans block bitmap per group), write data blocks, update indirect pointers as needed, write back inode.

**Create file**: Allocate inode via `allocate_inode()`, zero-fill inode struct, write inode, add directory entry to parent.

**Mkdir**: Allocate inode + block, initialize with `.` and `..` entries, write block + inode, add entry to parent.

## Block Layer

```
Physical Disk (IDE/AHCI/NVMe)
    ↓ BlockDevice trait (read_sector / write_sector / sector_count)
BlockCache (256-entry, clock eviction, write-back)
    ↓
PartitionDevice (sub-slices a parent device by LBA range)
    ↓
BLOCK_DEVICES global registry (Vec<Arc<Mutex<dyn BlockDevice>>>)
    ↓
Filesystem mount (ext2::mount, fat::FatFileSystem::new, etc.)
```

## Mount Process (vfs::init)

1. Attempt root filesystem:
   - Boot device (if specified via `BOOT_DEVICE`): try ext4 → ext2 → SkyFS
   - First block device partition: try ext4 → ext2 → SkyFS
   - First whole block device: try ext4 → ext2 → SkyFS
   - Fallback to bootloader initrd (TarFS)
2. Mount DevFS at `/dev`
3. Mount CtlFS at `/ctl`
4. Mount Tmpfs at `/tmp`
5. Scan all block devices and partitions:
   - Mount any ext4/ext2/FAT32/TarFS/SkyFS under `/mnt/`
   - Register device nodes in DevFS

## Permissions

UNIX permission bits are enforced in `sys_open()` (`syscalls/mod.rs:1003-1061`):

- `O_RDONLY` (0) → need `read` (4)
- `O_WRONLY` (1) → need `write` (2)
- `O_RDWR` (2) → need `read|write` (6)
- `O_CREAT` → also checks `write|execute` (3) on parent directory

`check_file_permission()` (`syscalls/mod.rs:67-76`):
- Root (euid=0) always permitted
- Owner permissions apply when euid matches `st_uid`
- Group permissions apply when egid matches `st_gid`
- Other permissions otherwise

Self-test entries validate permission bit reporting via `stat()`.

## Package Format (.sky)

`.sky` / `.skp` packages are **ustar tar archives** containing:

```
[512-byte ustar header]
[file data, padded to 512 bytes]
[512-byte ustar header]
[file data, padded to 512 bytes]
...
[two zero-filled 512-byte blocks]
```

Required entry: a `manifest` text file with:

```
name=package-name
version=1.0.0
description=Human-readable description
depends=dep1,dep2  (optional, comma-separated)
```

All other entries are extracted as files relative to `/`.

**Build tool**: `scripts/make_sky_pkg.py <pkg_dir> <output.skp>`
**Installation**: `spkg install <file.skp>` (userland, in spkg/)

## Testing

### In-Kernel Self-Tests (RamBlock-based)

Five ext2 tests registered under `ext2_*` prefix, run at boot with `self_test` feature:

| Test | Description |
|------|-------------|
| `ext2_format_mount` | Creates minimal ext2, mounts, verifies root is dir |
| `ext2_read_file` | Reads pre-written "hello.txt", verifies content |
| `ext2_write_file` | Creates new file, writes, reads back, checks content |
| `ext2_mkdir_and_stat` | Creates dir, verifies stat mode/ino, checks children |
| `ext2_permissions` | Verifies stat returns correct perm bits/uid/gid |

### Disk Image Testing

`scripts/make_test_ext2.py` generates a raw ext2 image with known files for QEMU:

```bash
python scripts/make_test_ext2.py test_ext2.img 32
qemu-system-x86_64 -bios OVMF.fd \
    -drive format=raw,file=skyos_uefi.img,if=ide,index=0 \
    -drive format=raw,file=test_ext2.img,if=ide,index=1 \
    -m 512M -smp 2 -serial stdio
```

The kernel auto-detects ext2 on the second drive, mounts it, and logs the mount point. In-kernel shell commands (`ls`, `cat`, `mkdir`, `touch`) work on the mounted filesystem.
