use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::IrqSafeMutex as Mutex;
use crate::vfs::{FileSystem, VfsNode, Stat, S_IFDIR, S_IFREG};
use crate::interrupts;

enum CtlInner {
    Dir,
    File(alloc::boxed::Box<dyn Fn() -> Vec<u8> + Send + Sync>),
}

struct CtlNode {
    name: String,
    inner: CtlInner,
    children: Mutex<Vec<Arc<CtlNode>>>,
}

impl VfsNode for CtlNode {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        matches!(self.inner, CtlInner::Dir)
    }

    fn read(&self, _max_len: usize) -> Result<Vec<u8>, ()> {
        match &self.inner {
            CtlInner::Dir => Err(()),
            CtlInner::File(func) => Ok(func()),
        }
    }

    fn statfs(&self) -> Result<crate::vfs::StatFs, ()> {
        Ok(crate::vfs::StatFs {
            f_type: 0x01021994, f_bsize: 4096,
            f_blocks: 0, f_bfree: 0, f_bavail: 0,
            f_files: 0, f_ffree: 0,
        })
    }

    fn stat(&self) -> Result<Stat, ()> {
        match &self.inner {
            CtlInner::Dir => Ok(Stat {
                st_dev: 0, st_ino: 0, st_mode: S_IFDIR | 0o555, st_nlink: 2,
                st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            
            ..Default::default()
            }),
            CtlInner::File(_) => Ok(Stat {
                st_dev: 0, st_ino: 0, st_mode: S_IFREG | 0o444, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            
            ..Default::default()
            }),
        }
    }

    fn children(&self) -> Result<Vec<Arc<dyn VfsNode>>, ()> {
        if !self.is_dir() { return Err(()); }
        let children = self.children.lock();
        Ok(children.iter().map(|c| c.clone() as Arc<dyn VfsNode>).collect())
    }

    fn find_child(&self, name: &str) -> Option<Arc<dyn VfsNode>> {
        let children = self.children.lock();
        children.iter().find(|c| c.name == name).map(|c| c.clone() as Arc<dyn VfsNode>)
    }
}

fn file_fn(f: impl Fn() -> Vec<u8> + Send + Sync + 'static) -> CtlInner {
    CtlInner::File(alloc::boxed::Box::new(f))
}

fn add_child(parent: &Arc<CtlNode>, name: &str, inner: CtlInner) -> Arc<CtlNode> {
    let node = Arc::new(CtlNode {
        name: String::from(name),
        inner,
        children: Mutex::new(Vec::new()),
    });
    parent.children.lock().push(node.clone());
    node
}

pub struct CtlFs {
    root: Arc<CtlNode>,
}

