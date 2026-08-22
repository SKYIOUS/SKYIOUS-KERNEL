//! Linux namespace support — process isolation primitives.
//!
//! Implements PID, Mount, Network, IPC, UTS, and User namespaces.
//! Provides unshare(), setns(), and clone() with CLONE_NEW* flags.

use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use hashbrown::HashMap;
use crate::task::process::CURRENT_PROCESS;
use crate::syscalls::errno;
use crate::sync::IrqSafeMutex as Mutex;

// Clone flags — correct Linux x86_64 values from include/uapi/linux/sched.h
pub const CLONE_VM: u64 = 0x0000_0100;
pub const CLONE_FS: u64 = 0x0000_0200;
pub const CLONE_FILES: u64 = 0x0000_0400;
pub const CLONE_SIGHAND: u64 = 0x0000_0800;
pub const CLONE_PIDFD: u64 = 0x0000_1000;
pub const CLONE_PTRACE: u64 = 0x0000_2000;
pub const CLONE_VFORK: u64 = 0x0000_4000;
pub const CLONE_PARENT: u64 = 0x0000_8000;
pub const CLONE_THREAD: u64 = 0x0001_0000;
pub const CLONE_NEWNS: u64 = 0x0002_0000;
pub const CLONE_SYSVSEM: u64 = 0x0004_0000;
pub const CLONE_SETTLS: u64 = 0x0008_0000;
pub const CLONE_PARENT_SETTID: u64 = 0x0010_0000;
pub const CLONE_CHILD_CLEARTID: u64 = 0x0020_0000;
pub const CLONE_CHILD_SETTID: u64 = 0x0100_0000;
pub const CLONE_NEWCGROUP: u64 = 0x0200_0000;
pub const CLONE_NEWUTS: u64 = 0x0400_0000;
pub const CLONE_NEWIPC: u64 = 0x0800_0000;
pub const CLONE_NEWUSER: u64 = 0x1000_0000;
pub const CLONE_NEWPID: u64 = 0x2000_0000;
pub const CLONE_NEWNET: u64 = 0x4000_0000;

/// Namespace types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NamespaceType {
    Pid = 0,
    Mount = 1,
    Network = 2,
    Ipc = 3,
    Uts = 4,
    User = 5,
}

impl NamespaceType {
    pub fn from_u64(val: u64) -> Option<Self> {
        match val {
            0 => Some(Self::Pid),
            1 => Some(Self::Mount),
            2 => Some(Self::Network),
            3 => Some(Self::Ipc),
            4 => Some(Self::Uts),
            5 => Some(Self::User),
            _ => None,
        }
    }
}

/// UTS namespace data
pub struct UtsNamespace {
    pub hostname: String,
    pub domainname: String,
}

impl Default for UtsNamespace {
    fn default() -> Self {
        Self {
            hostname: String::from("vahi"),
            domainname: String::from("(none)"),
        }
    }
}

/// PID namespace data
pub struct PidNamespace {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub pid_counter: u64,
}

/// Mount namespace data
pub struct MountNamespace {
    pub id: u64,
    pub mounts: Vec<MountEntry>,
}

#[derive(Clone)]
pub struct MountEntry {
    pub source: String,
    pub target: String,
    pub fstype: String,
    pub flags: u64,
}

/// Network namespace data
pub struct NetNamespace {
    pub id: u64,
    pub interfaces: HashMap<String, NetInterface>,
}

#[derive(Clone)]
pub struct NetInterface {
    pub name: String,
    pub ip_addr: u32,
    pub netmask: u32,
    pub mac: [u8; 6],
    pub flags: u32,
}

/// IPC namespace data
pub struct IpcNamespace {
    pub id: u64,
    pub shm_ids: Vec<u64>,
    pub sem_ids: Vec<u64>,
    pub msg_ids: Vec<u64>,
}

/// User namespace data
pub struct UserNamespace {
    pub id: u64,
    pub uid_map: Vec<UidMapping>,
    pub gid_map: Vec<GidMapping>,
}

#[derive(Clone, Copy)]
pub struct UidMapping {
    pub ns_id: u32,
    pub host_id: u32,
    pub count: u32,
}

#[derive(Clone, Copy)]
pub struct GidMapping {
    pub ns_id: u32,
    pub host_id: u32,
    pub count: u32,
}

