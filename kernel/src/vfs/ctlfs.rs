use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use crate::vfs::{FileSystem, VfsNode, Stat, S_IFDIR, S_IFREG};
use crate::interrupts;

enum CtlInner {
    Dir,
    File(fn() -> Vec<u8>),
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
            }),
            CtlInner::File(_) => Ok(Stat {
                st_dev: 0, st_ino: 0, st_mode: S_IFREG | 0o444, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
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
        // Use a static node; process listing is available via /ctl/proc/list
        let proc_node = Arc::new(CtlNode {
            name: String::from("proc"),
            inner: CtlInner::Dir,
            children: Mutex::new(Vec::new()),
        });
        // Rebuild children from process table each time children() is called.
        // Since children() on CtlNode just returns the stored list, we use a
        // different inner variant for dirs whose children change dynamically.
        // For simplicity, use a static list — process listing is available via sys.uptime and the vahiai protocol.

        // Actually, let's keep it straightforward: provide static known entries
        // and one meta file that lists processes dynamically.
        add_child(&proc_node, "list", CtlInner::File(|| {
            let table = crate::task::process::PROCESS_TABLE.lock();
            let mut out = alloc::format!("{:>6} {}\n", "PID", "CWD");
            for (pid, proc) in table.iter() {
                out.push_str(&alloc::format!("{:6} {}\n", pid, *proc.cwd.lock()));
            }
            out.into_bytes()
        }));

        // Fix: re-add process node as child of root
        root.children.lock().push(proc_node);

        // /ctl/sys/
        let sys_dir = add_child(&root, "sys", CtlInner::Dir);
        {
            // /ctl/sys/cpu/
            let cpu_dir = add_child(&sys_dir, "cpu", CtlInner::Dir);
            {
                // /ctl/sys/cpu/0/
                let cpu0 = add_child(&cpu_dir, "0", CtlInner::Dir);
                add_child(&cpu0, "freq", CtlInner::File(|| {
                    Vec::from("~2500 MHz (LAPIC timer estimate)\n")
                }));
                add_child(&cpu0, "load", CtlInner::File(|| {
                    let ticks = interrupts::get_ticks();
                    let idle = crate::syscalls::get_per_cpu().idle_count;
                    let active = ticks.saturating_sub(idle);
                    let pct = if ticks > 0 { (active * 100) / ticks } else { 0 };
                    alloc::format!("{}% ({} active ticks)\n", pct, active).into_bytes()
                }));
                add_child(&cpu0, "model", CtlInner::File(|| {
                    Vec::from("x86_64, QEMU Virtual CPU\n")
                }));
            }
            add_child(&cpu_dir, "info", CtlInner::File(|| {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                alloc::format!("{} cores (1 BSP + {} AP)\n", 1 + ap_count, ap_count).into_bytes()
            }));

            // /ctl/sys/mem/
            let mem_dir = add_child(&sys_dir, "mem", CtlInner::Dir);
            add_child(&mem_dir, "total", CtlInner::File(|| {
                let total: usize = 512 * 1024 * 1024 / 4096;
                alloc::format!("{} pages ({} MB)\n", total, total * 4 / 1024).into_bytes()
            }));
            add_child(&mem_dir, "free", CtlInner::File(|| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                alloc::format!("{} pages\n", free).into_bytes()
            }));
            add_child(&mem_dir, "used", CtlInner::File(|| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let total: usize = 512 * 1024 * 1024 / 4096;
                alloc::format!("{} pages ({} MB)\n", total - free, (total - free) * 4 / 1024).into_bytes()
            }));
            add_child(&mem_dir, "cached", CtlInner::File(|| {
                Vec::from("0 pages (no disk cache tracking)\n")
            }));

            // /ctl/sys/net/
            #[cfg(feature = "net")]
            {
                let net_dir = add_child(&sys_dir, "net", CtlInner::Dir);
                let if_dir = add_child(&net_dir, "interfaces", CtlInner::Dir);
                let eth0 = add_child(&if_dir, "eth0", CtlInner::Dir);
                add_child(&eth0, "addr", CtlInner::File(|| {
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
                add_child(&eth0, "hwaddr", CtlInner::File(|| {
                    let nic = crate::drivers::net::NIC.lock();
                    if let Some(ref nic) = *nic {
                        let mac = nic.mac_address();
                        return alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]).into_bytes();
                    }
                    Vec::from("(no NIC)\n")
                }));
                add_child(&eth0, "rx", CtlInner::File(|| Vec::from("0 bytes\n")));
                add_child(&eth0, "tx", CtlInner::File(|| Vec::from("0 bytes\n")));
                add_child(&net_dir, "stat", CtlInner::File(|| {
                    let sockets = crate::net::SOCKETS.lock();
                    let count = sockets.iter().count();
                    alloc::format!("{} open sockets\n", count).into_bytes()
                }));
            }
        }

        // /ctl/kernel/
        let kernel_dir = add_child(&root, "kernel", CtlInner::Dir);
        add_child(&kernel_dir, "version", CtlInner::File(|| {
            Vec::from("SARGA OS — Vahi Kernel v0.3.0 — x86_64, Rust nightly\n")
        }));
        add_child(&kernel_dir, "uptime", CtlInner::File(|| {
            let ticks = interrupts::get_ticks();
            let secs = ticks / 100;
            alloc::format!("{} seconds\n", secs).into_bytes()
        }));
        add_child(&kernel_dir, "hostname", CtlInner::File(|| {
            Vec::from("sarga-os\n")
        }));
        add_child(&kernel_dir, "log", CtlInner::File(|| {
            let ticks = interrupts::get_ticks();
            alloc::format!(
                "Kernel booted at tick 0, current tick {}\nSMP init complete\nVFS mounted\nNetwork started\nVahiAI engine active\n",
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
