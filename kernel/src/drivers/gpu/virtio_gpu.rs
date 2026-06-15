use alloc::boxed::Box;
use alloc::vec;
use spin::Mutex;
use x86_64::instructions::port::Port;
use x86_64::VirtAddr;

// ── VirtIO transport registers (legacy I/O) ─────────────────────────────────

const REG_QUEUE_PFN: u16 = 0x08;
const REG_QUEUE_SIZE: u16 = 0x0C;
const REG_QUEUE_SEL: u16 = 0x10;
const REG_QUEUE_NOTIFY: u16 = 0x12;
const REG_DEVICE_STATUS: u16 = 0x14;

const STATUS_ACK: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_DRIVER_OK: u8 = 4;

const VRING_DESC_F_NEXT: u16 = 1;
const VRING_DESC_F_WRITE: u16 = 2;

pub const VIRTIO_GPU_VENDOR: u16 = 0x1AF4;
pub const VIRTIO_GPU_DEVICE: u16 = 0x1050;

// ── VirtIO GPU protocol constants ───────────────────────────────────────────

const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;

const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

const FORMAT_B8G8R8A8_UNORM: u32 = 2;

// ── Helpers to read/write packed struct fields safely ───────────────────────

fn read_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([buf[offset], buf[offset+1], buf[offset+2], buf[offset+3]])
}

// Virtual offset of GpuHdr fields:
//   0:  ty       u32
const HDR_TYPE_OFF: usize = 0;

// RespDisplayInfo: GpuHdr(24) + Rect(16) + enabled(4) + flags(4) = 48
const DISPINFO_RECT_W_OFF: usize = 32;
const DISPINFO_RECT_H_OFF: usize = 36;
const DISPINFO_ENABLED_OFF: usize = 40;

// ── VirtIO queue structures (shared memory layout) ─────────────────────────

const QSIZE: u16 = 64;

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[repr(C, packed)]
struct VirtqAvail {
    flags: u16,
    idx: u16,
    ring: [u16; QSIZE as usize],
}

