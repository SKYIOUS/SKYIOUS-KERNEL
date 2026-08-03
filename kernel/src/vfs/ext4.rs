//! Read-only Ext4 filesystem driver with extent tree, 64-bit, and flex_bg support.

use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use crate::sync::IrqSafeMutex as Mutex;
use crate::drivers::block::BlockDevice;
use crate::vfs::{FileSystem, VfsNode, Stat};

const EXT4_SUPER_MAGIC: u16 = 0xEF53;
const EXT4_EXTENT_MAGIC: u16 = 0xF30A;

// Feature flags
const EXT4_FEATURE_INCOMPAT_FILE_TYPE: u32   = 0x0002;
const EXT4_FEATURE_INCOMPAT_RECOVER: u32     = 0x0004;
const EXT4_FEATURE_INCOMPAT_META_BG: u32     = 0x0010;
const EXT4_FEATURE_INCOMPAT_EXTENTS: u32     = 0x0040;
const EXT4_FEATURE_INCOMPAT_64BIT: u32       = 0x0080;
const EXT4_FEATURE_INCOMPAT_MMP: u32         = 0x0100;
const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32     = 0x0200;
const EXT4_FEATURE_INCOMPAT_CSUM_SEED: u32   = 0x2000;

const EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
const EXT4_FEATURE_RO_COMPAT_LARGE_FILE: u32   = 0x0002;
const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: u32    = 0x0008;
const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32    = 0x0020;
const EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE: u32  = 0x0040;
const EXT4_FEATURE_RO_COMPAT_GDT_CSUM: u32     = 0x0010;
const EXT4_FEATURE_RO_COMPAT_METADATA_CSUM: u32 = 0x0400;

const EXT4_INODE_FLAG_EXTENTS: u32 = 0x80000;

// Incompat features we accept (we understand these)
const SUPPORTED_INCOMPAT: u32 = EXT4_FEATURE_INCOMPAT_FILE_TYPE
    | EXT4_FEATURE_INCOMPAT_EXTENTS
    | EXT4_FEATURE_INCOMPAT_64BIT
    | EXT4_FEATURE_INCOMPAT_FLEX_BG
    | EXT4_FEATURE_INCOMPAT_META_BG;

// RO-compat features we accept for read-only mount
const SUPPORTED_RO_COMPAT: u32 = EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
    | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
    | EXT4_FEATURE_RO_COMPAT_HUGE_FILE
    | EXT4_FEATURE_RO_COMPAT_DIR_NLINK
    | EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE
    | EXT4_FEATURE_RO_COMPAT_GDT_CSUM;

// ??? On-disk structures ??????????????????????????????????????????????????????

#[repr(C, packed)]
struct Superblock {
    s_inodes_count: u32,
    s_blocks_count_lo: u32,
    s_r_blocks_count_lo: u32,
    s_free_blocks_count_lo: u32,
    s_free_inodes_count: u32,
    s_first_data_block: u32,
    s_log_block_size: u32,
    s_log_frag_size: u32,
    s_blocks_per_group: u32,
    s_frags_per_group: u32,
    s_inodes_per_group: u32,
    s_mtime: u32,
    s_wtime: u32,
    s_mnt_count: u16,
    s_max_mnt_count: u16,
    s_magic: u16,
    s_state: u16,
    s_errors: u16,
    s_minor_rev_level: u16,
    s_lastcheck: u32,
    s_checkinterval: u32,
    s_creator_os: u32,
    s_rev_level: u32,
    s_def_resuid: u16,
    s_def_resgid: u16,
    // ext4 dynamic fields
    s_first_ino: u32,
    s_inode_size: u16,
    s_block_group_nr: u16,
    s_feature_compat: u32,
    s_feature_incompat: u32,
    s_feature_ro_compat: u32,
    s_uuid: [u8; 16],
    s_volume_name: [u8; 16],
    s_last_mounted: [u8; 64],
    s_algo_bitmap: u32,
    // Performance hints
    s_prealloc_blocks: u8,
    s_prealloc_dir_blocks: u8,
    _pad1: u16,
    s_journal_uuid: [u8; 16],
    s_journal_inum: u32,
    s_journal_dev: u32,
    s_last_orphan: u32,
    s_hash_seed: [u32; 4],
    s_def_hash_version: u8,
    _pad2: [u8; 3],
    s_default_mount_opts: u32,
    s_first_meta_bg: u32,
    _unused: [u8; 760],
}

