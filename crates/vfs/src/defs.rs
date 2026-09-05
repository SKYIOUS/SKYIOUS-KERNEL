use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use hashbrown::HashMap;
use lazy_static::lazy_static;
use vahi_drivers::block::BlockDevice;
use vahi_sync::IrqSafeMutex as Mutex;

pub trait FileSystem: Send + Sync {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()>;
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_mode: u32,
    pub st_nlink: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_atime: i64,
    pub st_mtime: i64,
    pub st_ctime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime_nsec: i64,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct StatFs {
    pub f_type: u64,
    pub f_bsize: u64,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
}

const _MAX_CPUS: usize = 16;
pub const _S_IFMT: u32 = 0o170000;
pub const S_IFDIR: u32 = 0o040000;
pub const _S_IFCHR: u32 = 0o020000;
pub const _S_IFBLK: u32 = 0o060000;
pub const S_IFREG: u32 = 0o100000;
pub const _S_IFIFO: u32 = 0o010000;
pub const S_IFLNK: u32 = 0o120000;
pub const _S_IFSOCK: u32 = 0o140000;

pub trait VfsNode: Send + Sync {
    fn name(&self) -> String;
    fn is_dir(&self) -> bool;
    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()>;

    /// Get inode number for hard link operations (filesystem-specific)
    fn inode_num(&self) -> Option<u64> {
        None
    }

    fn stat(&self) -> Result<Stat, ()> {
        Err(()) // Default implementation, override in specific filesystems
    }
    fn statfs(&self) -> Result<StatFs, ()> {
        Err(()) // Default implementation
    }
    fn write(&self, _data: &[u8]) -> Result<(), ()> {
        Err(())
    }
    fn ioctl(&self, _request: u64, _argp: *mut u8) -> Result<u64, ()> {
        Err(())
    }
    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        Err(())
    }
    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        if let Ok(children) = self.children() {
            children.into_iter().find(|c| c.name() == name)
        } else {
            None
        }
    }

    fn mkdir(&self, _name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        Err(())
    }

    fn create(&self, _name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        Err(())
    }

    fn unlink(&self, _name: &str) -> Result<(), ()> {
        Err(())
    }

    fn chmod(&self, _mode: u32) -> Result<(), ()> {
        Err(())
    }

    fn chown(&self, _uid: u32, _gid: u32) -> Result<(), ()> {
        Err(())
    }

    fn readlink(&self) -> Result<String, ()> {
        Err(())
    }

    fn symlink(&self, _name: &str, _target: &str) -> Result<(), ()> {
        Err(())
    }

    fn rename(&self, _old_name: &str, _new_name: &str) -> Result<(), ()> {
        Err(())
    }

    fn truncate(&self, _len: i64) -> Result<(), ()> {
        Err(())
    }

    fn link(&self, _existing: alloc::sync::Arc<dyn VfsNode>, _name: &str) -> Result<(), ()> {
        Err(())
    }

    fn utimens(&self, _atime: (i64, i64), _mtime: (i64, i64)) -> Result<(), ()> {
        Err(())
    }

    fn fallocate(&self, _mode: i32, _offset: i64, _len: i64) -> Result<(), ()> {
        Err(())
    }
}

pub struct MountPoint {
    pub path: String,
    pub fs: Arc<dyn FileSystem>,
}

pub struct VfsManager {
    mounts: Vec<MountPoint>,
    mount_cache: HashMap<String, Arc<dyn VfsNode>>,
}

impl VfsManager {
    pub fn statfs_mount(&mut self, path: &str) -> Option<Arc<dyn VfsNode>> {
        if let Some(cached) = self.mount_cache.get(path) {
            return Some(cached.clone());
        }

        let result = self
            .mounts
            .iter()
            .filter(|m| path == m.path || path.starts_with(&m.path))
            .max_by_key(|m| m.path.len())
            .and_then(|m| m.fs.root().ok());

        if let Some(ref node) = result {
            self.mount_cache.insert(path.to_string(), node.clone());
        }

        result
    }

    pub fn new() -> Self {
        VfsManager {
            mounts: Vec::new(),
            mount_cache: HashMap::new(),
        }
    }

