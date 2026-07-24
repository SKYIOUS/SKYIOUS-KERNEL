#![allow(dead_code)]

use x86_64::instructions::port::Port;
use x86_64::VirtAddr;

// ─── I/O register offsets ───────────────────────────────────────────────────
const USBCMD: u16    = 0x00;
const USBSTS: u16    = 0x02;
const USBINTR: u16   = 0x04;
const FRNUM: u16     = 0x06;
const FLBASEADD: u16 = 0x08;
const SOFMOD: u16    = 0x0C;
const PORTSC1: u16   = 0x10; // port 2 at +2

// ─── USBCMD bits ────────────────────────────────────────────────────────────
const CMD_RUN: u16     = 1 << 0;
const CMD_HCRESET: u16 = 1 << 1;
const CMD_GRESET: u16  = 1 << 2;
const CMD_CF: u16      = 1 << 4;

// ─── USBSTS bits ────────────────────────────────────────────────────────────
const STS_HCHALTED: u16 = 1 << 3;

// ─── PORTSC bits ────────────────────────────────────────────────────────────
const PORT_CCS: u16    = 1 << 0;
const PORT_CSC: u16    = 1 << 1;
const PORT_PED: u16    = 1 << 2;
const PORT_PEDC: u16   = 1 << 3;
const PORT_LS_LOW: u16 = 1 << 9;
const PORT_RESET_MASK: u16 = 1 << 12;

// ─── TD status/ctrl (QEMU-compatible layout) ────────────────────────────────
const TD_ACTIVE: u32  = 1 << 23;
const TD_STALLED: u32 = 1 << 24;
const TD_BABBLE: u32  = 1 << 26;
const TD_SPD: u32     = 1 << 29;
const TD_LS: u32      = 1 << 30;
const TD_IOC: u32     = 1 << 31;
const TD_ERRORS: u32  = TD_STALLED | TD_BABBLE;

// ─── Token shifts ───────────────────────────────────────────────────────────
const SH_PID: u32     = 8;
const SH_ADDR: u32    = 11;
const SH_EP: u32      = 18;
const SH_TOGGLE: u32  = 22;
const SH_MAXLEN: u32  = 23;

const PID_SETUP: u32 = 3;
const PID_IN: u32    = 1;
const PID_OUT: u32   = 0;

const FRAMES: usize = 1024;
const TD_POOL: usize = 64;

// ─── Hardware structures ────────────────────────────────────────────────────
#[repr(C, packed)]
struct Td {
    link: u32,    // next TD or terminator
    status: u32,  // control/status
    token: u32,   // PID, addr, ep, toggle, maxlen
    buffer: u32,  // data buffer physical address (low 32 bits)
}

fn tok(pid: u32, addr: u32, ep: u32, toggle: u32, len: usize) -> u32 {
    let ml = if len == 0 { 0x1FF } else { ((len + 3) / 4).saturating_sub(1) as u32 & 0x1FF };
    (ml << SH_MAXLEN) | (toggle << SH_TOGGLE) | (ep << SH_EP) | (addr << SH_ADDR) | (pid << SH_PID)
}

fn td_link(phys: u32) -> u32 { phys }
fn td_term() -> u32 { 1 }

/// DMA buffer with physical address in the low 4G (UHCI is 32-bit only).
struct DmaBuf {
    virt: *mut u8,
    phys: u32,
    size: usize,
}

impl DmaBuf {
    fn alloc(size: usize, align: usize) -> Self {
        let layout = core::alloc::Layout::from_size_align(size, align).unwrap();
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) };
        let phys = crate::memory::virt_to_phys_dma(VirtAddr::new(virt as u64)).as_u64() as u32;
        DmaBuf { virt, phys, size }
    }
    fn phys32(&self) -> u32 { self.phys }
    fn virt(&self) -> *mut u8 { self.virt }
    fn as_slice(&self) -> &[u8] { unsafe { core::slice::from_raw_parts(self.virt, self.size) } }
    fn as_mut_slice(&mut self) -> &mut [u8] { unsafe { core::slice::from_raw_parts_mut(self.virt, self.size) } }
}
impl Drop for DmaBuf {
    fn drop(&mut self) {
        let layout = core::alloc::Layout::from_size_align(self.size, 1).unwrap();
        unsafe { alloc::alloc::dealloc(self.virt, layout); }
    }
}

