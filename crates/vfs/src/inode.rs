//! Inode cache and management.

use alloc::collections::BTreeMap;

/// Inode cache: maps inode numbers to filesystem-specific state.
pub struct InodeCache {
    #[allow(dead_code)]
    inodes: BTreeMap<u64, InodeEntry>,
}

struct InodeEntry {
    #[allow(dead_code)]
    #[allow(dead_code)]
    inode_type: u8, // InodeType enum (File=0, Dir=1, Link=2)
    #[allow(dead_code)]
    refcount: u32,
}

impl InodeCache {
    /// Create a new empty inode cache.
    pub const fn new() -> Self {
        Self {
            inodes: BTreeMap::new(),
        }
    }
}

impl Default for InodeCache {
    fn default() -> Self {
        Self::new()
    }
}