    pub fn mount(&mut self, path: &str, fs: Arc<dyn FileSystem>) {
        // Ensure path starts with / and doesn't end with / unless it's just /
        let mut path_fixed = String::from(path);
        if !path_fixed.starts_with('/') {
            path_fixed.insert(0, '/');
        }
        if path_fixed.len() > 1 && path_fixed.ends_with('/') {
            path_fixed.pop();
        }

        self.mounts.push(MountPoint {
            path: path_fixed,
            fs,
        });

        // Sort mounts by path length descending so longest matches take priority
        self.mounts.sort_by(|a, b| b.path.len().cmp(&a.path.len()));

        // Invalidate cache: a new mount may shadow previously-cached paths
        self.mount_cache.clear();
    }

    const MAX_SYMLINK_DEPTH: usize = 40;

    pub fn resolve_path(&self, path: &str) -> Option<Arc<dyn VfsNode>> {
        self.resolve_path_with_depth(path, 0)
    }

    fn resolve_path_with_depth(&self, path: &str, depth: usize) -> Option<Arc<dyn VfsNode>> {
        if depth > Self::MAX_SYMLINK_DEPTH {
            return None;
        }
        let cwd = { String::from("/") };

        // Normalize path and handle relative paths
        let mut path_norm = String::from(path);
        if !path_norm.starts_with('/') {
            if cwd == "/" {
                path_norm.insert(0, '/');
            } else {
                // Avoid format! which can hang
                let mut s = String::from(&cwd);
                s.push('/');
                s.push_str(&path_norm);
                path_norm = s;
            }
        }

        if path_norm.len() > 1 && path_norm.ends_with('/') {
            path_norm.pop();
        }

        // Find best mount point
        let mount = self.mounts.iter().find(|m| {
            if m.path == "/" || path_norm == m.path {
                true
            } else if path_norm.starts_with(m.path.as_str()) {
                // Check if the next char is '/' (i.e., path is a subpath of m.path)
                let next = path_norm.as_bytes().get(m.path.len()).copied().unwrap_or(0);
                next == b'/'
            } else {
                false
            }
        })?;

        let mut current = mount.fs.root().ok()?;

        // Relative path within the filesystem
        let rel_path = if mount.path == "/" {
            &path_norm[1..]
        } else {
            &path_norm[mount.path.len()..]
        };

        let components: Vec<&str> = rel_path.split('/').filter(|s| !s.is_empty()).collect();

        // Normalize `..` components before traversal — pop the last real component
        let mut normalized = Vec::with_capacity(components.len());
        for comp in &components {
            if *comp == ".." {
                normalized.pop();
            } else if *comp != "." {
                normalized.push(*comp);
            }
        }

        {
            let mut comps = normalized.as_slice();
            while let Some((&comp, rest)) = comps.split_first() {
                if !current.is_dir() {
                    return None;
                }
                if let Some(next) = current.find_child(comp) {
                    current = next;
                    // Check if this component is a symlink
                    if let Ok(stat) = current.stat() {
                        if stat.st_mode & S_IFLNK != 0 {
                            if let Ok(target) = current.readlink() {
                                let mut sym_path = if target.starts_with('/') {
                                    target
                                } else {
                                    let mut base = String::from("/");
                                    if mount.path != "/" {
                                        base = alloc::format!("{}/", mount.path);
                                    }
                                    for &c in components[..components.len() - rest.len() - 1].iter()
                                    {
                                        base.push_str(c);
                                        base.push('/');
                                    }
                                    base.push_str(&target);
                                    base
                                };
                                for &c in rest.iter() {
                                    sym_path.push('/');
                                    sym_path.push_str(c);
                                }
                                return self.resolve_path_with_depth(&sym_path, depth + 1);
                            }
                        }
                    }
                } else {
                    return None;
                }
                comps = rest;
            }
        }
        Some(current)
    }

    pub fn umount(&mut self, path: &str) -> Result<(), ()> {
        let mut path_fixed = String::from(path);
        if !path_fixed.starts_with('/') {
            path_fixed.insert(0, '/');
        }
        if path_fixed.len() > 1 && path_fixed.ends_with('/') {
            path_fixed.pop();
        }

        // Invalidate cache: removing a mount invalidates all cached paths
        // that may have resolved through it
        self.mount_cache.clear();
        let pos = self
            .mounts
            .iter()
            .position(|m| m.path == path_fixed)
            .ok_or(())?;
        self.mounts.remove(pos);
        Ok(())
    }

