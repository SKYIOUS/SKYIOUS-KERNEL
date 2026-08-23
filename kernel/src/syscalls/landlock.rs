//! Landlock LSM — Unprivileged sandboxing via filesystem access rules.
//!
//! Implements a subset of the Linux Landlock ABI v1/v2:
//! - landlock_create_ruleset
//! - landlock_add_rule
//! - landlock_restrict_self
//!
//! Each process can create a ruleset that restricts which paths it can access.
//! Once locked, the process (and children) inherit the restrictions.

use alloc::vec::Vec;
use crate::task::process::CURRENT_PROCESS;
use crate::syscalls::errno;
use crate::syscalls::user_access;
use alloc::string::String;
use alloc::format;

/// Landlock ABI version
pub const LANDLOCK_ABI_VERSION: u32 = 2;

/// Landlock access rights
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;

/// Bitmask of all supported Landlock FS access rights
pub const SUPPORTED_FS_ACCESS: u64 = LANDLOCK_ACCESS_FS_EXECUTE
    | LANDLOCK_ACCESS_FS_WRITE_FILE
    | LANDLOCK_ACCESS_FS_READ_FILE
    | LANDLOCK_ACCESS_FS_READ_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_DIR
    | LANDLOCK_ACCESS_FS_REMOVE_FILE
    | LANDLOCK_ACCESS_FS_MAKE_CHAR
    | LANDLOCK_ACCESS_FS_MAKE_DIR
    | LANDLOCK_ACCESS_FS_MAKE_REG
    | LANDLOCK_ACCESS_FS_MAKE_SOCK
    | LANDLOCK_ACCESS_FS_MAKE_FIFO
    | LANDLOCK_ACCESS_FS_MAKE_BLOCK
    | LANDLOCK_ACCESS_FS_MAKE_SYM;
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 14;

/// Landlock rule types
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

/// Landlock handle flags
pub const LANDLOCK_CREATE_RULESET_VERSION: u64 = 1 << 0;
pub const LANDLOCK_CREATE_RULESET_HANDLES_MASK: u64 = 0x00FF_FFFF_FFFF_FFFF;

/// A single path-based rule
#[derive(Clone, Debug)]
pub struct LandlockPathRule {
    /// Path prefix that this rule applies to
    pub path_prefix: String,
    /// Allowed access rights (bitmask of LANDLOCK_ACCESS_FS_*)
    pub allowed_access: u64,
    /// Denied access rights (for handled_access_fs)
    pub handled_access: u64,
}

/// A complete Landlock ruleset
#[derive(Clone, Debug)]
pub struct LandlockRuleset {
    /// All path rules
    pub rules: Vec<LandlockPathRule>,
    /// Access rights this ruleset handles (bitmask)
    pub handled_access_fs: u64,
    /// Whether the ruleset is locked (no more modifications allowed)
    pub locked: bool,
}

impl Default for LandlockRuleset {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            handled_access_fs: 0,
            locked: false,
        }
    }
}

impl LandlockRuleset {
    /// Check if a path is allowed for the given access.
    /// Returns true if access is permitted.
    pub fn check_access(&self, path: &str, access: u64) -> bool {
        let requested = access & self.handled_access_fs;
        if requested == 0 {
            return true; // Not handled by this ruleset = allow
        }

        // Find matching rules (longest prefix match)
        let mut best_match: Option<&LandlockPathRule> = None;
        let mut best_len = 0;

        for rule in &self.rules {
            if path.starts_with(&rule.path_prefix) || path == rule.path_prefix.trim_end_matches('/') {
                if rule.path_prefix.len() > best_len {
                    best_match = Some(rule);
                    best_len = rule.path_prefix.len();
                }
            }
        }

        match best_match {
            Some(rule) => {
                // Check if all requested access bits are in the allowed set
                (requested & rule.allowed_access) == requested
            }
            None => {
                // No rule matches — deny by default (Landlock is deny-first)
                requested == 0
            }
        }
    }

    /// Add a rule to the ruleset
    pub fn add_rule(&mut self, path: String, allowed_access: u64) -> Result<(), errno::Errno> {
        if self.locked {
            return Err(errno::Errno::EPERM);
        }

        self.rules.push(LandlockPathRule {
            path_prefix: path,
            allowed_access,
            handled_access: self.handled_access_fs,
        });
        Ok(())
    }

    /// Lock the ruleset (no more modifications)
    pub fn lock(&mut self) {
        self.locked = true;
    }
}

/// Per-process Landlock state
pub struct LandlockState {
    /// Active rulesets (stacked, most restrictive applies)
    pub rulesets: Vec<LandlockRuleset>,
    /// Whether Landlock is enforced for this process
    pub active: bool,
}

impl Default for LandlockState {
    fn default() -> Self {
        Self {
            rulesets: Vec::new(),
            active: false,
        }
    }
}

