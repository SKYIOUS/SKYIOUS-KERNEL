use super::{SkyFS, Extent, BLOCK_SIZE};
use crate::drivers::block::BlockDevice;
use crate::alloc::sync::Arc;
use spin::Mutex;

const BTREE_ORDER: usize = 16;
const KEY_MAX: usize = BTREE_ORDER - 1;
const CHILD_MAX: usize = BTREE_ORDER;

#[repr(C, packed)]
struct BTreeNode {
    is_leaf: u8,
    num_keys: u16,
    keys: [u64; KEY_MAX],
    values: [Extent; KEY_MAX],
    children: [u64; CHILD_MAX],
}

pub fn lookup_extent(fs: &Arc<Mutex<SkyFS>>, root_block: u64, block_offset: u64) -> Option<Extent> {
    let mut current = root_block;
    loop {
        let node = read_node(fs, current)?;
        let nk = node.num_keys as usize;
        if node.is_leaf != 0 {
            for i in 0..nk {
                if block_offset >= node.keys[i] {
                    let extent = node.values[i];
                    if block_offset < node.keys[i] + extent.block_count {
                        return Some(Extent {
                            start_block: extent.start_block + (block_offset - node.keys[i]),
                            block_count: extent.block_count - (block_offset - node.keys[i]),
                        });
                    }
                }
            }
            return None;
        }
        let mut idx = 0;
        while idx < nk && block_offset >= node.keys[idx] {
            idx += 1;
        }
        current = node.children[idx];
    }
}

pub fn insert_extent(fs: &Arc<Mutex<SkyFS>>, root_block: &mut u64, key: u64, value: Extent) -> Result<(), ()> {
    let dev_arc = fs.lock().device.clone();
    let mut dev = dev_arc.lock();
    if *root_block == 0 {
        let block = alloc_node_block(fs, &mut *dev)?;
        let mut node = BTreeNode::empty();
        node.is_leaf = 1;
        node.num_keys = 1;
        node.keys[0] = key;
        node.values[0] = value;
        write_node(&mut *dev, block, &node)?;
        *root_block = block;
        return Ok(());
    }
    let split = insert_non_full(fs, &mut *dev, *root_block, key, value)?;
    if let Some((median_key, median_extent, new_child)) = split {
        let block = alloc_node_block(fs, &mut *dev)?;
        let mut new_root = BTreeNode::empty();
        new_root.is_leaf = 0;
        new_root.num_keys = 1;
        new_root.keys[0] = median_key;
        new_root.values[0] = median_extent;
        new_root.children[0] = *root_block;
        new_root.children[1] = new_child;
        write_node(&mut *dev, block, &new_root)?;
        *root_block = block;
    }
    Ok(())
}