    pub fn _read_file(&self, path: &str) -> Result<Vec<u8>, ()> {
        self.resolve_path(path).ok_or(())?.read(usize::MAX)
    }

    pub fn search(&self, start_path: &str, pattern: &str) -> Vec<String> {
        let mut results = Vec::new();
        if let Some(root) = self.resolve_path(start_path) {
            self.search_recursive(root, start_path, pattern, &mut results);
        }
        results
    }

    fn search_recursive(
        &self,
        node: Arc<dyn VfsNode>,
        current_path: &str,
        pattern: &str,
        results: &mut Vec<String>,
    ) {
        if node.name().contains(pattern) {
            results.push(String::from(current_path));
        }

        if node.is_dir() {
            if let Ok(children) = node.children() {
                for child in children {
                    let child_name = child.name();
                    if child_name == "." || child_name == ".." {
                        continue;
                    }
                    let next_path = if current_path == "/" {
                        alloc::format!("/{}", child_name)
                    } else {
                        alloc::format!("{}/{}", current_path, child_name)
                    };
                    self.search_recursive(child, &next_path, pattern, results);
                }
            }
        }
    }
}

/// Boot device selection: None = initrd, Some(n) = block device index
pub static BOOT_DEVICE: vahi_sync::IrqSafeMutex<Option<usize>> = vahi_sync::IrqSafeMutex::new(None);

/// Ramdisk (initrd) provided by bootloader. Set before vfs::init() is called.
pub static RAMDISK: vahi_sync::IrqSafeMutex<Option<&'static [u8]>> =
    vahi_sync::IrqSafeMutex::new(None);

/// Set the boot device by index into BLOCK_DEVICES.
pub fn set_boot_device(index: usize) {
    *BOOT_DEVICE.lock() = Some(index);
    vfs_debug!("VFS: boot device set to block device {}", index);
}

lazy_static! {
    pub static ref VFS: Mutex<VfsManager> = Mutex::new(VfsManager::new());
}

// ─── Ext4 mount helpers (cfg-gated for build-flag fallback) ─────────────────

#[cfg(feature = "ext4")]
fn try_mnt_ext4(
    vfs: &mut VfsManager,
    dev: Arc<Mutex<dyn BlockDevice>>,
    path: &str,
    msg: &str,
) -> bool {
    match crate::ext4::mount(dev) {
        Ok(fs) => {
            vfs.mount(path, fs);
            vfs_debug!("VFS: Mounted Ext4 at {} ({})", path, msg);
            true
        }
        Err(_) => false,
    }
}
#[cfg(not(feature = "ext4"))]
fn try_mnt_ext4(_: &mut VfsManager, _: Arc<Mutex<dyn BlockDevice>>, _: &str, _: &str) -> bool {
    false
}

#[cfg(feature = "ext4")]
fn try_root_ext4(vfs: &mut VfsManager, dev: Arc<Mutex<dyn BlockDevice>>, tag: &str) -> bool {
    match crate::ext4::mount(dev) {
        Ok(fs) => {
            vfs.mount("/", fs);
            vfs_debug!("VFS: Root filesystem from {} (ext4).", tag);
            true
        }
        Err(_) => false,
    }
}
#[cfg(not(feature = "ext4"))]
fn try_root_ext4(_: &mut VfsManager, _: Arc<Mutex<dyn BlockDevice>>, _: &str) -> bool {
    false
}