// ext4 superblock fields beyond ext2 (at known offsets in the 1024-byte block)
// s_blocks_count_hi at offset 0x400 (byte 1024)
// s_r_blocks_count_hi at offset 0x404
// s_free_blocks_count_hi at offset 0x408
// s_minor_rev_level at offset 0x412 (already in ext2 struct)
// s_lastcheck_extra at offset 0x416
// s_wtime_extra at offset 0x41E
// s_mtime_extra at offset 0x422
// s_desc_size at offset 0x106 (relative to sb start in block)
// s_blocks_count_hi at offset - whoops this is tricky. Let me handle 64-bit elsewhere.

#[repr(C, packed)]
struct GroupDesc {
    bg_block_bitmap: u32,
    bg_inode_bitmap: u32,
    bg_inode_table: u32,
    bg_free_blocks_count: u16,
    bg_free_inodes_count: u16,
    bg_used_dirs_count: u16,
    bg_pad: u16,
    _reserved: [u8; 12],
}

// Extent tree structures
#[derive(Clone, Copy)]
#[repr(C, packed)]
struct ExtentHeader {
    eh_magic: u16,
    eh_entries: u16,
    eh_max: u16,
    eh_depth: u16,
    eh_generation: u32,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct Extent {
    ee_block: u32,
    ee_len: u16,
    ee_start_hi: u16,
    ee_start_lo: u32,
}

#[derive(Clone, Copy)]
#[repr(C, packed)]
struct ExtentIdx {
    ei_block: u32,
    ei_leaf_lo: u32,
    ei_leaf_hi: u16,
    ei_unused: u16,
}

// ??? Directory entry (same as ext2) ??????????????????????????????????????????

#[repr(C, packed)]
struct DirectoryEntry {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
}

// ??? Filesystem state ????????????????????????????????????????????????????????

pub struct Ext4FileSystem {
    device: Arc<Mutex<dyn BlockDevice>>,
    block_size: usize,
    _blocks_per_group: u32,
    inodes_per_group: u32,
    inode_size: u16,
    desc_size: u16,
    _has_64bit: bool,
    _has_flex_bg: bool,
    _blocks_count_hi: u32,
    _r_blocks_count_hi: u32,
    _free_blocks_count_hi: u32,
}

impl Ext4FileSystem {
    pub fn new(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Mutex<Self>>, ()> {
        let mut sb_buf = [0u8; 1024];
        device.lock().read_sector(2, &mut sb_buf).map_err(|_| ())?;

        let sb = unsafe { &*(sb_buf.as_ptr() as *const Superblock) };
        if sb.s_magic != EXT4_SUPER_MAGIC {
            return Err(());
        }

        let block_size = 1024 << sb.s_log_block_size;
        let has_64bit = (sb.s_feature_incompat & EXT4_FEATURE_INCOMPAT_64BIT) != 0;

        // Read desc_size from ext4 superblock extension (offset 0x106 within the 1024-byte block)
        let desc_size = if has_64bit {
            let ds = unsafe { *(sb_buf.as_ptr().add(0x106) as *const u16) };
            if ds < 64 { 64 } else { ds }
        } else {
            32
        };

        // Reject unsupported features
        let unknown_incompat = sb.s_feature_incompat & !SUPPORTED_INCOMPAT;
        if unknown_incompat != 0 {
            let reject = unknown_incompat & !(EXT4_FEATURE_INCOMPAT_RECOVER | EXT4_FEATURE_INCOMPAT_CSUM_SEED | EXT4_FEATURE_INCOMPAT_MMP);
            if reject != 0 {
                return Err(());
            }
        }

        // Reject unsupported RO-compat features
        let unknown_ro = sb.s_feature_ro_compat & !SUPPORTED_RO_COMPAT;
        if (unknown_ro & EXT4_FEATURE_RO_COMPAT_METADATA_CSUM) != 0 {
            return Err(());
        }

        // Read 64-bit sb extension fields
        let (blocks_count_hi, _, free_blocks_count_hi) = if has_64bit {
            let blocks_hi = unsafe { *(sb_buf.as_ptr().add(0x400) as *const u32) };
            let free_hi = unsafe { *(sb_buf.as_ptr().add(0x408) as *const u32) };
            (blocks_hi, 0u32, free_hi)
        } else {
            (0, 0, 0)
        };

        let inodes_per_group = sb.s_inodes_per_group;
        let inode_size = if sb.s_rev_level > 0 { sb.s_inode_size } else { 128 };

        Ok(Arc::new(Mutex::new(Ext4FileSystem {
            device,
            block_size,
            _blocks_per_group: sb.s_blocks_per_group,
            inodes_per_group,
            inode_size,
            desc_size,
            _has_64bit: has_64bit,
            _has_flex_bg: (sb.s_feature_incompat & EXT4_FEATURE_INCOMPAT_FLEX_BG) != 0,
            _blocks_count_hi: blocks_count_hi,
            _r_blocks_count_hi: 0,
            _free_blocks_count_hi: free_blocks_count_hi,
        })))
    }

    fn gdt_block(&self) -> u64 {
        if self.block_size == 1024 { 2 } else { 1 }
    }

    fn gdt_entry_count(&self) -> usize {
        (self.block_size / self.desc_size as usize).max(1)
    }

    fn block_to_sector(&self, block: u64) -> u64 {
        (block * self.block_size as u64) / 512
    }

    /// Read group descriptor fields as u64 (handles 32/64-bit).
    fn read_group_desc(&self, group: u32) -> Result<(u64, u64, u64), ()> {
        let gdt_block_base = self.gdt_block();
        let desc_per_block = self.gdt_entry_count() as u32;
        let desc_block_idx = group / desc_per_block;
        let desc_in_block = (group % desc_per_block) as usize;

        let block_num = gdt_block_base + desc_block_idx as u64;
        let mut buf = vec![0u8; self.block_size];
        self.device.lock().read_sector(self.block_to_sector(block_num), &mut buf).map_err(|_| ())?;

        let entry_offset = desc_in_block * self.desc_size as usize;
        let entry = unsafe { &*(buf.as_ptr().add(entry_offset) as *const GroupDesc) };

        let (bitmap, bmap, itable) = if self.desc_size >= 64 {
            let hi = unsafe { &*(buf.as_ptr().add(entry_offset + 32) as *const [u32; 3]) };
            (
                entry.bg_block_bitmap as u64 | (hi[0] as u64) << 32,
                entry.bg_inode_bitmap as u64 | (hi[1] as u64) << 32,
                entry.bg_inode_table as u64 | (hi[2] as u64) << 32,
            )
        } else {
            (entry.bg_block_bitmap as u64, entry.bg_inode_bitmap as u64, entry.bg_inode_table as u64)
        };

        Ok((bitmap, bmap, itable))
    }

    fn read_inode_raw(&self, inode_num: u32, buf: &mut [u8]) -> Result<(), ()> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let index = (inode_num - 1) % self.inodes_per_group;

        let (_, _, inode_table) = self.read_group_desc(group)?;
        let inode_offset = index as u64 * self.inode_size as u64;
        let byte_addr = inode_table * self.block_size as u64 + inode_offset;
        let sector = byte_addr / 512;
        let sector_off = byte_addr % 512;

        let mut sector_buf = [0u8; 512];
        self.device.lock().read_sector(sector, &mut sector_buf).map_err(|_| ())?;

        let copy_len = core::cmp::min(buf.len(), self.inode_size as usize);
        buf[..copy_len].copy_from_slice(&sector_buf[sector_off as usize..sector_off as usize + copy_len]);
        Ok(())
    }

