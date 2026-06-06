use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use crate::vfs::{FileSystem, VfsNode, Stat};
use spin::Mutex;

pub struct Tmpfs {
    root: Arc<TmpNode>,
}

impl Tmpfs {
    pub fn new() -> Self {
        Tmpfs {
            root: Arc::new(TmpNode {
                name: String::from("/"),
                is_dir: true,
                is_symlink: false,
                link_target: None,
                content: Mutex::new(Vec::new()),
                children: Mutex::new(Vec::new()),
                mode: Mutex::new(0o755),
                uid: Mutex::new(0),
                gid: Mutex::new(0),
            }),
        }
    }
    
    pub fn _add_file(&self, name: &str, data: Vec<u8>) {
        let node = Arc::new(TmpNode {
            name: String::from(name),
            is_dir: false,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(data),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o644),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
        });
        self.root.children.lock().push(node);
    }
}

impl FileSystem for Tmpfs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}

struct TmpNode {
    name: String,
    is_dir: bool,
    is_symlink: bool,
    link_target: Option<String>,
    content: Mutex<Vec<u8>>,
    children: Mutex<Vec<Arc<TmpNode>>>,
    mode: Mutex<u32>,
    uid: Mutex<u32>,
    gid: Mutex<u32>,
}

impl VfsNode for TmpNode {
    fn name(&self) -> String {
        self.name.clone()
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
            st_nlink: 1,
            st_uid: fuid,
            st_gid: fgid,
            st_rdev: 0,
            st_size: size,
            st_atime: 0,
            st_mtime: 0,
            st_ctime: 0,
        })
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        if !self.is_dir {
            return Err(());
        }
        let mut children = self.children.lock();
        if children.iter().any(|c| c.name == name) {
            return Err(());
        }
        
        let new_node = Arc::new(TmpNode {
            name: String::from(name),
            is_dir: true,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o755),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
        });
        children.push(new_node.clone());
        Ok(new_node as Arc<dyn VfsNode>)
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        if !self.is_dir {
            return Err(());
        }
        let mut children = self.children.lock();
        if children.iter().any(|c| c.name == name) {
            return Err(());
        }
        
        let new_node = Arc::new(TmpNode {
            name: String::from(name),
            is_dir: false,
            is_symlink: false,
            link_target: None,
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o644),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
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

    fn unlink(&self, name: &str) -> Result<(), ()> {
        if !self.is_dir {
            return Err(());
        }
        let mut children = self.children.lock();
        let pos = children.iter().position(|c| c.name == name).ok_or(())?;
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
        if children.iter().any(|c| c.name == name) {
            return Err(());
        }
        let new_node = Arc::new(TmpNode {
            name: String::from(name),
            is_dir: false,
            is_symlink: true,
            link_target: Some(String::from(target)),
            content: Mutex::new(Vec::new()),
            children: Mutex::new(Vec::new()),
            mode: Mutex::new(0o777),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
        });
        children.push(new_node);
        Ok(())
    }
}