pub fn init() {
    let mut vfs = VFS.lock();

    // Try to mount root from a block device first
    let root_mounted = {
        let devices = vahi_drivers::block::BLOCK_DEVICES.lock();
        let boot_idx = *BOOT_DEVICE.lock();
        let mut mounted = false;

        if let Some(idx) = boot_idx {
            if let Some(dev) = devices.get(idx) {
                vfs_debug!("VFS: Attempting root from block device {}...", idx);
                if try_root_ext4(
                    &mut vfs,
                    dev.clone(),
                    &alloc::format!("block device {}", idx),
                ) {
                    mounted = true;
                } else if let Ok(ext2fs) = crate::ext2::mount(dev.clone()) {
                    vfs.mount("/", ext2fs);
                    vfs_debug!(
                        "VFS: Root filesystem mounted from block device {} (ext2).",
                        idx
                    );
                    mounted = true;
                } else if let Ok(skyfs) = crate::skyfs::SkyFSHandle::mount(dev.clone()) {
                    vfs.mount("/", skyfs);
                    vfs_debug!(
                        "VFS: Root filesystem mounted from block device {} (SkyFS).",
                        idx
                    );
                    mounted = true;
                }
            }
        } else {
            // Check for any ext2 partition on first block device
            if let Some(dev) = devices.first() {
                let partitions = vahi_drivers::block::partition::parse_partitions(dev);
                if let Some(part) = partitions.first() {
                    let part_dev = Arc::new(vahi_sync::IrqSafeMutex::new(
                        vahi_drivers::block::partition::PartitionDevice::new(
                            dev.clone(),
                            part.lba_start,
                            part.sector_count,
                        ),
                    ));
                    if try_root_ext4(&mut vfs, part_dev.clone(), "first partition") {
                        mounted = true;
                    } else if let Ok(ext2fs) = crate::ext2::mount(part_dev.clone()) {
                        vfs.mount("/", ext2fs);
                        vfs_debug!("VFS: Root filesystem mounted from first partition (ext2).");
                        mounted = true;
                    } else if let Ok(skyfs) = crate::skyfs::SkyFSHandle::mount(part_dev) {
                        vfs.mount("/", skyfs);
                        vfs_debug!("VFS: Root filesystem mounted from first partition (SkyFS).");
                        mounted = true;
                    }
                }
            }
            if !mounted {
                // Try the whole device
                if let Some(dev) = devices.first() {
                    if try_root_ext4(&mut vfs, dev.clone(), "first block device") {
                        mounted = true;
                    } else if let Ok(ext2fs) = crate::ext2::mount(dev.clone()) {
                        vfs.mount("/", ext2fs);
                        vfs_debug!("VFS: Root filesystem mounted from first block device (ext2).");
                        mounted = true;
                    } else if let Ok(skyfs) = crate::skyfs::SkyFSHandle::mount(dev.clone()) {
                        vfs.mount("/", skyfs);
                        vfs_debug!("VFS: Root filesystem mounted from first block device (SkyFS).");
                        mounted = true;
                    }
                }
            }
        }
        mounted
    };

    if !root_mounted {
        // Fall back to bootloader-provided ramdisk
        if let Some(ramdisk) = RAMDISK.lock().take() {
            let initrd_fs = Arc::new(crate::tarfs::TarfsMemory::new(ramdisk));
            vfs.mount("/", initrd_fs);
            vfs_debug!(
                "VFS: Mounted bootloader-provided initrd ({} bytes) as root.",
                ramdisk.len()
            );
        } else {
            vfs_debug!("VFS: WARNING — no initrd available, root filesystem is empty!");
        }
    }

    // Mount DevFS at /dev
    let devfs = Arc::new(crate::devfs::DevFs::new());
    vfs.mount("/dev", devfs.clone());
    vfs_debug!("VFS: Mounted DevFS at /dev.");

    // Mount ctlFS at /ctl (Plan9-style control filesystem replacing /proc + /sys)
    let ctlfs = Arc::new(crate::ctlfs::CtlFs::new());
    vfs.mount("/ctl", ctlfs);
    vfs_debug!("VFS: Mounted CtlFs at /ctl.");

    // Mount a tmpfs for /tmp (writable shared temporary storage)
    let ramfs = Arc::new(crate::ramfs::Tmpfs::new());
    vfs.mount("/tmp", ramfs);
    vfs_debug!("VFS: Mounted Tmpfs at /tmp.");

    // Mount procfs at /proc for process introspection
    // Use init_with_vfs to avoid re-locking VFS (we already hold it here)
    // procfs init — stubbed in standalone crate (kernel provides real impl)

    // Scan block devices for partitions and mount filesystems
    let device_snapshots: Vec<_> = {
        let blk = vahi_drivers::block::BLOCK_DEVICES.lock();
        blk.iter()
            .enumerate()
            .map(|(i, d)| (i, d.clone()))
            .collect()
    };
    let letters = [b'a', b'b', b'c', b'd', b'e', b'f'];

    for (i, dev) in device_snapshots {
        let dev_name = if i < letters.len() {
            alloc::format!("sd{}", letters[i] as char)
        } else {
            alloc::format!("blk{}", i)
        };

        // Add the whole-disk device node to DevFS
        devfs.add_block_device(&dev_name, i);

        // Mount filesystems from the whole disk
        let mount_path_ext4 = alloc::format!("/mnt/ext4_{}", i);
        try_mnt_ext4(&mut vfs, dev.clone(), &mount_path_ext4, &dev_name);

        let mount_path_ext2 = alloc::format!("/mnt/ext2_{}", i);
        if let Ok(ext2fs) = crate::ext2::mount(dev.clone()) {
            vfs.mount(&mount_path_ext2, ext2fs);
            vfs_debug!("VFS: Mounted Ext2 at {}", mount_path_ext2);
        }

        let mount_path_fat = alloc::format!("/mnt/fat_{}", i);
        if let Ok(fatfs) = crate::fat::FatFileSystem::new(dev.clone()) {
            vfs.mount(&mount_path_fat, Arc::new(fatfs));
            vfs_debug!("VFS: Mounted FAT32 at {}", mount_path_fat);
        }

        let mount_path_tar = alloc::format!("/mnt/tar_{}", i);
        if let Ok(tarfs) = crate::tarfs::Tarfs::new(dev.clone()) {
            vfs.mount(&mount_path_tar, Arc::new(tarfs));
            vfs_debug!("VFS: Mounted TarFS at {}", mount_path_tar);
        }

        let mount_path_sky = alloc::format!("/mnt/skyfs_{}", i);
        if let Ok(skyfs) = crate::skyfs::SkyFSHandle::mount(dev.clone()) {
            vfs.mount(&mount_path_sky, skyfs);
            vfs_debug!("VFS: Mounted SkyFS at {}", mount_path_sky);
        }

        // Scan and register partitions
        let partitions = vahi_drivers::block::partition::parse_partitions(&dev);
        for part in partitions.iter() {
            let part_name = alloc::format!("{}{}", dev_name, part.index);

            // Register partition as a block device
            let part_dev = Arc::new(vahi_sync::IrqSafeMutex::new(
                vahi_drivers::block::partition::PartitionDevice::new(
                    dev.clone(),
                    part.lba_start,
                    part.sector_count,
                ),
            ));
            let part_dev_idx = vahi_drivers::block::BLOCK_DEVICES.lock().len();
            vahi_drivers::block::register_block_device(part_dev.clone());

            // Add partition device node to DevFS
            devfs.add_block_device(&part_name, part_dev_idx);

            // Try to mount filesystems on the partition
            let mount_path_ext4p = alloc::format!("/mnt/ext4_{}_{}", i, part.index);
            try_mnt_ext4(&mut vfs, part_dev.clone(), &mount_path_ext4p, &part_name);

            let mount_path_ext2p = alloc::format!("/mnt/ext2_{}_{}", i, part.index);
            if let Ok(ext2fs) = crate::ext2::mount(part_dev.clone()) {
                vfs.mount(&mount_path_ext2p, ext2fs);
                vfs_debug!("VFS: Mounted Ext2 on {} at {}", part_name, mount_path_ext2p);
            }

            let mount_path_fatp = alloc::format!("/mnt/fat_{}_{}", i, part.index);
            if let Ok(fatfs) = crate::fat::FatFileSystem::new(part_dev.clone()) {
                vfs.mount(&mount_path_fatp, Arc::new(fatfs));
                vfs_debug!("VFS: Mounted FAT32 on {} at {}", part_name, mount_path_fatp);
            }

            let mount_path_skyp = alloc::format!("/mnt/skyfs_{}_{}", i, part.index);
            if let Ok(skyfs) = crate::skyfs::SkyFSHandle::mount(part_dev.clone()) {
                vfs.mount(&mount_path_skyp, skyfs);
                vfs_debug!("VFS: Mounted SkyFS on {} at {}", part_name, mount_path_skyp);
            }
        }
    }
}

