use volatile::Volatile;
use crate::drivers::block::{BlockDevice, BlockDeviceError, register_block_device};
use alloc::sync::Arc;
use alloc::boxed::Box;
use spin::Mutex;
use core::alloc::Layout;
use x86_64::VirtAddr;

/// NVMe register layout (BAR0/1 MMIO)
#[repr(C)]
pub struct NvmeRegisters {
    pub cap: Volatile<u64>,       // 0x00: Controller Capabilities
    pub vs: Volatile<u32>,        // 0x08: Version
    pub intms: Volatile<u32>,     // 0x0C: Interrupt Mask Set
    pub intmc: Volatile<u32>,     // 0x10: Interrupt Mask Clear
    pub cc: Volatile<u32>,        // 0x14: Controller Configuration
    pub rsv0: [u8; 4],            // 0x18
    pub csts: Volatile<u32>,      // 0x1C: Controller Status
    pub rsv1: [u8; 8],            // 0x20
    pub aqa: Volatile<u32>,       // 0x24: Admin Queue Attributes
    pub asq: Volatile<u64>,       // 0x28: Admin Submission Queue Base Address
    pub acq: Volatile<u64>,       // 0x30: Admin Completion Queue Base Address
}

const CAP_MQES_MASK: u64 = 0xFFFF;
const CAP_DSTRD_SHIFT: u64 = 32;
const CAP_TO_SHIFT: u64 = 24;
const CAP_TO_MASK: u64 = 0xFF;
const CAP_CSS_NVME: u64 = 0x200;

const CC_EN: u32 = 1 << 0;
const CC_IOCQES_16: u32 = 4 << 20;
const CC_IOSQES_64: u32 = 6 << 16;
const CC_MPS_4K: u32 = 0 << 7;
const CSTS_RDY: u32 = 1 << 0;

const ADMIN_CREATE_IO_CQ: u8 = 0x05;
const ADMIN_CREATE_IO_SQ: u8 = 0x01;
const ADMIN_IDENTIFY: u8 = 0x06;
const IO_READ: u8 = 0x02;
const IO_WRITE: u8 = 0x01;

const QUEUE_PC: u16 = 1 << 0;
const QUEUE_EN: u16 = 1 << 1;

#[repr(C, packed)]
struct NvmeCmd {
    cdw0: u32,
    nsid: u32,
    rsvd2: u64,
    mptr: u64,
    prp1: u64,
    prp2: u64,
    cdw10: u32,
    cdw11: u32,
    cdw12: u32,
    cdw13: u32,
    cdw14: u32,
    cdw15: u32,
}

#[repr(C)]
struct NvmeCqe {
    dw0: u32,
    dw1: u32,
    sq_head: u16,
    sq_id: u16,
    cid: u16,
    status: u16,
}

struct DmaBuf {
    virt: *mut u8,
    phys: u64,
    layout: Layout,
}

impl DmaBuf {
    fn new(size: usize) -> Self {
        let layout = Layout::from_size_align(size, 4096).unwrap();
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) };
        let phys = crate::memory::virt_to_phys_dma(VirtAddr::new(virt as u64)).as_u64();
        DmaBuf { virt, phys, layout }
    }

    fn phys(&self) -> u64 { self.phys }
    fn as_ptr(&self) -> *const u8 { self.virt }
    fn as_mut_ptr(&mut self) -> *mut u8 { self.virt }
}

impl Drop for DmaBuf {
    fn drop(&mut self) {
        unsafe { alloc::alloc::dealloc(self.virt, self.layout); }
    }
}

struct RingBuf {
    entries: *mut u8,
    phys: u64,
    entry_size: usize,
    num_entries: u32,
    layout: Layout,
}

impl RingBuf {
    fn new(n: u32, es: usize) -> Self {
        let size = (n as usize) * es;
        let layout = Layout::from_size_align(size, 4096).unwrap();
        let virt = unsafe { alloc::alloc::alloc_zeroed(layout) };
        let phys = crate::memory::virt_to_phys_dma(VirtAddr::new(virt as u64)).as_u64();
        RingBuf { entries: virt, phys, entry_size: es, num_entries: n, layout }
    }

    fn phys(&self) -> u64 { self.phys }
    fn entry(&self, index: u32) -> *mut u8 {
        unsafe { self.entries.add((index % self.num_entries) as usize * self.entry_size) }
    }
}