/// Per-process namespace set
pub struct NamespaceSet {
    pub pid_ns: Arc<Mutex<PidNamespace>>,
    pub mount_ns: Arc<Mutex<MountNamespace>>,
    pub net_ns: Arc<Mutex<NetNamespace>>,
    pub ipc_ns: Arc<Mutex<IpcNamespace>>,
    pub uts_ns: Arc<Mutex<UtsNamespace>>,
    pub user_ns: Arc<Mutex<UserNamespace>>,
}

impl Default for NamespaceSet {
    fn default() -> Self {
        Self {
            pid_ns: Arc::new(Mutex::new(PidNamespace { id: 0, parent_id: None, pid_counter: 100 })),
            mount_ns: Arc::new(Mutex::new(MountNamespace { id: 0, mounts: Vec::new() })),
            net_ns: Arc::new(Mutex::new(NetNamespace { id: 0, interfaces: HashMap::new() })),
            ipc_ns: Arc::new(Mutex::new(IpcNamespace { id: 0, shm_ids: Vec::new(), sem_ids: Vec::new(), msg_ids: Vec::new() })),
            uts_ns: Arc::new(Mutex::new(UtsNamespace::default())),
            user_ns: Arc::new(Mutex::new(UserNamespace { id: 0, uid_map: Vec::new(), gid_map: Vec::new() })),
        }
    }
}

impl NamespaceSet {
    /// Clone this namespace set. If clone_flags has CLONE_NEW*, create new namespaces.
    /// Otherwise, share the parent's namespace.
    pub fn clone_with_flags(&self, clone_flags: u64, new_pid: u64) -> Self {
        Self {
            pid_ns: if clone_flags & CLONE_NEWPID != 0 {
                let parent = self.pid_ns.lock();
                Arc::new(Mutex::new(PidNamespace {
                    id: parent.id + 1,
                    parent_id: Some(parent.id),
                    pid_counter: 1,
                }))
            } else {
                self.pid_ns.clone()
            },
            mount_ns: if clone_flags & CLONE_NEWNS != 0 {
                let parent = self.mount_ns.lock();
                Arc::new(Mutex::new(MountNamespace {
                    id: parent.id + 1,
                    mounts: parent.mounts.clone(),
                }))
            } else {
                self.mount_ns.clone()
            },
            net_ns: if clone_flags & CLONE_NEWNET != 0 {
                Arc::new(Mutex::new(NetNamespace {
                    id: self.net_ns.lock().id + 1,
                    interfaces: HashMap::new(),
                }))
            } else {
                self.net_ns.clone()
            },
            ipc_ns: if clone_flags & CLONE_NEWIPC != 0 {
                Arc::new(Mutex::new(IpcNamespace {
                    id: self.ipc_ns.lock().id + 1,
                    shm_ids: Vec::new(),
                    sem_ids: Vec::new(),
                    msg_ids: Vec::new(),
                }))
            } else {
                self.ipc_ns.clone()
            },
            uts_ns: if clone_flags & CLONE_NEWUTS != 0 {
                let parent = self.uts_ns.lock();
                Arc::new(Mutex::new(UtsNamespace {
                    hostname: parent.hostname.clone(),
                    domainname: parent.domainname.clone(),
                }))
            } else {
                self.uts_ns.clone()
            },
            user_ns: if clone_flags & CLONE_NEWUSER != 0 {
                Arc::new(Mutex::new(UserNamespace {
                    id: self.user_ns.lock().id + 1,
                    uid_map: Vec::new(),
                    gid_map: Vec::new(),
                }))
            } else {
                self.user_ns.clone()
            },
        }
    }
}

// Global namespace counter
static NAMESPACE_COUNTER: crate::sync::IrqSafeMutex<u64> = crate::sync::IrqSafeMutex::new(1);

fn next_ns_id() -> u64 {
    let mut counter = NAMESPACE_COUNTER.lock();
    let id = *counter;
    *counter += 1;
    id
}