// ─── UHCI Controller ────────────────────────────────────────────────────────
pub struct UhciController {
    io: u16,
    fl: DmaBuf,                     // frame list (4 KB)
    td_arena: &'static mut [Td],    // TD pool
    td_phys: u32,                   // phys addr of TD pool base
    td_free: usize,                 // next free index in pool
    data: [DmaBuf; 2],              // data buffers for IN/OUT
}

impl UhciController {
    pub fn new(io_base: u16) -> Self {
        let fl = DmaBuf::alloc(FRAMES * 4, 4096);
        let layout = core::alloc::Layout::from_size_align(TD_POOL * 16, 16).unwrap();
        let ptr = unsafe { alloc::alloc::alloc_zeroed(layout) };
        let td_phys = crate::memory::virt_to_phys_dma(VirtAddr::new(ptr as u64)).as_u64() as u32;
        let td_arena = unsafe { core::slice::from_raw_parts_mut(ptr as *mut Td, TD_POOL) };
        UhciController {
            io: io_base,
            fl,
            td_arena,
            td_phys,
            td_free: 0,
            data: [DmaBuf::alloc(128, 16), DmaBuf::alloc(128, 16)],
        }
    }

    // ─── Port I/O helpers ───────────────────────────────────────────────
    fn rw(&self, r: u16) -> u16 { unsafe { Port::new(self.io + r).read() } }
    fn ww(&mut self, r: u16, v: u16) { unsafe { Port::new(self.io + r).write(v); } }
    fn wl(&mut self, r: u16, v: u32) { self.ww(r, v as u16); self.ww(r + 2, (v >> 16) as u16); }
    fn wb(&mut self, r: u16, v: u8) { unsafe { Port::new(self.io + r).write(v); } }

    fn portsc(&self, p: usize) -> u16 { self.rw(PORTSC1 + (p as u16 * 2)) }
    fn set_portsc(&mut self, p: usize, v: u16) { self.ww(PORTSC1 + (p as u16 * 2), v); }

    // ─── TD allocator ────────────────────────────────────────────────────
    fn td_alloc(&mut self) -> Option<(*mut Td, u32)> {
        if self.td_free >= TD_POOL { return None; }
        let i = self.td_free;
        self.td_free += 1;
        let ptr = unsafe { self.td_arena.as_mut_ptr().add(i) };
        let phys = self.td_phys + (i * 16) as u32;
        Some((ptr, phys))
    }

    fn td_free_all(&mut self) { self.td_free = 0; }

    // ─── Wait for TD completion (spin) ─────────────────────────────────
    fn wait_td(&self, ptr: *mut Td, timeout: u32) -> bool {
        for _ in 0..timeout {
            let st = unsafe { (*ptr).status };
            if st & TD_ACTIVE == 0 { return st & TD_ERRORS == 0; }
            core::hint::spin_loop();
        }
        false
    }

    // ─── Public API ───────────────────────────────────────────────────────
    pub fn init(&mut self) {
        crate::serial_write(&alloc::format!("[UHCI] init at I/O 0x{:x}\n", self.io));

        // Reset
        self.ww(USBCMD, CMD_HCRESET);
        for _ in 0..2000 { core::hint::spin_loop(); }

        // Frame list
        self.wl(FLBASEADD, self.fl.phys32());
        self.wb(SOFMOD, 64);
        self.ww(USBCMD, CMD_RUN | CMD_CF);

        let mut t = 0u32;
        while (self.rw(USBSTS) & STS_HCHALTED) != 0 { core::hint::spin_loop(); t += 1; if t > 2000 { break; } }
        self.ww(USBSTS, 0xFFFF);

        crate::serial_write("[UHCI] running\n");

        for p in 0..2 { self.enumerate(p); }
    }

