//! Cgroup v2 — Resource limiting and accounting.
//!
//! Implements a subset of Linux cgroup v2 for CPU, memory, and PID limits.
//! Uses a hierarchical structure matching Linux cgroup v2.

use alloc::vec::Vec;
use alloc::string::String;
use crate::syscalls::errno;
use crate::sync::IrqSafeMutex as Mutex;

/// Cgroup controller names
pub const CGROUP_CPU: &str = "cpu";
pub const CGROUP_MEMORY: &str = "memory";
pub const CGROUP_PIDS: &str = "pids";
pub const CGROUP_IO: &str = "io";

/// A cgroup controller
#[derive(Clone, Debug)]
pub struct CgroupController {
    pub name: String,
    pub enabled: bool,
}

/// CPU controller limits
#[derive(Clone, Debug)]
pub struct CpuController {
    pub max_usec: Option<u64>,    // cpu.max — max CPU time in microseconds per period
    pub period_usec: Option<u64>, // cpu.max — period in microseconds
    pub weight: u32,              // cpu.weight — proportional share
    pub nr_cpus: Option<u32>,     // cpu.max — max CPUs
}

impl Default for CpuController {
    fn default() -> Self {
        Self {
            max_usec: None,
            period_usec: Some(100_000), // 100ms default period
            weight: 100, // default weight
            nr_cpus: None,
        }
    }
}

/// Memory controller limits
#[derive(Clone, Debug)]
pub struct MemoryController {
    pub max: Option<u64>,         // memory.max — max memory in bytes
    pub high: Option<u64>,        // memory.high — high watermark
    pub low: Option<u64>,         // memory.low — best-effort protection
    pub min: Option<u64>,         // memory.min — hard protection
    pub swap_max: Option<u64>,    // memory.swap.max — max swap
    pub current: u64,             // memory.current — current usage
}

impl Default for MemoryController {
    fn default() -> Self {
        Self {
            max: None,
            high: None,
            low: None,
            min: None,
            swap_max: None,
            current: 0,
        }
    }
}

/// PID controller limits
#[derive(Clone, Debug)]
pub struct PidsController {
    pub max: Option<u32>,    // pids.max — max number of processes
    pub current: u32,        // pids.current — current number of processes
}

impl Default for PidsController {
    fn default() -> Self {
        Self {
            max: None,
            current: 0,
        }
    }
}

/// I/O controller limits
#[derive(Clone, Debug)]
pub struct IoController {
    pub max_read_bps: Option<u64>,   // io.max — max read bytes per second
    pub max_write_bps: Option<u64>,  // io.max — max write bytes per second
    pub max_read_iops: Option<u64>,  // io.max — max read IOPS
    pub max_write_iops: Option<u64>, // io.max — max write IOPS
}

impl Default for IoController {
    fn default() -> Self {
        Self {
            max_read_bps: None,
            max_write_bps: None,
            max_read_iops: None,
            max_write_iops: None,
        }
    }
}

/// A cgroup
#[derive(Clone, Debug)]
pub struct Cgroup {
    pub path: String,
    pub controllers: Vec<CgroupController>,
    pub cpu: CpuController,
    pub memory: MemoryController,
    pub pids: PidsController,
    pub io: IoController,
    pub children: Vec<String>,
}

impl Cgroup {
    pub fn new(path: &str) -> Self {
        Self {
            path: String::from(path),
            controllers: Vec::new(),
            cpu: CpuController::default(),
            memory: MemoryController::default(),
            pids: PidsController::default(),
            io: IoController::default(),
            children: Vec::new(),
        }
    }

    /// Check if a process can be created in this cgroup
    pub fn can_fork(&self) -> bool {
        match self.pids.max {
            Some(max) => self.pids.current < max,
            None => true, // No limit
        }
    }

    /// Check if memory allocation is allowed
    pub fn can_allocate(&self, bytes: u64) -> bool {
        match self.memory.max {
            Some(max) => self.memory.current + bytes <= max,
            None => true, // No limit
        }
    }
}

/// Global cgroup hierarchy — HashMap by path for O(1) lookup.
pub struct CgroupHierarchy {
    /// All cgroups keyed by absolute path (e.g. "/", "/system.slice", "/system.slice/docker.service")
    cgroups: hashbrown::HashMap<String, Cgroup>,
}

