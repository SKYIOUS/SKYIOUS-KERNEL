//! namespaces state for crate extraction.

/// Per-process namespace set.
#[derive(Debug, Clone, Default)]
pub struct NamespaceSet {
    /// PID namespace ID.
    pub pid_ns: u64,
    /// Mount namespace ID.
    pub mount_ns: u64,
    /// Network namespace ID.
    pub net_ns: u64,
    /// IPC namespace ID.
    pub ipc_ns: u64,
    /// UTS namespace ID.
    pub uts_ns: u64,
    /// User namespace ID.
    pub user_ns: u64,
    /// Cgroup namespace ID.
    pub cgroup_ns: u64,
}

/// Namespace type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceType {
    Cgroup,
    Ipc,
    Network,
    Mount,
    Pid,
    User,
    Uts,
}
