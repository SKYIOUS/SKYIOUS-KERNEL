//! Vm vocabulary for the vahi kernel crate boundary.

pub use crate::identity::VirtAddr;

// ─── Virtual Memory Area ────────────────────────────────────────────
// Shared between task (ProcessMemory) and memory (paging/CoW).

/// Permission bits for a virtual memory area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VmProt {
    None = 0,
    Read = 1,
    Write = 2,
    Exec = 4,
    ReadWrite = 3,
    ReadExec = 5,
    ReadWriteExec = 7,
}

/// Flags controlling mapping behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmFlags {
    pub read: bool,
    pub write: bool,
    pub exec: bool,
    pub shared: bool,    // MAP_SHARED vs MAP_PRIVATE
    pub fixed: bool,     // MAP_FIXED
    pub anonymous: bool, // MAP_ANONYMOUS
    pub populate: bool,  // MAP_POPULATE (prefault)
    pub stack: bool,     // MAP_GROWSDOWN (stack region)
}

impl VmFlags {
    /// All flags cleared (no permissions, no special behavior).
    pub const fn empty() -> Self {
        Self {
            read: false,
            write: false,
            exec: false,
            shared: false,
            fixed: false,
            anonymous: false,
            populate: false,
            stack: false,
        }
    }

    /// Read-only mapping.
    pub const READ: Self = Self {
        read: true,
        ..Self::empty()
    };
    /// Read-write mapping.
    pub const READ_WRITE: Self = Self {
        read: true,
        write: true,
        ..Self::empty()
    };
    /// Read-execute mapping (for code).
    pub const READ_EXEC: Self = Self {
        read: true,
        exec: true,
        ..Self::empty()
    };

    /// Convert permission flags to a [`VmProt`] value.
    #[must_use]
    pub fn to_prot(self) -> VmProt {
        match (self.read, self.write, self.exec) {
            (true, true, true) => VmProt::ReadWriteExec,
            (true, true, false) => VmProt::ReadWrite,
            (true, false, true) => VmProt::ReadExec,
            (true, false, false) => VmProt::Read,
            _ => VmProt::None,
        }
    }
}

/// Virtual memory area: a contiguous region of virtual address space
/// with consistent permissions and backing.
#[derive(Debug, Clone)]
pub struct Vma {
    pub start: VirtAddr,
    pub end: VirtAddr,
    pub flags: VmFlags,
    pub offset: u64,  // file offset (for file-backed mappings)
    pub inode: u64,   // backing file inode (0 = anonymous)
    pub cow: bool,    // copy-on-write page
    pub mapped: bool, // pages currently resident
}

impl Vma {
    /// Length of this VMA in bytes.
    #[must_use]
    pub const fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    /// Returns `true` if this VMA has zero length.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Returns `true` if the given virtual address falls within this VMA.
    #[must_use]
    pub const fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.start && addr < self.end
    }
}