impl CtlFs {
    pub fn new() -> Self {
        let root = Arc::new(CtlNode {
            name: String::from("/"),
            inner: CtlInner::Dir,
            children: Mutex::new(Vec::new()),
        });

        // /ctl/proc/
        let proc_node = Arc::new(CtlNode {
            name: String::from("proc"),
            inner: CtlInner::Dir,
            children: Mutex::new(Vec::new()),
        });
        add_child(&proc_node, "list", file_fn(|| {
            let table = crate::task::process::PROCESS_TABLE.lock();
            let mut out = alloc::format!("{:>6} {}\n", "PID", "CWD");
            for (pid, proc) in table.iter() {
                out.push_str(&alloc::format!("{:6} {}\n", pid, proc.files.lock().cwd));
            }
            out.into_bytes()
        }));

        root.children.lock().push(proc_node);

        // /ctl/sys/
        let sys_dir = add_child(&root, "sys", CtlInner::Dir);
        {
            // /ctl/sys/cpu/
            let cpu_dir = add_child(&sys_dir, "cpu", CtlInner::Dir);
            {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                let total_cpus = 1 + ap_count;

                // Create per-CPU directories for all detected cores
                for cpu_idx in 0..total_cpus {
                    let dir = add_child(&cpu_dir, &alloc::format!("{}", cpu_idx), CtlInner::Dir);
                    let cid = cpu_idx; // capture for closure
                    add_child(&dir, "cpu_id", file_fn(move || {
                        alloc::format!("{}\n", cid).into_bytes()
                    }));
                    add_child(&dir, "freq", file_fn(|| {
                        Vec::from("~2500 MHz (LAPIC timer estimate)\n")
                    }));
                    add_child(&dir, "model", file_fn(|| {
                        Vec::from("x86_64, QEMU Virtual CPU\n")
                    }));
                    add_child(&dir, "load", file_fn(|| {
                        let ticks = interrupts::get_ticks();
                        let idle = crate::syscalls::get_per_cpu().idle_count;
                        let active = ticks.saturating_sub(idle);
                        let pct = if ticks > 0 { (active * 100) / ticks } else { 0 };
                        alloc::format!("{}% ({} active ticks)\n", pct, active).into_bytes()
                    }));
                    // ponytail: try_lock can show "idle" for a running task if lock contended; no great fix
                    add_child(&dir, "current_task", file_fn(move || {
                        if let Some(sched) = crate::task::scheduler::cpu_sched(cid) {
                            if let Some(ref s) = sched.try_lock() {
                                if let Some(ref t) = s.current_thread {
                                    let pid = t.process.as_ref().map(|p| p.id).unwrap_or(0);
                                    return alloc::format!("PID {}\n", pid).into_bytes();
                                }
                            }
                        }
                        Vec::from("idle\n")
                    }));
                }
            }
            add_child(&cpu_dir, "info", file_fn(|| {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                let total = 1 + ap_count;
                let mut out = alloc::format!("{} cores\n", total);
                for i in 0..total {
                    out.push_str(&alloc::format!("  CPU{}: ", i));
                    let task = crate::task::scheduler::cpu_sched(i).and_then(|s| s.try_lock()).and_then(|s| {
                        s.current_thread.as_ref().map(|t| {
                            t.process.as_ref().map(|p| p.id).unwrap_or(0)
                        })
                    });
                    match task {
                        Some(pid) => out.push_str(&alloc::format!("running PID {}\n", pid)),
                        None => out.push_str("idle\n"),
                    }
                }
                out.into_bytes()
            }));
            add_child(&cpu_dir, "stat", file_fn(|| {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                let total = 1 + ap_count;
                let ticks = interrupts::get_ticks();
                let mut out = alloc::format!("CPU stats at tick {}:\n", ticks);
                for i in 0..total {
                    let pid = crate::task::scheduler::cpu_sched(i).and_then(|s| s.try_lock()).and_then(|s| {
                        s.current_thread.as_ref().and_then(|t| t.process.as_ref().map(|p| p.id))
                    });
                    match pid {
                        Some(pid) => out.push_str(&alloc::format!("  CPU{}: PID {}\n", i, pid)),
                        None => out.push_str(&alloc::format!("  CPU{}: idle\n", i)),
                    }
                }
                out.into_bytes()
            }));

            // /ctl/sys/mem/
            let mem_dir = add_child(&sys_dir, "mem", CtlInner::Dir);
            add_child(&mem_dir, "total", file_fn(|| {
                let total: usize = 512 * 1024 * 1024 / 4096;
                alloc::format!("{} pages ({} MB)\n", total, total * 4 / 1024).into_bytes()
            }));
            add_child(&mem_dir, "free", file_fn(|| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                alloc::format!("{} pages\n", free).into_bytes()
            }));
            add_child(&mem_dir, "used", file_fn(|| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let total: usize = 512 * 1024 * 1024 / 4096;
                alloc::format!("{} pages ({} MB)\n", total - free, (total - free) * 4 / 1024).into_bytes()
            }));
            add_child(&mem_dir, "cached", file_fn(|| {
                Vec::from("0 pages (no disk cache tracking)\n")
            }));

            // /ctl/sys/net/
            #[cfg(feature = "net")]
            {
                let net_dir = add_child(&sys_dir, "net", CtlInner::Dir);
                let if_dir = add_child(&net_dir, "interfaces", CtlInner::Dir);
                let eth0 = add_child(&if_dir, "eth0", CtlInner::Dir);
                add_child(&eth0, "addr", file_fn(|| {
                    let iface_lock = crate::net::NETWORK_INTERFACE.lock();
                    if let Some(ref iface) = *iface_lock {
                        let mut out = String::new();
                        for addr in iface.ip_addrs() {
                            out.push_str(&alloc::format!("{}\n", addr));
                        }
                        return out.into_bytes();
                    }
                    Vec::from("(no interface)\n")
                }));
                add_child(&eth0, "hwaddr", file_fn(|| {
                    let nic = crate::drivers::net::NIC.lock();
                    if let Some(ref nic) = *nic {
                        let mac = nic.mac_address();
                        return alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]).into_bytes();
                    }
                    Vec::from("(no NIC)\n")
                }));
                add_child(&eth0, "rx", file_fn(|| Vec::from("0 bytes\n")));
                add_child(&eth0, "tx", file_fn(|| Vec::from("0 bytes\n")));
                add_child(&net_dir, "stat", file_fn(|| {
                    let sockets = crate::net::SOCKETS.lock();
                    let count = sockets.iter().count();
                    alloc::format!("{} open sockets\n", count).into_bytes()
                }));
            }
        }

        // /ctl/kernel/
        let kernel_dir = add_child(&root, "kernel", CtlInner::Dir);
        add_child(&kernel_dir, "version", file_fn(|| {
            Vec::from("SARGA OS — Vahi Kernel v0.3.0 — x86_64, Rust nightly\n")
        }));
        add_child(&kernel_dir, "uptime", file_fn(|| {
            let ticks = interrupts::get_ticks();
            let secs = ticks / 100;
            alloc::format!("{} seconds\n", secs).into_bytes()
        }));
        add_child(&kernel_dir, "hostname", file_fn(|| {
            Vec::from("sarga-os\n")
        }));
        add_child(&kernel_dir, "log", file_fn(|| {
            let ticks = interrupts::get_ticks();
            alloc::format!(
                "Kernel booted at tick 0, current tick {}\nSMP init complete\nVFS mounted\nNetwork started\n",
                ticks
            ).into_bytes()
        }));

        CtlFs { root }
    }
}

impl FileSystem for CtlFs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}