impl Drop for RingBuf {
    fn drop(&mut self) {
        unsafe { alloc::alloc::dealloc(self.entries, self.layout); }
    }
}

unsafe impl Send for NvmeController {}
unsafe impl Sync for NvmeController {}

pub struct NvmeController {
    regs: &'static mut NvmeRegisters,
    db_stride: u32,
    admin_sq: RingBuf,
    admin_cq: RingBuf,
    io_sq: RingBuf,
    io_cq: RingBuf,
    nsid: u32,
    sector_count: u64,
    phase: u8,
    cq_head: u32,
    sq_tail: u32,
    next_cid: u16,
}

impl NvmeController {
    pub fn new(base_addr: usize) -> Option<&'static mut Self> {
        let regs = unsafe { &mut *(base_addr as *mut NvmeRegisters) };

        let cap = regs.cap.read();
        let max_entries = (cap & CAP_MQES_MASK) as u32 + 1;
        let to_val = ((cap >> CAP_TO_SHIFT) & CAP_TO_MASK) as u32;
        let db_stride = 4 << ((cap >> CAP_DSTRD_SHIFT) & 0xF);
        let timeout_ms = to_val * 500;
        let _ = max_entries;

        if (cap & CAP_CSS_NVME) == 0 { return None; }

        // Disable if enabled
        if (regs.cc.read() & CC_EN) != 0 {
            regs.cc.write(0);
            if !Self::wait_rdy(regs, false, timeout_ms) { return None; }
        }

        // Allocate admin queues
        let admin_sq = RingBuf::new(16, 64);
        let admin_cq = RingBuf::new(16, 16);

        regs.aqa.write(((15u32) << 16) | 15);
        regs.asq.write(admin_sq.phys());
        regs.acq.write(admin_cq.phys());

        // Enable
        regs.cc.write(CC_EN | CC_IOCQES_16 | CC_IOSQES_64 | CC_MPS_4K);
        if !Self::wait_rdy(regs, true, timeout_ms) { return None; }

        let io_cq = RingBuf::new(16, 16);
        let io_sq = RingBuf::new(16, 64);

        let ctrl = Box::new(NvmeController {
            regs,
            db_stride,
            admin_sq,
            admin_cq,
            io_sq,
            io_cq,
            nsid: 0,
            sector_count: 0,
            phase: 1,
            cq_head: 0,
            sq_tail: 0,
            next_cid: 1,
        });
        let ctrl = Box::leak(ctrl);

        if !ctrl.admin_cmd(ADMIN_CREATE_IO_CQ, 0, ctrl.io_cq.phys(), 0,
            ((15u32) << 16) | 1, (QUEUE_PC | QUEUE_EN) as u32, 0) {
            return None;
        }
        if !ctrl.admin_cmd(ADMIN_CREATE_IO_SQ, 0, ctrl.io_sq.phys(), 0,
            ((15u32) << 16) | 1, (QUEUE_EN | QUEUE_PC) as u32 | (1 << 16), 0) {
            return None;
        }

        let nsid = ctrl.identify_nsid();
        if nsid == 0 { return None; }
        ctrl.nsid = nsid;

        ctrl.sector_count = ctrl.identify_ns(nsid);
        if ctrl.sector_count == 0 { return None; }

        let d = NvmeDisk { ctrl: unsafe { &mut *(ctrl as *mut Self) } };
        register_block_device(Arc::new(Mutex::new(d)));

