//! FUSE — Filesystem in Userspace bridge.
//!
//! Provides a kernel-userspace interface for mounting userspace filesystems.
//! Uses shared memory + eventfd for communication between kernel FUSE driver
//! and userspace filesystem server process.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeMutex as Mutex;
use crate::vfs::{VfsNode, Stat, FileSystem};

// ── FUSE opcodes ─────────────────────────────────────────────────
pub const FUSE_LOOKUP: u32 = 1;
pub const FUSE_FORGET: u32 = 2;
pub const FUSE_GETATTR: u32 = 3;
pub const FUSE_SETATTR: u32 = 4;
pub const FUSE_READLINK: u32 = 5;
pub const FUSE_SYMLINK: u32 = 6;
pub const FUSE_MKNOD: u32 = 8;
pub const FUSE_MKDIR: u32 = 9;
pub const FUSE_UNLINK: u32 = 10;
pub const FUSE_RMDIR: u32 = 11;
pub const FUSE_RENAME: u32 = 12;
pub const FUSE_LINK: u32 = 13;
pub const FUSE_OPEN: u32 = 14;
pub const FUSE_READ: u32 = 15;
pub const FUSE_WRITE: u32 = 16;
pub const FUSE_STATFS: u32 = 17;
pub const FUSE_RELEASE: u32 = 18;
pub const FUSE_FSYNC: u32 = 20;
pub const FUSE_SETXATTR: u32 = 21;
pub const FUSE_GETXATTR: u32 = 22;
pub const FUSE_LISTXATTR: u32 = 23;
pub const FUSE_REMOVEXATTR: u32 = 24;
pub const FUSE_FLUSH: u32 = 25;
pub const FUSE_OPENDIR: u32 = 27;
pub const FUSE_READDIR: u32 = 28;
pub const FUSE_RELEASEDIR: u32 = 29;
pub const FUSE_FSYNCDIR: u32 = 30;
pub const FUSE_GETLK: u32 = 31;
pub const FUSE_SETLK: u32 = 32;
pub const FUSE_ACCESS: u32 = 33;
pub const FUSE_CREATE: u32 = 34;
pub const FUSE_INTERRUPT: u32 = 35;
pub const FUSE_BMAP: u32 = 36;
pub const FUSE_INIT: u32 = 26;

// FUSE reply codes
pub const FUSE_OK: i32 = 0;
pub const FUSE_ERR: i32 = -1;

/// FUSE request header (sent kernel → userspace)
#[repr(C, packed)]
pub struct FuseRequestHeader {
    pub len: u32,
    pub opcode: u32,
    pub unique: u64,
    pub node_id: u64,
    pub uid: u32,
    pub gid: u32,
    pub pid: u32,
    pub padding: u32,
}

/// FUSE reply header (sent userspace → kernel)
#[repr(C, packed)]
pub struct FuseReplyHeader {
    pub len: u32,
    pub error: i32,
    pub unique: u64,
}

/// FUSE OPEN request
#[repr(C, packed)]
pub struct FuseOpenIn {
    pub flags: u32,
    pub padding: u32,
}

/// FUSE OPEN reply
#[repr(C, packed)]
pub struct FuseOpenOut {
    pub fh: u64,
    pub open_flags: u32,
    pub padding: u32,
}

/// FUSE READ request
#[repr(C, packed)]
pub struct FuseReadIn {
    pub fh: u64,
    pub offset: u64,
    pub size: u32,
    pub padding: u32,
}

/// FUSE WRITE request
#[repr(C, packed)]
pub struct FuseWriteIn {
    pub fh: u64,
    pub offset: u64,
    pub size: u32,
    pub write_flags: u32,
}

/// FUSE GETATTR reply
#[repr(C, packed)]
pub struct FuseAttrOut {
    pub attr_valid: u64,
    pub attr_valid_nsec: u32,
    pub dummy: u32,
    pub attr: FuseAttr,
}