/// unshare() — Remove the calling process from one or more namespaces.
///
/// flags: CLONE_NEW* flags indicating which namespaces to unshare.
pub fn sys_unshare(flags: u64) -> u64 {
    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Validate flags
    let valid_flags = CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS | CLONE_NEWUSER;
    if flags & !valid_flags != 0 {
        return errno::Errno::EINVAL as u64;
    }

    // User namespace requires privileges unless we have user namespaces
    if flags & CLONE_NEWUSER != 0 {
        // User namespaces can be created by any process
        // But we need CAP_SYS_ADMIN for other namespace types
        if flags & !CLONE_NEWUSER != 0 {
            let cred = proc.creds.lock();
            if cred.euid != 0 && !crate::syscalls::has_capability(crate::syscalls::helpers::CAP_SYS_ADMIN) {
                return errno::Errno::EPERM as u64;
            }
        }
    } else if flags != 0 {
        let cred = proc.creds.lock();
        if cred.euid != 0 && !crate::syscalls::has_capability(crate::syscalls::helpers::CAP_SYS_ADMIN) {
            return errno::Errno::EPERM as u64;
        }
    }

    // Hold the namespaces lock once for the entire operation
    {
        let ns = proc.namespaces.lock();

        if flags & CLONE_NEWPID != 0 {
            let parent_id = ns.pid_ns.lock().id;
            *ns.pid_ns.lock() = PidNamespace {
                id: next_ns_id(),
                parent_id: Some(parent_id),
                pid_counter: 1,
            };
        }
        if flags & CLONE_NEWNS != 0 {
            let mounts = ns.mount_ns.lock().mounts.clone();
            *ns.mount_ns.lock() = MountNamespace { id: next_ns_id(), mounts };
        }
        if flags & CLONE_NEWNET != 0 {
            *ns.net_ns.lock() = NetNamespace { id: next_ns_id(), interfaces: HashMap::new() };
        }
        if flags & CLONE_NEWIPC != 0 {
            *ns.ipc_ns.lock() = IpcNamespace {
                id: next_ns_id(), shm_ids: Vec::new(), sem_ids: Vec::new(), msg_ids: Vec::new(),
            };
        }
        if flags & CLONE_NEWUTS != 0 {
            let (hostname, domainname) = {
                let parent = ns.uts_ns.lock();
                (parent.hostname.clone(), parent.domainname.clone())
            };
            *ns.uts_ns.lock() = UtsNamespace { hostname, domainname };
        }
        if flags & CLONE_NEWUSER != 0 {
            *ns.user_ns.lock() = UserNamespace {
                id: next_ns_id(), uid_map: Vec::new(), gid_map: Vec::new(),
            };
        }
    } // ns lock released here

    crate::serial_write("[NS] unshare flags=0x");
    crate::serial_write(&alloc::format!("{:x} pid={}\n", flags, proc.id));
    0
}

/// setns() — Join an existing namespace.
///
/// fd: File descriptor referring to a namespace (stored in namespace_fds)
/// nstype: Namespace type (0 = auto-detect, or CLONE_NEW*)
pub fn sys_setns(fd: u64, nstype: u64) -> u64 {
    if nstype != 0 {
        let valid = CLONE_NEWPID | CLONE_NEWNS | CLONE_NEWNET | CLONE_NEWIPC | CLONE_NEWUTS | CLONE_NEWUSER;
        if nstype & !valid != 0 {
            return errno::Errno::EINVAL as u64;
        }
    }

    let lock = CURRENT_PROCESS.lock();
    let proc = match *lock {
        Some(ref p) => p,
        None => return errno::Errno::ESRCH as u64,
    };

    // Look up the namespace type from namespace_fds
    let ns_type = {
        let ns_fds = proc.namespace_fds.lock();
        match ns_fds.get(&(fd as usize)) {
            Some(t) => *t,
            None => return errno::Errno::EBADF as u64,
        }
    };

    // Validate nstype matches if specified
    if nstype != 0 {
        let requested_type = match nstype {
            x if x & CLONE_NEWPID != 0 => NamespaceType::Pid,
            x if x & CLONE_NEWNS != 0 => NamespaceType::Mount,
            x if x & CLONE_NEWNET != 0 => NamespaceType::Network,
            x if x & CLONE_NEWIPC != 0 => NamespaceType::Ipc,
            x if x & CLONE_NEWUTS != 0 => NamespaceType::Uts,
            x if x & CLONE_NEWUSER != 0 => NamespaceType::User,
            _ => return errno::Errno::EINVAL as u64,
        };
        if requested_type != ns_type {
            return errno::Errno::EINVAL as u64;
        }
    }

    // In a full implementation, we'd actually swap the namespace reference.
    // For now, validate the fd exists and the type matches, then succeed.
    crate::serial_write("[NS] setns fd=");
    crate::serial_write(&alloc::format!("{} type={:?}\n", fd, ns_type));
    0
}

/// Clone with namespace flags — creates new namespaces for the child.
/// Called from sys_clone when CLONE_NEW* flags are set.
pub fn clone_namespaces(parent: &crate::task::process::Process, child: &crate::task::process::Process, clone_flags: u64) {
    let parent_ns = parent.namespaces.lock();
    let mut child_ns = child.namespaces.lock();
    *child_ns = parent_ns.clone_with_flags(clone_flags, child.id);
}