pub fn _mount_fat32(_path: &str, _device: Arc<Mutex<dyn BlockDevice>>) {}

// ─── VfsObject: bridges VfsNode → KernelObject ─────────────────────

use vahi_objects::{security::SecurityDescriptor, KernelObject, ObjectHeader, ObjectTypeId};

/// Wraps any VfsNode as a KernelObject. The ObjectHeader is populated
/// from the node's `stat()` at construction time (mode/uid/gid) or
/// falls back to defaults for filesystems that don't implement stat().
pub struct VfsObject {
    pub header: ObjectHeader,
    pub node: Arc<dyn VfsNode>,
}

impl VfsObject {
    pub fn new(node: Arc<dyn VfsNode>, type_id: ObjectTypeId) -> Arc<Self> {
        let sec = SecurityDescriptor::default();
        Arc::new(VfsObject {
            header: ObjectHeader::new(type_id, sec),
            node,
        })
    }

    pub fn new_with_sec(
        node: Arc<dyn VfsNode>,
        type_id: ObjectTypeId,
        sec: SecurityDescriptor,
    ) -> Arc<Self> {
        Arc::new(VfsObject {
            header: ObjectHeader::new(type_id, sec),
            node,
        })
    }
}

impl KernelObject for VfsObject {
    fn header(&self) -> &ObjectHeader {
        &self.header
    }

