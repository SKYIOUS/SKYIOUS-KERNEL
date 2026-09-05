# vahi-vfs

Virtual filesystem layer — mount management, inode cache, path resolution.

## Supported Filesystems

| Filesystem | Lines | Type |
|-----------|-------|------|
| SkyFS | 723+ | Journaling, custom B-tree |
| ext2 | 500+ | Full R/W |
| ext4 | ~200 | Read-only |
| FAT32 | ~200 | Via `fatfs` crate |
| ramfs | ~400 | Volatile, in-memory |
| devfs | ~150 | /dev/null, /dev/zero, /dev/tty |
| tarfs | ~204 | Read-only tar archive |
| FUSE | ~200 | Userspace filesystem bridge |
| procfs | — | /proc entries |
| ctlfs | — | /sys kernel parameters |

## Trait Interfaces

| Trait | Purpose |
|-------|---------|
| `InodeOps` | Read/write/stat/truncate for filesystem objects |
| `MountOps` | Mount/unmount/sync for filesystem types |

## Dependency Breaking

```text
Original:     vfs → task (FileDescriptor) + task (CURRENT_PROCESS)
With traits:  vfs → vahi_types::{FileOps, ProcessProvider}
```

## Invariants

- VFS lock held for path resolution
- File operations validate FD bounds before access
- Reference counting prevents use-after-close
- Inode operations are per-filesystem (vtable dispatch)
- Page cache is per-inode, invalidated on write
- Pipe buffers bounded at 64 KiB

## Migration Guide

1. Extract `pipe.rs` → no deps beyond alloc
2. Extract `ramfs.rs` → no deps beyond alloc
3. Extract `devfs.rs` → depends on pipe
4. Extract `tarfs.rs` → depends on alloc
5. Extract `ext2/` → depends on block device trait
6. Extract `skyfs/` → depends on block device + journal
7. Extract `mod.rs` (VFS core) last
