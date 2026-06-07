# File System Formats Supported

This page lists filesystem formats supported by SkyOS.

## Current Support

| Format | Type | Status | Operations |
|--------|------|--------|------------|
| tmpfs | In-memory | Done | Full (read/write/create/delete) |
| devfs | Virtual | Done | Read-only (device enumeration) |
| procfs | Virtual | Done | Read-only (process info) |
| ext2 | Disk | In progress | Read-only |

## tmpfs

The temporary filesystem stores all data in memory:
- Backed by the kernel's page cache (anonymous pages)
- Supports files, directories, symlinks, and device nodes
- Maximum size limited by available physical memory
- Used for `/tmp` and `/dev/shm`

## devfs

The device filesystem exposes kernel device information:
- `/dev/console` - System console
- `/dev/ttyS0` - Serial port 0 (COM1)
- `/dev/fb0` - Framebuffer device
- `/dev/input/mouse0` - Mouse input device
- `/dev/input/kbd0` - Keyboard input device
- `/dev/null`, `/dev/zero`, `/dev/random`

## procfs

The process filesystem provides process information:
- `/proc/cpuinfo` - CPU information
- `/proc/meminfo` - Memory usage statistics
- `/proc/[pid]/status` - Process status
- `/proc/[pid]/maps` - Memory mappings
- `/proc/uptime` - System uptime

## ext2

The Second Extended Filesystem:
- Blocks: 1024, 2048, or 4096 bytes
- Inode-based metadata
- Directory entries with variable-length names
- Support for symbolic links and hard links
- Current: read-only with write planned

## Future Formats

| Format | Priority | Notes |
|--------|----------|-------|
| FAT32 | High | Essential for UEFI boot partition |
| ext4 | Medium | Journaling, extents, backward compatible |
| ISO 9660 | Medium | Optical media support |
| FUSE | Low | Userspace filesystem framework |
