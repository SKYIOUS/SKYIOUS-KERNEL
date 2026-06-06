# SkyOS Virtual Filesystem (VFS) Design

This document outlines the design of the SkyOS Virtual Filesystem (VFS) layer, which provides a unified interface for user applications to interact with different filesystems and I/O devices.

## 1. Core Concepts

The VFS is built around two primary traits:

-   **`VfsNode`**: Represents an object in the filesystem tree, which can be a file, directory, device, or pipe.
-   **`FileSystem`**: Represents a mounted filesystem instance, responsible for providing the root `VfsNode` of its own tree.

A global `VFS` manager (an `Arc<Mutex<VfsManager>>`) tracks all mounted filesystems and handles path resolution.

## 2. `VfsNode` Trait

The `VfsNode` trait is the core abstraction. All filesystem objects must implement this trait.

```rust
pub trait VfsNode: Send + Sync {
    fn name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn read(&self) -> Result<Vec<u8>, ()>;
    fn write(&self, data: &[u8]) -> Result<(), ()>;
    fn stat(&self) -> Result<Stat, ()>;
    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()>;
    // ... other methods like create, mkdir, unlink ...
}
```

## 3. Path Resolution

Path resolution starts at the root (`/`) and traverses the VFS tree.

1.  The `VfsManager` starts with the root filesystem (`Tmpfs`).
2.  It splits the path into components (e.g., `/home/user/file` -> `home`, `user`, `file`).
3.  For each component, it checks if the current path is a mount point. If so, it switches to the root of the mounted filesystem.
4.  It calls the `children()` method on the current directory node and finds the node with the matching name.
5.  This process repeats until the final component is resolved.

## 4. Supported Filesystems

-   **`Tmpfs` (`ramfs.rs`)**: A simple in-memory filesystem used for the root (`/`) and temporary files.
-   **`Ext2` (`ext2.rs`)**: A read-only implementation of the Second Extended Filesystem.
-   **`FAT32` (`fat.rs`)**: A wrapper around the `fatfs` crate for interoperability with FAT32-formatted devices.
-   **`Pipe` (`pipe.rs`)**: An in-memory pipe for inter-process communication (IPC), exposed via the `sys_pipe` syscall.

## 5. File Descriptors

Each `Process` has its own file descriptor table (`fd_table`), which is a `Vec<Option<FileDescriptor>>`. A file descriptor can point to a `VfsNode` (for files) or a `Socket` handle. This per-process table allows for standard I/O redirection and inheritance across forks.
