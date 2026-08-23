#![allow(unused_imports, unused_variables, dead_code, unused_doc_comments)]
use crate::task::process::{FileDescriptor, CURRENT_PROCESS};
use crate::objects::KernelObject;
use crate::vfs::{VFS, VfsNode, Stat};
use crate::sync::IrqSafeMutex as Mutex;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::vec;
use x86_64::VirtAddr;
use x86_64::structures::paging::{Page, Size4KiB, Mapper, FrameAllocator, PageTableFlags};
use crate::gdt;
use crate::interrupts::IrqFmtBuf;
use super::errno;
use super::numbers;
use crate::task::process::Process;

pub const CAP_CHOWN: u64 = 1 << 0;
pub const CAP_DAC_OVERRIDE: u64 = 1 << 1;
pub const CAP_DAC_READ_SEARCH: u64 = 1 << 2;
pub const CAP_FOWNER: u64 = 1 << 3;
#[allow(dead_code)] pub const CAP_FSETID: u64 = 1 << 4;
pub const CAP_KILL: u64 = 1 << 5;
pub const CAP_SETUID: u64 = 1 << 6;
pub const CAP_SETGID: u64 = 1 << 7;
#[allow(dead_code)] pub const CAP_SETPCAP: u64 = 1 << 8;
#[allow(dead_code)] pub const CAP_NET_BIND_SERVICE: u64 = 1 << 10;
#[allow(dead_code)] pub const CAP_NET_ADMIN: u64 = 1 << 12;
pub const CAP_NET_RAW: u64 = 1 << 13;
pub const CAP_SYS_ADMIN: u64 = 1 << 21;
pub const CAP_SYS_BOOT: u64 = 1 << 22;

// *at constants
pub const AT_FDCWD: i64 = -100;
pub const AT_REMOVEDIR: i32 = 0x200;
pub const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
pub const AT_EMPTY_PATH: i32 = 0x1000;
pub const AT_EACCESS: i32 = 0x200;
pub const AT_SYMLINK_FOLLOW: i32 = 0x400;

/// Check if the current process has the given capability in its effective set.
pub fn has_capability(cap_bit: u64) -> bool {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().is_some_and(|p| {
        let cred = p.creds.lock();
        cred.euid == 0 || (cred.cap_effective & cap_bit) != 0
    })
}

/// Log a security-relevant event to serial for audit trail.
pub fn audit_log(event: &str, detail: &str) {
    let pid = {
        let lock = CURRENT_PROCESS.lock();
        lock.as_ref().map(|p| p.id).unwrap_or(0)
    };
    crate::serial_write("[AUDIT] ");
    crate::serial_write(event);
    crate::serial_write(" pid=");
    let pid_str = alloc::format!("{}", pid);
    crate::serial_write(&pid_str);
    crate::serial_write(" ");
    crate::serial_write(detail);
    crate::serial_write("\n");
}

/// Get euid for the current process. Returns 0 (root) if no process.
pub fn get_current_euid() -> u32 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(0, |p| p.creds.lock().euid)
}

/// Get egid for the current process. Returns 0 (root) if no process.
pub fn get_current_egid() -> u32 {
    let lock = CURRENT_PROCESS.lock();
    lock.as_ref().map_or(0, |p| p.creds.lock().egid)
}

/// Check if the current process can access a file with given mode/uid/gid.
/// `need` is the access bits required (4=read, 2=write, 1=execute).
/// Returns true if access is granted.
pub fn check_file_permission(st_mode: u32, st_uid: u32, st_gid: u32, need: u32) -> bool {
    let euid = get_current_euid();
    let egid = get_current_egid();
    // CAP_DAC_OVERRIDE: bypass DAC entirely (rwx)
    if has_capability(CAP_DAC_OVERRIDE) { return true; }
    // CAP_DAC_READ_SEARCH: bypass DAC for read/search only
    if has_capability(CAP_DAC_READ_SEARCH) && (need & 2) == 0 { return true; }
    let bits = if euid == st_uid { (st_mode >> 6) & 7 }
               else if egid == st_gid { (st_mode >> 3) & 7 }
               else { st_mode & 7 };
    (bits & need) == need
}

/// Check if current process can access a VfsNode with the given required permission bits.
pub fn check_node_permission(node: &Arc<dyn VfsNode>, need: u32) -> bool {
    if let Ok(stat) = node.stat() {
        check_file_permission(stat.st_mode, stat.st_uid, stat.st_gid, need)
    } else {
        true // If we can't stat, allow (compatibility with special filesystems)
    }
}

/// Check if current process owns the given file (euid matches st_uid).
pub fn check_file_owner(node: &Arc<dyn VfsNode>) -> bool {
    let euid = get_current_euid();
    // CAP_FOWNER: bypass ownership checks for permission-changing ops
    if has_capability(CAP_FOWNER) { return true; }
    if let Ok(stat) = node.stat() {
        euid == stat.st_uid
    } else {
        true
    }
}

