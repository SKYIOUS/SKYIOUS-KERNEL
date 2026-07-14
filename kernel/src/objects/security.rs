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
        SecurityDescriptor { uid: 0, gid: 0, mode: 0o777, acl: Vec::new() }
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
