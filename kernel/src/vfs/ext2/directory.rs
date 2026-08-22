//! EXT2 directory entry operations.
//!
//! Finding, adding, and removing directory entries, plus trailing-block
//! shrinkage after removals.

use alloc::vec;

use super::Ext2FileSystem;
use super::types::{DirectoryEntry, Inode};

impl Ext2FileSystem {
    pub(crate) fn find_dentry(
        &self,
        dir_inode: &Inode,
        name: &str,
    ) -> Result<Option<(u32, u32, u32)>, ()> {
        let blocks = self.read_all_block_indices(dir_inode)?;
        for &bnum in &blocks {
            if bnum == 0 {
                continue;
            }
            let data = self.read_block(bnum)?;
            let mut off = 0usize;
            while off < self.block_size {
                let e =
                    unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 {
                    break;
                }
                let enm = core::str::from_utf8(unsafe {
                    core::slice::from_raw_parts(
                        data.as_ptr().add(off + 8),
                        e.name_len as usize,
                    )
                })
                .map_err(|_| ())?;
                if enm == name {
                    return Ok(Some((e.inode, bnum, off as u32)));
                }
                if e.rec_len == 0 {
                    break;
                }
                off += e.rec_len as usize;
            }
        }
        Ok(None)
    }

    pub(crate) fn remove_dentry(
        &self,
        dir_inode_num: u32,
        name: &str,
    ) -> Result<(), ()> {
        let dir_inode = self.read_inode(dir_inode_num)?;
        let blocks = self.read_all_block_indices(&dir_inode)?;
        for &bnum in &blocks {
            if bnum == 0 {
                continue;
            }
            let mut data = self.read_block(bnum)?;
            let mut off = 0usize;
            let mut prev = 0usize;
            while off < self.block_size {
                let e =
                    unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 {
                    break;
                }
                let enm = core::str::from_utf8(unsafe {
                    core::slice::from_raw_parts(
                        data.as_ptr().add(off + 8),
                        e.name_len as usize,
                    )
                })
                .map_err(|_| ())?;
                if enm == name {
                    if off == prev {
                        unsafe {
                            *(data.as_mut_ptr().add(off) as *mut u32) = 0;
                        }
                    } else {
                        let pe = unsafe {
                            &mut *(data.as_mut_ptr().add(prev) as *mut DirectoryEntry)
                        };
                        pe.rec_len = pe.rec_len.wrapping_add(e.rec_len);
                    }
                    self.write_block(bnum, &data)?;
                    self.shrink_dir_blocks(dir_inode_num, &blocks)?;
                    return Ok(());
                }
                if e.rec_len == 0 {
                    break;
                }
                prev = off;
                off += e.rec_len as usize;
            }
        }
        Err(())
    }

    fn shrink_dir_blocks(
        &self,
        dir_inode_num: u32,
        all_blocks: &[u32],
    ) -> Result<(), ()> {
        for &bnum in all_blocks.iter().rev() {
            if bnum == 0 {
                continue;
            }
            let data = self.read_block(bnum)?;
            let first_entry =
                unsafe { &*(data.as_ptr() as *const DirectoryEntry) };
            if first_entry.inode == 0 {
                self.free_block(bnum)?;
                let mut dir_inode = self.read_inode(dir_inode_num)?;
                for slot in 0..12 {
                    if dir_inode.i_block[slot] == bnum {
                        dir_inode.i_block[slot] = 0;
                        break;
                    }
                }
                dir_inode.i_size_lo = dir_inode
                    .i_size_lo
                    .saturating_sub(self.block_size as u32);
                dir_inode.i_blocks_lo = dir_inode
                    .i_blocks_lo
                    .saturating_sub((self.block_size / 512) as u32);
                self.write_inode(dir_inode_num, &dir_inode)?;
            } else {
                break;
            }
        }
        Ok(())
    }

    pub(crate) fn add_dentry(
        &self,
        parent_inode_num: u32,
        child_inode_num: u32,
        name: &str,
        file_type: u8,
    ) -> Result<(), ()> {
        let parent_inode = self.read_inode(parent_inode_num)?;
        let blocks = self.read_all_block_indices(&parent_inode)?;
        let new_len = (8 + name.len() + 3) & !3;

        for &bnum in &blocks {
            if bnum == 0 {
                continue;
            }
            let mut data = self.read_block(bnum)?;
            let mut off = 0;
            while off < self.block_size {
                let e =
                    unsafe { &*(data.as_ptr().add(off) as *const DirectoryEntry) };
                if e.inode == 0 && e.rec_len as usize >= new_len {
                    let e2 = unsafe {
                        &mut *(data.as_mut_ptr().add(off) as *mut DirectoryEntry)
                    };
                    e2.inode = child_inode_num;
                    e2.name_len = name.len() as u8;
                    e2.file_type = file_type;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            name.as_ptr(),
                            data.as_mut_ptr().add(off + 8),
                            name.len(),
                        );
                    }
                    return self.write_block(bnum, &data);
                }
                let cur_len = (8 + e.name_len as usize + 3) & !3;
                let avail = e.rec_len as usize - cur_len;
                if avail >= new_len {
                    let old_rec = e.rec_len;
                    let e2 = unsafe {
                        &mut *(data.as_mut_ptr().add(off) as *mut DirectoryEntry)
                    };
                    e2.rec_len = cur_len as u16;
                    let noff = off + cur_len;
                    let ne = unsafe {
                        &mut *(data.as_mut_ptr().add(noff) as *mut DirectoryEntry)
                    };
                    ne.inode = child_inode_num;
                    ne.rec_len = old_rec - cur_len as u16;
                    ne.name_len = name.len() as u8;
                    ne.file_type = file_type;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            name.as_ptr(),
                            data.as_mut_ptr().add(noff + 8),
                            name.len(),
                        );
                    }
                    return self.write_block(bnum, &data);
                }
                if e.rec_len == 0 {
                    break;
                }
                off += e.rec_len as usize;
            }
        }

        // Need a new block — find a free slot in the direct block pointers.
        let mut upd = parent_inode;
        let slot = (0..12).find(|&i| upd.i_block[i] == 0).ok_or(())?;
        let nb = self.allocate_block()?;
        let mut new_data = vec![0u8; self.block_size];
        let e = unsafe {
            &mut *(new_data.as_mut_ptr() as *mut DirectoryEntry)
        };
        e.inode = child_inode_num;
        e.rec_len = self.block_size as u16;
        e.name_len = name.len() as u8;
        e.file_type = file_type;
        unsafe {
            core::ptr::copy_nonoverlapping(
                name.as_ptr(),
                new_data.as_mut_ptr().add(8),
                name.len(),
            );
        }
        self.write_block(nb, &new_data)?;
        upd.i_block[slot] = nb;
        upd.i_size_lo += self.block_size as u32;
        upd.i_blocks_lo += (self.block_size / 512) as u32;
        self.write_inode(parent_inode_num, &upd)
    }
}