impl Default for VirtqAvail {
    fn default() -> Self {
        Self { flags: 0, idx: 0, ring: [0; QSIZE as usize] }
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct VirtqUsedElem {
    id: u32,
    len: u32,
}

#[repr(C, packed)]
struct VirtqUsed {
    flags: u16,
    idx: u16,
    ring: [VirtqUsedElem; QSIZE as usize],
}

fn alloc_virtq() -> (&'static mut [VirtqDesc], &'static mut VirtqAvail, &'static mut VirtqUsed) {
    let descs = Box::leak(vec![VirtqDesc::default(); QSIZE as usize].into_boxed_slice());
    for i in 0..(QSIZE - 1) as usize {
        descs[i].next = i as u16 + 1;
        descs[i].flags = VRING_DESC_F_NEXT;
    }
    descs[QSIZE as usize - 1].next = 0;
    descs[QSIZE as usize - 1].flags = 0;

    let avail = Box::leak(Box::new(VirtqAvail::default()));
    let used = Box::leak(Box::new(unsafe { core::mem::zeroed::<VirtqUsed>() }));
    (descs, avail, used)
}

fn phys_of<T>(x: &T) -> u64 {
    crate::memory::virt_to_phys(VirtAddr::from_ptr(x as *const T))
        .expect("virtio_gpu: cannot translate").as_u64()
}

fn phys_of_mut<T>(x: &mut T) -> u64 {
    crate::memory::virt_to_phys(VirtAddr::from_ptr(x as *mut T))
        .expect("virtio_gpu: cannot translate").as_u64()
}

// ── VirtIO GPU Driver ───────────────────────────────────────────────────────

pub struct VirtioGpu {
    io_base: u16,
    descs: &'static mut [VirtqDesc],
    avail: &'static mut VirtqAvail,
    used: &'static mut VirtqUsed,
    last_used: u16,
    next_desc: u16,
    pub width: u32,
    pub height: u32,
    fb_mem: Option<&'static mut [u32]>,
}

impl VirtioGpu {
    pub fn new(io_base: u16) -> Self {
        let mut status = Port::<u8>::new(io_base + REG_DEVICE_STATUS);
        unsafe { status.write(0); }
        unsafe { status.write(STATUS_ACK | STATUS_DRIVER); }

        let mut qsel = Port::<u16>::new(io_base + REG_QUEUE_SEL);
        let mut qsz = Port::<u16>::new(io_base + REG_QUEUE_SIZE);
        let mut qpfn = Port::<u32>::new(io_base + REG_QUEUE_PFN);

        unsafe { qsel.write(0); }
        let _hw_qsz = unsafe { qsz.read() };

        let (descs, avail, used) = alloc_virtq();
        unsafe { qpfn.write((phys_of(&descs[0]) >> 12) as u32); }

        unsafe { status.write(STATUS_ACK | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK); }

        let mut gpu = Self {
            io_base,
            descs,
            avail,
            used,
            last_used: 0,
            next_desc: 0,
            width: 800,
            height: 600,
            fb_mem: None,
        };

        // Query display info
        let mut rsp = [0u8; 48];
        let mut gdi_cmd = [0u8; 24];
        gdi_cmd[..4].copy_from_slice(&CMD_GET_DISPLAY_INFO.to_le_bytes());
        gpu.submit(&gdi_cmd, &mut rsp);
        let rsp_ty = read_u32(&rsp, HDR_TYPE_OFF);
        let enabled = read_u32(&rsp, DISPINFO_ENABLED_OFF);
        if rsp_ty == RESP_OK_DISPLAY_INFO && enabled != 0 {
            gpu.width = read_u32(&rsp, DISPINFO_RECT_W_OFF);
            gpu.height = read_u32(&rsp, DISPINFO_RECT_H_OFF);
        }
        crate::println!("VirtIO-GPU: {}x{}", gpu.width, gpu.height);

        gpu.create_fb();
        gpu
    }

    /// Submit a 2-descriptor chain: command (device-read-only) + response (device-write-only).
    fn submit(&mut self, cmd: &[u8], rsp: &mut [u8]) {
        let head = self.next_desc;
        let d0 = head as usize;

        self.descs[d0].addr = phys_of(&cmd[0]);
        self.descs[d0].len = cmd.len() as u32;
        self.descs[d0].flags = VRING_DESC_F_NEXT;

        let d1 = self.descs[d0].next as usize;
        self.descs[d1].addr = phys_of_mut(&mut rsp[0]);
        self.descs[d1].len = rsp.len() as u32;
        self.descs[d1].flags = VRING_DESC_F_WRITE;

        let slot = self.avail.idx as usize % QSIZE as usize;
        self.avail.ring[slot] = head;
        self.avail.idx = self.avail.idx.wrapping_add(1);

        let mut notify = Port::<u16>::new(self.io_base + REG_QUEUE_NOTIFY);
        unsafe { notify.write(0); }

        for _ in 0..2_000_000 {
            if self.used.idx != self.last_used {
                self.last_used = self.last_used.wrapping_add(1);
                break;
            }
            core::hint::spin_loop();
        }

        self.next_desc = (self.next_desc + 2) % QSIZE;
    }

    fn create_fb(&mut self) {
        let rid = 1u32;
        let w = self.width;
        let h = self.height;
        let fb_pixels = (w * h) as usize;
        let fb_bytes = fb_pixels * 4;

        // 1. Create 2D resource
        let mut cmd = [0u8; 40];
        cmd[..4].copy_from_slice(&CMD_RESOURCE_CREATE_2D.to_le_bytes());
        cmd[24..28].copy_from_slice(&rid.to_le_bytes());
        cmd[28..32].copy_from_slice(&FORMAT_B8G8R8A8_UNORM.to_le_bytes());
        cmd[32..36].copy_from_slice(&w.to_le_bytes());
        cmd[36..40].copy_from_slice(&h.to_le_bytes());
        let mut rsp = [0u8; 24];
        self.submit(&cmd, &mut rsp);
        if read_u32(&rsp, HDR_TYPE_OFF) != RESP_OK_NODATA {
            crate::println!("VirtIO-GPU: create_2d failed");
            return;
        }

        // 2. Allocate framebuffer
        let fb: &'static mut [u32] = Box::leak(vec![0u32; fb_pixels].into_boxed_slice());
        let fb_phys = phys_of(&fb[0]);
        let n_pages = (fb_bytes + 4095) / 4096;
        let entry_size = 12; // MemEntry: addr(8) + len(4)
        let cmd_hdr_len = 28; // GpuHdr(24) + resource_id(4) + nr_entries(4)
        let attach_len = cmd_hdr_len + n_pages * entry_size;
        let mut attach = vec![0u8; attach_len];
        attach[..4].copy_from_slice(&CMD_RESOURCE_ATTACH_BACKING.to_le_bytes());
        attach[24..28].copy_from_slice(&rid.to_le_bytes());
        attach[28..32].copy_from_slice(&(n_pages as u32).to_le_bytes());
        for i in 0..n_pages {
            let off = cmd_hdr_len + i * entry_size;
            attach[off..off+8].copy_from_slice(&(fb_phys + (i as u64) * 4096).to_le_bytes());
            attach[off+8..off+12].copy_from_slice(&4096u32.to_le_bytes());
        }
        let mut rsp2 = [0u8; 24];
        self.submit(&attach, &mut rsp2);
        if read_u32(&rsp2, HDR_TYPE_OFF) != RESP_OK_NODATA {
            crate::println!("VirtIO-GPU: attach_backing failed");
            return;
        }

        // 3. Set scanout
        let mut cmd3 = [0u8; 48];
        cmd3[..4].copy_from_slice(&CMD_SET_SCANOUT.to_le_bytes());
        // rect (24..40): x=0, y=0, w, h
        cmd3[32..36].copy_from_slice(&w.to_le_bytes());
        cmd3[36..40].copy_from_slice(&h.to_le_bytes());
        // scanout_id at 40
        cmd3[40..44].copy_from_slice(&0u32.to_le_bytes());
        // resource_id at 44
        cmd3[44..48].copy_from_slice(&rid.to_le_bytes());
        let mut rsp3 = [0u8; 24];
        self.submit(&cmd3, &mut rsp3);
        if read_u32(&rsp3, HDR_TYPE_OFF) != RESP_OK_NODATA {
            crate::println!("VirtIO-GPU: set_scanout failed");
            return;
        }

        self.fb_mem = Some(fb);
        crate::println!("VirtIO-GPU: fb ready {}x{}", w, h);
    }

    pub fn flip(&mut self) {
        let w = self.width;
        let h = self.height;

        // Transfer to host
        let mut cmd = [0u8; 48];
        cmd[..4].copy_from_slice(&CMD_TRANSFER_TO_HOST_2D.to_le_bytes());
        cmd[24..28].copy_from_slice(&0u32.to_le_bytes()); // rect.x
        cmd[28..32].copy_from_slice(&0u32.to_le_bytes()); // rect.y
        cmd[32..36].copy_from_slice(&w.to_le_bytes());    // rect.w
        cmd[36..40].copy_from_slice(&h.to_le_bytes());    // rect.h
        cmd[40..48].copy_from_slice(&0u64.to_le_bytes()); // offset
        cmd[48..52].copy_from_slice(&1u32.to_le_bytes()); // resource_id
        cmd[52..56].copy_from_slice(&0u32.to_le_bytes()); // padding
        let mut rsp = [0u8; 24];
        self.submit(&cmd, &mut rsp);

        // Flush
        let mut cmd2 = [0u8; 44];
        cmd2[..4].copy_from_slice(&CMD_RESOURCE_FLUSH.to_le_bytes());
        cmd2[24..28].copy_from_slice(&0u32.to_le_bytes()); // rect.x
        cmd2[28..32].copy_from_slice(&0u32.to_le_bytes()); // rect.y
        cmd2[32..36].copy_from_slice(&w.to_le_bytes());    // rect.w
        cmd2[36..40].copy_from_slice(&h.to_le_bytes());    // rect.h
        cmd2[40..44].copy_from_slice(&1u32.to_le_bytes()); // resource_id
        // padding at 44
        let mut rsp2 = [0u8; 24];
        self.submit(&cmd2, &mut rsp2);
    }
}

pub(crate) static GPU: Mutex<Option<VirtioGpu>> = Mutex::new(None);

pub fn init(io_base: u16) {
    let gpu = VirtioGpu::new(io_base);
    if let Some(ref fb) = gpu.fb_mem {
        let ptr = fb.as_ptr() as *mut u32;
        crate::drivers::graphics::FRAMEBUFFER.store(ptr, core::sync::atomic::Ordering::SeqCst);
        crate::drivers::graphics::WIDTH.store(gpu.width as usize, core::sync::atomic::Ordering::SeqCst);
        crate::drivers::graphics::HEIGHT.store(gpu.height as usize, core::sync::atomic::Ordering::SeqCst);
        crate::drivers::graphics::STRIDE.store(gpu.width as usize, core::sync::atomic::Ordering::SeqCst);
    }
    *GPU.lock() = Some(gpu);
}

pub fn flip() {
    let mut guard = GPU.lock();
    if let Some(gpu) = guard.as_mut() {
        gpu.flip();
    }
}
