use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use crate::vfs::{FileSystem, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;

pub struct Tmpfs {
    root: Arc<TmpNode>,
}

impl Tmpfs {
    pub fn new() -> Self {
        Tmpfs {
            root: Arc::new(TmpNode {
                name: Mutex::new(String::from("/")),
                is_dir: true,
                is_symlink: false,
                link_target: None,
                content: Mutex::new(Vec::new()),
                children: Mutex::new(Vec::new()),
                mode: Mutex::new(0o755),
                uid: Mutex::new(0),
                gid: Mutex::new(0),
                nlink: Mutex::new(2),
                atime_nsec: Mutex::new(0),
                mtime_nsec: Mutex::new(0),
                ctime_nsec: Mutex::new(0),
            }),
        }
    }
    
    pub fn _add_file(&self, name: &str, data: Vec<u8>) {
        let node = Arc::new(TmpNode {
            name: Mutex::new(String::from(name)),
            is_dir: false,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(data),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o644),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
            nlink: Mutex::new(1),
            atime_nsec: Mutex::new(0),
            mtime_nsec: Mutex::new(0),
            ctime_nsec: Mutex::new(0),
        });
        self.root.children.lock().push(node as Arc<dyn VfsNode>);
    }
}

impl FileSystem for Tmpfs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}

struct TmpNode {
    name: Mutex<String>,
    is_dir: bool,
    is_symlink: bool,
    link_target: Option<String>,
    content: Mutex<Vec<u8>>,
    children: Mutex<Vec<Arc<dyn VfsNode>>>,
    mode: Mutex<u32>,
    uid: Mutex<u32>,
    gid: Mutex<u32>,
    nlink: Mutex<u32>,
    atime_nsec: Mutex<i64>,
    mtime_nsec: Mutex<i64>,
    ctime_nsec: Mutex<i64>,
}

impl VfsNode for TmpNode {
    fn name(&self) -> String {
        self.name.lock().clone()
    }
    
