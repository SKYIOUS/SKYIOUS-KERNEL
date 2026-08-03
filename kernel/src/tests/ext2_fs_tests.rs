//! Ext2 integration self-tests using a RamBlock-backed device.
//! Creates a minimal valid ext2 filesystem in memory, mounts it,
//! and exercises read/write/mkdir/create operations.

use crate::drivers::block::{BlockDevice, BlockDeviceError};
use crate::alloc::sync::Arc;
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::vfs::FileSystem;
use crate::sync::IrqSafeMutex as Mutex;

const SECTOR_SIZE: usize = 512;
const BLOCK_SIZE: usize = 1024;

struct RamBlock {
    data: Vec<u8>,
    sectors: u64,
}

impl BlockDevice for RamBlock {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        let start = sector as usize * SECTOR_SIZE;
        if start + buf.len() > self.data.len() { return Err(BlockDeviceError::InvalidSector); }
        buf.copy_from_slice(&self.data[start..start + buf.len()]);
        Ok(())
    }
    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        let start = sector as usize * SECTOR_SIZE;
        if start + buf.len() > self.data.len() { return Err(BlockDeviceError::InvalidSector); }
        self.data[start..start + buf.len()].copy_from_slice(buf);
        Ok(())
    }
    fn sector_count(&self) -> Result<u64, BlockDeviceError> { Ok(self.sectors) }
}

fn ram_disk(sectors: u64) -> Arc<Mutex<dyn BlockDevice>> {
    Arc::new(Mutex::new(RamBlock { data: vec![0u8; sectors as usize * SECTOR_SIZE], sectors }))
}

// ─── Minimal ext2 filesystem formatter ─────────────────────────────────

const EXT2_MAGIC: u16 = 0xEF53;
const EXT2_INODE_SIZE: u16 = 128;

fn write_le32(buf: &mut [u8], pos: usize, val: u32) {
    buf[pos..pos+4].copy_from_slice(&val.to_le_bytes());
}

fn write_le16(buf: &mut [u8], pos: usize, val: u16) {
    buf[pos..pos+2].copy_from_slice(&val.to_le_bytes());
}