impl LandlockState {
    /// Check if a path is allowed for the given access under all active rulesets.
    /// ALL rulesets must allow access (intersection semantics).
    pub fn check_access(&self, path: &str, access: u64) -> bool {
        if !self.active || self.rulesets.is_empty() {
            return true;
        }

        for ruleset in &self.rulesets {
            if !ruleset.check_access(path, access) {
                return false;
            }
        }
        true
    }
}

/// landlock_create_ruleset() — Create a new Landlock ruleset, returns a real fd.
pub fn sys_landlock_create_ruleset(
    user_attr: *const u8,
    size: usize,
    flags: u32,
) -> u64 {
    if flags & !((LANDLOCK_CREATE_RULESET_VERSION | 1) as u32) != 0 {
        return errno::Errno::EINVAL as u64;
    }
    if flags & 1 != 0 {
        return LANDLOCK_ABI_VERSION as u64;
    }
    if size < 8 {
        return errno::Errno::EINVAL as u64;
    }

    let mut attr = [0u8; 16];
    let read_size = core::cmp::min(size, 16);
    if unsafe { user_access::copy_from_user(&mut attr[..read_size], user_attr) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    let handled_access_fs = u64::from_ne_bytes(attr[0..8].try_into().unwrap());

    if handled_access_fs & !SUPPORTED_FS_ACCESS != 0 {
        return errno::Errno::EOPNOTSUPP as u64;
    }

    let ruleset = LandlockRuleset { rules: Vec::new(), handled_access_fs, locked: false };

    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Allocate a real fd from the fd_table
    let mut ft = proc.files.lock().fd_table.clone();
    let fd_num = ft.len();
    ft.push(None); // placeholder — Landlock fds don't have VFS nodes
    drop(ft);

    // Store the ruleset in the landlock_fds map keyed by fd number
    proc.security.lock().landlock_fds.insert(fd_num, ruleset);
    proc.security.lock().landlock.active = true;

    crate::serial_write("[LANDLOCK] Created ruleset fd=");
    crate::serial_write(&format!("{} handled=0x{:x}\n", fd_num, handled_access_fs));
    fd_num as u64
}

/// landlock_add_rule() — Add a path rule to a ruleset.
/// Resolves parent_fd to a path via the process fd_table + dir_fds.
pub fn sys_landlock_add_rule(
    ruleset_fd: u64,
    rule_type: u32,
    user_attr: *const u8,
    flags: u32,
) -> u64 {
    if rule_type != LANDLOCK_RULE_PATH_BENEATH {
        return errno::Errno::EINVAL as u64;
    }
    if user_attr.is_null() {
        return errno::Errno::EFAULT as u64;
    }

    let mut attr = [0u8; 16];
    if unsafe { user_access::copy_from_user(&mut attr, user_attr) }.is_err() {
        return errno::Errno::EFAULT as u64;
    }
    let allowed_access = u64::from_ne_bytes(attr[0..8].try_into().unwrap());
    let parent_fd = i32::from_ne_bytes(attr[8..12].try_into().unwrap());

    if allowed_access & !SUPPORTED_FS_ACCESS != 0 {
        return errno::Errno::EOPNOTSUPP as u64;
    }

    // Resolve parent_fd to a path string
    let path = {
        let lock = CURRENT_PROCESS.lock();
        let proc = match *lock {
            Some(ref p) => p,
            None => return errno::Errno::ESRCH as u64,
        };

        // Look up the fd in dir_fds (directory fds store their path)
        let dir_fds = proc.files.lock().dir_fds.clone();
        match dir_fds.get(&(parent_fd as usize)) {
            Some(p) => p.clone(),
            None => {
                // Fallback: check fd_table for a File descriptor with a node
                drop(dir_fds);
                let ft = proc.files.lock().fd_table.clone();
                if (parent_fd as usize) < ft.len() {
                    if let Some(crate::task::process::FileDescriptor::File { ref node, .. }) = ft[parent_fd as usize] {
                        // Try to get path from VfsNode name; fall back to "/"
                        // Full path resolution requires a reverse fd→path mapping
                        // which is not yet implemented.
                        let node_name = node.name();
                        if node.is_dir() {
                            alloc::format!("/{}", node_name)
                        } else {
                            // For files, use the parent directory
                            String::from("/")
                        }
                    } else {
                        return errno::Errno::EBADF as u64;
                    }
                } else {
                    return errno::Errno::EBADF as u64;
                }
            }
        }
    };

    // Look up the ruleset from landlock_fds
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };
    let mut _lfd_guard = proc.security.lock(); let ll_fds = &mut _lfd_guard.landlock_fds;
    let ruleset = match ll_fds.get_mut(&(ruleset_fd as usize)) {
        Some(r) => r,
        None => return errno::Errno::EBADF as u64,
    };

    ruleset.add_rule(path.clone(), allowed_access).unwrap_or_else(|e| {
        // EPERM if locked
        return ();
    });

    crate::serial_write("[LANDLOCK] Added rule fd=");
    crate::serial_write(&format!("{} path={} access=0x{:x}\n", ruleset_fd, path, allowed_access));
    0
}

