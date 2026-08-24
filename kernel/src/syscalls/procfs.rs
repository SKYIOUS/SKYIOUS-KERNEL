//! procfs — virtual filesystem exposing process information at /proc.
//!
//! Provides /proc/meminfo, /proc/version, /proc/self/ directory, and basic
//! process listings needed by ps, top, and other userspace utilities.

use alloc::string::String;
use alloc::vec::Vec;
use alloc::sync::Arc;
use crate::task::process::{PROCESS_TABLE, FileDescriptor};
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

/// Read /proc/cpuinfo — CPU information for userspace tools.
fn read_cpuinfo() -> String {
    let mut s = String::new();
    // Report as a single logical CPU (the BSP).
    // SMP: iterate APIC IDs when SMP topology enumeration is wired.
    s.push_str("processor\t: 0\n");
    s.push_str("vendor_id\t: Vahi CPU\n");
    s.push_str("cpu family\t: 6\n");
    s.push_str("model\t\t: 158\n");
    s.push_str("model name\t: Vahi KVM64\n");
    s.push_str("stepping\t: 13\n");
    s.push_str(&alloc::format!("cpu MHz\t\t: {}.000\n", 2400));
    s.push_str("cache size\t: 16384 KB\n");
    s.push_str("bogomips\t: 4800.00\n");
    s.push_str("clflush size\t: 64\n");
    s.push_str("cache_alignment\t: 64\n");
    s.push_str("address sizes\t: 46 bits physical, 48 bits virtual\n");
    s.push_str("flags\t\t: fpu vme de pse tsc msr pae mce cx8 apic sep \n\t\t");
    s.push_str("mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 \n\t\t");
    s.push_str("ht syscall nx pdpe1gb rdtscp lm constant_tsc avx avx2 \n\t\t");
    s.push_str("fsgsbase bmi1 bmi2 avx512f avx512dq invpcid avx512cd \n\t\t");
    s.push_str("avx512bw avx512vl xsave xsaveopt avx512_vnni\n");
    s
}

/// Read /proc/PID/status — comprehensive process status for ps/top.
fn read_proc_status(pid: u64) -> String {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        let ppid = proc.parent_id.unwrap_or(0);
        let uid = proc.creds.lock().uid;
        let gid = proc.creds.lock().gid;
        let mem = proc.memory.lock();
        let vsize = mem.vmas.iter().map(|v| v.end - v.start).sum::<u64>();
        drop(mem);
        let name = proc.name.lock().clone();
        let utime = proc.utime.load(core::sync::atomic::Ordering::Relaxed);
        let stime = proc.stime.load(core::sync::atomic::Ordering::Relaxed);
        let cutime = proc.cutime.load(core::sync::atomic::Ordering::Relaxed);
        let cstime = proc.cstime.load(core::sync::atomic::Ordering::Relaxed);
        let children_count = proc.children.lock().len();
        let threads = 1; // TODO: thread count

        let mut s = String::new();
        s.push_str(&alloc::format!("Name:\t{}\n", name));
        s.push_str("State:\tS (sleeping)\n");
        s.push_str(&alloc::format!("Tgid:\t{}\n", pid));
        s.push_str(&alloc::format!("Pid:\t{}\n", pid));
        s.push_str(&alloc::format!("PPid:\t{}\n", ppid));
        s.push_str(&alloc::format!("TracerPid:\t0\n"));
        s.push_str(&alloc::format!("Uid:\t{}\t{}\t{}\t{}\n", uid, uid, uid, uid));
        s.push_str(&alloc::format!("Gid:\t{}\t{}\t{}\t{}\n", gid, gid, gid, gid));
        s.push_str(&alloc::format!("FDSize:\t{}\n", 256));
        s.push_str(&alloc::format!("Threads:\t{}\n", threads));
        s.push_str(&alloc::format!("SigPnd:\t0\n"));
        s.push_str(&alloc::format!("ShdPnd:\t0\n"));
        s.push_str(&alloc::format!("SigBlk:\t0\n"));
        s.push_str(&alloc::format!("SigIgn:\t0\n"));
        s.push_str(&alloc::format!("SigCgt:\t0\n"));
        s.push_str("CapInh:\t0000000000000000\n");
        s.push_str("CapPrm:\t0000000000000000\n");
        s.push_str("CapEff:\t0000000000000000\n");
        s.push_str("CapBnd:\t0000000000000000\n");
        s.push_str("CapAmb:\t0000000000000000\n");
        s.push_str(&alloc::format!("VmPeak:\t{} kB\n", vsize / 1024));
        s.push_str(&alloc::format!("VmSize:\t{} kB\n", vsize / 1024));
        s.push_str("VmLck:\t       0 kB\n");
        s.push_str("VmPin:\t       0 kB\n");
        s.push_str("VmHWM:\t       0 kB\n");
        s.push_str("VmRSS:\t       0 kB\n");
        s.push_str("VmData:\t       0 kB\n");
        s.push_str("VmStk:\t       0 kB\n");
        s.push_str("VmExe:\t       0 kB\n");
        s.push_str("VmLib:\t       0 kB\n");
        s.push_str("VmSwap:\t       0 kB\n");
        s.push_str(&alloc::format!("VmSwap:\t{} kB\n", 0));
        s.push_str(&alloc::format!("Threads:\t{}\n", threads));
        s.push_str(&alloc::format!(" voluntary_ctxt_switches:\t0\n"));
        s.push_str(&alloc::format!(" nonvoluntary_ctxt_switches:\t0\n"));
        // time fields (in clock ticks, 100 Hz)
        s.push_str(&alloc::format!("\n utime:\t{}\n", utime));
        s.push_str(&alloc::format!(" stime:\t{}\n", stime));
        s.push_str(&alloc::format!(" cutime:\t{}\n", cutime));
        s.push_str(&alloc::format!(" cstime:\t{}\n", cstime));
        s.push_str(&alloc::format!(" num_children:\t{}\n", children_count));
        s
    } else {
        String::new()
    }
}