fn format_ext2(dev: &Arc<Mutex<dyn BlockDevice>>, total_blocks: u32, inodes_per_group: u16) -> Result<(), &'static str> {
    // Pre-compute layout: see blocks used below
    let inodes_per_block = BLOCK_SIZE as u16 / EXT2_INODE_SIZE;
    let inode_blocks = (inodes_per_group + inodes_per_block - 1) / inodes_per_block;
    let data_start_block = 5u32 + inode_blocks as u32;
    // We place root dir at data_start_block, test file at data_start_block+1
    let used_blocks = data_start_block + 2;
    let used_inodes = 4u32; // inodes 0, 1, 2, 3

    let mut sb = [0u8; BLOCK_SIZE];
    // s_inodes_count
    write_le32(&mut sb, 0, inodes_per_group as u32);
    // s_blocks_count
    write_le32(&mut sb, 4, total_blocks);
    // s_r_blocks_count
    write_le32(&mut sb, 8, 0);
    // s_free_blocks_count
    write_le32(&mut sb, 12, total_blocks - used_blocks);
    // s_free_inodes_count
    write_le32(&mut sb, 16, inodes_per_group as u32 - used_inodes);
    // s_first_data_block = 1 (for 1K blocks)
    write_le32(&mut sb, 20, 1);
    // s_log_block_size = 0 (1024 bytes)
    write_le32(&mut sb, 24, 0);
    // s_log_frag_size = 0
    write_le32(&mut sb, 28, 0);
    // s_blocks_per_group
    write_le32(&mut sb, 32, total_blocks);
    // s_frags_per_group
    write_le32(&mut sb, 36, total_blocks);
    // s_inodes_per_group
    write_le32(&mut sb, 40, inodes_per_group as u32);
    // s_magic
    write_le16(&mut sb, 56, EXT2_MAGIC);
    // s_state = 1 (clean)
    write_le16(&mut sb, 58, 1);
    // s_rev_level = 1 (dynamic)
    write_le32(&mut sb, 76, 1);
    // s_first_ino = 11
    write_le32(&mut sb, 84, 11);
    // s_inode_size
    write_le16(&mut sb, 88, EXT2_INODE_SIZE);
    // s_feature_incompat = 0
    write_le32(&mut sb, 96, 0);
    // s_feature_ro_compat = 0

    // Write superblock at block 1 (byte offset 1024, sector 2)
    let sb_sector = (1u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    let mut sector_buf = [0u8; SECTOR_SIZE];
    sector_buf.copy_from_slice(&sb[..SECTOR_SIZE]);
    dev.lock().write_sector(sb_sector, &sector_buf).map_err(|_| "sb write failed")?;
    sector_buf.copy_from_slice(&sb[SECTOR_SIZE..]);
    dev.lock().write_sector(sb_sector + 1, &sector_buf).map_err(|_| "sb write2 failed")?;

    // Block group descriptor at block 2
    let mut gd = [0u8; BLOCK_SIZE];
    // bg_block_bitmap = block 3
    write_le32(&mut gd, 0, 3);
    // bg_inode_bitmap = block 4
    write_le32(&mut gd, 4, 4);
    // bg_inode_table = block 5
    write_le32(&mut gd, 8, 5);
    // bg_free_blocks_count
    write_le16(&mut gd, 12, (total_blocks - used_blocks) as u16);
    // bg_free_inodes_count
    write_le16(&mut gd, 14, inodes_per_group as u16 - used_inodes as u16);
    // bg_used_dirs_count = 1 (just root)
    write_le16(&mut gd, 16, 1);

    let gd_sector = (2u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    sector_buf.copy_from_slice(&gd[..SECTOR_SIZE]);
    dev.lock().write_sector(gd_sector, &sector_buf).map_err(|_| "gd write failed")?;

    // Block bitmap at block 3: mark blocks 0..used_blocks as used
    let mut bmap = [0u8; BLOCK_SIZE];
    for b in 0..used_blocks as usize {
        bmap[b / 8] |= 1 << (b % 8);
    }

    let bmap_sector = (3u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    for i in 0..2 {
        let off = i * SECTOR_SIZE;
        let mut sec = [0u8; SECTOR_SIZE];
        sec.copy_from_slice(&bmap[off..off + SECTOR_SIZE]);
        dev.lock().write_sector(bmap_sector + i as u64, &sec).map_err(|_| "bmap write failed")?;
    }

    // Inode bitmap at block 4: mark inodes 0, 1, 2, 3 as used
    let mut imap = [0u8; BLOCK_SIZE];
    imap[0] = 0x0F; // bits 0, 1, 2, 3 set

    let imap_sector = (4u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    for i in 0..2 {
        let off = i * SECTOR_SIZE;
        let mut sec = [0u8; SECTOR_SIZE];
        sec.copy_from_slice(&imap[off..off + SECTOR_SIZE]);
        dev.lock().write_sector(imap_sector + i as u64, &sec).map_err(|_| "imap write failed")?;
    }

    // Inode table at blocks 5..5+inode_blocks-1
    let root_inode_num = 2u32;
    let root_data_block = data_start_block;

    // Inode 2: root directory
    let mut inode_buf = [0u8; 128];
    // i_mode: directory (0x4000) | 0x1FF (777 permissions for test)
    write_le16(&mut inode_buf, 0, 0x41FF);
    // i_uid = 0
    write_le16(&mut inode_buf, 2, 0);
    // i_size = BLOCK_SIZE
    write_le32(&mut inode_buf, 4, BLOCK_SIZE as u32);
    // i_links_count = 2
    write_le16(&mut inode_buf, 26, 2);
    // i_blocks = BLOCK_SIZE/512 = 2
    write_le32(&mut inode_buf, 28, 2);
    // i_block[0] = root_data_block
    write_le32(&mut inode_buf, 40, root_data_block);

    let itable_sector = (5u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    // Inode 2 is at index 1 (0-indexed) within the inode table
    let inode_offset = 1u64 * EXT2_INODE_SIZE as u64;
    let sector_offset = (itable_sector * SECTOR_SIZE as u64) + inode_offset;
    let sector_num = sector_offset / SECTOR_SIZE as u64;
    let byte_off = (sector_offset % SECTOR_SIZE as u64) as usize;
    let mut sec = [0u8; SECTOR_SIZE];
    dev.lock().read_sector(sector_num, &mut sec).map_err(|_| "itable read failed")?;
    sec[byte_off..byte_off + 128].copy_from_slice(&inode_buf);
    dev.lock().write_sector(sector_num, &sec).map_err(|_| "itable write failed")?;

    // Root directory data block: "." and ".." entries
    let mut dir_block = [0u8; BLOCK_SIZE];
    // "." entry: inode=2, rec_len=12, name_len=1, type=2
    write_le32(&mut dir_block, 0, root_inode_num);
    write_le16(&mut dir_block, 4, 12);
    dir_block[6] = 1; // name_len
    dir_block[7] = 2; // file_type = directory
    dir_block[8] = b'.';
    // ".." entry: inode=2, rec_len=BLOCK_SIZE-12, name_len=2, type=2
    write_le32(&mut dir_block, 12, root_inode_num);
    write_le16(&mut dir_block, 16, (BLOCK_SIZE - 12) as u16);
    dir_block[18] = 2; // name_len
    dir_block[19] = 2; // file_type
    dir_block[20] = b'.';
    dir_block[21] = b'.';

    let root_data_sector = (root_data_block as u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    for i in 0..2 {
        let off = i * SECTOR_SIZE;
        let mut sec = [0u8; SECTOR_SIZE];
        sec.copy_from_slice(&dir_block[off..off + SECTOR_SIZE]);
        dev.lock().write_sector(root_data_sector + i as u64, &sec).map_err(|_| "rootdir write failed")?;
    }

    // Inode 3: a test file "hello.txt" with content "Hello ext2!"
    let test_inode_num = 3u32;
    let test_data_block = data_start_block + 1;
    let test_content = b"Hello from ext2!\n";
    let test_inode = {
        let mut inode = [0u8; 128];
        write_le16(&mut inode, 0, 0x81FF); // regular file, 777
        write_le16(&mut inode, 2, 0);
        write_le32(&mut inode, 4, test_content.len() as u32);
        write_le16(&mut inode, 26, 1);
        write_le32(&mut inode, 28, 2);
        write_le32(&mut inode, 40, test_data_block);
        inode
    };

    let inode3_offset = 2u64 * EXT2_INODE_SIZE as u64;
    let sector3_num = (itable_sector * SECTOR_SIZE as u64 + inode3_offset) / SECTOR_SIZE as u64;
    let byte3_off = ((itable_sector * SECTOR_SIZE as u64 + inode3_offset) % SECTOR_SIZE as u64) as usize;
    let mut sec3 = [0u8; SECTOR_SIZE];
    dev.lock().read_sector(sector3_num, &mut sec3).map_err(|_| "itable3 read failed")?;
    sec3[byte3_off..byte3_off + 128].copy_from_slice(&test_inode);
    dev.lock().write_sector(sector3_num, &sec3).map_err(|_| "itable3 write failed")?;

    // Test file data block
    let test_data_sector = (test_data_block as u64 * BLOCK_SIZE as u64) / SECTOR_SIZE as u64;
    let mut tsec = [0u8; SECTOR_SIZE];
    let copy_len = core::cmp::min(SECTOR_SIZE, test_content.len());
    tsec[..copy_len].copy_from_slice(&test_content[..copy_len]);
    dev.lock().write_sector(test_data_sector, &tsec).map_err(|_| "testfile write failed")?;

    // Add "hello.txt" entry to root directory
    // Need to shrink the ".." entry to make room
    // Current layout: "." (12 bytes), ".." (1012 bytes)
    // New layout: "." (12 bytes), ".." (12 bytes), "hello.txt" (22 bytes rounded up)
    // Wait: the ext2 driver's add_directory_entry does this dynamically.
    // Since we're manually creating, let's update the dir block:

    // Re-read root dir block
    let mut rsec = [0u8; SECTOR_SIZE];
    dev.lock().read_sector(root_data_sector, &mut rsec).map_err(|_| "reread root failed")?;
    let mut dir_data = [0u8; BLOCK_SIZE];
    dir_data[..SECTOR_SIZE].copy_from_slice(&rsec);
    dev.lock().read_sector(root_data_sector + 1, &mut rsec).map_err(|_| "reread root2 failed")?;
    dir_data[SECTOR_SIZE..].copy_from_slice(&rsec[..SECTOR_SIZE]);

    // Modify ".." entry: shrink from BLOCK_SIZE-12 to 12
    write_le16(&mut dir_data, 16, 12);
    // "hello.txt" entry at offset 24
    write_le32(&mut dir_data, 24, test_inode_num);
    write_le16(&mut dir_data, 28, (BLOCK_SIZE - 24) as u16);
    dir_data[30] = 9; // name_len ("hello.txt" = 9)
    dir_data[31] = 1; // file_type = regular
    dir_data[32..41].copy_from_slice(b"hello.txt");

    // Write back
    for i in 0..2 {
        let off = i * SECTOR_SIZE;
        let mut sec = [0u8; SECTOR_SIZE];
        sec.copy_from_slice(&dir_data[off..off + SECTOR_SIZE]);
        dev.lock().write_sector(root_data_sector + i as u64, &sec).map_err(|_| "root update failed")?;
    }

    Ok(())
}

fn test_ext2_format_mount() -> Result<(), &'static str> {
    let dev = ram_disk(16384);
    format_ext2(&dev, 8192, 256)?;
    let fs = crate::vfs::ext2::mount(dev).map_err(|_| "ext2 mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    if !root.is_dir() { return Err("root must be dir"); }
    Ok(())
}

fn test_ext2_read_file() -> Result<(), &'static str> {
    let dev = ram_disk(16384);
    format_ext2(&dev, 8192, 256)?;
    let fs = crate::vfs::ext2::mount(dev).map_err(|_| "ext2 mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    let file = root.find_child("hello.txt").ok_or("hello.txt not found")?;
    if file.is_dir() { return Err("hello.txt must not be dir"); }
    let data = file.read(256).map_err(|_| "read failed")?;
    let expected = b"Hello from ext2!\n";
    if data.as_slice() != expected {
        return Err("content mismatch");
    }
    Ok(())
}

fn test_ext2_write_file() -> Result<(), &'static str> {
    let dev = ram_disk(16384);
    format_ext2(&dev, 8192, 256)?;
    let fs = crate::vfs::ext2::mount(dev).map_err(|_| "ext2 mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;

    // Create a new file
    let file = root.create("write_test.txt").map_err(|_| "create failed")?;
    let content = b"Written by ext2 test!\nLine 2\n";
    file.write(content).map_err(|_| "write failed")?;

    // Verify stat
    let stat = file.stat().map_err(|_| "stat failed")?;
    if stat.st_size != content.len() as i64 {
        return Err("stat size mismatch after write");
    }

    // Re-read and verify
    let data = file.read(512).map_err(|_| "read after write failed")?;
    if data.as_slice() != content {
        return Err("read-back content mismatch");
    }
    Ok(())
}

fn test_ext2_mkdir_and_stat() -> Result<(), &'static str> {
    let dev = ram_disk(16384);
    format_ext2(&dev, 8192, 256)?;
    let fs = crate::vfs::ext2::mount(dev).map_err(|_| "ext2 mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;

    root.mkdir("subdir").map_err(|_| "mkdir failed")?;
    let dir = root.find_child("subdir").ok_or("subdir missing")?;
    if !dir.is_dir() { return Err("subdir must be dir"); }

    // Verify stat on the new dir
    let stat = dir.stat().map_err(|_| "stat failed")?;
    if stat.st_mode & 0xF000 != 0x4000 {
        return Err("stat mode must indicate directory");
    }
    if stat.st_ino == 0 {
        return Err("inode number must be nonzero");
    }

    // Children of root should include subdir
    let children = root.children().map_err(|_| "children failed")?;
    let names: Vec<crate::alloc::string::String> = children.iter().map(|c| c.name()).collect();
    if !names.iter().any(|n| n == "subdir") {
        return Err("subdir not in children list");
    }
    Ok(())
}

fn test_ext2_permissions() -> Result<(), &'static str> {
    let dev = ram_disk(16384);
    format_ext2(&dev, 8192, 256)?;
    let fs = crate::vfs::ext2::mount(dev).map_err(|_| "ext2 mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    let file = root.find_child("hello.txt").ok_or("hello.txt not found")?;

    // Stat should report mode, uid, gid
    let stat = file.stat().map_err(|_| "stat failed")?;
    // Our format sets mode to 0x81FF = regular file | 0777
    if stat.st_mode & 0x1FF != 0x1FF {
        return Err("permission bits incorrect");
    }
    // uid should be 0
    if stat.st_uid != 0 { return Err("uid mismatch"); }
    if stat.st_gid != 0 { return Err("gid mismatch"); }
    Ok(())
}

fn test_ext2_hardlink() -> Result<(), &'static str> {
    let dev = ram_disk(16384);
    format_ext2(&dev, 8192, 256)?;
    let fs = crate::vfs::ext2::mount(dev).map_err(|_| "ext2 mount failed")?;
    let root = fs.root().map_err(|_| "root failed")?;
    
    // Create a file to link to
    let file = root.create("link_target.txt").map_err(|_| "create failed")?;
    let content = b"Hard link test content";
    file.write(content).map_err(|_| "write failed")?;
    
    // Get initial stat
    let stat_before = file.stat().map_err(|_| "stat failed")?;
    if stat_before.st_nlink != 1 {
        return Err("initial link count should be 1");
    }
    
    // Create hard link
    root.link(file.clone(), "link_alias.txt").map_err(|_| "link failed")?;
    
    // Verify link count increased
    let stat_after = file.stat().map_err(|_| "stat after link failed")?;
    if stat_after.st_nlink != 2 {
        return Err("link count should be 2 after link");
    }
    
    // Verify both paths point to same inode
    let alias = root.find_child("link_alias.txt").ok_or("alias not found")?;
    let alias_stat = alias.stat().map_err(|_| "alias stat failed")?;
    if alias_stat.st_ino != stat_after.st_ino {
        return Err("alias should have same inode number");
    }
    
    // Verify content is identical
    let alias_data = alias.read(512).map_err(|_| "alias read failed")?;
    if alias_data.as_slice() != content {
        return Err("alias content mismatch");
    }
    
    Ok(())
}

pub fn register() {
    crate::selftest::register("ext2_format_mount", test_ext2_format_mount);
    crate::selftest::register("ext2_read_file", test_ext2_read_file);
    crate::selftest::register("ext2_write_file", test_ext2_write_file);
    crate::selftest::register("ext2_mkdir_and_stat", test_ext2_mkdir_and_stat);
    crate::selftest::register("ext2_permissions", test_ext2_permissions);
    crate::selftest::register("ext2_hardlink", test_ext2_hardlink);
}