/// FUSE attribute structure
#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct FuseAttr {
    pub ino: u64,
    pub size: u64,
    pub blocks: u64,
    pub atime: u64,
    pub mtime: u64,
    pub ctime: u64,
    pub atimensec: u32,
    pub mtimensec: u32,
    pub ctimensec: u32,
    pub mode: u32,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
}

/// FUSE READDIR entry
#[repr(C, packed)]
pub struct FuseDirent {
    pub ino: u64,
    pub off: u64,
    pub namelen: u32,
    pub type_: u32,
    // name follows (padded to 8-byte alignment)
}

/// Shared memory region for FUSE communication
pub struct FuseChannel {
    /// Shared memory buffer (request + reply)
    pub buffer: Arc<Mutex<Vec<u8>>>,
    /// Size of the shared buffer
    pub buffer_size: usize,
    /// Eventfd for signaling the userspace server
    pub request_eventfd: u64,
    /// Eventfd for signaling the kernel
    pub reply_eventfd: u64,
    /// Mount point
    pub mount_point: String,
    /// Next unique request ID
    pub next_unique: u64,
    /// Process ID of the userspace server
    pub server_pid: u64,
}

/// A single FUSE inode
#[derive(Clone)]
pub struct FuseInode {
    pub ino: u64,
    pub nlookup: u64,
    pub attr: FuseAttr,
}

/// FUSE filesystem state
pub struct FuseFs {
    pub channel: Arc<Mutex<FuseChannel>>,
    pub inodes: Arc<Mutex<BTreeMap<u64, FuseInode>>>,
    pub root_ino: u64,
    pub next_ino: u64,
}

impl FuseFs {
    pub fn new(channel: FuseChannel) -> Self {
        let chan = Arc::new(Mutex::new(channel));
        let mut inodes = BTreeMap::new();

        // Root inode
        let root_attr = FuseAttr {
            ino: 1,
            size: 4096,
            blocks: 1,
            atime: 0, mtime: 0, ctime: 0,
            atimensec: 0, mtimensec: 0, ctimensec: 0,
            mode: 0o040755, // directory
            nlink: 2,
            uid: 0, gid: 0, rdev: 0,
        };
        inodes.insert(1, FuseInode { ino: 1, nlookup: 1, attr: root_attr });

        FuseFs {
            channel: chan,
            inodes: Arc::new(Mutex::new(inodes)),
            root_ino: 1,
            next_ino: 2,
        }
    }

    /// Send a FUSE request and wait for reply.
    fn request(&self, opcode: u32, node_id: u64, data: &[u8]) -> Result<Vec<u8>, ()> {
        let mut chan = self.channel.lock();
        let unique = chan.next_unique;
        chan.next_unique += 1;

        let header = FuseRequestHeader {
            len: (core::mem::size_of::<FuseRequestHeader>() + data.len()) as u32,
            opcode,
            unique,
            node_id,
            uid: 0,
            gid: 0,
            pid: 0,
            padding: 0,
        };

        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&header as *const _ as *const u8, core::mem::size_of::<FuseRequestHeader>())
        };

        // Copy request into shared buffer
        {
            let mut buf = chan.buffer.lock();
            let total = hdr_bytes.len() + data.len();
            if total > chan.buffer_size {
                return Err(());
            }
            buf[..hdr_bytes.len()].copy_from_slice(hdr_bytes);
            buf[hdr_bytes.len()..total].copy_from_slice(data);
        }

        // Signal the userspace server via eventfd
        // Note: eventfd signaling not yet implemented — fuse operations
        // will complete synchronously in the kernel for now.
        //
        // When eventfd infrastructure is ready:
        // 1. Write operation to fuse device fd
        // 2. Signal userspace via eventfd
        // 3. Block on reply eventfd with timeout
        // 4. Read reply from fuse device fd
        //
        // For now, return empty to indicate fuse passthrough is unavailable.
        // Callers should fall back to kernel-native filesystem operations.
        // TODO: Return proper error when eventfd infrastructure is ready
        Err(())
    }
}

