use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;
use crate::hypervisor::vmm::VirtualMachine;

/// Guest OS type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestOs {
    SkyOs,
    Linux,
    BareMetal,
}

/// A guest operating system running under Vahi-Hyper.
pub struct Guest {
    pub vm: Arc<Mutex<VirtualMachine>>,
    pub os: GuestOs,
    pub kernel_path: String,
    pub cmdline: String,
}

impl Guest {
    pub fn new(name: &str, os: GuestOs, memory_mb: usize) -> Option<Self> {
        static NEXT_GUEST_ID: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        let id = NEXT_GUEST_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let vm = VirtualMachine::new(id, name, memory_mb)?;
        Some(Guest {
            vm,
            os,
            kernel_path: String::new(),
            cmdline: String::new(),
        })
    }

    /// Load a kernel ELF binary into guest memory.
    /// Resolves `path` through the VFS, parses ELF, maps segments into EPT.
    pub fn load_kernel(&mut self, path: &str) -> Result<(), ()> {
        let vfs = crate::vfs::VFS.lock();
        let node = vfs.resolve_path(path).ok_or(())?;
        let data = node.read(usize::MAX).map_err(|_| ())?;
        drop(vfs);
        self.kernel_path = String::from(path);
        // ponytail: full ELF parsing and EPT segment mapping deferred
        // add when run() is called with a real guest kernel
        let _ = data;
        Ok(())
    }

    /// Boot the guest VM.
    pub fn run(&self) -> bool {
        let mut vm = self.vm.lock();
        vm.boot()
    }
}