    fn inode_size(&self, raw: &[u8]) -> u64 {
        let size_lo = unsafe { *(raw.as_ptr().add(4) as *const u32) } as u64;
        if self.inode_size >= 256 {
            let size_hi = unsafe { *(raw.as_ptr().add(128) as *const u32) } as u64;
            (size_hi << 32) | size_lo
        } else {
            size_lo
        }
    }

    fn inode_flags(&self, raw: &[u8]) -> u32 {
        unsafe { *(raw.as_ptr().add(32) as *const u32) }
    }

    fn _inode_blocks_count(&self, raw: &[u8]) -> u64 {
        let blocks_lo = unsafe { *(raw.as_ptr().add(28) as *const u32) } as u64;
        // HUGE_FILE uses i_blocks_high at offset 136 in 256-byte inode
        if self.inode_size >= 256 {
            let blocks_hi = unsafe { *(raw.as_ptr().add(136) as *const u16) } as u64;
            blocks_lo | (blocks_hi << 32)
        } else {
            blocks_lo
        }
    }

    fn inode_mode(&self, raw: &[u8]) -> u16 {
        unsafe { *(raw.as_ptr() as *const u16) }
    }

    fn inode_is_dir(&self, raw: &[u8]) -> bool {
        self.inode_mode(raw) & 0xF000 == 0x4000
    }

