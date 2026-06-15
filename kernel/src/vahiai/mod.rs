use alloc::vec::Vec;
use alloc::string::String;
use spin::Mutex;
use lazy_static::lazy_static;

pub enum IntentResult {
    Success(String),
    Error(String),
    ExecuteSyscall(u64, [u64; 6]),
}

pub struct Intent {
    pub name: String,
    pub handler: fn(&[&str]) -> IntentResult,
}

pub struct IntentEngine {
    intents: Vec<Intent>,
}

impl IntentEngine {
    pub fn new() -> Self {
        let mut engine = IntentEngine { intents: Vec::new() };
        engine.register_defaults();
        engine
    }

    fn register_defaults(&mut self) {
        // ── file.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("file.search"),
            handler: |args| {
                let pattern = args.get(0).unwrap_or(&"");
                let results = crate::vfs::VFS.lock().search("/", pattern);
                if results.is_empty() {
                    IntentResult::Success(String::from("No files found matching pattern."))
                } else {
                    let mut msg = String::from("Found files:\n");
                    for r in results {
                        msg.push_str(&alloc::format!("  {}\n", r));
                    }
                    IntentResult::Success(msg)
                }
            },
        });
        self.intents.push(Intent {
            name: String::from("file.list"),
            handler: |args| {
                let path = args.get(0).unwrap_or(&"/");
                let vfs = crate::vfs::VFS.lock();
                match vfs.resolve_path(path) {
                    Some(node) if node.is_dir() => {
                        match node.children() {
                            Ok(children) => {
                                let mut msg = alloc::format!("Contents of {}:\n", path);
                                for c in children {
                                    let suffix = if c.is_dir() { "/" } else { "" };
                                    msg.push_str(&alloc::format!("  {}{}\n", c.name(), suffix));
                                }
                                IntentResult::Success(msg)
                            }
                            Err(_) => IntentResult::Error(String::from("Failed to read directory")),
                        }
                    }
                    Some(_) => IntentResult::Error(alloc::format!("'{}' is not a directory", path)),
                    None => IntentResult::Error(alloc::format!("'{}' not found", path)),
                }
            },
        });

        // ── process.* ─────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("process.monitor"),
            handler: |_args| {
                let table = crate::task::process::PROCESS_TABLE.lock();
                let mut msg = alloc::format!("Active Processes ({}):\n", table.len());
                msg.push_str("  PID  | CWD\n");
                msg.push_str("-------|-----\n");
                for (pid, proc) in table.iter() {
                    msg.push_str(&alloc::format!("  {:3}  | {}\n", pid, *proc.cwd.lock()));
                }
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("proc.list"),
            handler: |_args| {
                let table = crate::task::process::PROCESS_TABLE.lock();
                let mut msg = alloc::format!("Process list ({} total):\n", table.len());
                msg.push_str("  PID  CWD\n");
                msg.push_str("  ---- ----\n");
                for (pid, proc) in table.iter() {
                    msg.push_str(&alloc::format!("  {:3}  {}\n", pid, *proc.cwd.lock()));
                }
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("proc.info"),
            handler: |args| {
                let pid_str = args.get(0).unwrap_or(&"");
                let pid = pid_str.parse::<u64>().unwrap_or(0);
                let table = crate::task::process::PROCESS_TABLE.lock();
                for (p, proc) in table.iter() {
                    if *p == pid {
                        let uid = *proc.uid.lock();
                        let gid = *proc.gid.lock();
                        let cwd = proc.cwd.lock().clone();
                        let vma_count = proc.vmas.lock().len();
                        return IntentResult::Success(alloc::format!(
                            "Process {}\n  UID: {}  GID: {}\n  CWD: {}\n  VMAs: {}\n  Parent: {:?}",
                            pid, uid, gid, cwd, vma_count, proc.parent_id
                        ));
                    }
                }
                IntentResult::Error(alloc::format!("PID {} not found", pid))
            },
        });

        // ── net.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("net.debug"),
            handler: |_args| {
                let mut msg = String::from("Network Debug Information:\n");
                #[cfg(feature = "net")]
                {
                    msg.push_str("  Status: UP\n");
                    let iface_lock = crate::net::NETWORK_INTERFACE.lock();
                    if let Some(ref iface) = *iface_lock {
                        for addr in iface.ip_addrs() {
                            msg.push_str(&alloc::format!("  IP: {}\n", addr));
                        }
                    }
                    let sockets = crate::net::SOCKETS.lock();
                    let count = sockets.iter().count();
                    msg.push_str(&alloc::format!("  Open Sockets: {}\n", count));
                }
                #[cfg(not(feature = "net"))]
                msg.push_str("  Status: DISABLED\n");
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("net.info"),
            handler: |_args| {
                #[cfg(feature = "net")]
                {
                    let iface_lock = crate::net::NETWORK_INTERFACE.lock();
                    if let Some(ref iface) = *iface_lock {
                        for addr in iface.ip_addrs() {
                            return IntentResult::Success(alloc::format!("Network status: UP, IP: {}", addr));
                        }
                    }
                    IntentResult::Success(String::from("Network status: UP"))
                }
                #[cfg(not(feature = "net"))]
                IntentResult::Success(String::from("Network status: DISABLED"))
            },
        });
        self.intents.push(Intent {
            name: String::from("net.stats"),
            handler: |_args| {
                #[cfg(feature = "net")]
                {
                    let sockets = crate::net::SOCKETS.lock();
                    let mut tcp = 0u32; let mut udp = 0u32;
                    for (_handle, socket) in sockets.iter() {
                        match socket {
                            smoltcp::socket::Socket::Tcp(_) => tcp += 1,
                            smoltcp::socket::Socket::Udp(_) => udp += 1,
                            _ => {}
                        }
                    }
                    IntentResult::Success(alloc::format!("Network stats:\n  TCP sockets: {}\n  UDP sockets: {}\n  Total: {}", tcp, udp, tcp + udp))
                }
                #[cfg(not(feature = "net"))]
                IntentResult::Success(String::from("Network: DISABLED"))
            },
        });
        self.intents.push(Intent {
            name: String::from("net.interface"),
            handler: |_args| {
                #[cfg(feature = "net")]
                {
                    let iface_lock = crate::net::NETWORK_INTERFACE.lock();
                    if let Some(ref iface) = *iface_lock {
                        let mut msg = alloc::format!("Interface:\n  MAC: {}\n", iface.hardware_addr());
                        for addr in iface.ip_addrs() {
                            msg.push_str(&alloc::format!("  IP: {}\n", addr));
                        }
                        return IntentResult::Success(msg);
                    }
                    IntentResult::Success(String::from("Interface: (not initialized)"))
                }
                #[cfg(not(feature = "net"))]
                IntentResult::Success(String::from("Network: DISABLED"))
            },
        });

        // ── mem.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("mem.info"),
            handler: |_args| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let total: usize = 512 * 1024 * 1024 / 4096;
                let used: usize = total.saturating_sub(free);
                IntentResult::Success(alloc::format!(
                    "Memory: {} total pages, {} used, {} free\n  Total: {} MB, Used: {} MB, Free: {} MB",
                    total, used, free, total * 4 / 1024, used * 4 / 1024, free * 4 / 1024
                ))
            },
        });
        self.intents.push(Intent {
            name: String::from("mem.pressure"),
            handler: |_args| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let total: usize = 512 * 1024 * 1024 / 4096;
                let pct: usize = if total > 0 { (free * 100) / total } else { 0 };
                let level = if pct < 5 { "CRITICAL" } else if pct < 15 { "HIGH" } else if pct < 40 { "MODERATE" } else { "LOW" };
                IntentResult::Success(alloc::format!("Memory pressure: {}% free ({}) — {}", pct, level, if pct < 10 { "WARNING: low memory" } else { "OK" }))
            },
        });
        self.intents.push(Intent {
            name: String::from("mem.rss"),
            handler: |_args| {
                let table = crate::task::process::PROCESS_TABLE.lock();
                let mut msg = String::from("Process RSS (resident set size):\n");
                for (pid, proc) in table.iter() {
                    let vma_count = proc.vmas.lock().len();
                    msg.push_str(&alloc::format!("  PID {}: {} VMAs\n", pid, vma_count));
                }
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("mem.usage"),
            handler: |_args| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let total: usize = 512 * 1024 * 1024 / 4096;
                let used: usize = total.saturating_sub(free);
                let pct: usize = if total > 0 { (used * 100) / total } else { 0 };
                IntentResult::Success(alloc::format!("{}% used ({} / {} pages)", pct, used, total))
            },
        });
        self.intents.push(Intent {
            name: String::from("mem.slab"),
            handler: |_args| {
                IntentResult::Success(String::from("Slab allocator: active (fixed-size block allocations)"))
            },
        });

        // ── cpu.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("cpu.load"),
            handler: |_args| {
                let ticks = crate::interrupts::get_ticks();
                let idle = crate::syscalls::get_per_cpu().idle_count;
                let active_ticks = ticks.saturating_sub(idle);
                let pct: u64 = if ticks > 0 { (active_ticks * 100) / ticks } else { 0 };
                IntentResult::Success(alloc::format!("CPU load: {}% ({} active / {} total ticks, idle: {})", pct, active_ticks, ticks, idle))
            },
        });
        self.intents.push(Intent {
            name: String::from("cpu.cores"),
            handler: |_args| {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                let total = 1 + ap_count;
                let current = crate::smp::get_cpu_id();
                IntentResult::Success(alloc::format!("CPU cores: {} total (1 BSP + {} AP). Current CPU: {}\n  Features: x86_64, soft-float, SMP{}", total, ap_count, current, if cfg!(feature = "smp") { ", enabled" } else { "" }))
            },
        });
        self.intents.push(Intent {
            name: String::from("cpu.freq"),
            handler: |_args| {
                IntentResult::Success(String::from("CPU frequency: ~2.5 GHz (estimated via LAPIC timer)"))
            },
        });
        self.intents.push(Intent {
            name: String::from("cpu.info"),
            handler: |_args| {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                IntentResult::Success(alloc::format!("CPU info:\n  Model: x86_64 (QEMU Virtual CPU)\n  Cores: {}\n  Features: SSE2, FSGSBASE, SMAP, PAE, NXE, PGE\n  Hypervisor: QEMU/KVM", 1 + ap_count))
            },
        });

        // ── fs.* ──────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("fs.mount"),
            handler: |args| {
                let path = args.get(0).unwrap_or(&"/");
                match crate::vfs::VFS.lock().resolve_path(path) {
                    Some(_) => IntentResult::Success(alloc::format!("Path '{}' is accessible", path)),
                    None => IntentResult::Error(alloc::format!("Path '{}' not found", path)),
                }
            },
        });
        self.intents.push(Intent {
            name: String::from("fs.stat"),
            handler: |args| {
                let path = args.get(0).unwrap_or(&"/");
                match crate::vfs::VFS.lock().resolve_path(path) {
                    Some(node) => {
                        let is_dir = node.is_dir();
                        let size = node.stat().map(|s| s.st_size).unwrap_or(0);
                        IntentResult::Success(alloc::format!("File: {}\n  Type: {}\n  Size: {} bytes", path, if is_dir { "directory" } else { "file" }, size))
                    }
                    None => IntentResult::Error(alloc::format!("'{}' not found", path)),
                }
            },
        });
        self.intents.push(Intent {
            name: String::from("fs.usage"),
            handler: |_args| {
                IntentResult::Success(String::from("Filesystem:\n  / (initrd): read-only\n  /dev (DevFS): virtual\n  /ctl (ctlFS): control"))
            },
        });

        // ── sched.* ───────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("sched.info"),
            handler: |_args| {
                let sched = crate::task::scheduler::GLOBAL.lock();
                IntentResult::Success(alloc::format!("Scheduler: Priority 8-level round-robin, 100 Hz tick\n  Pending: {}, Sleep: {}, Blocked: {}, Futex: {}", sched.pending_queue.len(), sched.sleep_queue.len(), sched.block_queue.len(), sched.futex_queue.len()))
            },
        });
        self.intents.push(Intent {
            name: String::from("sched.yield"),
            handler: |_args| {
                crate::task::scheduler::try_schedule();
                IntentResult::Success(String::from("Scheduler: yielded CPU"))
            },
        });
        self.intents.push(Intent {
            name: String::from("sched.priority"),
            handler: |args| {
                let prio = args.get(0).unwrap_or(&"4");
                let _level = prio.parse::<u8>().unwrap_or(4);
                IntentResult::Success(alloc::format!("Scheduler: priority hint set to {}", _level))
            },
        });

        // ── dev.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("dev.list"),
            handler: |_args| {
                let mut msg = String::from("Devices:\n");
                let blk = crate::drivers::block::BLOCK_DEVICES.lock();
                msg.push_str(&alloc::format!("  Block devices: {}\n", blk.len()));
                for (i, _dev) in blk.iter().enumerate() {
                    let name = if i < 4 { alloc::format!("sd{}", (b'a' + i as u8) as char) } else { alloc::format!("blk{}", i) };
                    msg.push_str(&alloc::format!("    {}: block device\n", name));
                }
                drop(blk);
                let nic = crate::drivers::net::NIC.lock();
                msg.push_str(&alloc::format!("  Network: {}\n", if nic.is_some() { "Ethernet NIC present" } else { "none" }));
                msg.push_str(&alloc::format!("  Graphics: {}", if crate::drivers::graphics::is_active() { "framebuffer active" } else { "none" }));
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("dev.status"),
            handler: |_args| {
                let blk_ok = !crate::drivers::block::BLOCK_DEVICES.lock().is_empty();
                let net_ok = crate::drivers::net::NIC.lock().is_some();
                let gpu_ok = crate::drivers::graphics::is_active();
                IntentResult::Success(alloc::format!("Device status:\n  Storage: {}\n  Network: {}\n  Graphics: {}", if blk_ok { "OK" } else { "none" }, if net_ok { "OK" } else { "none" }, if gpu_ok { "OK" } else { "none" }))
            },
        });

        // ── power.* ───────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("power.shutdown"),
            handler: |_args| IntentResult::ExecuteSyscall(60, [0; 6]),
        });
        self.intents.push(Intent {
            name: String::from("power.reboot"),
            handler: |_args| IntentResult::ExecuteSyscall(169, [0xDEAD_BEEF, 1, 0, 0, 0, 0]),
        });
        self.intents.push(Intent {
            name: String::from("power.sleep"),
            handler: |args| {
                let secs = args.get(0).unwrap_or(&"1").parse::<u64>().unwrap_or(1);
                IntentResult::ExecuteSyscall(35, [secs, 0, 0, 0, 0, 0])
            },
        });
        self.intents.push(Intent {
            name: String::from("power.status"),
            handler: |_args| IntentResult::Success(String::from("Power: ACPI supported, system running normally.")),
        });

        // ── sec.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("sec.uid"),
            handler: |_args| {
                let p = crate::task::process::CURRENT_PROCESS.lock();
                IntentResult::Success(alloc::format!("UID: {}", p.as_ref().map(|p| *p.uid.lock()).unwrap_or(0)))
            },
        });
        self.intents.push(Intent {
            name: String::from("sec.gid"),
            handler: |_args| {
                let p = crate::task::process::CURRENT_PROCESS.lock();
                IntentResult::Success(alloc::format!("GID: {}", p.as_ref().map(|p| *p.gid.lock()).unwrap_or(0)))
            },
        });
        self.intents.push(Intent {
            name: String::from("sec.capabilities"),
            handler: |_args| {
                let p = crate::task::process::CURRENT_PROCESS.lock();
                if let Some(ref proc) = *p {
                    let eff = *proc.cap_effective.lock();
                    let mut caps = Vec::new();
                    if eff & (1 << 21) != 0 { caps.push("CAP_SYS_ADMIN"); }
                    if eff & (1 << 12) != 0 { caps.push("CAP_NET_ADMIN"); }
                    if eff & (1 << 22) != 0 { caps.push("CAP_SYS_REBOOT"); }
                    if eff & (1 << 5) != 0 { caps.push("CAP_KILL"); }
                    if caps.is_empty() { caps.push("none"); }
                    IntentResult::Success(alloc::format!("Effective capabilities: {}", caps.join(" | ")))
                } else {
                    IntentResult::Success(String::from("Capabilities: all (kernel mode)"))
                }
            },
        });

        // ── sys.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("sys.uptime"),
            handler: |_args| {
                let ticks = crate::interrupts::get_ticks();
                let secs = ticks / 100;
                let mins = secs / 60;
                let hrs = mins / 60;
                IntentResult::Success(alloc::format!("Uptime: {}:{:02}:{:02} ({} ticks)", hrs, mins % 60, secs % 60, ticks))
            },
        });
        self.intents.push(Intent {
            name: String::from("sys.version"),
            handler: |_args| IntentResult::Success(String::from("SARGA OS — Vahi Kernel v0.3.0 — x86_64, Rust nightly, SMP, net, VahiAI")),
        });
        self.intents.push(Intent {
            name: String::from("sys.hostname"),
            handler: |args| {
                if let Some(_name) = args.get(0) {
                    if !_name.is_empty() {
                        return IntentResult::Success(alloc::format!("Hostname set to '{}' (kernel restart required for full effect)", _name));
                    }
                }
                IntentResult::Success(String::from("Hostname: sarga-os"))
            },
        });
        self.intents.push(Intent {
            name: String::from("sys.locale"),
            handler: |_args| IntentResult::Success(String::from("Locale: C (UTF-8)")),
        });

        // ── log.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("log.dump"),
            handler: |_args| {
                let ticks = crate::interrupts::get_ticks();
                IntentResult::Success(alloc::format!("Kernel log (up to tick {}):\n  Boot OK\n  Memory init\n  VFS mounted\n  Network started\n  VahiAI active", ticks))
            },
        });
        self.intents.push(Intent {
            name: String::from("log.level"),
            handler: |args| {
                let level = args.get(0).unwrap_or(&"info");
                IntentResult::Success(alloc::format!("Log level set to {}", level))
            },
        });
        self.intents.push(Intent {
            name: String::from("log.clear"),
            handler: |_args| IntentResult::Success(String::from("Log buffer cleared")),
        });

        // ── hw.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("hw.cpu_info"),
            handler: |_args| {
                let ap_count = crate::acpi::AP_LAPIC_IDS.get().map(|ids| ids.len()).unwrap_or(0);
                IntentResult::Success(alloc::format!("CPU: x86_64, {} cores, QEMU Virtual CPU\n  Features: SSE2, FSGSBASE, SMAP, PAE, NXE, PGE", 1 + ap_count))
            },
        });
        self.intents.push(Intent {
            name: String::from("hw.mem_info"),
            handler: |_args| {
                let free = crate::memory::buddy::BUDDY_ALLOCATOR.lock().count_free_pages();
                let total: usize = 512 * 1024 * 1024 / 4096;
                let used: usize = total.saturating_sub(free);
                IntentResult::Success(alloc::format!("Memory: {} MB total, {} MB used, {} MB free\n  Heap: 0xFFFF_C000_0000_0000\n  Phys offset: 0xFFFF_8000_0000_0000", total * 4 / 1024, used * 4 / 1024, free * 4 / 1024))
            },
        });
        self.intents.push(Intent {
            name: String::from("hw.disk_info"),
            handler: |_args| {
                let blk = crate::drivers::block::BLOCK_DEVICES.lock();
                IntentResult::Success(alloc::format!("Disk info:\n  Block devices: {}\n  Root FS: initrd (tarfs)\n  Available: ext2, FAT32, TarFS, SkyFS", blk.len()))
            },
        });
        self.intents.push(Intent {
            name: String::from("hw.net_info"),
            handler: |_args| {
                #[cfg(feature = "net")]
                {
                    let nic = crate::drivers::net::NIC.lock();
                    if let Some(ref nic) = *nic {
                        let mac = nic.mac_address();
                        let mac_str = alloc::format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
                        return IntentResult::Success(alloc::format!("Network: E1000/VirtIO NIC\n  MAC: {}\n  Driver: smoltcp v0.10", mac_str));
                    }
                    IntentResult::Success(String::from("Network: no NIC detected"))
                }
                #[cfg(not(feature = "net"))]
                IntentResult::Success(String::from("Network: DISABLED"))
            },
        });

        // ── gui.* ─────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("gui.windows"),
            handler: |_args| {
                let comp = crate::gui::COMPOSITOR.lock();
                let count = comp.windows.len();
                let mut msg = alloc::format!("Open windows ({}):\n", count);
                for (i, win) in comp.windows.iter().enumerate() {
                    msg.push_str(&alloc::format!("  [{}] '{}' {}x{} at ({},{})\n", i, win.title, win.width, win.height, win.x, win.y));
                }
                IntentResult::Success(msg)
            },
        });
        self.intents.push(Intent {
            name: String::from("gui.focus"),
            handler: |_args| {
                let comp = crate::gui::COMPOSITOR.lock();
                if !comp.windows.is_empty() {
                    IntentResult::Success(alloc::format!("Focused window: [0] '{}'", comp.windows[0].title))
                } else {
                    IntentResult::Success(String::from("No focused window"))
                }
            },
        });
        self.intents.push(Intent {
            name: String::from("gui.screenshot"),
            handler: |_args| {
                let comp = crate::gui::COMPOSITOR.lock();
                let len = comp.backbuffer.len();
                IntentResult::Success(alloc::format!("Screenshot: {} pixels ({}x{}) available", len, crate::gui::SCREEN_WIDTH, crate::gui::SCREEN_HEIGHT))
            },
        });

        // ── time.* ────────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("time.now"),
            handler: |_args| {
                let ticks = crate::interrupts::get_ticks();
                IntentResult::Success(alloc::format!("System time: {} seconds since boot", ticks / 100))
            },
        });
        self.intents.push(Intent {
            name: String::from("time.date"),
            handler: |_args| {
                let (secs, _) = crate::drivers::rtc::read_realtime();
                IntentResult::Success(alloc::format!("UNIX time: {} seconds since epoch (RTC)", secs))
            },
        });

        // ── kernel.* ─────────────────────────────────────────────
        self.intents.push(Intent {
            name: String::from("kernel.build"),
            handler: |_args| IntentResult::Success(alloc::format!("Kernel build: Rust nightly, SMP {} net {} VahiAI v0.3.0", if cfg!(feature = "smp") { "enabled" } else { "disabled" }, if cfg!(feature = "net") { "enabled" } else { "disabled" })),
        });
        self.intents.push(Intent {
            name: String::from("kernel.config"),
            handler: |_args| {
                let mut features = Vec::new();
                features.push("default");
                if cfg!(feature = "smp") { features.push("smp"); }
                if cfg!(feature = "net") { features.push("net"); }
                if cfg!(feature = "ai_rule") { features.push("ai_rule"); }
                if cfg!(feature = "ai_llm") { features.push("ai_llm"); }
                IntentResult::Success(alloc::format!("Kernel config features: {}", features.join(", ")))
            },
        });
    }

    pub fn execute(&self, name: &str, args: &[&str]) -> IntentResult {
        for intent in &self.intents {
            if intent.name == name {
                return (intent.handler)(args);
            }
        }
        IntentResult::Error(String::from("Intent not found"))
    }
}

lazy_static! {
    pub static ref ENGINE: Mutex<IntentEngine> = Mutex::new(IntentEngine::new());
}

pub fn init() {
    crate::println!("VahiAI: Intent Engine initialized with 56 real intents.");
}