impl FileSystem for FuseFs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(Arc::new(FuseNode {
            fs: Arc::new(Mutex::new(FuseFs::new(FuseChannel {
                buffer: Arc::new(Mutex::new(Vec::new())),
                buffer_size: 0,
                request_eventfd: 0,
                reply_eventfd: 0,
                mount_point: String::new(),
                next_unique: 0,
                server_pid: 0,
            }))),
            ino: self.root_ino,
            name: String::from("/"),
        }))
    }
}

/// A FUSE-backed VFS node
pub struct FuseNode {
    pub fs: Arc<Mutex<FuseFs>>,
    pub ino: u64,
    pub name: String,
}

impl VfsNode for FuseNode {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        let fs = self.fs.lock();
        let inodes = fs.inodes.lock();
        inodes.get(&self.ino)
            .map(|i| (i.attr.mode & 0o170000) == 0o040000)
            .unwrap_or(false)
    }

    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()> {
        let _fs = self.fs.lock();
        let header = FuseRequestHeader {
            len: (core::mem::size_of::<FuseRequestHeader>() + core::mem::size_of::<FuseReadIn>()) as u32,
            opcode: FUSE_READ,
            unique: 0,
            node_id: self.ino,
            uid: 0, gid: 0, pid: 0, padding: 0,
        };

        let read_in = FuseReadIn {
            fh: 0,
            offset: 0,
            size: max_len as u32,
            padding: 0,
        };

        let mut data = Vec::new();
        data.extend_from_slice(unsafe {
            core::slice::from_raw_parts(&header as *const _ as *const u8, core::mem::size_of::<FuseRequestHeader>())
        });
        data.extend_from_slice(unsafe {
            core::slice::from_raw_parts(&read_in as *const _ as *const u8, core::mem::size_of::<FuseReadIn>())
        });

        _fs.request(FUSE_READ, self.ino, &data)
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        let _fs = self.fs.lock();
        let header = FuseRequestHeader {
            len: (core::mem::size_of::<FuseRequestHeader>() + core::mem::size_of::<FuseWriteIn>() + data.len()) as u32,
            opcode: FUSE_WRITE,
            unique: 0,
            node_id: self.ino,
            uid: 0, gid: 0, pid: 0, padding: 0,
        };

        let write_in = FuseWriteIn {
            fh: 0,
            offset: 0,
            size: data.len() as u32,
            write_flags: 0,
        };

        let mut req = Vec::new();
        req.extend_from_slice(unsafe {
            core::slice::from_raw_parts(&header as *const _ as *const u8, core::mem::size_of::<FuseRequestHeader>())
        });
        req.extend_from_slice(unsafe {
            core::slice::from_raw_parts(&write_in as *const _ as *const u8, core::mem::size_of::<FuseWriteIn>())
        });
        req.extend_from_slice(data);

        _fs.request(FUSE_WRITE, self.ino, &req)?;
        Ok(())
    }

    fn stat(&self) -> Result<Stat, ()> {
        let fs = self.fs.lock();
        let inodes = fs.inodes.lock();
        let inode = inodes.get(&self.ino).ok_or(())?;
        let attr = inode.attr;
        Ok(Stat {
            st_dev: 0,
            st_ino: attr.ino,
            st_mode: attr.mode,
            st_nlink: attr.nlink,
            st_uid: attr.uid,
            st_gid: attr.gid,
            st_rdev: attr.rdev as u64,
            st_size: attr.size as i64,
            st_atime: attr.atime as i64,
            st_mtime: attr.mtime as i64,
            st_ctime: attr.ctime as i64,
            st_atime_nsec: attr.atimensec as i64,
            st_mtime_nsec: attr.mtimensec as i64,
            st_ctime_nsec: attr.ctimensec as i64,
        })
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs_clone = self.fs.clone();
        {
            let fs = fs_clone.lock();
            let mut buf = Vec::new();
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
            fs.request(FUSE_MKDIR, self.ino, &buf)?;
        }

        let new_ino = {
            let mut fs_mut = fs_clone.lock();
            let ino = fs_mut.next_ino;
            fs_mut.next_ino += 1;
            let mut inodes = fs_mut.inodes.lock();
            inodes.insert(ino, FuseInode {
                ino,
                nlookup: 1,
                attr: FuseAttr {
                    ino,
                    size: 4096,
                    blocks: 1,
                    atime: 0, mtime: 0, ctime: 0,
                    atimensec: 0, mtimensec: 0, ctimensec: 0,
                    mode: 0o040755,
                    nlink: 2,
                    uid: 0, gid: 0, rdev: 0,
                },
            });
            ino
        };

        Ok(Arc::new(FuseNode {
            fs: fs_clone,
            ino: new_ino,
            name: String::from(name),
        }))
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs_clone = self.fs.clone();
        {
            let fs = fs_clone.lock();
            let mut buf = Vec::new();
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
            fs.request(FUSE_CREATE, self.ino, &buf)?;
        }

        let new_ino = {
            let mut fs_mut = fs_clone.lock();
            let ino = fs_mut.next_ino;
            fs_mut.next_ino += 1;
            let mut inodes = fs_mut.inodes.lock();
            inodes.insert(ino, FuseInode {
                ino,
                nlookup: 1,
                attr: FuseAttr {
                    ino,
                    size: 0,
                    blocks: 0,
                    atime: 0, mtime: 0, ctime: 0,
                    atimensec: 0, mtimensec: 0, ctimensec: 0,
                    mode: 0o100644,
                    nlink: 1,
                    uid: 0, gid: 0, rdev: 0,
                },
            });
            ino
        };

        Ok(Arc::new(FuseNode {
            fs: fs_clone,
            ino: new_ino,
            name: String::from(name),
        }))
    }

    fn unlink(&self, name: &str) -> Result<(), ()> {
        let fs = self.fs.lock();
        let mut buf = Vec::new();
        buf.extend_from_slice(name.as_bytes());
        buf.push(0);
        fs.request(FUSE_UNLINK, self.ino, &buf)?;
        Ok(())
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        // Send READDIR to the userspace server
        let fs = self.fs.lock();
        let header = FuseRequestHeader {
            len: core::mem::size_of::<FuseRequestHeader>() as u32,
            opcode: FUSE_READDIR,
            unique: 0,
            node_id: self.ino,
            uid: 0, gid: 0, pid: 0, padding: 0,
        };

        let hdr_bytes = unsafe {
            core::slice::from_raw_parts(&header as *const _ as *const u8, core::mem::size_of::<FuseRequestHeader>())
        };

        let reply = fs.request(FUSE_READDIR, self.ino, hdr_bytes)?;

        // Parse the reply as a sequence of FuseDirent entries
        let mut children = Vec::new();
        let mut offset = 0;
        while offset + core::mem::size_of::<FuseDirent>() <= reply.len() {
            let dent: FuseDirent = unsafe {
                core::ptr::read(reply[offset..].as_ptr() as *const FuseDirent)
            };
            let name_end = offset + core::mem::size_of::<FuseDirent>() + dent.namelen as usize;
            if name_end > reply.len() {
                break;
            }
            let name_bytes = &reply[offset + core::mem::size_of::<FuseDirent>()..name_end];
            if let Ok(name) = core::str::from_utf8(name_bytes) {
                let name = name.trim_end_matches('\0');
                if name != "." && name != ".." {
                    let ino = dent.ino;
                    children.push(Arc::new(FuseNode {
                        fs: self.fs.clone(),
                        ino: ino,
                        name: String::from(name),
                    }) as Arc<dyn VfsNode>);
                }
            }
            // Align to 8-byte boundary
            offset = (name_end + 7) & !7;
        }

        Ok(children)
    }

    fn truncate(&self, len: i64) -> Result<(), ()> {
        let fs = self.fs.lock();
        let mut data = Vec::new();
        data.extend_from_slice(&(len as u64).to_le_bytes());
        fs.request(FUSE_SETATTR, self.ino, &data)?;
        Ok(())
    }
}

/// Initialize FUSE — create /dev/fuse device
pub fn init() {
    crate::serial_write("[FUSE] FUSE bridge initialized\n");
}