/// Read /proc/PID/maps — virtual memory maps for pmap/valgrind/debuggers.
fn read_proc_maps(pid: u64) -> String {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        let mem = proc.memory.lock();
        let mut s = String::new();
        for vma in &mem.vmas {
            let perms = {
                let p = &vma.flags;
                let mut r = String::new();
                if p.contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
                    r.push('r');
                } else {
                    r.push('-');
                }
                if p.contains(x86_64::structures::paging::PageTableFlags::WRITABLE) {
                    r.push('w');
                } else {
                    r.push('-');
                }
                if !p.contains(x86_64::structures::paging::PageTableFlags::NO_EXECUTE) {
                    r.push('x');
                } else {
                    r.push('-');
                }
                r.push('p');
                r
            };
            s.push_str(&alloc::format!(
                "{:016x}-{:016x} {} {:08x} 00:00 0{}\n",
                vma.start, vma.end, perms, vma.file_offset,
                if vma.is_shared { " shmem" } else { ""
            }));
        }
        s
    } else {
        String::new()
    }
}

/// Read /proc/PID/fd/ directory listing — shows open file descriptors.
fn list_proc_fds(pid: u64) -> Vec<String> {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        let files = proc.files.lock();
        let mut result = Vec::new();
        for (fd, _entry) in files.fd_table.iter().enumerate() {
            if _entry.is_some() {
                result.push(alloc::format!("{}", fd));
            }
        }
        result
    } else {
        Vec::new()
    }
}

/// Read /proc/PID/fd/N — symlink target description.
fn read_proc_fd(pid: u64, fd: usize) -> String {
    let table = PROCESS_TABLE.lock();
    if let Some(proc) = table.get(&pid) {
        let files = proc.files.lock();
        if fd < files.fd_table.len() {
            if let Some(ref entry) = files.fd_table[fd] {
                match entry {
                    FileDescriptor::File { node, .. } => {
                        alloc::format!("/dev/{}", node.name())
                    }
                    FileDescriptor::Socket(..) => String::from("socket:"),
                    FileDescriptor::UnixSocket(..) => String::from("socket:"),
                    FileDescriptor::PtyMaster { .. } => String::from("/dev/ptmx"),
                    FileDescriptor::PtySlave { .. } => String::from("/dev/pts/0"),
                    FileDescriptor::SignalFd(..) => String::from("signalfd"),
                    FileDescriptor::EventFd(..) => String::from("eventfd"),
                    FileDescriptor::TimerFd(..) => String::from("timerfd"),
                    FileDescriptor::InotifyFd { .. } => String::from("inotify"),
                    FileDescriptor::IoUringFd(..) => String::from("io_uring"),
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    }
}

/// Read /proc/PID/io — I/O statistics.
fn read_proc_io(pid: u64) -> String {
    let _ = pid; // TODO: track per-process I/O counters
    let mut s = String::new();
    s.push_str("rchar: 0\n");
    s.push_str("wchar: 0\n");
    s.push_str("syscr: 0\n");
    s.push_str("syscw: 0\n");
    s.push_str("read_bytes: 0\n");
    s.push_str("write_bytes: 0\n");
    s.push_str("cancelled_write_bytes: 0\n");
    s
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
            "/proc/cpuinfo" => read_cpuinfo(),
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
                                "maps" => read_proc_maps(pid),
                                "oom_score" => read_proc_oom_score(pid),
                                "oom_score_adj" => read_proc_oom_adj(pid),
                                "io" => read_proc_io(pid),
                                _ => {
                                    // Check for /proc/PID/fd/N
                                    if file.starts_with("fd/") {
                                        let fd_str = &file[3..];
                                        if let Ok(fd_num) = fd_str.parse::<usize>() {
                                            read_proc_fd(pid, fd_num)
                                        } else {
                                            String::new()
                                        }
                                    } else {
                                        alloc::format!("{} is not an integer\n", file)
                                    }
                                }
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
        let mut entries = Vec::new();        if self.path == "/proc" || self.path == "/proc/" {
            entries.push(Arc::new(ProcFsNode::new("/proc/meminfo")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/version")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/uptime")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/stat")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/loadavg")) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new("/proc/cpuinfo")) as Arc<dyn VfsNode>);

            let table = PROCESS_TABLE.lock();
            for pid in table.keys() {
                entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("/proc/{}", pid),
                )) as Arc<dyn VfsNode>);
            }
        } else if self.path.starts_with("/proc/") && !self.path[6..].contains('/') {
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/status", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/cmdline", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/maps", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/io", self.path),
                )) as Arc<dyn VfsNode>);
            entries.push(Arc::new(ProcFsNode::new(
                    &alloc::format!("{}/fd", self.path),
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
