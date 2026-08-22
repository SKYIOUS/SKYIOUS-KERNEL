use alloc::vec::Vec;
use crate::vfs::Stat;

/// Effective credentials snapshot for access checks.
#[derive(Clone, Copy, Debug)]
pub struct Credentials {
    pub uid: u32,
    pub gid: u32,
    pub euid: u32,
    pub egid: u32,
    pub fsuid: u32,
    pub fsgid: u32,
    pub cap_effective: u64,
}

impl Credentials {
    pub fn new() -> Self {
        Credentials { uid: 0, gid: 0, euid: 0, egid: 0, fsuid: 0, fsgid: 0, cap_effective: 0 }
    }
}

/// Security descriptor attached to every kernel object.
#[derive(Clone, Debug)]
pub struct SecurityDescriptor {
    pub uid: u32,
    pub gid: u32,
    pub mode: u32,
    pub acl: Vec<Ace>,
}

impl SecurityDescriptor {
    pub fn new(uid: u32, gid: u32, mode: u32) -> Self {
        SecurityDescriptor { uid, gid, mode, acl: Vec::new() }
    }
}

impl Default for SecurityDescriptor {
    fn default() -> Self {
        SecurityDescriptor { uid: 0, gid: 0, mode: 0o644, acl: Vec::new() }
    }
}

impl SecurityDescriptor {
    pub fn default_socket() -> Self {
        SecurityDescriptor { uid: 0, gid: 0, mode: 0o600, acl: Vec::new() }
    }
}

impl From<&Stat> for SecurityDescriptor {
    fn from(s: &Stat) -> Self {
        SecurityDescriptor::new(s.st_uid, s.st_gid, s.st_mode & 0o777)
    }
}

/// Access Control Entry for ACL-based security.
#[derive(Clone, Debug)]
pub struct Ace {
    pub ace_type: AceType,
    pub flags: u8,
    pub access_mask: u32,
    pub uid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AceType {
    Allow,
    Deny,
}

// ── Access mask bits (matching our DAC bit layout) ────────────────
pub const ACCESS_READ: u32 = 4;
pub const ACCESS_WRITE: u32 = 2;
pub const ACCESS_EXEC: u32 = 1;

// Capability bit positions
const CAP_DAC_OVERRIDE: u64 = 1 << 1;
const CAP_DAC_READ_SEARCH: u64 = 1 << 2;

/// Per-object capability rights (Vahi-native capability model).
///
/// Each capability is a bitfield that specifies what operations
/// are allowed on a specific kernel object. Capabilities can be
/// inherited, composed, and dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    /// Rights bitmask
    pub rights: u64,
    /// Object type this capability applies to
    pub object_type: u32,
    /// Unique object ID (0 = wildcard)
    pub object_id: u64,
}

// Capability rights bits
pub const CAP_RIGHT_READ: u64 = 1 << 0;
pub const CAP_RIGHT_WRITE: u64 = 1 << 1;
pub const CAP_RIGHT_EXEC: u64 = 1 << 2;
pub const CAP_RIGHT_CREATE: u64 = 1 << 3;
pub const CAP_RIGHT_DELETE: u64 = 1 << 4;
pub const CAP_RIGHT_MODIFY: u64 = 1 << 5;
pub const CAP_RIGHT_ADMIN: u64 = 1 << 6;
pub const CAP_RIGHT_CONNECT: u64 = 1 << 7;
pub const CAP_RIGHT_LISTEN: u64 = 1 << 8;
pub const CAP_RIGHT_BIND: u64 = 1 << 9;
pub const CAP_RIGHT_SEND: u64 = 1 << 10;
pub const CAP_RIGHT_RECV: u64 = 1 << 11;
pub const CAP_RIGHT_IOCTL: u64 = 1 << 12;
pub const CAP_RIGHT_MMAP: u64 = 1 << 13;
pub const CAP_RIGHT_SHMEM: u64 = 1 << 14;
pub const CAP_RIGHT_SIGNAL: u64 = 1 << 15;

/// All valid rights
pub const CAP_ALL_RIGHTS: u64 = CAP_RIGHT_READ | CAP_RIGHT_WRITE | CAP_RIGHT_EXEC
    | CAP_RIGHT_CREATE | CAP_RIGHT_DELETE | CAP_RIGHT_MODIFY | CAP_RIGHT_ADMIN
    | CAP_RIGHT_CONNECT | CAP_RIGHT_LISTEN | CAP_RIGHT_BIND
    | CAP_RIGHT_SEND | CAP_RIGHT_RECV | CAP_RIGHT_IOCTL
    | CAP_RIGHT_MMAP | CAP_RIGHT_SHMEM | CAP_RIGHT_SIGNAL;

impl Capability {
    /// Create a new capability with the given rights.
    pub fn new(rights: u64, object_type: u32, object_id: u64) -> Self {
        Self { rights: rights & CAP_ALL_RIGHTS, object_type, object_id }
    }
    
    /// Check if this capability grants a specific right.
    pub fn has_right(&self, right: u64) -> bool {
        (self.rights & right) == right
    }
    
    /// Grant additional rights to this capability.
    pub fn grant(&mut self, rights: u64) {
        self.rights |= rights & CAP_ALL_RIGHTS;
    }
    
    /// Drop specific rights from this capability.
    pub fn drop_rights(&mut self, rights: u64) {
        self.rights &= !rights;
    }
    
    /// Compose two capabilities (intersect rights).
    pub fn compose(&self, other: &Capability) -> Capability {
        Capability {
            rights: self.rights & other.rights,
            object_type: self.object_type,
            object_id: self.object_id,
        }
    }
    
    /// Create a child capability with reduced rights.
    pub fn fork(&self, additional_rights: u64) -> Capability {
        Capability {
            rights: self.rights & additional_rights,
            object_type: self.object_type,
            object_id: self.object_id,
        }
    }
}

/// Unified bind-time access check.
///
/// Checks: owner/group/other DAC bits → capabilities → ACL → LSM.
/// All checks must pass for access to be granted.
pub fn access_check(cred: &Credentials, sec: &SecurityDescriptor, desired: u32) -> bool {
    // Root bypass
    if cred.euid == 0 { return true; }

    // DAC check
    let dac_bits = if cred.euid == sec.uid { (sec.mode >> 6) & 7 }
                   else if cred.egid == sec.gid { (sec.mode >> 3) & 7 }
                   else { sec.mode & 7 };
    if (dac_bits & desired) == desired { return true; }

    // Capability override
    if (cred.cap_effective & CAP_DAC_OVERRIDE) != 0 { return true; }
    if (cred.cap_effective & CAP_DAC_READ_SEARCH) != 0 && (desired & ACCESS_WRITE) == 0 { return true; }

    // ACL check
    for ace in &sec.acl {
        if ace.uid == cred.euid || ace.uid == cred.uid {
            if ace.access_mask & desired == desired {
                return ace.ace_type == AceType::Allow;
            }
        }
    }

    false
}
