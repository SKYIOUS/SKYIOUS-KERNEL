# Module Interface: `vfs`

**Path:** `kernel/src/vfs/`
**Owner:** TBD
**Tier:** 2 (depends on memory, sync, drivers)

---

## Public API

### Core Traits (`vfs/mod.rs`)

```rust
/// Filesystem trait. Implement this for each filesystem type.
pub trait FileSystem: Send + Sync {
    fn root(&self) -> Arc<dyn VfsNode>;
    fn name(&self) -> &str;
}

/// VFS node trait. Represents a file, directory, or device in the filesystem.
pub trait VfsNode: Send + Sync {
    fn name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()>;
    fn write(&self, data: &[u8]) -> Result<(), ()>;
    fn stat(&self) -> Result<Stat, ()>;
    fn statfs(&self) -> Result<StatFs, ()>;
    // ... additional methods
}
```

### VFS Manager (`vfs/mod.rs`)

```rust
/// Global VFS manager. Mounts, resolves paths, manages filesystems.
pub struct VfsManager;

impl VfsManager {
    /// Create a new VFS manager.
    pub fn new() -> Self

    /// Mount a filesystem at the given path.
    pub fn mount(&mut self, path: &str, fs: Arc<dyn FileSystem>)

    /// Unmount a filesystem.
    pub fn umount(&mut self, path: &str) -> Result<(), ()>

    /// Resolve a path to a VFS node.
    pub fn resolve_path(&self, path: &str) -> Option<Arc<dyn VfsNode>>
}

/// Global VFS instance.
pub static VFS: Mutex<VfsManager>
```

### Filesystem Implementations

| Filesystem | Path | Status |
|------------|------|--------|
| TarFS | `vfs/tarfs.rs` | Working (read-only) |
| DevFS | `vfs/devfs.rs` | Working |
| SkyFS | `vfs/skyfs/mod.rs` | Experimental |
| Ext2 | `vfs/ext2/` | Read-only |
| Ext4 | `vfs/ext4.rs` | Stub |
| FAT32 | `vfs/fat.rs` | Delegates to fatfs crate |
| RamFS | `vfs/ramfs.rs` | Working |
| CtlFS | `vfs/ctlfs.rs` | Working |
| FUSE | `vfs/fuse.rs` | Stub |

---

## Invariants

1. **Path resolution is deterministic:** The same path always resolves to the same node (given the same mount state).
2. **Mount point uniqueness:** Each mount point path is unique. Mounting at an existing path replaces the previous mount.
3. **Node lifecycle:** A VFS node is alive as long as at least one Arc reference exists. Dropping all references frees the node.
4. **Thread safety:** All VFS operations are thread-safe (VfsNode: Send + Sync).
5. **No allocation in IRQ context:** VFS operations may allocate (Vec, String). They must not be called from IRQ context.

---

## Testing Requirements

| Test | What It Validates | Priority |
|------|-------------------|----------|
| `vfs:tarfs_read` | TarFS file reading | ✅ Exists |
| `vfs:devfs_nodes` | DevFS device nodes | ✅ Exists |
| `vfs:resolve_path` | Path resolution | ✅ Exists |
| **NEW: vfs:mount_unmount** | Mount/unmount cycle | ❌ Needed |
| **NEW: vfs:concurrent_access** | Multiple threads reading same file | ❌ Needed |
| **NEW: vfs:invalid_path** | Bad path returns None | ❌ Needed |

---

## Common Pitfalls

1. **Lock ordering:** VFS locks must be acquired after REFCOUNTS and BUDDY_ALLOCATOR (per global lock ordering).
2. **IRQ context:** VFS operations allocate memory. Never call from IRQ context.
3. **Arc cycles:** VFS nodes use Arc references. Circular references cause memory leaks. Use Weak where appropriate.
4. **Path parsing:** Path strings are parsed manually. Watch for buffer overflows in path components.
