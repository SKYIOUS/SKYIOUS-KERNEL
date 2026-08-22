//! procfs — virtual filesystem exposing process information at /proc.
//!
//! Provides /proc/meminfo, /proc/version, /proc/self/ directory, and basic
//! process listings needed by ps, top, and other userspace utilities.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use crate::task::process::PROCESS_TABLE;
use crate::vfs::{VfsNode, Stat};

/// Read /proc/meminfo
fn read_meminfo() -> String {
    let page_size = 4096u64;
    let total_pages = 256 * 1024; // 1 GiB default
    let free_pages = crate::memory::phys::total_free_frames() as u64;

    let total_kb = total_pages * page_size / 1024;
    let free_kb = free_pages * page_size / 1024;
    let used_kb = total_kb - free_kb;

    let mut s = String::new();
    s.push_str("MemTotal:       ");
    s.push_str(&alloc::format!("{}", total_kb));
    s.push_str(" kB\n");
    s.push_str("MemFree:        ");
    s.push_str(&alloc::format!("{}", free_kb));
    s.push_str(" kB\n");
    s.push_str("MemAvailable:   ");
    s.push_str(&alloc::format!("{}", free_kb));
    s.push_str(" kB\n");
    s.push_str("Buffers:             0 kB\n");
    s.push_str("Cached:              0 kB\n");
    s.push_str("SwapCached:          0 kB\n");
    s.push_str("Active:         ");
    s.push_str(&alloc::format!("{}", used_kb));
    s.push_str(" kB\n");
    s.push_str("Inactive:             0 kB\n");
    s.push_str("SwapTotal:           0 kB\n");
    s.push_str("SwapFree:            0 kB\n");
    s.push_str("Dirty:                0 kB\n");
    s.push_str("Writeback:            0 kB\n");
    s.push_str("AnonPages:      ");
    s.push_str(&alloc::format!("{}", used_kb));
    s.push_str(" kB\n");
    s.push_str("Mapped:              0 kB\n");
    s.push_str("Shmem:                0 kB\n");
    s.push_str("Slab:           ");
    s.push_str(&alloc::format!("{}", 1024));
    s.push_str(" kB\n");
    s.push_str("SReclaimable:       64 kB\n");
    s.push_str("SUnreclaim:        960 kB\n");
    s.push_str("PageTables:        128 kB\n");
    let (oom_kills, _) = crate::task::oom::oom_stats();
    s.push_str(&alloc::format!("OOMKills:          {}\n", oom_kills));
    s
}

/// Read /proc/version
fn read_version() -> String {
    String::from("Vahi OS version 0.3.0 (sarga-os) (gcc version 13.2.0) 2026-08-21\n")
}

/// Read /proc/uptime
fn read_uptime() -> String {
    let ticks = crate::interrupts::get_ticks();
    let secs = ticks / 100;
    let idle = 0;
    alloc::format!("{}.00 {}.{:02}\n", secs, idle, 0)
}

/// Read /proc/stat — CPU statistics
fn read_stat() -> String {
    let ticks = crate::interrupts::get_ticks();
    let secs = ticks / 100;
    alloc::format!("cpu  {} 0 0 {} 0 0 0 0 0 0\n", secs, secs)
}

/// Read /proc/loadavg
fn read_loadavg() -> String {
    String::from("1.00 1.00 1.00 1/1 1\n")
}

/// Read /proc/PID/oom_score — computed OOM score for a process.
fn read_proc_oom_score(pid: u64) -> String {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        let rss = crate::task::oom::estimate_process_rss(proc);
        let total_mem = crate::task::oom::total_system_memory();
        let (score, _, _) = crate::task::oom::compute_oom_score(pid, rss, total_mem);
        alloc::format!("{}\n", score)
    } else {
        String::from("0\n")
    }
}

/// Read /proc/PID/oom_score_adj — user-tunable OOM adjustment.
fn read_proc_oom_adj(pid: u64) -> String {
    let adj = crate::task::oom::get_oom_score_adj(pid);
    alloc::format!("{}\n", adj)
}

