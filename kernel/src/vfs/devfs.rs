use alloc::sync::Arc;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use crossbeam_queue::ArrayQueue;
use crate::vfs::{FileSystem, VfsNode, Stat};
use crate::drivers::input::InputEvent;
use crate::syscalls::user_access;

enum DevNodeInner {
    Dir,
    Null,
    Zero,
    Tty0,
    Framebuffer,
    InputEvent(&'static ArrayQueue<InputEvent>),
    Speaker,
    BlockDevice(usize),
}

struct DevNode {
    name: String,
    inner: DevNodeInner,
    children: Mutex<Vec<Arc<DevNode>>>,
}

impl VfsNode for DevNode {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn is_dir(&self) -> bool {
        matches!(self.inner, DevNodeInner::Dir)
    }

    fn read(&self, max_len: usize) -> Result<Vec<u8>, ()> {
        match &self.inner {
            DevNodeInner::Dir => Err(()),
            DevNodeInner::Null => Err(()),
            DevNodeInner::Zero => Ok(vec![0u8; max_len]),
            DevNodeInner::Framebuffer => {
                let h = crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed);
                let stride = crate::drivers::graphics::STRIDE.load(core::sync::atomic::Ordering::Relaxed);
                let size = h * stride * 4;
                let mut buf = vec![0u8; size];
                let ptr = crate::drivers::graphics::FRAMEBUFFER.load(core::sync::atomic::Ordering::Relaxed);
                if !ptr.is_null() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(ptr as *const u8, buf.as_mut_ptr(), size);
                    }
                }
                Ok(buf)
            }
            DevNodeInner::Tty0 => {
                let n = core::cmp::min(max_len, 256);
                let mut buf = Vec::with_capacity(n);
                while buf.len() < n {
                    if let Some(c) = crate::tty::TTY_INPUT.pop() {
                        buf.push(c);
                    } else {
                        break;
                    }
                }
                Ok(buf)
            }
            DevNodeInner::InputEvent(queue) => {
                let event_size = core::mem::size_of::<InputEvent>();
                let max_events = max_len / event_size;
                let mut buf = Vec::with_capacity(max_events * event_size);
                for _ in 0..max_events {
                    if let Some(ev) = queue.pop() {
                        let bytes = unsafe {
                            core::slice::from_raw_parts(
                                &ev as *const InputEvent as *const u8,
                                event_size,
                            )
                        };
                        buf.extend_from_slice(bytes);
                    } else {
                        break;
                    }
                }
                Ok(buf)
            }
            DevNodeInner::Speaker => Err(()),
            DevNodeInner::BlockDevice(_) => Err(()),
        }
    }

    fn write(&self, data: &[u8]) -> Result<(), ()> {
        match &self.inner {
            DevNodeInner::Dir => Err(()),
            DevNodeInner::Null => Ok(()),
            DevNodeInner::Zero => Ok(()),
            DevNodeInner::Framebuffer => {
                let h = crate::drivers::graphics::HEIGHT.load(core::sync::atomic::Ordering::Relaxed);
                let stride = crate::drivers::graphics::STRIDE.load(core::sync::atomic::Ordering::Relaxed);
                let fb_size = h * stride * 4;
                let len = core::cmp::min(data.len(), fb_size);
                let ptr = crate::drivers::graphics::FRAMEBUFFER.load(core::sync::atomic::Ordering::Relaxed);
                if !ptr.is_null() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, len);
                    }
                }
                Ok(())
            }
            DevNodeInner::Tty0 => {
                let mut writer = crate::drivers::graphics::console::WRITER.lock();
                for &b in data {
                    writer.write_byte(b);
                }
                for &b in data {
                    crate::serial_putc(b);
                }
                Ok(())
            }
            DevNodeInner::InputEvent(_) => Err(()),
            DevNodeInner::Speaker => {
                if data.len() >= 8 {
                    let freq = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                    let dur = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                    crate::drivers::audio::pcspeaker::beep(freq, dur);
                }
                Ok(())
            }
            DevNodeInner::BlockDevice(_) => Err(()),
        }
    }

    fn stat(&self) -> Result<Stat, ()> {
        match &self.inner {
            DevNodeInner::Dir => Ok(Stat {
                st_dev: 0, st_ino: 0, st_mode: 0o040755, st_nlink: 2,
                st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::Null => Ok(Stat {
                st_dev: 0, st_ino: 1, st_mode: 0o020666, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0x0103, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::Zero => Ok(Stat {
                st_dev: 0, st_ino: 2, st_mode: 0o020666, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0x0105, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::Framebuffer => Ok(Stat {
                st_dev: 0, st_ino: 6, st_mode: 0o020666, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0x001e, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::Tty0 => Ok(Stat {
                st_dev: 0, st_ino: 3, st_mode: 0o020666, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0x0400, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::InputEvent(_) => Ok(Stat {
                st_dev: 0, st_ino: 0, st_mode: 0o020440, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::Speaker => Ok(Stat {
                st_dev: 0, st_ino: 7, st_mode: 0o020666, st_nlink: 1,
                st_uid: 0, st_gid: 0, st_rdev: 0x0106, st_size: 0,
                st_atime: 0, st_mtime: 0, st_ctime: 0,
            }),
            DevNodeInner::BlockDevice(idx) => {
                let size = {
                    let devices = crate::drivers::block::BLOCK_DEVICES.lock();
                    if *idx < devices.len() {
                        devices[*idx].lock().sector_count().unwrap_or(0) * 512
                    } else { 0 }
                };
                Ok(Stat {
                    st_dev: 0, st_ino: 10 + *idx as u64, st_mode: 0o060660, st_nlink: 1,
                    st_uid: 0, st_gid: 0, st_rdev: 0, st_size: size as i64,
                    st_atime: 0, st_mtime: 0, st_ctime: 0,
                })
            }
        }
    }

    fn ioctl(&self, request: u64, argp: *mut u8) -> Result<u64, ()> {
        const BLKGETSIZE64: u64 = 0x80081272;
        const BLKSSZGET: u64 = 0x00001268;
        const BLKRD_SEC: u64 = 0x40001260;
        const BLKWR_SEC: u64 = 0x40001261;

        #[repr(C, packed)]
        struct BlkIoctlOp {
            sector: u64,
            count: u64,
            buf: u64,
        }

        match &self.inner {
            DevNodeInner::BlockDevice(idx) => {
                let devices = crate::drivers::block::BLOCK_DEVICES.lock();
                if *idx >= devices.len() { return Err(()); }
                let mut dev = devices[*idx].lock();
                match request {
                    BLKGETSIZE64 => {
                        let bytes = dev.sector_count().unwrap_or(0) * 512;
                        if unsafe { user_access::copy_to_user(argp, &bytes.to_le_bytes()) }.is_err() {
                            return Err(());
                        }
                        Ok(0)
                    }
                    BLKSSZGET => {
                        let sector_size: u32 = 512;
                        if unsafe { user_access::copy_to_user(argp, &sector_size.to_le_bytes()) }.is_err() {
                            return Err(());
                        }
                        Ok(0)
                    }
                    BLKRD_SEC => {
                        let op: BlkIoctlOp = unsafe { core::ptr::read_unaligned(argp as *const BlkIoctlOp) };
                        let data_size = (op.count * 512) as usize;
                        let mut buf = alloc::vec![0u8; data_size];
                        for j in 0..op.count {
                            let offset = (j * 512) as usize;
                            if dev.read_sector(op.sector + j, &mut buf[offset..offset + 512]).is_err() {
                                return Err(());
                            }
                        }
                        if unsafe { user_access::copy_to_user(op.buf as *mut u8, &buf) }.is_err() {
                            return Err(());
                        }
                        Ok(0)
                    }
                    BLKWR_SEC => {
                        let op: BlkIoctlOp = unsafe { core::ptr::read_unaligned(argp as *const BlkIoctlOp) };
                        let data_size = (op.count * 512) as usize;
                        let mut buf = alloc::vec![0u8; data_size];
                        if unsafe { user_access::copy_from_user(&mut buf, op.buf as *const u8) }.is_err() {
                            return Err(());
                        }
                        for j in 0..op.count {
                            let offset = (j * 512) as usize;
                            if dev.write_sector(op.sector + j, &buf[offset..offset + 512]).is_err() {
                                return Err(());
                            }
                        }
                        Ok(0)
                    }
                    _ => Err(()),
                }
            }
            _ => Err(()),
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

pub struct DevFs {
    root: Arc<DevNode>,
}

impl DevFs {
    pub fn add_block_device(&self, name: &str, idx: usize) {
        let node = Arc::new(DevNode {
            name: String::from(name),
            inner: DevNodeInner::BlockDevice(idx),
            children: Mutex::new(Vec::new()),
        });
        self.root.children.lock().push(node);
    }

    pub fn new() -> Self {
        let root = Arc::new(DevNode {
            name: String::from("/"),
            inner: DevNodeInner::Dir,
            children: Mutex::new(Vec::new()),
        });

        let null = Arc::new(DevNode {
            name: String::from("null"),
            inner: DevNodeInner::Null,
            children: Mutex::new(Vec::new()),
        });
        let zero = Arc::new(DevNode {
            name: String::from("zero"),
            inner: DevNodeInner::Zero,
            children: Mutex::new(Vec::new()),
        });
        let tty0 = Arc::new(DevNode {
            name: String::from("tty0"),
            inner: DevNodeInner::Tty0,
            children: Mutex::new(Vec::new()),
        });
        let tty = Arc::new(DevNode {
            name: String::from("tty"),
            inner: DevNodeInner::Tty0,
            children: Mutex::new(Vec::new()),
        });
        let fb0 = Arc::new(DevNode {
            name: String::from("fb0"),
            inner: DevNodeInner::Framebuffer,
            children: Mutex::new(Vec::new()),
        });

        let speaker = Arc::new(DevNode {
            name: String::from("speaker"),
            inner: DevNodeInner::Speaker,
            children: Mutex::new(Vec::new()),
        });

        let input_dir = Arc::new(DevNode {
            name: String::from("input"),
            inner: DevNodeInner::Dir,
            children: Mutex::new(Vec::new()),
        });

        let event0 = Arc::new(DevNode {
            name: String::from("event0"),
            inner: DevNodeInner::InputEvent(&crate::drivers::input::KEYBOARD_EVENTS),
            children: Mutex::new(Vec::new()),
        });
        let event1 = Arc::new(DevNode {
            name: String::from("event1"),
            inner: DevNodeInner::InputEvent(&crate::drivers::input::MOUSE_EVENTS),
            children: Mutex::new(Vec::new()),
        });

        input_dir.children.lock().push(event0);
        input_dir.children.lock().push(event1);

        root.children.lock().push(null);
        root.children.lock().push(zero);
        root.children.lock().push(tty0);
        root.children.lock().push(tty);
        root.children.lock().push(fb0);
        root.children.lock().push(speaker);
        root.children.lock().push(input_dir);

        DevFs { root }
    }
}

impl FileSystem for DevFs {
    fn root(&self) -> Result<Arc<dyn VfsNode>, ()> {
        Ok(self.root.clone())
    }
}