    /// Collect all physical extents for an inode by walking the extent tree.
    fn collect_extents(&self, inode_raw: &[u8]) -> Result<Vec<(u64, u32, u32)>, ()> {
        // i_block at offset 40 in the inode, 60 bytes
        let i_block = unsafe { core::slice::from_raw_parts(inode_raw.as_ptr().add(40), 60) };
        let header = unsafe { &*(i_block.as_ptr() as *const ExtentHeader) };
        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(());
        }
        let mut extents = Vec::new();
        self.walk_extent_tree(i_block, header.eh_depth, &mut extents)?;
        Ok(extents)
    }

    fn walk_extent_tree(&self, data: &[u8], depth: u16, out: &mut Vec<(u64, u32, u32)>) -> Result<(), ()> {
        let header = unsafe { &*(data.as_ptr() as *const ExtentHeader) };
        if header.eh_magic != EXT4_EXTENT_MAGIC {
            return Err(());
        }
        if header.eh_depth == 0 {
            let entries = unsafe {
                core::slice::from_raw_parts(
                    data.as_ptr().add(12) as *const Extent,
                    header.eh_entries as usize,
                )
            };
            for e in entries {
                let start = ((e.ee_start_hi as u64) << 32) | e.ee_start_lo as u64;
                let len = (e.ee_len & 0x7FFF) as u32;
                if len == 0 { continue; }
                out.push((start, e.ee_block, len));
            }
        } else {
            let entries = unsafe {
                core::slice::from_raw_parts(
                    data.as_ptr().add(12) as *const ExtentIdx,
                    header.eh_entries as usize,
                )
            };
            for idx in entries {
                let child = ((idx.ei_leaf_hi as u64) << 32) | idx.ei_leaf_lo as u64;
                let mut child_buf = vec![0u8; self.block_size];
                self.device.lock().read_sector(self.block_to_sector(child), &mut child_buf).map_err(|_| ())?;
                self.walk_extent_tree(&child_buf, depth - 1, out)?;
            }
        }
        Ok(())
    }

    /// Read a physical block from the device.
    fn read_block(&self, phys_block: u64, buf: &mut [u8]) -> Result<(), ()> {
        let sector = self.block_to_sector(phys_block);
        let sectors_per_block = self.block_size / 512;
        for i in 0..sectors_per_block {
            let off = i * 512;
            let mut sector_buf = [0u8; 512];
            self.device.lock().read_sector(sector + i as u64, &mut sector_buf).map_err(|_| ())?;
            let copy_end = core::cmp::min(off + 512, buf.len());
            buf[off..copy_end].copy_from_slice(&sector_buf[..copy_end - off]);
        }
        Ok(())
    }

    /// Check if the superblock at the given sectors is ext2/3/4.
    /// This is a fast probe used by the mount loop.
    #[allow(dead_code)]
    pub fn self_test(device: &Arc<Mutex<dyn BlockDevice>>) -> Result<(), ()> {
        let fs = Ext4FileSystem::new(device.clone())?;
        let fsl = fs.lock();
        let mut raw = vec![0u8; fsl.inode_size as usize];
        fsl.read_inode_raw(2, &mut raw)?;
        let flags = fsl.inode_flags(&raw);
        if (flags & EXT4_INODE_FLAG_EXTENTS) == 0 {
            crate::serial_write("[ext4] self-test WARN: root inode missing EXTENTS flag\n");
        }
        let extents = fsl.collect_extents(&raw)?;
        if extents.is_empty() {
            crate::serial_write("[ext4] self-test: empty root dir\n");
        } else {
            crate::serial_write(&alloc::format!("[ext4] self-test: {} extents\n", extents.len()));
        }
        Ok(())
    }

