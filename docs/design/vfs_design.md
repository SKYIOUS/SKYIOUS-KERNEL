# Virtual File System Design Decisions

The VFS layer provides a unified interface for diverse filesystem implementations.

## Node-Based Architecture

The VFS represents all filesystem objects as nodes in a tree. Each node is an `Arc<dyn VfsNodeOps>` that can be a file, directory, symlink, device, or mount point. This uniform representation simplifies path resolution and traversal.

## Mount Hierarchy

Mounts form a stack at each directory node. When a filesystem is mounted at `/mnt/usb`, the `/mnt/usb` node is replaced by the root of the mounted filesystem. The previous node is preserved and accessible after unmounting.

## I/O Model

VFS read and write operations are async and can return `WouldBlock` for non-blocking file descriptors. The VFS layer itself is non-blocking internally, using async operations for backing storage access.

## Design Decisions

1. **No built-in caching**: The VFS layer does not cache file data. Caching is left to individual filesystem implementations or dedicated cache services.

2. **Path resolution is lock-free**: Directory entries are reference-counted and immutable after creation. Renames and removals use atomic operations on the parent directory's entry table.

3. **Maximum path depth**: 4096 bytes, with individual component names limited to 255 bytes.

4. **File descriptor table**: Each process has a fixed-size file descriptor table (default 1024 entries), backed by an array of `Option<Arc<FileDescription>>`.

## Future Extensions

- FUSE (Filesystem in Userspace) support for userspace filesystems
- Stackable filesystem layers (encryption, compression, union mounts)
- Distributed filesystem support through networked VFS
