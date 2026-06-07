# Filesystem System Calls

The filesystem syscalls provide directory and file manipulation operations.

## stat / fstat / lstat (syscalls 4-6)

```c
int stat(const char *pathname, struct stat *statbuf);
int fstat(int fd, struct stat *statbuf);
int lstat(const char *pathname, struct stat *statbuf);
```

Retrieves file status information. `lstat` returns information about the symbolic link itself, not the target.

## mkdir (syscall 55)

```c
int mkdir(const char *pathname, mode_t mode);
```

Creates a new directory with the specified permissions.

## rmdir (syscall 56)

```c
int rmdir(const char *pathname);
```

Removes an empty directory.

## unlink (syscall 63)

```c
int unlink(const char *pathname);
```

Removes a name from the filesystem. The file data is freed when no more references exist.

## link (syscall 64)

```c
int link(const char *oldpath, const char *newpath);
```

Creates a hard link to an existing file. Both names refer to the same inode.

## symlink (syscall 65)

```c
int symlink(const char *target, const char *linkpath);
```

Creates a symbolic link containing the string `target`.

## readlink (syscall 66)

```c
ssize_t readlink(const char *pathname, char *buf, size_t bufsiz);
```

Reads the target of a symbolic link into `buf`.

## rename (syscall 70)

```c
int rename(const char *oldpath, const char *newpath);
```

Renames a file or directory, potentially moving it between directories on the same filesystem.

## truncate / ftruncate (syscalls 71-72)

```c
int truncate(const char *path, off_t length);
int ftruncate(int fd, off_t length);
```

Truncates or extends a file to the specified length.

## chmod / chown (syscalls 67-68)

```c
int chmod(const char *pathname, mode_t mode);
int chown(const char *pathname, uid_t owner, gid_t group);
```

Changes file permissions and ownership.

## getdents (syscall 78)

```c
int getdents(unsigned int fd, struct linux_dirent *dirp, unsigned int count);
```

Reads directory entries from a directory file descriptor.

## mount / umount2 (syscalls 75-76)

```c
int mount(const char *source, const char *target, const char *fstype, unsigned long flags, const void *data);
int umount2(const char *target, int flags);
```

Mounts and unmounts filesystems.
