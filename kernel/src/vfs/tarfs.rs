use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::string::ToString;
use crate::vfs::{FileSystem, VfsNode, Stat, S_IFDIR, S_IFREG, S_IFLNK};
use crate::drivers::block::BlockDevice;
use spin::Mutex;

// ── Shared node type ──────────────────────────────────────────────────────────

struct TarNode {
    name: String,
    is_dir: bool,
    is_symlink: bool,
    link_target: Option<String>,
    data: Option<Vec<u8>>,
    children: Mutex<Vec<Arc<TarNode>>>,
}

impl VfsNode for TarNode {
    fn name(&self) -> String { self.name.clone() }
    fn is_dir(&self) -> bool { self.is_dir && !self.is_symlink }
    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        if self.is_symlink {
            return self.link_target.clone().map(|s| s.into_bytes()).ok_or(());
        }
        self.data.as_ref().cloned().ok_or(())
    }
    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir { return Err(()); }
        let children = self.children.lock();
        Ok(children.iter().map(|c| c.clone() as Arc<dyn VfsNode>).collect())
    }
    fn stat(&self) -> Result<Stat, ()> {
        let st_mode = if self.is_symlink {
            S_IFLNK | 0o777
        } else if self.is_dir {
            S_IFDIR | 0o555
        } else {
            S_IFREG | 0o555
        };
        let st_size = if self.is_symlink {
            self.link_target.as_ref().map(|s| s.len() as i64).unwrap_or(0)
        } else {
            self.data.as_ref().map(|d| d.len() as i64).unwrap_or(4096)
        };
        Ok(Stat {
            st_dev: 0, st_ino: 0,
            st_mode,
            st_nlink: 1, st_uid: 0, st_gid: 0, st_rdev: 0,
            st_size,
            st_atime: 0, st_mtime: 0, st_ctime: 0,
        })
    }
    fn readlink(&self) -> Result<String, ()> {
        if !self.is_symlink {
            return Err(());
        }
        self.link_target.clone().ok_or(())
    }
}

// ── Shared parser ─────────────────────────────────────────────────────────────

fn parse_tar(data: &[u8]) -> Arc<TarNode> {
    let root = Arc::new(TarNode {
        name: String::from("/"),
        is_dir: true,
        is_symlink: false,
        link_target: None,
        data: None,
        children: Mutex::new(Vec::new()),
    });

    let mut offset = 0;
    let mut count = 0u32;
    while offset + 512 <= data.len() {
        let header = &data[offset..offset + 512];
        if header[0] == 0 { break; }

        let name = core::str::from_utf8(&header[0..100]).unwrap_or("").trim_matches('\0');
        let size_str = core::str::from_utf8(&header[124..136]).unwrap_or("").trim().trim_matches('\0');
        let size = usize::from_str_radix(size_str, 8).unwrap_or(0);
        let type_flag = header[156];
        let is_dir = type_flag == b'5' || name.ends_with('/');
        let is_symlink = type_flag == b'2';

        let link_target = if is_symlink {
            Some(core::str::from_utf8(&header[157..317]).unwrap_or("").trim_matches('\0').to_string())
        } else {
            None
        };

        let end = (offset + 512 + size).min(data.len());
        let file_data = if !is_dir && size > 0 { &data[offset + 512..end] } else { &[] };

        add_to_tree(root.clone(), name, is_dir, is_symlink, link_target, file_data);
        offset += 512 + ((size + 511) & !511);
        count += 1;
    }

    crate::serial_write("[TARFS] parsed "); 
    let mut b = [0u8; 12]; let mut i = 12u8; let mut n = count;
    loop { i -= 1; b[i as usize] = b'0' + (n % 10) as u8; n /= 10; if n == 0 { break; } }
    crate::serial_write(core::str::from_utf8(&b[i as usize..]).unwrap_or("?"));
    crate::serial_write(" entries\n");
    root
}

fn add_to_tree(root: Arc<TarNode>, path: &str, is_dir: bool, is_symlink: bool, link_target: Option<String>, data: &[u8]) {
    let components: Vec<&str> = path.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if components.is_empty() { return; }
    let mut current = root;

    for (i, comp) in components.iter().enumerate() {
        let is_last = i == components.len() - 1;

        let next = {
            let children = current.children.lock();
            children.iter().find(|c| c.name == *comp).cloned()
        };

        if let Some(existing) = next {
            current = existing;
        } else {
            let node_link = if is_last && is_symlink { link_target.clone() } else { None };
            let node = Arc::new(TarNode {
                name: String::from(*comp),
                is_dir: if is_last { is_dir } else { true },
                is_symlink: is_last && is_symlink,
                link_target: node_link,
                data: if is_last && !is_dir && !is_symlink { Some(Vec::from(data)) } else { None },
                children: Mutex::new(Vec::new()),
            });
            current.children.lock().push(node.clone());
            current = node;
        }
    }
}

// ── TarfsMemory — from embedded &[u8] (initrd baked into kernel) ──────────────

pub struct TarfsMemory {
    root: Arc<TarNode>,
}

impl TarfsMemory {
    pub fn new(data: &[u8]) -> Self {
        TarfsMemory { root: parse_tar(data) }
    }
}

impl FileSystem for TarfsMemory {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}

// ── Tarfs — from block device (second drive, fallback) ────────────────────────

pub struct Tarfs {
    root: Arc<TarNode>,
}

impl Tarfs {
    pub fn new(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Self, ()> {
        let mut data = Vec::new();
        let mut block: u64 = 0;
        loop {
            let mut buf = [0u8; 512];
            if device.lock().read_sector(block, &mut buf).is_err() { break; }
            if block == 0 && buf[0] != b'.' && buf[0] != b'/' { return Err(()); }
            data.extend_from_slice(&buf);
            block += 1;
            if block > 131072 { break; } // 64MB limit
        }
        if data.is_empty() { return Err(()); }
        Ok(Tarfs { root: parse_tar(&data) })
    }
}

impl FileSystem for Tarfs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}