/// Read /proc/PID/status (simplified)
fn read_proc_status(pid: u64) -> String {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        let ppid = proc.parent_id.unwrap_or(0);
        let uid = proc.creds.lock().uid;
        let gid = proc.creds.lock().gid;
        let vsize = proc.vmas.lock().iter().map(|v| v.end - v.start).sum::<u64>();

        let mut s = String::new();
        s.push_str("Name:\tinit\n");
        s.push_str("State:\tS (sleeping)\n");
        s.push_str(&alloc::format!("Tgid:\t{}\n", pid));
        s.push_str(&alloc::format!("Pid:\t{}\n", pid));
        s.push_str(&alloc::format!("PPid:\t{}\n", ppid));
        s.push_str(&alloc::format!("Uid:\t{}\t{}\t{}\t{}\n", uid, uid, uid, uid));
        s.push_str(&alloc::format!("Gid:\t{}\t{}\t{}\t{}\n", gid, gid, gid, gid));
        s.push_str(&alloc::format!("VmSize:\t{} kB\n", vsize / 1024));
        s.push_str(&alloc::format!("VmRSS:\t{} kB\n", vsize / 1024));
        s
    } else {
        String::new()
    }
}

/// Read /proc/PID/cmdline
fn read_proc_cmdline(pid: u64) -> String {
    if pid == 1 {
        String::from("/bin/init\0")
    } else {
        alloc::format!("/bin/unknown\0")
    }
}

/// Procfs VFS node implementation
pub struct ProcFsNode {
    path: String,
}

impl ProcFsNode {
    pub fn new(path: &str) -> Self {
        Self {
            path: String::from(path),
        }
    }
}

impl VfsNode for ProcFsNode {
    fn name(&self) -> String {
        if let Some(pos) = self.path.rfind('/') {
            String::from(&self.path[pos + 1..])
        } else {
            self.path.clone()
        }
    }

    fn is_dir(&self) -> bool {
        if self.path == "/proc" || self.path == "/proc/" {
            return true;
        }
        let rest = if self.path.starts_with("/proc/") {
            &self.path[6..]
        } else {
            return false;
        };
        if !rest.contains('/') {
            return rest.parse::<u64>().is_ok();
        }
        false
    }

    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        let content = match self.path.as_str() {
            "/proc/meminfo" => read_meminfo(),
            "/proc/version" => read_version(),
            "/proc/uptime" => read_uptime(),
            "/proc/stat" => read_stat(),
            "/proc/loadavg" => read_loadavg(),
            _ => {
                if self.path.starts_with("/proc/") {
                    let rest = &self.path[6..];
                    if let Some(slash_pos) = rest.find('/') {
                        let pid_str = &rest[..slash_pos];
                        let file = &rest[slash_pos + 1..];
                        if let Ok(pid) = pid_str.parse::<u64>() {
                            match file {
                                "status" => read_proc_status(pid),
                                "cmdline" => read_proc_cmdline(pid),
                                "oom_score" => read_proc_oom_score(pid),
                                "oom_score_adj" => read_proc_oom_adj(pid),
                                _ => String::new(),
                            }
                        } else {
                            String::new()
                        }
                    } else if let Ok(pid) = rest.parse::<u64>() {
                        read_proc_status(pid)
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }
        };

        Ok(content.into_bytes())
    }

    fn write(&self, _data: &[u8]) -> Result<(), ()> {
        Err(())
    }

    fn stat(&self) -> Result<Stat, ()> {
        Ok(Stat {
            st_dev: 0,
            st_ino: 0,
            st_mode: 0o040555,
            st_nlink: 1,
            st_uid: 0,
            st_gid: 0,
            st_rdev: 0,
            st_size: 0,
            st_atime: 0,
            st_mtime: 0,
            st_ctime: 0,
            st_atime_nsec: 0,
            st_mtime_nsec: 0,
            st_ctime_nsec: 0,
        })
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        let mut entries = Vec::new();

        if self.path == "/proc" || self.path == "/proc/" {
            entries.push(Arc::new(ProcFsNode::new("/proc/meminfo")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/version")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/uptime")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/stat")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/loadavg")) as Arc<dyn VfsNode>);

            let table = PROCESS_TABLE.lock();
            for pid in table.keys() {
                entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("/proc/{}", pid),
                )) as Arc<dyn VfsNode>);
            }
        } else if self.path.starts_with("/proc/") && !self.path[6..].contains('/') {            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/status", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/cmdline", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/oom_score", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/oom_score_adj", self.path),
                )) as Arc<dyn VfsNode>);
        }

        Ok(entries)
    }
}

/// Procfs filesystem implementation
pub struct ProcFs;

impl crate::vfs::FileSystem for ProcFs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(Arc::new(ProcFsNode::new("/proc")))
    }
}

/// Mount procfs at /proc
pub fn mount_procfs() {
    let fs = Arc::new(ProcFs);
    crate::vfs::VFS.lock().mount("/proc", fs);
    crate::serial_write("[VFS] Mounted procfs at /proc\n");
}

/// Initialize procfs — called during VFS init
pub fn init() {
    mount_procfs();
}
