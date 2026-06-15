use crate::drivers::block::{BlockDevice, BlockDeviceError};
use crate::alloc::sync::Arc;
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::vfs::FileSystem;
use spin::Mutex;

struct RamBlock {
    data: Vec<u8>,
    sectors: u64,
}

impl BlockDevice for RamBlock {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        let start = sector as usize * 512;
        if start + buf.len() > self.data.len() { return Err(BlockDeviceError::InvalidSector); }
        buf.copy_from_slice(&self.data[start..start + buf.len()]);
        Ok(())
    }
    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        let start = sector as usize * 512;
        if start + buf.len() > self.data.len() { return Err(BlockDeviceError::InvalidSector); }
        self.data[start..start + buf.len()].copy_from_slice(buf);
        Ok(())
    }
    fn sector_count(&self) -> Result<u64, BlockDeviceError> { Ok(self.sectors) }
}

pub fn register() {
    crate::selftest::register("skyfs_format_mount", test_format_mount);
    crate::selftest::register("skyfs_create_file", test_create_file);
    crate::selftest::register("skyfs_write_read", test_write_read);
    crate::selftest::register("skyfs_mkdir_children", test_mkdir_children);
    crate::selftest::register("skyfs_unlink", test_unlink);
}

fn make_ram_disk(sectors: u64) -> Arc<Mutex<dyn BlockDevice>> {
    Arc::new(Mutex::new(RamBlock { data: vec![0u8; sectors as usize * 512], sectors }))
}

fn test_format_mount() -> Result<(), &'static str> {
    let dev = make_ram_disk(65536);
    crate::vfs::skyfs::SkyFSHandle::format(dev.clone()).map_err(|_| "format failed")?;
    let fs = crate::vfs::skyfs::SkyFSHandle::mount(dev).map_err(|_| "mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    if !root.is_dir() { return Err("root should be a dir"); }
    Ok(())
}

fn test_create_file() -> Result<(), &'static str> {
    let dev = make_ram_disk(65536);
    crate::vfs::skyfs::SkyFSHandle::format(dev.clone()).map_err(|_| "format failed")?;
    let fs = crate::vfs::skyfs::SkyFSHandle::mount(dev).map_err(|_| "mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    let file = root.create("test.txt").map_err(|_| "create failed")?;
    if file.is_dir() { return Err("file should not be a dir"); }
    Ok(())
}

fn test_write_read() -> Result<(), &'static str> {
    let dev = make_ram_disk(65536);
    crate::vfs::skyfs::SkyFSHandle::format(dev.clone()).map_err(|_| "format failed")?;
    let fs = crate::vfs::skyfs::SkyFSHandle::mount(dev).map_err(|_| "mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    let file = root.create("hello.txt").map_err(|_| "create failed")?;
    let content = b"Hello SkyFS!";
    file.write(content).map_err(|_| "write failed")?;
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_size != content.len() as i64 { return Err("size mismatch"); }
    let data = file.read(256).map_err(|_| "read failed")?;
    if data.as_slice() != content { return Err("content mismatch"); }
    Ok(())
}

fn test_mkdir_children() -> Result<(), &'static str> {
    let dev = make_ram_disk(65536);
    crate::vfs::skyfs::SkyFSHandle::format(dev.clone()).map_err(|_| "format failed")?;
    let fs = crate::vfs::skyfs::SkyFSHandle::mount(dev).map_err(|_| "mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    root.mkdir("subdir").map_err(|_| "mkdir failed")?;
    let children = root.children().map_err(|_| "children failed")?;
    let names: Vec<crate::alloc::string::String> = children.iter().map(|c| c.name()).collect();
    if !names.iter().any(|n| n == "ino:2") { return Err("expected child ino:2"); }
    let child = root.find_child("subdir").ok_or("find_child failed")?;
    if !child.is_dir() { return Err("subdir should be a dir"); }
    Ok(())
}

fn test_unlink() -> Result<(), &'static str> {
    let dev = make_ram_disk(65536);
    crate::vfs::skyfs::SkyFSHandle::format(dev.clone()).map_err(|_| "format failed")?;
    let fs = crate::vfs::skyfs::SkyFSHandle::mount(dev).map_err(|_| "mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    root.create("todelete.txt").map_err(|_| "create failed")?;
    let before = root.children().map_err(|_| "children failed")?.len();
    if before == 0 { return Err("should have children before unlink"); }
    root.unlink("todelete.txt").map_err(|_| "unlink failed")?;
    let after = root.children().map_err(|_| "children failed")?.len();
    if after != before - 1 { return Err("child count should decrease"); }
    Ok(())
}