impl CgroupHierarchy {
    fn new() -> Self {
        let mut cgroups = hashbrown::HashMap::new();
        let mut root = Cgroup::new("/");
        root.controllers.push(CgroupController { name: String::from("cpu"), enabled: true });
        root.controllers.push(CgroupController { name: String::from("memory"), enabled: true });
        root.controllers.push(CgroupController { name: String::from("pids"), enabled: true });
        root.controllers.push(CgroupController { name: String::from("io"), enabled: true });
        cgroups.insert(String::from("/"), root);
        Self { cgroups }
    }

    pub fn create_cgroup(&mut self, path: &str) -> Result<(), errno::Errno> {
        if self.cgroups.contains_key(path) {
            return Err(errno::Errno::EEXIST);
        }

        // Validate parent exists
        let parent_path = match path.rfind('/') {
            Some(0) => String::from("/"),
            Some(pos) => String::from(&path[..pos]),
            None => String::from("/"),
        };
        if !self.cgroups.contains_key(&parent_path) {
            return Err(errno::Errno::ENOENT);
        }

        let cg = Cgroup::new(path);
        self.cgroups.insert(String::from(path), cg);
        // Register as child of parent
        if let Some(parent) = self.cgroups.get_mut(&parent_path) {
            parent.children.push(String::from(path));
        }
        Ok(())
    }

    pub fn find_cgroup(&self, path: &str) -> Option<&Cgroup> {
        self.cgroups.get(path)
    }

    pub fn find_cgroup_mut(&mut self, path: &str) -> Option<&mut Cgroup> {
        self.cgroups.get_mut(path)
    }

    /// Walk up the hierarchy to find the nearest ancestor with a matching controller.
    pub fn find_ancestor_with_controller(&self, path: &str, controller: &str) -> Option<&Cgroup> {
        let mut current = path;
        loop {
            if let Some(cg) = self.cgroups.get(current) {
                if cg.controllers.iter().any(|c| c.name == controller && c.enabled) {
                    return Some(cg);
                }
            }
            // Walk to parent
            match current.rfind('/') {
                Some(0) => {
                    // Check root
                    return self.cgroups.get("/");
                }
                Some(pos) => current = &current[..pos],
                None => return self.cgroups.get("/"),
            }
        }
    }
}

lazy_static::lazy_static! {
    static ref CGROUPS: Mutex<CgroupHierarchy> = Mutex::new(CgroupHierarchy::new());
}

pub fn cgroup_ensure() -> crate::sync::IrqSafeMutexGuard<'static, CgroupHierarchy> {
    CGROUPS.lock()
}

/// cgroup_mkdir — Create a new cgroup
pub fn cgroup_mkdir(path: &str) -> Result<(), errno::Errno> {
    let mut hierarchy = cgroup_ensure();
    hierarchy.create_cgroup(path)?;
    crate::serial_write("[CGROUP] Created ");
    crate::serial_write(path);
    crate::serial_write("\n");
    Ok(())
}

/// cgroup_write_controller — Write to a cgroup controller
pub fn cgroup_write(path: &str, controller: &str, value: &str) -> Result<(), errno::Errno> {
    let mut hierarchy = cgroup_ensure();
    let cg = hierarchy.find_cgroup_mut(path).ok_or(errno::Errno::ENOENT)?;

    match controller {
        "cpu.max" => {
            // Parse "max" or "period max"
            if value.trim() == "max" {
                cg.cpu.max_usec = None;
            } else if let Some((period, max)) = value.trim().split_once(' ') {
                cg.cpu.period_usec = period.parse().ok();
                cg.cpu.max_usec = max.parse().ok();
            }
        }
        "cpu.weight" => {
            cg.cpu.weight = value.trim().parse().unwrap_or(100);
        }
        "memory.max" => {
            cg.memory.max = if value.trim() == "max" { None } else { value.trim().parse().ok() };
        }
        "memory.high" => {
            cg.memory.high = if value.trim() == "max" { None } else { value.trim().parse().ok() };
        }
        "memory.min" => {
            cg.memory.min = value.trim().parse().ok();
        }
        "pids.max" => {
            cg.pids.max = if value.trim() == "max" { None } else { value.trim().parse().ok() };
        }
        "io.max" => {
            // Format: "MAJ:MIN rbps=1234 wbps=5678 riops=100 wiops=200"
            for token in value.split_whitespace() {
                if let Some(val) = token.strip_prefix("rbps=") {
                    cg.io.max_read_bps = val.parse().ok();
                } else if let Some(val) = token.strip_prefix("wbps=") {
                    cg.io.max_write_bps = val.parse().ok();
                } else if let Some(val) = token.strip_prefix("riops=") {
                    cg.io.max_read_iops = val.parse().ok();
                } else if let Some(val) = token.strip_prefix("wiops=") {
                    cg.io.max_write_iops = val.parse().ok();
                }
            }
        }
        _ => return Err(errno::Errno::ENOENT),
    }
    Ok(())
}