    fn type_name(&self) -> &'static str {
        if self.header.object_type == vahi_objects::TYPE_DIR {
            "VfsDir"
        } else {
            "VfsFile"
        }
    }

    fn query_name(&self) -> Option<alloc::string::String> {
        Some(self.node.name())
    }

    fn on_close(&self) {}

    fn read(&self, offset: &mut u64, buf: &mut [u8]) -> Result<usize, ()> {
        let data = self.node.read(buf.len())?;
        let start = core::cmp::min(*offset as usize, data.len());
        let available = &data[start..];
        let len = core::cmp::min(available.len(), buf.len());
        buf[..len].copy_from_slice(&available[..len]);
        *offset += len as u64;
        Ok(len)
    }

    fn write(&self, _offset: &mut u64, buf: &[u8]) -> Result<usize, ()> {
        self.node.write(buf).map(|_| buf.len())
    }

    fn ioctl(&self, request: u64, argp: *mut u8) -> Result<u64, ()> {
        self.node.ioctl(request, argp)
    }

    fn stat(&self) -> Result<vahi_objects::StatLike, ()> {
        let s = self.node.stat()?;
        Ok(vahi_objects::StatLike {
            ino: s.st_ino,
            mode: s.st_mode,
            nlink: s.st_nlink,
            uid: s.st_uid,
            gid: s.st_gid,
            size: s.st_size as u64,
            blocks: 0,
        })
    }

    fn truncate(&self, len: i64) -> Result<(), ()> {
        self.node.truncate(len)
    }

    fn poll_readable(&self) -> bool {
        if self.node.is_dir() {
            return true;
        }
        // For dirs, check children; for files, try reading a byte
        if let Ok(children) = self.node.children() {
            return !children.is_empty();
        }
        self.node.read(1).is_ok_and(|d| !d.is_empty())
    }

    fn poll_writable(&self) -> bool {
        self.node.write(&[]).is_ok()
    }
}

