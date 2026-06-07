# Working with the Virtual File System

The Virtual File System (VFS) provides a unified interface for different filesystem implementations.

## VFS Architecture

The VFS uses a node-based architecture where files, directories, and devices are represented as `VfsNode` entries in a tree structure. Each node implements the `VfsNodeOps` trait:

```rust
pub trait VfsNodeOps: Send + Sync {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&self, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn lookup(&self, name: &str) -> Result<Arc<dyn VfsNodeOps>, VfsError>;
    fn create(&self, name: &str, ty: FileType) -> Result<Arc<dyn VfsNodeOps>, VfsError>;
}
```

## Mounts

Filesystems are mounted at mount points in the VFS tree. The root filesystem is mounted during boot by the init process.

```rust
vfs::mount("ext2", "/dev/sda1", "/")?;
vfs::mount("tmpfs", "none", "/tmp")?;
```

## File Operations

Standard file operations go through the VFS layer:
- `open()` → VFS lookup + file descriptor allocation
- `read()`/`write()` → VFS dispatch to the underlying filesystem
- `close()` → File descriptor release and filesystem cleanup

## Supported Filesystems

- **ext2**: Read-only support for boot partitions
- **tmpfs**: In-memory filesystem for temporary files
- **devfs**: Virtual filesystem exposing device nodes
- **procfs**: Process information pseudo-filesystem
