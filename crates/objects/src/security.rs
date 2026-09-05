//! Security descriptors and access control for kernel objects.
//!
//! Simplified Windows-style security model:
//! - Owner SID, Group SID
//! - DACL (Discretionary Access Control List)
//! - Access check on every handle insert

/// Security credentials for the current process.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    /// Effective user ID.
    pub euid: u32,
    /// Effective group ID.
    pub egid: u32,
    /// Real user ID.
    pub uid: u32,
    /// Real group ID.
    pub gid: u32,
    /// File system user ID.
    pub fsuid: u32,
    /// File system group ID.
    pub fsgid: u32,
    /// Effective capability set.
    pub cap_effective: u64,
}

impl Credentials {
    /// Create zero-filled credentials (kernel context).
    pub fn new() -> Self {
        Self::default()
    }

    /// Get credentials for the current process.
    ///
    /// Stub: real implementation reads from CURRENT_PROCESS.
    pub fn current() -> Self {
        Self::default()
    }
}

/// Access rights bitfield.
pub mod access {
    pub const READ: u32 = 0x0001;
    pub const WRITE: u32 = 0x0002;
    pub const EXECUTE: u32 = 0x0004;
    pub const DELETE: u32 = 0x0008;
    pub const FULL_CONTROL: u32 = 0x1FFF;
}

/// Access Control Entry (ACE).
#[derive(Debug, Clone)]
pub struct Ace {
    /// SID of the trustee.
    pub sid: u32,
    /// Access mask granted/denied.
    pub access_mask: u32,
    /// Whether this is a deny ACE (otherwise allow).
    pub deny: bool,
}

/// Discretionary Access Control List.
#[derive(Debug, Clone, Default)]
pub struct Dacl {
    pub entries: alloc::vec::Vec<Ace>,
}

/// Security descriptor attached to every kernel object.
#[derive(Debug, Clone, Default)]
pub struct SecurityDescriptor {
    /// Owner SID.
    pub owner: u32,
    /// Group SID.
    pub group: u32,
    /// Discretionary ACL.
    pub dacl: Dacl,
}

impl SecurityDescriptor {
    /// Create a default security descriptor (owner=root, no ACL).
    pub fn default_for_owner(owner_uid: u32) -> Self {
        Self {
            owner: owner_uid,
            group: 0,
            dacl: Dacl::default(),
        }
    }

    /// Create a default security descriptor for sockets (mode 0o600).
    pub fn default_socket() -> Self {
        Self {
            owner: 0,
            group: 0,
            dacl: Dacl::default(),
        }
    }
}

/// Check if the given credentials have the requested access to the security descriptor.
///
/// Returns `true` if access is granted. Empty DACL = everyone has full access.
#[must_use]
pub fn access_check(cred: &Credentials, sec: &SecurityDescriptor, desired_access: u32) -> bool {
    // Owner always has full access
    if cred.euid == sec.owner || cred.uid == sec.owner {
        return true;
    }

    // Root always has full access
    if cred.euid == 0 {
        return true;
    }

    // Empty DACL = everyone has access
    if sec.dacl.entries.is_empty() {
        return true;
    }

    // Check DACL entries (deny first, then allow)
    for ace in &sec.dacl.entries {
        if ace.deny
            && (cred.euid == ace.sid || cred.egid == ace.sid)
            && desired_access & ace.access_mask != 0
        {
            return false;
        }
    }

    for ace in &sec.dacl.entries {
        if !ace.deny
            && (cred.euid == ace.sid || cred.egid == ace.sid)
            && desired_access & ace.access_mask == desired_access
        {
            return true;
        }
    }

    false
}
