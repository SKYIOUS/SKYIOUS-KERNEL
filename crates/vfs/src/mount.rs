//! Mount table management.

/// A mount point entry.
#[derive(Debug, Clone)]
pub struct MountPoint {
    pub path: alloc::string::String,
    pub fs_type: &'static str,
    pub flags: u32,
}