fn insert_non_full(fs: &Arc<Mutex<SkyFS>>, dev: &mut dyn BlockDevice, node_block: u64, key: u64, value: Extent) -> Result<Option<(u64, Extent, u64)>, ()> {
    let mut node = read_node_from_dev(dev, node_block)?;
    let nk = node.num_keys as usize;

    if node.is_leaf != 0 {
        if nk < KEY_MAX {
            let mut i = nk;
            while i > 0 && key < node.keys[i - 1] {
                node.keys[i] = node.keys[i - 1];
                node.values[i] = node.values[i - 1];
                i -= 1;
            }
            node.keys[i] = key;
            node.values[i] = value;
            node.num_keys = (nk + 1) as u16;
            write_node(dev, node_block, &node)?;
            return Ok(None);
        }
        let mut all_keys = [(0u64, Extent { start_block: 0, block_count: 0 }); KEY_MAX + 1];
        let mut inserted = false;
        let mut ai = 0;
        for i in 0..KEY_MAX {
            if !inserted && key < node.keys[i] {
                all_keys[ai] = (key, value);
                ai += 1;
                inserted = true;
            }
            all_keys[ai] = (node.keys[i], node.values[i]);
            ai += 1;
        }
        if !inserted {
            all_keys[KEY_MAX] = (key, value);
        }
        let mid = KEY_MAX / 2;
        let median = all_keys[mid];
        node.num_keys = mid as u16;
        for i in 0..mid {
            node.keys[i] = all_keys[i].0;
            node.values[i] = all_keys[i].1;
        }
        write_node(dev, node_block, &node)?;
        let new_block = alloc_node_block(fs, dev)?;
        let mut new_node = BTreeNode::empty();
        new_node.is_leaf = 1;
        new_node.num_keys = (KEY_MAX - mid) as u16;
        for i in 0..(KEY_MAX - mid) {
            new_node.keys[i] = all_keys[mid + 1 + i].0;
            new_node.values[i] = all_keys[mid + 1 + i].1;
        }
        write_node(dev, new_block, &new_node)?;
        return Ok(Some((median.0, median.1, new_block)));
    }

    let mut i = nk;
    while i > 0 && key < node.keys[i - 1] {
        i -= 1;
    }
    let child = node.children[i];
    let split = insert_non_full(fs, dev, child, key, value)?;
    if let Some((med_key, med_val, new_child)) = split {
        if nk < KEY_MAX {
            let mut j = nk;
            while j > i {
                node.keys[j] = node.keys[j - 1];
                node.values[j] = node.values[j - 1];
                node.children[j + 1] = node.children[j];
                j -= 1;
            }
            node.keys[i] = med_key;
            node.values[i] = med_val;
            node.children[i + 1] = new_child;
            node.num_keys = (nk + 1) as u16;
            write_node(dev, node_block, &node)?;
            return Ok(None);
        }
        let mut all_keys = [(0u64, Extent { start_block: 0, block_count: 0 }); KEY_MAX + 1];
        let mut all_children = [0u64; KEY_MAX + 2];
        let mut inserted = false;
        let mut ai = 0;
        for ci in 0..=nk {
            all_children[ai] = node.children[ci];
            if ci < nk {
                if !inserted && med_key < node.keys[ci] {
                    all_keys[ai] = (med_key, med_val);
                    all_children[ai + 1] = new_child;
                    ai += 1;
                    inserted = true;
                }
                all_keys[ai] = (node.keys[ci], node.values[ci]);
                all_children[ai + 1] = node.children[ci + 1];
            }
            if ci == i && !inserted {
                all_keys[nk] = (med_key, med_val);
                all_children[nk + 1] = new_child;
                inserted = true;
            }
            if ci < nk || (ci == nk && inserted) {
                ai += 1;
            }
        }
        let mid = KEY_MAX / 2;
        let median = all_keys[mid];
        node.num_keys = mid as u16;
        for j in 0..mid {
            node.keys[j] = all_keys[j].0;
            node.values[j] = all_keys[j].1;
            node.children[j] = all_children[j];
        }
        node.children[mid] = all_children[mid];
        write_node(dev, node_block, &node)?;
        let new_block = alloc_node_block(fs, dev)?;
        let mut new_node = BTreeNode::empty();
        new_node.is_leaf = 0;
        new_node.num_keys = (KEY_MAX - mid) as u16;
        for j in 0..(KEY_MAX - mid) {
            new_node.keys[j] = all_keys[mid + 1 + j].0;
            new_node.values[j] = all_keys[mid + 1 + j].1;
            new_node.children[j] = all_children[mid + 1 + j];
        }
        new_node.children[KEY_MAX - mid] = all_children[KEY_MAX + 1];
        write_node(dev, new_block, &new_node)?;
        return Ok(Some((median.0, median.1, new_block)));
    }
    Ok(None)
}

fn alloc_node_block(fs: &Arc<Mutex<SkyFS>>, dev: &mut dyn BlockDevice) -> Result<u64, ()> {
    super::alloc::allocate_block_inner(fs, dev)
}

fn read_node(fs: &Arc<Mutex<SkyFS>>, block: u64) -> Option<BTreeNode> {
    let mut buf = [0u8; BLOCK_SIZE];
    let dev_arc = fs.lock().device.clone();
    let mut dev = dev_arc.lock();
    SkyFS::read_block(&mut *dev, block, &mut buf).ok()?;
    Some(unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const BTreeNode) })
}

fn read_node_from_dev(dev: &mut dyn BlockDevice, block: u64) -> Result<BTreeNode, ()> {
    let mut buf = [0u8; BLOCK_SIZE];
    SkyFS::read_block(dev, block, &mut buf)?;
    Ok(unsafe { core::ptr::read_unaligned(buf.as_ptr() as *const BTreeNode) })
}

fn write_node(dev: &mut dyn BlockDevice, block: u64, node: &BTreeNode) -> Result<(), ()> {
    let mut buf = [0u8; BLOCK_SIZE];
    let src = unsafe {
        core::slice::from_raw_parts(node as *const BTreeNode as *const u8, core::mem::size_of::<BTreeNode>())
    };
    buf[..src.len()].copy_from_slice(src);
    SkyFS::write_block(dev, block, &buf)
}

impl BTreeNode {
    fn empty() -> Self {
        BTreeNode {
            is_leaf: 0,
            num_keys: 0,
            keys: [0u64; KEY_MAX],
            values: [Extent { start_block: 0, block_count: 0 }; KEY_MAX],
            children: [0u64; CHILD_MAX],
        }
    }
}