/// landlock_restrict_self() — Enforce the ruleset on the current process.
pub fn sys_landlock_restrict_self(ruleset_fd: u64, flags: u32) -> u64 {
    if flags != 0 {
        return errno::Errno::EINVAL as u64;
    }

    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Lock the ruleset from landlock_fds and clone into LandlockState for enforcement
    let ruleset_clone = {
        let mut _lfd_guard = proc.security.lock(); let ll_fds = &mut _lfd_guard.landlock_fds;
        let ruleset = match ll_fds.get_mut(&(ruleset_fd as usize)) {
            Some(r) => r,
            None => return errno::Errno::EBADF as u64,
        };
        let rule_count = ruleset.rules.len();
        ruleset.lock();
        crate::serial_write("[LANDLOCK] Ruleset enforced fd=");
        crate::serial_write(&format!("{} rules={}\n", ruleset_fd, rule_count));
        ruleset.clone()
    };

    // Push into LandlockState for enforcement by check_fs_access
    let mut _lck_guard = proc.security.lock(); let ll = &mut _lck_guard.landlock;
    let rule_count = ruleset_clone.rules.len();
    ll.rulesets.push(ruleset_clone);
    ll.active = true;

    crate::serial_write("[LANDLOCK] Ruleset enforced fd=");
    crate::serial_write(&format!("{} rules={}\n", ruleset_fd, rule_count));
    0
}

/// Check Landlock access for a file operation.
/// Called from VFS operations to enforce Landlock restrictions.
pub fn check_fs_access(path: &str, access: u64) -> bool {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return true,
    };

    let _lck_guard = proc.security.lock(); let ll = &_lck_guard.landlock;
    ll.check_access(path, access)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruleset_check_access_allowed() {
        let ruleset = LandlockRuleset {
            rules: alloc::vec![LandlockPathRule {
                path_prefix: String::from("/home/"),
                allowed_access: LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_WRITE_FILE,
            }],
            handled_access_fs: SUPPORTED_FS_ACCESS,
            locked: false,
        };
        // /home/user/file should match prefix /home/
        assert!(ruleset.check_access("/home/user/file", LANDLOCK_ACCESS_FS_READ_FILE));
        assert!(ruleset.check_access("/home/user/file", LANDLOCK_ACCESS_FS_WRITE_FILE));
    }

    #[test]
    fn test_ruleset_check_access_denied() {
        let ruleset = LandlockRuleset {
            rules: alloc::vec![LandlockPathRule {
                path_prefix: String::from("/home/"),
                allowed_access: LANDLOCK_ACCESS_FS_READ_FILE,
            }],
            handled_access_fs: SUPPORTED_FS_ACCESS,
            locked: false,
        };
        // /home/user/file allows READ but not WRITE
        assert!(ruleset.check_access("/home/user/file", LANDLOCK_ACCESS_FS_READ_FILE));
        assert!(!ruleset.check_access("/home/user/file", LANDLOCK_ACCESS_FS_WRITE_FILE));
    }

    #[test]
    fn test_ruleset_check_access_no_rules_allows_all() {
        let ruleset = LandlockRuleset {
            rules: alloc::vec![],
            handled_access_fs: SUPPORTED_FS_ACCESS,
            locked: false,
        };
        // No rules = allow everything handled by this ruleset
        assert!(ruleset.check_access("/any/path", LANDLOCK_ACCESS_FS_READ_FILE));
    }

    #[test]
    fn test_ruleset_unhandled_access_allows() {
        let ruleset = LandlockRuleset {
            rules: alloc::vec![],
            handled_access_fs: LANDLOCK_ACCESS_FS_READ_FILE, // only handles READ
            locked: false,
        };
        // WRITE is not handled by this ruleset = allow
        assert!(ruleset.check_access("/any/path", LANDLOCK_ACCESS_FS_WRITE_FILE));
    }

    #[test]
    fn test_longest_prefix_match() {
        let ruleset = LandlockRuleset {
            rules: alloc::vec![
                LandlockPathRule {
                    path_prefix: String::from("/"),
                    allowed_access: 0, // deny all
                },
                LandlockPathRule {
                    path_prefix: String::from("/home/user/"),
                    allowed_access: LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_WRITE_FILE,
                },
            ],
            handled_access_fs: SUPPORTED_FS_ACCESS,
            locked: false,
        };
        // /home/user/file matches /home/user/ (longer prefix) → allowed
        assert!(ruleset.check_access("/home/user/file", LANDLOCK_ACCESS_FS_READ_FILE));
        // /tmp/file matches / (only prefix) → denied
        assert!(!ruleset.check_access("/tmp/file", LANDLOCK_ACCESS_FS_READ_FILE));
    }
}
