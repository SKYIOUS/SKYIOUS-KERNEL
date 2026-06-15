use alloc::vec::Vec;
use alloc::string::String;
use crate::drivers::block::BlockDevice;
use alloc::sync::Arc;
use spin::Mutex;

pub mod ramfs;
pub mod fat;
pub mod ext2;
pub mod pipe;
pub mod tarfs;
pub mod devfs;
pub mod ctlfs;
pub mod skyfs;

pub trait FileSystem: Send + Sync {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()>;
}

#[derive(Debug, Clone, Copy)]
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
            for child in children {
                if child.name() == name {
                    return Some(child);
                }
            }
        }
        None
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
}

pub struct MountPoint {
    pub path: String,
    pub fs: Arc<dyn FileSystem>,
}

pub struct VfsManager {
    mounts: Vec<MountPoint>,
}

impl VfsManager {
    pub fn statfs_mount(&self, path: &str) -> Option<Arc<dyn VfsNode>> {
        self.mounts.iter()
            .filter(|m| path == m.path || path.starts_with(&m.path))
            .max_by_key(|m| m.path.len())
            .and_then(|m| m.fs.root().ok())
    }

    pub const fn new() -> Self {
        VfsManager { mounts: Vec::new() }
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
    }

    const MAX_SYMLINK_DEPTH: usize = 40;

    pub fn resolve_path(&self, path: &str) -> Option<Arc<dyn VfsNode>> {
        self.resolve_path_with_depth(path, 0)
    }