/// Helper: open a VFS path and return a KernelObject handle.
/// Used by sys_open to create handles with bind-time security.
pub fn resolve_as_object(path: &str) -> Option<Arc<VfsObject>> {
    let vfs = VFS.lock();
    let node = vfs.resolve_path(path)?;
    let type_id = if node.is_dir() {
        ObjectTypeId(2)
    } else {
        ObjectTypeId(1)
    };
    Some(VfsObject::new(node, type_id))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path;

    // ── Path utilities ──────────────────────────────────────────────────────

    #[test]
    fn path_split_root() {
        // split("/") finds '/' at index 0, parent="", name=""
        let (parent, name) = path::split("/");
        assert_eq!(parent, "");
        assert_eq!(name, "");
    }

    #[test]
    fn path_split_simple() {
        let (parent, name) = path::split("/foo/bar");
        assert_eq!(parent, "/foo");
        assert_eq!(name, "bar");
    }

    #[test]
    fn path_split_no_slash() {
        let (parent, name) = path::split("file.txt");
        assert_eq!(parent, ".");
        assert_eq!(name, "file.txt");
    }

    #[test]
    fn path_split_deep() {
        let (parent, name) = path::split("/a/b/c/d.txt");
        assert_eq!(parent, "/a/b/c");
        assert_eq!(name, "d.txt");
    }

    #[test]
    fn path_is_absolute() {
        assert!(path::is_absolute("/foo"));
        assert!(path::is_absolute("/"));
        assert!(!path::is_absolute("foo"));
        assert!(!path::is_absolute("./foo"));
    }

    // ── VfsManager: mount / umount ──────────────────────────────────────────

    struct MockFs {
        root_name: &'static str,
    }
    impl FileSystem for MockFs {
        fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
            Ok(Arc::new(MockNode {
                name: String::from(self.root_name),
                is_dir: true,
            }))
        }
    }

    struct MockNode {
        name: String,
        is_dir: bool,
    }
    impl VfsNode for MockNode {
        fn name(&self) -> String {
            self.name.clone()
        }
        fn is_dir(&self) -> bool {
            self.is_dir
        }
        fn read(&self, _: usize) -> Result<Vec<u8>, ()> {
            Ok(Vec::new())
        }
        fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
            Ok(Vec::new())
        }
    }

    fn mock_fs(name: &'static str) -> Arc<dyn FileSystem> {
        Arc::new(MockFs { root_name: name })
    }

    #[test]
    fn mount_adds_entry() {
        let mut vfs = VfsManager::new();
        assert!(vfs.mounts.is_empty());
        vfs.mount("/", mock_fs("root"));
        assert_eq!(vfs.mounts.len(), 1);
        assert_eq!(vfs.mounts[0].path, "/");
    }

    #[test]
    fn mount_normalizes_path() {
        let mut vfs = VfsManager::new();
        vfs.mount("/foo", mock_fs("foo"));
        assert_eq!(vfs.mounts[0].path, "/foo");

        // Trailing slash removed
        vfs.mount("/bar/", mock_fs("bar"));
        assert_eq!(vfs.mounts[1].path, "/bar");

        // Leading slash added
        vfs.mount("baz", mock_fs("baz"));
        assert_eq!(vfs.mounts[2].path, "/baz");
    }

    #[test]
    fn mount_sorts_by_path_length_desc() {
        let mut vfs = VfsManager::new();
        vfs.mount("/", mock_fs("root"));
        vfs.mount("/usr", mock_fs("usr"));
        vfs.mount("/usr/bin", mock_fs("bin"));
        // Longest path first
        assert_eq!(vfs.mounts[0].path, "/usr/bin");
        assert_eq!(vfs.mounts[1].path, "/usr");
        assert_eq!(vfs.mounts[2].path, "/");
    }

    #[test]
    fn umount_removes_entry() {
        let mut vfs = VfsManager::new();
        vfs.mount("/", mock_fs("root"));
        vfs.mount("/usr", mock_fs("usr"));
        assert_eq!(vfs.mounts.len(), 2);
        vfs.umount("/usr").unwrap();
        assert_eq!(vfs.mounts.len(), 1);
        assert_eq!(vfs.mounts[0].path, "/");
    }

    #[test]
    fn umount_nonexistent_returns_err() {
        let mut vfs = VfsManager::new();
        assert!(vfs.umount("/nope").is_err());
    }

    #[test]
    fn umount_normalizes_path() {
        let mut vfs = VfsManager::new();
        vfs.mount("/foo", mock_fs("foo"));
        // umount without leading slash
        vfs.umount("foo").unwrap();
        assert!(vfs.mounts.is_empty());
    }

    // ── VfsManager: resolve_path ────────────────────────────────────────────

    #[test]
    fn resolve_root_path() {
        let mut vfs = VfsManager::new();
        vfs.mount("/", mock_fs("root"));
        let node = vfs.resolve_path("/").unwrap();
        assert_eq!(node.name(), "root");
    }

    #[test]
    fn resolve_subpath() {
        let mut vfs = VfsManager::new();
        // Create a filesystem with a child named "bin"
        struct ChildFs;
        impl FileSystem for ChildFs {
            fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
                let root = Arc::new(MockNode {
                    name: String::from("root"),
                    is_dir: true,
                });
                Ok(root)
            }
        }
        vfs.mount("/usr", Arc::new(ChildFs));
        let node = vfs.resolve_path("/usr").unwrap();
        assert_eq!(node.name(), "root");
    }

    #[test]
    fn resolve_nonexistent_returns_none() {
        let vfs = VfsManager::new();
        assert!(vfs.resolve_path("/nothing").is_none());
    }

    #[test]
    fn resolve_longest_mount_wins() {
        let mut vfs = VfsManager::new();
        vfs.mount("/", mock_fs("root"));
        vfs.mount("/usr", mock_fs("usr"));
        // /usr should resolve through the /usr mount, not /
        let node = vfs.resolve_path("/usr").unwrap();
        assert_eq!(node.name(), "usr");
    }
}