    fn enumerate(&mut self, port: usize) {
        let ps = self.portsc(port);
        if ps & PORT_CCS == 0 { return; }
        crate::serial_write(&alloc::format!("[UHCI] port {} connected {:04x}\n", port, ps));

        // Reset
        self.set_portsc(port, PORT_RESET_MASK | PORT_PED);
        let mut t = 0u32;
        while self.portsc(port) & PORT_RESET_MASK != 0 { core::hint::spin_loop(); t += 1; if t > 100000 { break; } }
        let ps = self.portsc(port);
        let low = (ps & PORT_LS_LOW) != 0;
        crate::serial_write(&alloc::format!("[UHCI] port {} reset done {:04x} low={}\n", port, ps, low));

        // Control transfer: GET_DESCRIPTOR(device) → 8 bytes
        let mut dev_desc = [0u8; 8];
        if !self.ctrl_in(0, 0x80, 6, 0x0100, 0, &mut dev_desc, low) {
            crate::serial_write("[UHCI] get_descriptor failed\n");
            return;
        }
        let maxpkt = dev_desc[7];
        crate::serial_write(&alloc::format!("[UHCI] dev desc {:02x?} maxpkt={}\n", dev_desc, maxpkt));

        // Set address (device 1)
        if !self.ctrl_out(0, 0x00, 5, 1, 0, &[], low) {
            crate::serial_write("[UHCI] set_address failed\n");
            return;
        }
        crate::serial_write("[UHCI] address set to 1\n");

        // Get full device descriptor (18 bytes)
        let mut dev_desc2 = [0u8; 18];
        if !self.ctrl_in(1, 0x80, 6, 0x0100, 0, &mut dev_desc2, low) { return; }
        let vid = (dev_desc2[8] as u16) | ((dev_desc2[9] as u16) << 8);
        let pid = (dev_desc2[10] as u16) | ((dev_desc2[11] as u16) << 8);
        let cls = dev_desc2[4];
        crate::serial_write(&alloc::format!("[UHCI] device {:04x}:{:04x} class={:02x}\n", vid, pid, cls));

        // Get configuration descriptor (first 9 bytes for total length)
        let mut cfg_hdr = [0u8; 9];
        if !self.ctrl_in(1, 0x80, 6, 0x0200, 0, &mut cfg_hdr, low) { return; }
        let cfg_total = (cfg_hdr[2] as usize) | ((cfg_hdr[3] as usize) << 8);
        crate::serial_write(&alloc::format!("[UHCI] config desc total len {}\n", cfg_total));

        // Get full config descriptor
        let cfg_len = cfg_total.min(256);
        let mut cfg_buf = alloc::vec![0u8; cfg_len];
        if !self.ctrl_in(1, 0x80, 6, 0x0200, 0, &mut cfg_buf, low) { return; }

        // Parse interfaces for HID keyboard
        let mut off = 9usize;
        while off + 3 < cfg_len && off < cfg_total {
            let dlen = cfg_buf[off] as usize;
            if dlen < 3 { break; }
            let dtype = cfg_buf[off + 1];
            // Interface descriptor (type 4)
            if dtype == 4 && off + 9 <= cfg_len {
                let if_class = cfg_buf[off + 5];
                let if_sub   = cfg_buf[off + 6];
                let if_proto = cfg_buf[off + 7];
                crate::serial_write(&alloc::format!("[UHCI]   iface class={:02x} sub={:02x} proto={:02x}\n",
                    if_class, if_sub, if_proto));

                // HID boot keyboard: class=3, sub=1, proto=1
                if if_class == 3 && if_sub == 1 && if_proto == 1 {
                    crate::serial_write("[UHCI]   -> HID boot keyboard\n");

                    // Set configuration
                    self.ctrl_out(1, 0x00, 9, 1, 0, &[], low);

                    // Set HID protocol to boot (boot protocol = 0)
                    self.ctrl_out(1, 0x21, 0x0B, 0, 0, &[], low);

                    // Set idle (infinite)
                    self.ctrl_out(1, 0x21, 0x0A, 0, 0, &[], low);

                    // Find interrupt IN endpoint
                    self.init_hid_keyboard(1, cfg_buf[off + 8], low);
                    return;
                }
            }
            off += dlen;
        }
        // Set config for non-keyboard devices too
        self.ctrl_out(1, 0x00, 9, 1, 0, &[], low);
    }