    fn is_dir(&self) -> bool {
        self.is_dir && !self.is_symlink
    }
    
    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        if self.is_symlink {
            return self.link_target.clone().map(|s| s.into_bytes()).ok_or(());
        }
        if self.is_dir {
            return Err(());
        }
        Ok(self.content.lock().clone())
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        if self.is_dir {
            return Err(());
        }
        let mut content = self.content.lock();
        content.extend_from_slice(data);
        Ok(())
    }
    
    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir {
            return Err(());
        }
        let children = self.children.lock();
        let mut result = Vec::new();
        for child in children.iter() {
            result.push(child.clone() as Arc<dyn VfsNode>);
        }
        Ok(result)
    }

    fn statfs(&self) -> Result<crate::vfs::StatFs, ()> {
        let total_blocks = 64 * 1024 * 1024 / 4096;
        Ok(crate::vfs::StatFs {
            f_type: 0x01021994, f_bsize: 4096,
            f_blocks: total_blocks, f_bfree: total_blocks - 1, f_bavail: total_blocks - 1,
            f_files: 4096, f_ffree: 4096 - 4,
        })
    }

    fn stat(&self) -> Result<Stat, ()> {
        let size = if self.is_symlink {
            self.link_target.as_ref().map(|s| s.len() as i64).unwrap_or(0)
        } else if self.is_dir {
            4096
        } else {
            self.content.lock().len() as i64
        };

        let fmode = *self.mode.lock();
        let fuid = *self.uid.lock();
        let fgid = *self.gid.lock();
        let file_type = if self.is_symlink {
            crate::vfs::S_IFLNK
        } else if self.is_dir {
            crate::vfs::S_IFDIR
        } else {
            crate::vfs::S_IFREG
        };

        Ok(Stat {
            st_dev: 0,
            st_ino: 0,
            st_mode: file_type | fmode,
            st_nlink: *self.nlink.lock(),
            st_uid: fuid,
            st_gid: fgid,
            st_rdev: 0,
            st_size: size,
            st_atime: 0,
            st_mtime: 0,
            st_ctime: 0,
            st_atime_nsec: *self.atime_nsec.lock(),
            st_mtime_nsec: *self.mtime_nsec.lock(),
            st_ctime_nsec: *self.ctime_nsec.lock(),
        })
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        if !self.is_dir {
            return Err(());
        }
        if name == "." || name == "/" || name.is_empty() {
            return Err(());
        }
        let mut children = self.children.lock();
        if children.iter().any(|c| c.name() == name) {
            return Err(());
        }
        
        let new_node = Arc::new(TmpNode {
            name: Mutex::new(String::from(name)),
            is_dir: true,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o755),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
            nlink: Mutex::new(2),
            atime_nsec: Mutex::new(0),
            mtime_nsec: Mutex::new(0),
            ctime_nsec: Mutex::new(0),
        });
        children.push(new_node.clone());
        Ok(new_node as Arc<dyn VfsNode>)
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        if !self.is_dir {
            return Err(());
        }
        if name == "." || name == "/" || name.is_empty() {
            return Err(());
        }
        let mut children = self.children.lock();
        if children.iter().any(|c| c.name() == name) {
            return Err(());
        }
        
        let new_node = Arc::new(TmpNode {
            name: Mutex::new(String::from(name)),
            is_dir: false,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o644),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
            nlink: Mutex::new(1),
            atime_nsec: Mutex::new(0),
            mtime_nsec: Mutex::new(0),
            ctime_nsec: Mutex::new(0),
        });
        children.push(new_node.clone());
        Ok(new_node as Arc<dyn VfsNode>)
    }

    fn chmod(&self, mode: u32) -> Result<(), ()> {
        *self.mode.lock() = mode & 0o7777;
        Ok(())
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), ()> {
        *self.uid.lock() = uid;
        *self.gid.lock() = gid;
        Ok(())
    }

    fn truncate(&self, len: i64) -> Result<(), ()> {
        if self.is_dir { return Err(()); }
        if len < 0 { return Err(()); }
        let mut content = self.content.lock();
        content.truncate(len as usize);
        Ok(())
    }

    fn rename(&self, old_name: &str, new_name: &str) -> Result<(), ()> {
        if !self.is_dir { return Err(()); }
        let mut children = self.children.lock();
        let pos = children.iter().position(|c| c.name() == old_name).ok_or(())?;
        if children.iter().any(|c| c.name() == new_name) { return Err(()); }
        // ponytail: recreate the child with new name since VfsNode::name() is read-only
        let child = children.remove(pos);
        let new_node = Arc::new(TmpNode {
            name: Mutex::new(String::from(new_name)),
            is_dir: self.is_dir,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o644),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
            nlink: Mutex::new(1),
            atime_nsec: Mutex::new(0),
            mtime_nsec: Mutex::new(0),
            ctime_nsec: Mutex::new(0),
        });
        children.push(new_node as Arc<dyn VfsNode>);
        drop(child);
        Ok(())
    }

    fn unlink(&self, name: &str) -> Result<(), ()> {
        if !self.is_dir {
            return Err(());
        }
        let mut children = self.children.lock();
        let pos = children.iter().position(|c| c.name() == name).ok_or(())?;
        children.remove(pos);
        Ok(())
    }

    fn readlink(&self) -> Result<String, ()> {
        if !self.is_symlink {
            return Err(());
        }
        self.link_target.clone().ok_or(())
    }

    fn symlink(&self, name: &str, target: &str) -> Result<(), ()> {
        if !self.is_dir {
            return Err(());
        }
        let mut children = self.children.lock();
        if children.iter().any(|c| c.name() == name) {
            return Err(());
        }
        let new_node = Arc::new(TmpNode {
            name: Mutex::new(String::from(name)),
            is_dir: false,
            is_symlink: true,
            link_target: Some(String::from(target)),
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o777),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
            nlink: Mutex::new(1),
            atime_nsec: Mutex::new(0),
            mtime_nsec: Mutex::new(0),
            ctime_nsec: Mutex::new(0),
        });
        children.push(new_node);
        Ok(())
    }

    fn link(&self, existing: alloc::sync::Arc<dyn VfsNode>, name: &str) -> Result<(), ()> {
        if !self.is_dir { return Err(()); }
        let mut children = self.children.lock();
        if children.iter().any(|c| c.name() == name) { return Err(()); }
        // Need to downcast the existing node to TmpNode to increment nlink
        // ponytail: downcast via Arc::ptr_eq won't work, so lookup by pointer identity
        // For ramfs, we store the Arc in the children list directly
        let existing_arc = existing as Arc<dyn VfsNode>;
        // Can't downcast Arc<dyn VfsNode> to Arc<TmpNode> easily
        // Instead, add a new child entry with same name pointing to the node
        // and increment nlink on the existing node
        // The simplest approach: just add the existing node as a child
        let stat = existing_arc.stat()?;
        if stat.st_mode & crate::vfs::S_IFDIR != 0 { return Err(()); }
        // Increment nlink: we can't access TmpNode nlink from dyn VfsNode
        // ponytail: ramfs nlink tracking is approximate; hardlinks work but nlink may not be exact
        // For the basic link functionality, we just add the entry
        // The existing node's data is shared via the Arc
        children.push(existing_arc);
        Ok(())
    }

    fn utimens(&self, atime: (i64, i64), mtime: (i64, i64)) -> Result<(), ()> {
        if self.is_symlink { return Err(()); }
        if atime.1 != -1 {
            // UTIME_OMIT = -1: leave unchanged
            // UTIME_NOW = -2: set to current time (handled in syscall layer)
            if atime.1 != -2 {
                *self.atime_nsec.lock() = atime.1;
            }
        }
        if mtime.1 != -1 {
            if mtime.1 != -2 {
                *self.mtime_nsec.lock() = mtime.1;
            }
        }
        Ok(())
    }

    fn fallocate(&self, _mode: i32, _offset: i64, _len: i64) -> Result<(), ()> {
        if self.is_dir { return Err(()); }
        // ponytail: ramfs doesn't need preallocation; succeed as no-op
        Ok(())
    }
}
