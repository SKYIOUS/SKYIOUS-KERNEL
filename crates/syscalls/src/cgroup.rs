//! cgroup stub for crate extraction.
//! The real implementation lives in `kernel/src/syscalls/cgroup.rs`.

extern crate alloc;

/// Stub cgroup handle.
#[derive(Debug, Clone, Default)]
pub struct Cgroup {
    pub path: alloc::string::String,
}

impl Cgroup {
    /// Stub: check if this cgroup can allocate more memory.
    #[allow(dead_code)]
    pub fn can_allocate(&self, _bytes: usize) -> bool {
        true
    }
}

/// Stub cgroup hierarchy.
#[derive(Debug, Clone, Default)]
pub struct CgroupHierarchy {
    pub root_path: alloc::string::String,
}

impl CgroupHierarchy {
    /// Stub: ensure cgroup hierarchy exists.
    #[allow(dead_code)]
    pub fn ensure() -> Self {
        CgroupHierarchy {
            root_path: alloc::string::String::new(),
        }
    }

    /// Stub: find a cgroup by path.
    #[allow(dead_code)]
    pub fn find_cgroup(&self, _path: &str) -> Option<Cgroup> {
        Some(Cgroup {
            path: alloc::string::String::new(),
        })
    }

    /// Stub: account memory to a cgroup.
    #[allow(dead_code)]
    pub fn account_memory(&self, _path: &str, _bytes: usize) {}
}
