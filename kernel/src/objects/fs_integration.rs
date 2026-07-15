use alloc::sync::Arc;
use crate::objects::KernelObject;
use crate::vfs::VfsObject;

/// Open a VFS path as a KernelObject handle in the current process.
/// Returns the handle value on success.
pub fn open_path_as_handle(path: &str, access_mask: u32) -> Result<u64, u64> {
    let process = {
        let lock = crate::task::process::CURRENT_PROCESS.lock();
        match *lock {
            Some(ref p) => p.clone(),
            None => return Err(crate::syscalls::errno::Errno::ESRCH as u64),
        }
    };
    let vfs = crate::vfs::VFS.lock();
    let node = match vfs.resolve_path(path) {
        Some(n) => n,
        None => return Err(crate::syscalls::errno::Errno::ENOENT as u64),
    };
    drop(vfs);
    let type_id = if node.is_dir() { crate::objects::TYPE_DIR } else { crate::objects::TYPE_FILE };
    let obj = VfsObject::new(node, type_id);
    process.new_handle(obj as Arc<dyn KernelObject>, access_mask, 0)
        .map(|hv| hv as u64)
        .map_err(|_| crate::syscalls::errno::Errno::EACCES as u64)
}