    fn resolve_path_with_depth(&self, path: &str, depth: usize) -> Option<Arc<dyn VfsNode>> {
        if depth > Self::MAX_SYMLINK_DEPTH {
            return None;
        }
        let cwd = {
            let proc_lock = crate::task::process::CURRENT_PROCESS.lock();
            if let Some(ref proc) = *proc_lock {
                let tl = proc.cwd.try_lock();
                match tl {
                    Some(g) => {
                        let s = g.clone();
                        drop(g);
                        s
                    }
                    None => {
                        String::from("/")
                    }
                }
            } else {
                String::from("/")
            }
        };

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
            if m.path == "/" {
                true
            } else if path_norm == m.path {
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

        {
            let mut comps = components.as_slice();
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
                                    for &c in components[..components.len() - rest.len() - 1].iter() {
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

        let pos = self.mounts.iter().position(|m| m.path == path_fixed).ok_or(())?;
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

    fn search_recursive(&self, node: Arc<dyn VfsNode>, current_path: &str, pattern: &str, results: &mut Vec<String>) {
        if node.name().contains(pattern) {
            results.push(String::from(current_path));
        }

        if node.is_dir() {
            if let Ok(children) = node.children() {
                for child in children {
                    let child_name = child.name();
                    if child_name == "." || child_name == ".." { continue; }
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
pub static BOOT_DEVICE: spin::Mutex<Option<usize>> = spin::Mutex::new(None);

/// Set the boot device by index into BLOCK_DEVICES.
#[allow(dead_code)]
pub fn set_boot_device(index: usize) {
    *BOOT_DEVICE.lock() = Some(index);
    crate::println!("VFS: boot device set to block device {}", index);
}

pub static VFS: Mutex<VfsManager> = Mutex::new(VfsManager::new());

pub fn init() {
    let mut vfs = VFS.lock();

    // Try to mount root from a block device first
    let root_mounted = {
        let devices = crate::drivers::block::BLOCK_DEVICES.lock();
        let boot_idx = *BOOT_DEVICE.lock();
        let mut mounted = false;

        if let Some(idx) = boot_idx {
            if let Some(dev) = devices.get(idx) {
                crate::println!("VFS: Attempting root from block device {}...", idx);
                if let Ok(ext2fs) = ext2::mount(dev.clone()) {
                    vfs.mount("/", ext2fs);
                    crate::println!("VFS: Root filesystem mounted from block device {} (ext2).", idx);
                    mounted = true;
                } else if let Ok(skyfs) = skyfs::SkyFSHandle::mount(dev.clone()) {
                    vfs.mount("/", skyfs);
                    crate::println!("VFS: Root filesystem mounted from block device {} (SkyFS).", idx);
                    mounted = true;
                }
            }
        } else {
            // Check for any ext2 partition on first block device
            if let Some(dev) = devices.first() {
                let partitions = crate::drivers::block::partition::parse_partitions(dev);
                if let Some(part) = partitions.first() {
                    let part_dev = Arc::new(spin::Mutex::new(
                        crate::drivers::block::partition::PartitionDevice::new(
                            dev.clone(), part.lba_start, part.sector_count,
                        )
                    ));
                    if let Ok(ext2fs) = ext2::mount(part_dev.clone()) {
                        vfs.mount("/", ext2fs);
                        crate::println!("VFS: Root filesystem mounted from first partition (ext2).");
                        mounted = true;
                    } else if let Ok(skyfs) = skyfs::SkyFSHandle::mount(part_dev) {
                        vfs.mount("/", skyfs);
                        crate::println!("VFS: Root filesystem mounted from first partition (SkyFS).");
                        mounted = true;
                    }
                }
            }
            if !mounted {
                // Try the whole device
                if let Some(dev) = devices.first() {
                    if let Ok(ext2fs) = ext2::mount(dev.clone()) {
                        vfs.mount("/", ext2fs);
                        crate::println!("VFS: Root filesystem mounted from first block device (ext2).");
                        mounted = true;
                    } else if let Ok(skyfs) = skyfs::SkyFSHandle::mount(dev.clone()) {
                        vfs.mount("/", skyfs);
                        crate::println!("VFS: Root filesystem mounted from first block device (SkyFS).");
                        mounted = true;
                    }
                }
            }
        }
        mounted
    };

    if !root_mounted {
        // Fall back to embedded initrd
        let _initrd_hash = env!("INITRD_HASH");
        static INITRD: &[u8] = include_bytes!("../../../SkyOS/initrd.tar");
        let initrd_fs = Arc::new(tarfs::TarfsMemory::new(INITRD));
        vfs.mount("/", initrd_fs);
        crate::println!("VFS: Mounted embedded initrd ({} bytes) as root.", INITRD.len());
    }

    // Mount DevFS at /dev
    let devfs = Arc::new(devfs::DevFs::new());
    vfs.mount("/dev", devfs.clone());
    crate::println!("VFS: Mounted DevFS at /dev.");

    // Mount ctlFS at /ctl (Plan9-style control filesystem replacing /proc + /sys)
    let ctlfs = Arc::new(ctlfs::CtlFs::new());
    vfs.mount("/ctl", ctlfs);
    crate::println!("VFS: Mounted CtlFs at /ctl.");

    // Mount a tmpfs for /tmp (writable shared temporary storage)
    let ramfs = Arc::new(ramfs::Tmpfs::new());
    vfs.mount("/tmp", ramfs);
    crate::println!("VFS: Mounted Tmpfs at /tmp.");


    // Scan block devices for partitions and mount filesystems
    let device_snapshots: Vec<_> = {
        let blk = crate::drivers::block::BLOCK_DEVICES.lock();
        blk.iter().enumerate().map(|(i, d)| (i, d.clone())).collect()
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
        let mount_path_ext2 = alloc::format!("/mnt/ext2_{}", i);
        if let Ok(ext2fs) = ext2::mount(dev.clone()) {
            vfs.mount(&mount_path_ext2, ext2fs);
            crate::println!("VFS: Mounted Ext2 at {}", mount_path_ext2);
        }

        let mount_path_fat = alloc::format!("/mnt/fat_{}", i);
        if let Ok(fatfs) = fat::FatFileSystem::new(dev.clone()) {
            vfs.mount(&mount_path_fat, Arc::new(fatfs));
            crate::println!("VFS: Mounted FAT32 at {}", mount_path_fat);
        }

        let mount_path_tar = alloc::format!("/mnt/tar_{}", i);
        if let Ok(tarfs) = tarfs::Tarfs::new(dev.clone()) {
            vfs.mount(&mount_path_tar, Arc::new(tarfs));
            crate::println!("VFS: Mounted TarFS at {}", mount_path_tar);
        }

        let mount_path_sky = alloc::format!("/mnt/skyfs_{}", i);
        if let Ok(skyfs) = skyfs::SkyFSHandle::mount(dev.clone()) {
            vfs.mount(&mount_path_sky, skyfs);
            crate::println!("VFS: Mounted SkyFS at {}", mount_path_sky);
        }

        // Scan and register partitions
        let partitions = crate::drivers::block::partition::parse_partitions(&dev);
        for (_p_idx, part) in partitions.iter().enumerate() {
            let part_name = alloc::format!("{}{}", dev_name, part.index);

            // Register partition as a block device
            let part_dev = Arc::new(spin::Mutex::new(
                crate::drivers::block::partition::PartitionDevice::new(
                    dev.clone(), part.lba_start, part.sector_count,
                )
            ));
            let part_dev_idx = crate::drivers::block::BLOCK_DEVICES.lock().len();
            crate::drivers::block::register_block_device(part_dev.clone());

            // Add partition device node to DevFS
            devfs.add_block_device(&part_name, part_dev_idx);

            // Try to mount filesystems on the partition
            let mount_path_ext2p = alloc::format!("/mnt/ext2_{}_{}", i, part.index);
            if let Ok(ext2fs) = ext2::mount(part_dev.clone()) {
                vfs.mount(&mount_path_ext2p, ext2fs);
                crate::println!("VFS: Mounted Ext2 on {} at {}", part_name, mount_path_ext2p);
            }

            let mount_path_fatp = alloc::format!("/mnt/fat_{}_{}", i, part.index);
            if let Ok(fatfs) = fat::FatFileSystem::new(part_dev.clone()) {
                vfs.mount(&mount_path_fatp, Arc::new(fatfs));
                crate::println!("VFS: Mounted FAT32 on {} at {}", part_name, mount_path_fatp);
            }

            let mount_path_skyp = alloc::format!("/mnt/skyfs_{}_{}", i, part.index);
            if let Ok(skyfs) = skyfs::SkyFSHandle::mount(part_dev.clone()) {
                vfs.mount(&mount_path_skyp, skyfs);
                crate::println!("VFS: Mounted SkyFS on {} at {}", part_name, mount_path_skyp);
            }
        }
    }
}

pub fn _mount_fat32(_path: &str, _device: Arc<Mutex<dyn BlockDevice>>) {
}


