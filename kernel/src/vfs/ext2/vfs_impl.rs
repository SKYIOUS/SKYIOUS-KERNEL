//! VfsNode trait implementation for EXT2 inodes.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

use crate::vfs::{Stat, StatFs, VfsNode};

use super::Ext2FileSystem;
use super::Ext2Node;
use super::types::{DirectoryEntry, Inode, S_IFMT};

impl VfsNode for Ext2Node {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        Self::inode_type(&self.inode) == 0x4000
    }

    fn inode_num(&self) -> Option<u64> {
        Some(self.inode_num as u64)
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir() {
            return Err(());
        }
        let fs = self.fs.lock();
        let mut out = Vec::new();
        for &b in &fs.read_all_block_indices(&self.inode)? {
            if b == 0 {
                continue;
            }
            let data = fs.read_block(b)?;
            let mut off = 0;
            while off < fs.block_size {
                let e = unsafe {
                    &*(data.as_ptr().add(off) as *const DirectoryEntry)
                };
                if e.inode == 0 {
                    break;
                }
                let nm = core::str::from_utf8(unsafe {
                    core::slice::from_raw_parts(
                        data.as_ptr().add(off + 8),
                        e.name_len as usize,
                    )
                })
                .map_err(|_| ())?;
                if nm != "." && nm != ".." {
                    if let Ok(ci) = fs.read_inode(e.inode) {
                        out.push(Arc::new(Ext2Node {
                            fs: self.fs.clone(),
                            name: String::from(nm),
                            inode_num: e.inode,
                            inode: ci,
                        }) as Arc<dyn VfsNode>);
                    }
                }
                if e.rec_len == 0 {
                    break;
                }
                off += e.rec_len as usize;
            }
        }
        Ok(out)
    }

    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        if !self.is_dir() {
            return None;
        }
        let fs = self.fs.lock();
        let blocks = fs.read_all_block_indices(&self.inode).ok()?;
        for &b in &blocks {
            if b == 0 {
                continue;
            }
            let data = fs.read_block(b).ok()?;
            let mut off = 0;
            while off < fs.block_size {
                let e = unsafe {
                    &*(data.as_ptr().add(off) as *const DirectoryEntry)
                };
                if e.inode == 0 {
                    break;
                }
                let enm = core::str::from_utf8(unsafe {
                    core::slice::from_raw_parts(
                        data.as_ptr().add(off + 8),
                        e.name_len as usize,
                    )
                })
                .ok()?;
                if enm == name {
                    let ci = fs.read_inode(e.inode).ok()?;
                    return Some(Arc::new(Ext2Node {
                        fs: self.fs.clone(),
                        name: String::from(enm),
                        inode_num: e.inode,
                        inode: ci,
                    }));
                }
                if e.rec_len == 0 {
                    break;
                }
                off += e.rec_len as usize;
            }
        }
        None
    }

    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        let fs = self.fs.lock();
        let inode = fs.read_inode(self.inode_num)?;
        if (inode.i_mode & S_IFMT) == 0x4000 {
            return Err(());
        }
        let size = inode.i_size_lo as usize;
        let mut data = Vec::with_capacity(size);
        for &b in &fs.read_all_block_indices(&inode)? {
            let buf = if b == 0 {
                vec![0u8; fs.block_size]
            } else {
                fs.read_block(b)?
            };
            let rem = size - data.len();
            let cp = core::cmp::min(rem, fs.block_size);
            data.extend_from_slice(&buf[..cp]);
            if data.len() >= size {
                break;
            }
        }
        Ok(data)
    }

    fn stat(&self) -> Result<Stat, ()> {
        let inode = self.fs.lock().read_inode(self.inode_num)?;
        Ok(Stat {
            st_dev: 0,
            st_ino: self.inode_num as u64,
            st_mode: inode.i_mode as u32,
            st_nlink: inode.i_links_count as u32,
            st_uid: inode.i_uid as u32,
            st_gid: inode.i_gid as u32,
            st_rdev: 0,
            st_size: inode.i_size_lo as i64,
            st_atime: inode.i_atime as i64,
            st_mtime: inode.i_mtime as i64,
            st_ctime: inode.i_ctime as i64,
            ..Default::default()
        })
    }

    fn statfs(&self) -> Result<StatFs, ()> {
        let fs = self.fs.lock();
        let sb = fs.read_raw_sb()?;
        Ok(StatFs {
            f_type: 0xEF53,
            f_bsize: fs.block_size as u64,
            f_blocks: sb.s_blocks_count as u64,
            f_bfree: sb.s_free_blocks_count as u64,
            f_bavail: sb.s_free_blocks_count as u64,
            f_files: sb.s_inodes_count as u64,
            f_ffree: sb.s_free_inodes_count as u64,
        })
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        let fs = self.fs.lock();
        let mut inode = fs.read_inode(self.inode_num)?;
        if (inode.i_mode & S_IFMT) == 0x4000 {
            return Err(());
        }
        fs.write_file_blocks(&mut inode, data)?;
        inode.i_mtime = fs.now();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }

    fn create(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let num = fs.allocate_inode()?;
        let now = fs.now();
        let inode = Inode {
            i_mode: 0x81A4,
            i_uid: 0,
            i_size_lo: 0,
            i_atime: now,
            i_ctime: now,
            i_mtime: now,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 1,
            i_blocks_lo: 0,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; 15],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };
        fs.write_inode(num, &inode)?;
        fs.add_dentry(self.inode_num, num, name, 1)?;
        Ok(Arc::new(Ext2Node {
            fs: self.fs.clone(),
            name: String::from(name),
            inode_num: num,
            inode,
        }))
    }

    fn mkdir(&self, name: &str) -> Result<Arc<dyn VfsNode>, ()> {
        let fs = self.fs.lock();
        let num = fs.allocate_inode()?;
        let bn = fs.allocate_block()?;
        let now = fs.now();
        let mut inode = Inode {
            i_mode: 0x41ED,
            i_uid: 0,
            i_size_lo: fs.block_size as u32,
            i_atime: now,
            i_ctime: now,
            i_mtime: now,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 2,
            i_blocks_lo: (fs.block_size / 512) as u32,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; 15],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };
        inode.i_block[0] = bn;

        let mut block_data = vec![0u8; fs.block_size];
        {
            let dot = unsafe {
                &mut *(block_data.as_mut_ptr() as *mut DirectoryEntry)
            };
            dot.inode = num;
            dot.rec_len = 12;
            dot.name_len = 1;
            dot.file_type = 2;
            block_data[8] = b'.';
        }
        {
            let dotdot = unsafe {
                &mut *(block_data.as_mut_ptr().add(12) as *mut DirectoryEntry)
            };
            dotdot.inode = self.inode_num;
            dotdot.rec_len = (fs.block_size - 12) as u16;
            dotdot.name_len = 2;
            dotdot.file_type = 2;
            block_data[12 + 8] = b'.';
            block_data[12 + 9] = b'.';
        }
        fs.write_block(bn, &block_data)?;
        fs.write_inode(num, &inode)?;
        fs.add_dentry(self.inode_num, num, name, 2)?;
        Ok(Arc::new(Ext2Node {
            fs: self.fs.clone(),
            name: String::from(name),
            inode_num: num,
            inode,
        }))
    }

    fn unlink(&self, name: &str) -> Result<(), ()> {
        if name == "." || name == ".." {
            return Err(());
        }
        let fs = self.fs.lock();
        let dir_inode = fs.read_inode(self.inode_num)?;
        let (child_inum, _, _) =
            fs.find_dentry(&dir_inode, name)?.ok_or(())?;
        let child = fs.read_inode(child_inum)?;

        if Self::inode_type(&child) == 0x4000 {
            let blocks = fs.read_all_block_indices(&child)?;
            let mut count = 0;
            for &b in &blocks {
                if b == 0 {
                    continue;
                }
                let data = fs.read_block(b)?;
                let mut off = 0;
                while off < fs.block_size {
                    let e = unsafe {
                        &*(data.as_ptr().add(off) as *const DirectoryEntry)
                    };
                    if e.inode == 0 {
                        break;
                    }
                    count += 1;
                    if e.rec_len == 0 {
                        break;
                    }
                    off += e.rec_len as usize;
                }
            }
            if count > 2 {
                return Err(());
            }
        }

        let mut upd = child;
        upd.i_links_count -= 1;
        if upd.i_links_count == 0 {
            upd.i_dtime = fs.now();
            fs.write_inode(child_inum, &upd)?;
            fs.free_all_blocks(&child)?;
            fs.free_inode(child_inum)?;
        } else {
            fs.write_inode(child_inum, &upd)?;
        }
        fs.remove_dentry(self.inode_num, name)
    }

    fn chmod(&self, mode: u32) -> Result<(), ()> {
        let mut inode = self.inode;
        inode.i_mode = (inode.i_mode & 0xF000) | (mode as u16 & 0x0FFF);
        let fs = self.fs.lock();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }

    fn chown(&self, uid: u32, gid: u32) -> Result<(), ()> {
        let mut inode = self.inode;
        inode.i_uid = uid as u16;
        inode.i_gid = gid as u16;
        let fs = self.fs.lock();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }

    fn readlink(&self) -> Result<String, ()> {
        if Self::inode_type(&self.inode) != 0xA000 {
            return Err(());
        }
        let size = self.inode.i_size_lo as usize;
        if size <= 60 {
            let ptr =
                core::ptr::addr_of!(self.inode.i_block) as *const u8;
            let bytes =
                unsafe { core::slice::from_raw_parts(ptr, size) };
            Ok(String::from_utf8_lossy(bytes).into_owned())
        } else {
            let data = self.read(usize::MAX)?;
            Ok(String::from_utf8_lossy(&data).into_owned())
        }
    }

    fn symlink(&self, name: &str, target: &str) -> Result<(), ()> {
        let fs = self.fs.lock();
        let num = fs.allocate_inode()?;
        let now = fs.now();
        let tgt = target.as_bytes();
        let mut inode = Inode {
            i_mode: 0xA1FF,
            i_uid: 0,
            i_size_lo: tgt.len() as u32,
            i_atime: now,
            i_ctime: now,
            i_mtime: now,
            i_dtime: 0,
            i_gid: 0,
            i_links_count: 1,
            i_blocks_lo: 0,
            i_flags: 0,
            i_osd1: 0,
            i_block: [0; 15],
            i_generation: 0,
            i_file_acl: 0,
            i_dir_acl: 0,
            i_faddr: 0,
            i_osd2: [0; 12],
        };
        if tgt.len() <= 60 {
            let ptr =
                core::ptr::addr_of_mut!(inode.i_block) as *mut u8;
            unsafe {
                core::ptr::copy_nonoverlapping(tgt.as_ptr(), ptr, tgt.len());
            }
        } else {
            let bn = fs.allocate_block()?;
            inode.i_block[0] = bn;
            inode.i_blocks_lo = (fs.block_size / 512) as u32;
            fs.write_block(bn, tgt)?;
        }
        fs.write_inode(num, &inode)?;
        fs.add_dentry(self.inode_num, num, name, 7)?;
        Ok(())
    }

    fn link(
        &self,
        existing: Arc<dyn VfsNode>,
        name: &str,
    ) -> Result<(), ()> {
        if !self.is_dir() {
            return Err(());
        }
        if name == "." || name == ".." {
            return Err(());
        }

        let existing_inum = existing.inode_num().ok_or(())? as u32;

        if existing.is_dir() {
            return Err(());
        }

        let stat = existing.stat()?;
        let ftype = if (stat.st_mode & 0xF000) == 0x4000 {
            2u8
        } else {
            1u8
        };

        let fs = self.fs.lock();
        let dir_inode = fs.read_inode(self.inode_num)?;
        if fs.find_dentry(&dir_inode, name)?.is_some() {
            return Err(());
        }
        let _ = dir_inode;

        fs.add_dentry(self.inode_num, existing_inum, name, ftype)?;

        let mut target_inode = fs.read_inode(existing_inum)?;
        target_inode.i_links_count += 1;
        target_inode.i_ctime = fs.now();
        fs.write_inode(existing_inum, &target_inode)?;

        Ok(())
    }

    fn rename(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), ()> {
        if old_name == "."
            || old_name == ".."
            || new_name == "."
            || new_name == ".."
        {
            return Err(());
        }
        if !self.is_dir() {
            return Err(());
        }
        let fs = self.fs.lock();
        let dir_inode = fs.read_inode(self.inode_num)?;

        let (child_inum, _old_block, _old_off) =
            fs.find_dentry(&dir_inode, old_name)?.ok_or(())?;

        if let Some((existing_inum, _, _)) =
            fs.find_dentry(&dir_inode, new_name)?
        {
            let target_inode = fs.read_inode(existing_inum)?;
            if Self::inode_type(&target_inode) == 0x4000 {
                return Err(());
            }
            let mut upd = target_inode;
            upd.i_links_count -= 1;
            if upd.i_links_count == 0 {
                fs.free_all_blocks(&target_inode)?;
                fs.free_inode(existing_inum)?;
            } else {
                fs.write_inode(existing_inum, &upd)?;
            }
            fs.remove_dentry(self.inode_num, new_name)?;
        }

        let child = fs.read_inode(child_inum)?;
        let ftype = if Self::inode_type(&child) == 0x4000 {
            2u8
        } else {
            1u8
        };
        if fs
            .add_dentry(self.inode_num, child_inum, new_name, ftype)
            .is_err()
        {
            return Err(());
        }
        fs.remove_dentry(self.inode_num, old_name)?;
        Ok(())
    }

    fn truncate(&self, len: i64) -> Result<(), ()> {
        if (self.inode.i_mode & S_IFMT) == 0x4000 {
            return Err(());
        }
        if len < 0 {
            return Err(());
        }
        let new_len = len as usize;
        let fs = self.fs.lock();
        let mut inode = self.inode;
        let bs = fs.block_size;
        let new_blocks_needed = if new_len == 0 {
            0
        } else {
            (new_len + bs - 1) / bs
        };

        let old_blocks = fs.read_all_block_indices(&inode)?;

        if new_blocks_needed < old_blocks.len() {
            for &b in &old_blocks[new_blocks_needed..] {
                if b != 0 {
                    fs.free_block(b)?;
                }
            }
            for i in new_blocks_needed..12 {
                if i < old_blocks.len() {
                    inode.i_block[i] = 0;
                }
            }
            if new_blocks_needed <= 12 {
                if inode.i_block[12] != 0 {
                    fs.free_indirect(inode.i_block[12], 1)?;
                    inode.i_block[12] = 0;
                }
                if inode.i_block[13] != 0 {
                    fs.free_indirect(inode.i_block[13], 2)?;
                    inode.i_block[13] = 0;
                }
                if inode.i_block[14] != 0 {
                    fs.free_indirect(inode.i_block[14], 3)?;
                    inode.i_block[14] = 0;
                }
            }
        }

        if new_len > 0 && new_len % bs != 0 {
            let last_block_idx = new_blocks_needed - 1;
            if last_block_idx < old_blocks.len()
                && old_blocks[last_block_idx] != 0
            {
                let mut buf =
                    fs.read_block(old_blocks[last_block_idx])?;
                let zero_start = new_len % bs;
                for byte in buf[zero_start..].iter_mut() {
                    *byte = 0;
                }
                fs.write_block(
                    old_blocks[last_block_idx],
                    &buf,
                )?;
            }
        }

        if new_blocks_needed > old_blocks.len() {
            let epb = bs / 4;
            for i in old_blocks.len()..new_blocks_needed {
                let block_data = vec![0u8; bs];
                let bnum = if i < 12 {
                    let nb = fs.allocate_block()?;
                    inode.i_block[i] = nb;
                    nb
                } else {
                    let ndb = fs.allocate_block()?;
                    let idx = i - 12;
                    if idx < epb {
                        let mut blk = inode.i_block[12];
                        Ext2FileSystem::set_block_ptr(
                            &fs,
                            &mut blk,
                            1,
                            idx,
                            epb,
                            ndb,
                        )?;
                        inode.i_block[12] = blk;
                    } else if idx < epb * epb {
                        let mut blk = inode.i_block[13];
                        Ext2FileSystem::set_block_ptr(
                            &fs,
                            &mut blk,
                            2,
                            idx,
                            epb,
                            ndb,
                        )?;
                        inode.i_block[13] = blk;
                    } else {
                        let mut blk = inode.i_block[14];
                        Ext2FileSystem::set_block_ptr(
                            &fs,
                            &mut blk,
                            3,
                            idx,
                            epb,
                            ndb,
                        )?;
                        inode.i_block[14] = blk;
                    }
                    ndb
                };
                fs.write_block(bnum, &block_data)?;
            }
        }

        inode.i_size_lo = new_len as u32;
        inode.i_blocks_lo = (if new_blocks_needed == 0 {
            0
        } else {
            new_blocks_needed * bs / 512
        }) as u32;
        inode.i_mtime = fs.now();
        inode.i_ctime = fs.now();
        fs.write_inode(self.inode_num, &inode)
    }
}