        crate::println!("NVMe: ns {} ({} sectors)", nsid, ctrl.sector_count);
        Some(ctrl)
    }

    fn wait_rdy(r: &NvmeRegisters, ready: bool, timeout_ms: u32) -> bool {
        let want = if ready { CSTS_RDY } else { 0 };
        for _ in 0..(timeout_ms * 100) {
            if (r.csts.read() & CSTS_RDY) == want { return true; }
            core::hint::spin_loop();
        }
        false
    }

    fn db_offset(&self, qid: u32, sq: bool) -> usize {
        0x1000 + ((2 * qid + if sq { 0 } else { 1 }) * self.db_stride) as usize
    }

    fn ring_db(&self, qid: u32, sq: bool, val: u32) {
        let base = self.regs as *const NvmeRegisters as usize;
        let ptr = (base + self.db_offset(qid, sq)) as *mut Volatile<u32>;
        unsafe { (*ptr).write(val); }
    }

    fn submit(sq_tail: &mut u32, cmd: &NvmeCmd, sq: &RingBuf) {
        let tail = *sq_tail % sq.num_entries;
        unsafe {
            core::ptr::copy_nonoverlapping(
                cmd as *const _ as *const u8, sq.entry(tail), core::mem::size_of::<NvmeCmd>());
        }
        *sq_tail += 1;
    }

    fn reap(cq_head: &mut u32, phase: &mut u8, cq: &RingBuf, _qid: u32) -> bool {
        let head = *cq_head % cq.num_entries;
        let cqe = unsafe { &*(cq.entry(head) as *const NvmeCqe) };
        let p = ((cqe.status >> 15) & 1) as u8;
        if p != *phase { return false; }
        *cq_head += 1;
        if *cq_head % cq.num_entries == 0 { *phase ^= 1; }
        // ring_db is called separately
        true
    }

    fn admin_cmd(&mut self, opcode: u8, nsid: u32, prp1: u64, prp2: u64,
                 cdw10: u32, cdw11: u32, cdw12: u32) -> bool {
        let cmd = NvmeCmd {
            cdw0: (opcode as u32) | ((self.next_cid as u32) << 18),
            nsid, rsvd2: 0, mptr: 0, prp1, prp2,
            cdw10, cdw11, cdw12, cdw13: 0, cdw14: 0, cdw15: 0,
        };
        Self::submit(&mut self.sq_tail, &cmd, &self.admin_sq);
        self.ring_db(0, true, self.sq_tail);
        for _ in 0..200_000 {
            if Self::reap(&mut self.cq_head, &mut self.phase, &self.admin_cq, 0) { return true; }
            core::hint::spin_loop();
        }
        false
    }

    fn io_cmd(&mut self, opcode: u8, nsid: u32, prp1: u64,
              lba: u64, count: u32) -> bool {
        let cmd = NvmeCmd {
            cdw0: (opcode as u32) | ((self.next_cid as u32) << 18),
            nsid, rsvd2: 0, mptr: 0, prp1, prp2: 0,
            cdw10: lba as u32, cdw11: (lba >> 32) as u32, cdw12: count - 1,
            cdw13: 0, cdw14: 0, cdw15: 0,
        };
        Self::submit(&mut self.sq_tail, &cmd, &self.io_sq);
        self.ring_db(1, true, self.sq_tail);
        for _ in 0..200_000 {
            if Self::reap(&mut self.cq_head, &mut self.phase, &self.io_cq, 1) { return true; }
            core::hint::spin_loop();
        }
        false
    }

    fn identify_nsid(&mut self) -> u32 {
        let buf = DmaBuf::new(4096);
        if !self.admin_cmd(ADMIN_IDENTIFY, 0, buf.phys(), 0, 1, 0, 0) {
            return 0;
        }
        unsafe { *(buf.as_ptr().add(0x504) as *const u32) }
    }

    fn identify_ns(&mut self, nsid: u32) -> u64 {
        let buf = DmaBuf::new(4096);
        if !self.admin_cmd(ADMIN_IDENTIFY, nsid, buf.phys(), 0, 0, 0, 0) {
            return 0;
        }
        unsafe { *(buf.as_ptr() as *const u64) }
    }
}

unsafe impl Send for NvmeDisk {}
unsafe impl Sync for NvmeDisk {}

struct NvmeDisk {
    ctrl: &'static mut NvmeController,
}

impl BlockDevice for NvmeDisk {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        let dma = DmaBuf::new(4096);
        if !self.ctrl.io_cmd(IO_READ, self.ctrl.nsid, dma.phys(), sector, 1) {
            return Err(BlockDeviceError::ReadError);
        }
        let len = core::cmp::min(buf.len(), 512);
        unsafe { core::ptr::copy_nonoverlapping(dma.as_ptr(), buf.as_mut_ptr(), len); }
        Ok(())
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        let mut dma = DmaBuf::new(4096);
        let len = core::cmp::min(buf.len(), 512);
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), dma.as_mut_ptr(), len); }
        if !self.ctrl.io_cmd(IO_WRITE, self.ctrl.nsid, dma.phys(), sector, 1) {
            return Err(BlockDeviceError::WriteError);
        }
        Ok(())
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        Ok(self.ctrl.sector_count)
    }
}