/// cgroup_read_controller — Read from a cgroup controller
pub fn cgroup_read(path: &str, controller: &str) -> Result<String, errno::Errno> {
    let hierarchy = cgroup_ensure();
    let cg = hierarchy.find_cgroup(path).ok_or(errno::Errno::ENOENT)?;

    match controller {
        "cpu.max" => Ok(match cg.cpu.max_usec {
            Some(max) => alloc::format!("{} {}", cg.cpu.period_usec.unwrap_or(100_000), max),
            None => String::from("max 100000"),
        }),
        "cpu.weight" => Ok(alloc::format!("{}", cg.cpu.weight)),
        "memory.max" => Ok(match cg.memory.max {
            Some(max) => alloc::format!("{}", max),
            None => String::from("max"),
        }),
        "memory.current" => Ok(alloc::format!("{}", cg.memory.current)),
        "pids.max" => Ok(match cg.pids.max {
            Some(max) => alloc::format!("{}", max),
            None => String::from("max"),
        }),
        "pids.current" => Ok(alloc::format!("{}", cg.pids.current)),
        _ => Err(errno::Errno::ENOENT),
    }
}

// ─── Syscall wrappers (handle copy_from_user / copy_to_user) ──────

fn read_user_string(ptr: *const u8, max: usize) -> Result<String, errno::Errno> {
    let mut buf = [0u8; 256];
    let copy_len = core::cmp::min(max, 255);
    if unsafe { crate::syscalls::user_access::copy_from_user(&mut buf[..copy_len], ptr) }.is_err() {
        return Err(errno::Errno::EFAULT);
    }
    let trimmed = core::str::from_utf8(&buf).unwrap_or("").trim_matches(|c| c == '\0');
    Ok(alloc::format!("{}", trimmed))
}

/// sys_cgroup_mkdir — handle copy_from_user and delegate
pub fn sys_cgroup_mkdir(path_ptr: *const u8) -> u64 {
    let path = match read_user_string(path_ptr, 256) {
        Ok(s) => s,
        Err(e) => return e as u64,
    };
    match cgroup_mkdir(&path) {
        Ok(()) => 0,
        Err(e) => e as u64,
    }
}

/// sys_cgroup_write — handle copy_from_user for path, controller, value
pub fn sys_cgroup_write(path_ptr: *const u8, ctrl_ptr: *const u8, val_ptr: *const u8) -> u64 {
    let path = match read_user_string(path_ptr, 256) {
        Ok(s) => s,
        Err(e) => return e as u64,
    };
    let ctrl = match read_user_string(ctrl_ptr, 64) {
        Ok(s) => s,
        Err(e) => return e as u64,
    };
    let val = match read_user_string(val_ptr, 128) {
        Ok(s) => s,
        Err(e) => return e as u64,
    };
    match cgroup_write(&path, &ctrl, &val) {
        Ok(()) => 0,
        Err(e) => e as u64,
    }
}

/// sys_cgroup_read — handle copy_from_user for path, controller, copy_to_user for result
pub fn sys_cgroup_read(path_ptr: *const u8, ctrl_ptr: *const u8, out_ptr: *mut u8) -> u64 {
    let path = match read_user_string(path_ptr, 256) {
        Ok(s) => s,
        Err(e) => return e as u64,
    };
    let ctrl = match read_user_string(ctrl_ptr, 64) {
        Ok(s) => s,
        Err(e) => return e as u64,
    };
    match cgroup_read(&path, &ctrl) {
        Ok(val) => {
            let bytes = val.as_bytes();
            let len = core::cmp::min(bytes.len(), 255);
            let mut buf = [0u8; 256];
            buf[..len].copy_from_slice(&bytes[..len]);
            if unsafe { crate::syscalls::user_access::copy_to_user(out_ptr, &buf) }.is_err() {
                errno::Errno::EFAULT as u64
            } else {
                len as u64
            }
        }
        Err(e) => e as u64,
    }
}