    /// Benchmark: read `len` bytes from inode `ino` using RDTSC.
    #[allow(dead_code)]
    pub fn benchmark_read(&self, ino: u32, len: usize) -> Result<(), ()> {
        let mut raw = vec![0u8; self.inode_size as usize];
        self.read_inode_raw(ino, &mut raw)?;
        let flags = self.inode_flags(&raw);
        if (flags & EXT4_INODE_FLAG_EXTENTS) == 0 { return Err(()); }
        let extents = self.collect_extents(&raw)?;
        let start = rdtsc();
        let mut data = Vec::with_capacity(len);
        for (phys, _log, extent_len) in &extents {
            let mut block_data = vec![0u8; self.block_size];
            for bi in 0..*extent_len as usize {
                self.read_block(phys + bi as u64, &mut block_data)?;
                let remaining = len - data.len();
                let to_copy = core::cmp::min(remaining, self.block_size);
                data.extend_from_slice(&block_data[..to_copy]);
                if data.len() >= len { break; }
            }
            if data.len() >= len { break; }
        }
        let elapsed = rdtsc() - start;
        if elapsed == 0 { return Err(()); }
        let rate = (data.len() as u64 * 1_000_000) / elapsed;
        crate::serial_write(&alloc::format!("[ext4] bench ino={} size={} {} ticks rate={} B/Mticks\n",
            ino, data.len(), elapsed, rate));
        Ok(())
    }

    pub fn _probe(device: &Arc<Mutex<dyn BlockDevice>>) -> bool {
        let mut buf = [0u8; 1024];
        if device.lock().read_sector(2, &mut buf).is_err() {
            return false;
        }
        let magic = unsafe { *(buf.as_ptr().add(56) as *const u16) };
        magic == EXT4_SUPER_MAGIC
    }
}

// ─── Filesystem handle ───────────────────────────────────────────────────────

#[cfg(target_arch = "x86_64")]
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe { core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nostack, preserves_flags)); }
    (hi as u64) << 32 | lo as u64
}

pub fn mount(device: Arc<Mutex<dyn BlockDevice>>) -> Result<Arc<Ext4FileSystemHandle>, ()> {
    let raw_fs = Ext4FileSystem::new(device)?;
    Ok(Arc::new(Ext4FileSystemHandle { fs: raw_fs }))
}

pub struct Ext4FileSystemHandle {
    fs: Arc<Mutex<Ext4FileSystem>>,
}

impl FileSystem for Ext4FileSystemHandle {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let mut raw = vec![0u8; fs.inode_size as usize];
        fs.read_inode_raw(2, &mut raw)?;
        Ok(Arc::new(Ext4Node {
            fs: self.fs.clone(),
            name: String::new(),
            inode_num: 2,
            raw,
        }))
    }
}

// ─── VFS Node ────────────────────────────────────────────────────────────────

pub struct Ext4Node {
    fs: Arc<Mutex<Ext4FileSystem>>,
    name: String,
    inode_num: u32,
    raw: Vec<u8>,
}

impl VfsNode for Ext4Node {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        self.fs.lock().inode_is_dir(&self.raw)
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir() {
            return Err(());
        }
        let fs = self.fs.lock();
        let file_size = fs.inode_size(&self.raw) as usize;
        let has_extents = (fs.inode_flags(&self.raw) & EXT4_INODE_FLAG_EXTENTS) != 0;

        let mut data = Vec::with_capacity(file_size);
        if has_extents {
            let extents = fs.collect_extents(&self.raw)?;
            for (phys, _log_block, len) in &extents {
                let mut block_data = vec![0u8; fs.block_size];
                for bi in 0..*len as usize {
                    fs.read_block(phys + bi as u64, &mut block_data)?;
                    let remaining = file_size - data.len();
                    let to_copy = core::cmp::min(remaining, fs.block_size);
                    data.extend_from_slice(&block_data[..to_copy]);
                    if data.len() >= file_size { break; }
                }
                if data.len() >= file_size { break; }
            }
        } else {
            return Err(());
        }
        drop(fs);