/// Normalize a (possibly relative) path to an absolute path using the process cwd.
pub fn normalize_path(path_str: &str, process: &Arc<crate::task::process::Process>) -> String {
    let mut new_path = String::from(path_str);
    if !new_path.starts_with('/') {
        let cur_cwd = process.files.lock().cwd.clone();
        if cur_cwd == "/" {
            new_path = alloc::format!("/{}", new_path);
        } else {
            new_path = alloc::format!("{}/{}", cur_cwd, new_path);
        }
    }
    if new_path.len() > 1 && new_path.ends_with('/') {
        new_path.pop();
    }
    new_path
}

/// Resolve a path relative to a dirfd (or AT_FDCWD for cwd).
/// Returns the absolute path string.
pub fn resolve_path_at(dirfd: i64, pathname: &str, process: &Arc<crate::task::process::Process>) -> Result<alloc::string::String, errno::Errno> {
    if pathname.starts_with('/') {
        return Ok(alloc::string::String::from(pathname));
    }
    match dirfd {
        AT_FDCWD => Ok(normalize_path(pathname, process)),
        fd => {
            let dir_fds = process.files.lock().dir_fds.clone();
            match dir_fds.get(&(fd as usize)) {
                Some(dir_path) => {
                    if dir_path.ends_with('/') {
                        Ok(alloc::format!("{}{}", dir_path, pathname))
                    } else {
                        Ok(alloc::format!("{}/{}", dir_path, pathname))
                    }
                }
                None => Err(errno::Errno::EBADF),
            }
        }
    }
}

/// Helper: store a directory's normalized path in dir_fds when a directory fd is created.
pub fn store_dir_path(process: &Arc<crate::task::process::Process>, fd: u64, path_str: &str) {
    if (fd as i64) < 0 { return; }
    let abs_path = normalize_path(path_str, process);
    process.files.lock().dir_fds.insert(fd as usize, abs_path);
}

/// Write the `syscall`/`sysret` MSRs (STAR, LSTAR, SFMask) for the current CPU.
/// These MSRs are per-logical-processor and reset to 0, so they must be set on
/// every core Ã¢â‚¬â€ the BSP via `init()`, each AP via `init_syscall_msrs()` in
/// `ap_kernel_entry`. Without this, a user `syscall` on an AP loads CS=0/SS=8
/// from STAR and jumps to LSTAR=0 (fetch at address 0).
pub(crate) const MAX_IO_CHUNK: usize = 1 << 20;

pub fn get_current_process() -> Option<Arc<Process>> {
    CURRENT_PROCESS.lock().as_ref().map(|p| p.clone())
}



/// Add a file descriptor via the Object Manager (bind-time security).
pub fn add_fd(process: &Arc<Process>, node: Arc<dyn VfsNode>, open_flags: i32) -> u64 {
    let type_id = if node.is_dir() { crate::objects::TYPE_DIR } else { crate::objects::TYPE_FILE };
    let obj = crate::vfs::VfsObject::new(node.clone(), type_id);
    let access = match open_flags & 3 {
        1 => crate::objects::security::ACCESS_WRITE,
        2 => crate::objects::security::ACCESS_READ | crate::objects::security::ACCESS_WRITE,
        _ => crate::objects::security::ACCESS_READ,
    };
    let mut ht = process.handle_table.lock();
    // Bind-time security check (mirrors HandleTable::insert).
    let cred = crate::objects::current_credentials();
    {
        let sec = obj.header().security.lock();
        if !crate::objects::security::access_check(&cred, &sec, access) {
            return errno::Errno::EACCES as u64;
        }
    }
    // Pick a slot free in BOTH the handle table and the legacy fd_table.
    // sys_open returns this value as the fd, and sys_read/sys_write/dup/
    // fstat index fd_table by it Ã¢â‚¬â€ so the two tables must agree on what an
    // fd number means. A forked child has an empty handle table but inherits
    // stdio fds 0-2 in fd_table, so without this the first open returned fd
    // 0 and read/write hit the tty instead of the opened file (e.g. the
    // /ctl/sys/mem/free read came back empty for exactly this reason).
    let mut ft = process.files.lock().fd_table.clone();
    let limit = core::cmp::max(ht.len(), ft.len());
    let mut idx = 0usize;
    while idx < limit && (ht.is_valid(idx as u64) || ft.get(idx).map_or(false, |s| s.is_some())) {
        idx += 1;
    }
    ht.insert_at(idx as u64, obj, access, open_flags as u64);
    if ft.len() <= idx { ft.resize(idx + 1, None); }
    ft[idx] = Some(FileDescriptor::File { node, offset: crate::sync::IrqSafeMutex::new(0) });
    // Keep fd_flags in lockstep so read/write access-mode checks see the
    // O_ACCMODE bits; sys_openat ORs O_CLOEXEC in afterwards.
    drop(ft);
    let mut ffl = process.files.lock().fd_flags.clone();
    if ffl.len() <= idx { ffl.resize(idx + 1, 0); }
    ffl[idx] = open_flags as u64;
    idx as u64
}
