# VFS API Reference

The Virtual File System API provides the interface for filesystem implementations and file operations.

## Core Types

```rust
pub struct FileDescriptor {
    pub node: Arc<dyn VfsNodeOps>,
    pub offset: u64,
    pub flags: OpenFlags,
    pub seekable: bool,
}
```

## VfsNodeOps Trait

Every filesystem must implement this trait:

```rust
pub trait VfsNodeOps: Send + Sync {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize>;
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize>;
    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNodeOps>>;
    fn create(&self, name: &str, ty: FileType) -> Result<Arc<dyn VfsNodeOps>>;
    fn remove(&self, name: &str) -> Result<()>;
    fn rename(&self, old_name: &str, new_name: &str) -> Result<()>;
    fn stat(&self) -> Result<FileStat>;
    fn readdir(&self) -> Result<Vec<DirEntry>>;
    fn truncate(&self, len: u64) -> Result<()>;
}
```

## Mount Functions

```rust
/// Mount a filesystem at a path
pub fn mount(fs_type: &str, source: &str, target: &str) -> Result<()>;

/// Unmount a filesystem
pub fn unmount(path: &str) -> Result<()>;

/// Get mount information
pub fn mount_info(path: &str) -> Result<MountInfo>;
```

## Path Resolution

Paths are resolved from the root directory. `.` and `..` components are handled during resolution. Symlinks are followed (up to a maximum depth of 40). Absolute paths start with `/`; relative paths are resolved from the current working directory.