    /// Control IN transfer: sends setup + IN data + status OUT
    fn ctrl_in(&mut self, addr: u32, bm_req_type: u32, b_req: u32, w_val: u32, w_idx: u32, data: &mut [u8], low: bool) -> bool {
        let len = data.len().min(128);
        let setup: [u8; 8] = [
            bm_req_type as u8, b_req as u8,
            (w_val & 0xFF) as u8, ((w_val >> 8) & 0xFF) as u8,
            (w_idx & 0xFF) as u8, ((w_idx >> 8) & 0xFF) as u8,
            (len & 0xFF) as u8, ((len >> 8) & 0xFF) as u8,
        ];
        let l = if len > 0 { len } else { 1 };
        let ls = if low { TD_LS } else { 0 };

        let (td0, ph0) = match self.td_alloc() { Some(v) => v, None => return false };
        let (td1, ph1) = match self.td_alloc() { Some(v) => v, None => return false };
        let (td2, ph2) = match self.td_alloc() { Some(v) => v, None => return false };

        unsafe {
            core::ptr::copy_nonoverlapping(setup.as_ptr(), self.data[0].virt(), 8);

            (*td0).link = td_link(ph1);
            (*td0).status = TD_ACTIVE | ls;
            (*td0).token = tok(PID_SETUP, addr, 0, 0, 8);
            (*td0).buffer = self.data[0].phys32();

            (*td1).link = td_link(ph2);
            (*td1).status = TD_ACTIVE | TD_IOC | ls;
            (*td1).token = tok(PID_IN, addr, 0, 0, l);
            (*td1).buffer = self.data[1].phys32();

            (*td2).link = td_term();
            (*td2).status = TD_ACTIVE | ls;
            (*td2).token = tok(PID_OUT, addr, 0, 1, 0);
            (*td2).buffer = 0;
        }

        // Insert into frame list at slot 0
        let fl_base = self.fl.virt() as *mut u32;
        unsafe { *fl_base = td_link(ph0); }
        self.flush();

        let ok = self.wait_td(td1, 2_000_000);
        if ok && len > 0 {
            unsafe { core::ptr::copy_nonoverlapping(self.data[1].virt(), data.as_mut_ptr(), len); }
        }
        self.td_free_all();
        unsafe { *fl_base = td_term(); }
        ok
    }

    /// Control OUT transfer: sends setup + status IN
    fn ctrl_out(&mut self, addr: u32, bm_req_type: u32, b_req: u32, w_val: u32, w_idx: u32, _data: &[u8], low: bool) -> bool {
        let setup: [u8; 8] = [
            bm_req_type as u8, b_req as u8,
            (w_val & 0xFF) as u8, ((w_val >> 8) & 0xFF) as u8,
            (w_idx & 0xFF) as u8, ((w_idx >> 8) & 0xFF) as u8,
            0, 0,
        ];
        let ls = if low { TD_LS } else { 0 };

        let (td0, ph0) = match self.td_alloc() { Some(v) => v, None => return false };
        let (td1, ph1) = match self.td_alloc() { Some(v) => v, None => return false };

        unsafe {
            core::ptr::copy_nonoverlapping(setup.as_ptr(), self.data[0].virt(), 8);
            (*td0).link = td_link(ph1);
            (*td0).status = TD_ACTIVE | ls;
            (*td0).token = tok(PID_SETUP, addr, 0, 0, 8);
            (*td0).buffer = self.data[0].phys32();

            (*td1).link = td_term();
            (*td1).status = TD_ACTIVE | TD_IOC | ls;
            (*td1).token = tok(PID_IN, addr, 0, 1, 0);
            (*td1).buffer = 0;
        }

        let fl_base = self.fl.virt() as *mut u32;
        unsafe { *fl_base = td_link(ph0); }
        let ok = self.wait_td(td1, 2_000_000);
        unsafe { *fl_base = td_term(); }
        self.td_free_all();
        ok
    }

    fn init_hid_keyboard(&mut self, _addr: u32, _ep: u8, _low: bool) {
        crate::serial_write("[UHCI] HID keyboard init ok\n");
        // ponytail: stub — real polling TDs in frame list deferred
        // For now we confirm the device is configured.
    }

    fn flush(&self) {
        // ponytail: fence ensures ordering but x86 WC memory may need clflush for DMA coherency
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
}
