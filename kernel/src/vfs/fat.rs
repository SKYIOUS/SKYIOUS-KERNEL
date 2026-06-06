use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use spin::Mutex;
use crate::vfs::{FileSystem, VfsNode, Stat, S_IFDIR, S_IFREG};
use crate::drivers::block::BlockDevice;
use fatfs::{Read, Write, Seek, SeekFrom};

// We need to implement fatfs traits for our block device

pub struct FatFileSystem {
        _fs: Arc<Mutex<fatfs::FileSystem<BlockIoAdapter>>>,
    root: Arc<FatNode>,
}

impl FatFileSystem {
    pub fn new(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Self, ()> {
        let adapter = BlockIoAdapter::new(device);
        let fs = fatfs::FileSystem::new(adapter, fatfs::FsOptions::new()).map_err(|_| ())?;
        let fs_arc = Arc::new(Mutex::new(fs));
        
        let root_node = Arc::new(FatNode {
            name: String::from("/"),
            is_dir: true,
            fs: fs_arc.clone(),
            path: String::from("/"),
        });

        Ok(FatFileSystem {
            _fs: fs_arc,
            root: root_node,
        })
    }
}

impl FileSystem for FatFileSystem {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}

struct FatNode {
    name: String,
    is_dir: bool,
    fs: Arc<Mutex<fatfs::FileSystem<BlockIoAdapter>>>,
    path: String,
}

impl VfsNode for FatNode {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        self.is_dir
    }

    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        if self.is_dir {
            return Err(());
        }

        let fs = self.fs.lock();
        let mut file = fs.root_dir().open_file(&self.path).map_err(|_| ())?;
        
        let mut data = Vec::new();
        // Read all (simplified)
        let mut buf = [0u8; 512];
        loop {
            let n = file.read(&mut buf).map_err(|_| ())?;
            if n == 0 { break; }
            data.extend_from_slice(&buf[..n]);
        }
        Ok(data)
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir {
            return Err(());
        }

        let fs = self.fs.lock();
        let dir = if self.path == "/" {
            fs.root_dir()
        } else {
            fs.root_dir().open_dir(&self.path).map_err(|_| ())?
        };

        let mut entries = Vec::new();
        for entry_res in dir.iter() {
            let entry = entry_res.map_err(|_| ())?;
            let name = entry.file_name();
            if name == "." || name == ".." { continue; }
            
            let mut sub_path = if self.path == "/" { 
                String::from("/") 
            } else { 
                format!("{}/", self.path) 
            };
            sub_path.push_str(&name);

            entries.push(Arc::new(FatNode {
                name,
                is_dir: entry.is_dir(),
                fs: self.fs.clone(),
                path: sub_path,
            }) as Arc<dyn VfsNode>);
        }
        Ok(entries)
    }

    fn stat(&self) -> Result<Stat, ()> {
        let mode = if self.is_dir { S_IFDIR | 0o755 } else { S_IFREG | 0o644 };
        
        let size = if self.is_dir {
            4096
        } else {
            let fs = self.fs.lock();
            let mut file = fs.root_dir().open_file(&self.path).map_err(|_| ())?;
            file.seek(SeekFrom::End(0)).map_err(|_| ())? as i64
        };

        Ok(Stat {
            st_dev: 1, // Disk device
            st_ino: 0,
            st_mode: mode,
            st_nlink: 1,
            st_uid: 0,
            st_gid: 0,
            st_rdev: 0,
            st_size: size,
            st_atime: 0,
            st_mtime: 0,
            st_ctime: 0,
        })
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        if self.is_dir {
            return Err(());
        }
        let fs = self.fs.lock();
        let mut file = fs.root_dir().open_file(&self.path).map_err(|_| ())?;
        file.truncate().map_err(|_| ())?;
        file.seek(SeekFrom::Start(0)).map_err(|_| ())?;
        file.write_all(data).map_err(|_| ())?;
        Ok(())
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let dir = if self.path == "/" {
            fs.root_dir()
        } else {
            fs.root_dir().open_dir(&self.path).map_err(|_| ())?
        };
        dir.create_file(name).map_err(|_| ())?;

        let mut sub_path = if self.path == "/" {
            String::from("/")
        } else {
            format!("{}/", self.path)
        };
        sub_path.push_str(name);

        Ok(Arc::new(FatNode {
            name: String::from(name),
            is_dir: false,
            fs: self.fs.clone(),
            path: sub_path,
        }) as Arc<dyn VfsNode>)
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let dir = if self.path == "/" {
            fs.root_dir()
        } else {
            fs.root_dir().open_dir(&self.path).map_err(|_| ())?
        };
        dir.create_dir(name).map_err(|_| ())?;

        let mut sub_path = if self.path == "/" {
            String::from("/")
        } else {
            format!("{}/", self.path)
        };
        sub_path.push_str(name);

        Ok(Arc::new(FatNode {
            name: String::from(name),
            is_dir: true,
            fs: self.fs.clone(),
            path: sub_path,
        }) as Arc<dyn VfsNode>)
    }

    fn unlink(&self, name: &str) -> Result<(), ()> {
        let fs = self.fs.lock();
        let dir = if self.path == "/" {
            fs.root_dir()
        } else {
            fs.root_dir().open_dir(&self.path).map_err(|_| ())?
        };
        dir.remove(name).map_err(|_| ())?;
        Ok(())
    }
}

// Adapter to bridge BlockDevice to fatfs expectations
pub struct BlockIoAdapter {
    device: Arc<Mutex<dyn BlockDevice>>,
    offset: u64,
}

impl BlockIoAdapter {
    pub fn new(device: Arc<Mutex<dyn BlockDevice>>) -> Self {
        Self { device, offset: 0 }
    }
}

impl fatfs::IoBase for BlockIoAdapter {
    type Error = ();
}

impl fatfs::Read for BlockIoAdapter {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let sector = self.offset / 512;
        let sector_offset = (self.offset % 512) as usize;
        
        let mut temp_buf = [0u8; 512];
        self.device.lock().read_sector(sector, &mut temp_buf).map_err(|_| ())?;
        
        let to_read = core::cmp::min(buf.len(), 512 - sector_offset);
        buf[..to_read].copy_from_slice(&temp_buf[sector_offset..sector_offset + to_read]);
        
        self.offset += to_read as u64;
        Ok(to_read)
    }
}

impl fatfs::Write for BlockIoAdapter {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let sector = self.offset / 512;
        let sector_offset = (self.offset % 512) as usize;

        let mut temp_buf = [0u8; 512];
        // Must read first if not writing full sector
        if buf.len() < 512 || sector_offset != 0 {
            self.device.lock().read_sector(sector, &mut temp_buf).map_err(|_| ())?;
        }
        
        let to_write = core::cmp::min(buf.len(), 512 - sector_offset);
        temp_buf[sector_offset..sector_offset + to_write].copy_from_slice(&buf[..to_write]);
        
        self.device.lock().write_sector(sector, &temp_buf).map_err(|_| ())?;
        
        self.offset += to_write as u64;
        Ok(to_write)
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl fatfs::Seek for BlockIoAdapter {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, Self::Error> {
        match pos {
            SeekFrom::Start(n) => self.offset = n,
            SeekFrom::Current(n) => self.offset = (self.offset as i64 + n) as u64,
            SeekFrom::End(n) => {
                let count = self.device.lock().sector_count().map_err(|_| ())?;
                self.offset = ((count * 512) as i64).saturating_add(n) as u64;
            }
        }
        Ok(self.offset)
    }
}