        let mut children = Vec::new();
        let mut offset = 0;
        while offset + 8 <= data.len() {
            let entry = unsafe { &*(data.as_ptr().add(offset) as *const DirectoryEntry) };
            if entry.inode == 0 { break; }
            let name_len = entry.name_len as usize;
            if offset + 8 + name_len > data.len() { break; }
            let name = unsafe {
                let p = data.as_ptr().add(offset + 8);
                core::str::from_utf8(core::slice::from_raw_parts(p, name_len)).unwrap_or("")
            };
            if name != "." && name != ".." {
                let fs_lock = self.fs.lock();
                let mut child_raw = vec![0u8; fs_lock.inode_size as usize];
                if fs_lock.read_inode_raw(entry.inode, &mut child_raw).is_ok() {
                    children.push(Arc::new(Ext4Node {
                        fs: self.fs.clone(),
                        name: String::from(name),
                        inode_num: entry.inode,
                        raw: child_raw,
                    }) as Arc<dyn VfsNode>);
                }
            }
            if entry.rec_len == 0 { break; }
            offset += entry.rec_len as usize;
        }
        Ok(children)
    }

    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        if !self.is_dir() { return None; }
        let fs = self.fs.lock();
        let file_size = fs.inode_size(&self.raw) as usize;
        let has_extents = (fs.inode_flags(&self.raw) & EXT4_INODE_FLAG_EXTENTS) != 0;

        let mut data = Vec::with_capacity(file_size);
        if has_extents {
            if let Ok(extents) = fs.collect_extents(&self.raw) {
                for (phys, _log, len) in &extents {
                    let mut block_data = vec![0u8; fs.block_size];
                    for bi in 0..*len as usize {
                        if fs.read_block(phys + bi as u64, &mut block_data).is_err() { break; }
                        let remaining = file_size - data.len();
                        let to_copy = core::cmp::min(remaining, fs.block_size);
                        data.extend_from_slice(&block_data[..to_copy]);
                        if data.len() >= file_size { break; }
                    }
                    if data.len() >= file_size { break; }
                }
            }
        }
        drop(fs);

        let mut offset = 0;
        while offset + 8 <= data.len() {
            let entry = unsafe { &*(data.as_ptr().add(offset) as *const DirectoryEntry) };
            if entry.inode == 0 { break; }
            let name_len = entry.name_len as usize;
            if offset + 8 + name_len > data.len() { break; }
            let entry_name = unsafe {
                let p = data.as_ptr().add(offset + 8);
                core::str::from_utf8(core::slice::from_raw_parts(p, name_len)).ok()?
            };
            if entry_name == name {
                let fs_lock = self.fs.lock();
                let mut child_raw = vec![0u8; fs_lock.inode_size as usize];
                fs_lock.read_inode_raw(entry.inode, &mut child_raw).ok()?;
                return Some(Arc::new(Ext4Node {
                    fs: self.fs.clone(),
                    name: String::from(entry_name),
                    inode_num: entry.inode,
                    raw: child_raw,
                }));
            }
            if entry.rec_len == 0 { break; }
            offset += entry.rec_len as usize;
        }
        None
    }

    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        if self.is_dir() { return Err(()); }
        let fs = self.fs.lock();
        let file_size = fs.inode_size(&self.raw) as usize;
        let flags = fs.inode_flags(&self.raw);

        if (flags & EXT4_INODE_FLAG_EXTENTS) != 0 {
            let extents = fs.collect_extents(&self.raw)?;
            let mut data = Vec::with_capacity(file_size);
            for (phys, _log, len) in &extents {
                let mut block_data = vec![0u8; fs.block_size];
                for bi in 0..*len as usize {
                    fs.read_block(phys + bi as u64, &mut block_data)?;
                    let remaining = file_size - data.len();
                    let to_copy = core::cmp::min(remaining, fs.block_size);
                    data.extend_from_slice(&block_data[..to_copy]);
                    if data.len() >= file_size { break; }
                }
                if data.len() >= file_size { break; }
            }
            return Ok(data);
        }

        Err(())
    }

    fn stat(&self) -> Result<Stat, ()> {
        let fs = self.fs.lock();
        let mode = fs.inode_mode(&self.raw) as u32;
        let size = fs.inode_size(&self.raw) as i64;
        let uid = unsafe { *(self.raw.as_ptr().add(2) as *const u16) } as u32;
        let gid = unsafe { *(self.raw.as_ptr().add(24) as *const u16) } as u32;
        let links = unsafe { *(self.raw.as_ptr().add(26) as *const u16) } as u32;
        let atime = unsafe { *(self.raw.as_ptr().add(8) as *const u32) } as i64;
        let ctime = unsafe { *(self.raw.as_ptr().add(12) as *const u32) } as i64;
        let mtime = unsafe { *(self.raw.as_ptr().add(16) as *const u32) } as i64;
        Ok(Stat {
            st_dev: 0,
            st_ino: self.inode_num as u64,
            st_mode: mode,
            st_nlink: links,
            st_uid: uid,
            st_gid: gid,
            st_rdev: 0,
            st_size: size,
            st_atime: atime,
            st_mtime: mtime,
            st_ctime: ctime,
        
            ..Default::default()
        })
    }
}
