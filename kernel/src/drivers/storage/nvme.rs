use volatile::Volatile;
use crate::drivers::block::{BlockDevice, BlockDeviceError, register_block_device};
use alloc::sync::Arc;
use alloc::boxed::Box;
use crate::sync::IrqSafeMutex as Mutex;
use crate::hal::dma::{DmaBuf, PooledDma};

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

/// Timeout in spin-loop iterations for command completion.
/// 200K iterations ≈ 200ms on typical hardware; 500K for slow I/O.
const ADMIN_TIMEOUT_LOOPS: u32 = 200_000;
const IO_TIMEOUT_LOOPS: u32 = 500_000;

const ADMIN_CREATE_IO_CQ: u8 = 0x05;
const ADMIN_CREATE_IO_SQ: u8 = 0x01;
const ADMIN_IDENTIFY: u8 = 0x06;
const ADMIN_SET_FEATURES: u8 = 0x09;
const ADMIN_FLUSH: u8 = 0x0C;
const ADMIN_SECURITY_SEND: u8 = 0x81;
const IO_READ: u8 = 0x02;
const IO_WRITE: u8 = 0x01;
const IO_FLUSH: u8 = 0x0C;

const QUEUE_PC: u16 = 1 << 0;
const QUEUE_EN: u16 = 1 << 1;

/// PCI Capability IDs
const PCI_CAP_MSI: u8 = 0x05;
const PCI_CAP_MSIX: u8 = 0x11;

// NVMe SMART Health Log (after IDENTIFY with CNS=2)
const SMART_LOG_LBA: u8 = 0x02; // CNS=2 → Command Set specific (Log Page)
const SMART_TEMPERATURE_OFFSET: usize = 1;
const SMART_AVAIL_SPARE_OFFSET: usize = 2;
const SMART_PERCENT_USED_OFFSET: usize = 3;
const SMART_CRITICAL_WARNING_OFFSET: usize = 0;

/// NVMe SMART/Health data read from the controller
#[derive(Debug, Clone, Copy)]
pub struct NvmeHealth {
    pub temperature_celsius: u16,
    pub available_spare_pct: u8,
    pub percentage_used: u8,
    pub critical_warning: u8,
    pub data_read_units: u64,     // total data read (bytes)
    pub data_written_units: u64,  // total data written (bytes)
    pub power_cycles: u64,
    pub power_on_hours: u64,
    pub unsafe_shutdowns: u64,
    pub media_errors: u64,
    pub error_log_entries: u64,
}

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

struct RingBuf {
    buf: DmaBuf,
    entry_size: usize,
    num_entries: u32,
}

impl RingBuf {
    /// Allocate a DMA-safe ring buffer. Uses the buddy allocator for
    /// physically contiguous, cache-line-aligned memory.
    fn new(n: u32, es: usize) -> Self {
        let size = (n as usize * es + 4095) & !4095; // round up to page
        let buf = DmaBuf::new(size).expect("NVMe: DMA ring buffer allocation failed");
        RingBuf { buf, entry_size: es, num_entries: n }
    }

    fn phys(&self) -> u64 { self.buf.phys() }
    fn entry(&self, index: u32) -> *mut u8 {
        unsafe { self.buf.as_ptr().add((index % self.num_entries) as usize * self.entry_size) as *mut u8 }
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
    sector_size: u16,   // bytes per sector (typically 512 or 4096)
    phase: u8,
    cq_head: u32,
    sq_tail: u32,
    next_cid: u16,
}

impl NvmeController {
    /// Create a new NVMe controller.
    /// `pci_bus/slot/func` are used for MSI-X/INTx interrupt setup.
    pub fn new(base_addr: usize, pci_bus: u8, pci_slot: u8, pci_func: u8) -> Option<&'static mut Self> {
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
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
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
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
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
            sector_size: 512, // default; updated after identify
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
        ctrl.sector_size = ctrl.identify_sector_size(nsid);

        // Query SMART health on init
        if let Some(health) = ctrl.smart_health() {
            crate::println!("NVMe: SMART health: temp={}C spare={}% used={}%", 
                health.temperature_celsius, health.available_spare_pct, health.percentage_used);
        }

        // Set up interrupts: try MSI-X first, then MSI, then legacy INTx
        let irq_vec = ctrl.setup_interrupts(pci_bus, pci_slot, pci_func);
        if irq_vec != 0 {
            crate::println!("NVMe: interrupt vector {} configured", irq_vec);
        } else {
            crate::println!("NVMe: no interrupt available, operating in polled mode");
        }

        let d = NvmeDisk { ctrl: unsafe { &mut *(ctrl as *mut Self) } };
        register_block_device(Arc::new(Mutex::new(d)));

        crate::println!("NVMe: ns {} ({} sectors, {} bytes/sector)",
            nsid, ctrl.sector_count, ctrl.sector_size);
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
        unsafe {
            (*ptr).write(val);
            // Ensure the doorbell write is visible to the controller before
            // the next submission. Bare-metal NVMe controllers may reorder
            // MMIO writes without this fence.
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
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
        let cqe = unsafe { core::ptr::read_volatile(cq.entry(head) as *const NvmeCqe) };
        let p = ((cqe.status >> 15) & 1) as u8;
        if p != *phase { return false; }
        *cq_head += 1;
        if (*cq_head).is_multiple_of(cq.num_entries) { *phase ^= 1; }
        true
    }

    /// Spin-wait for a completion queue entry. Returns true on success,
    /// false on timeout (caller should attempt error recovery).
    fn reap_with_timeout(cq_head: &mut u32, phase: &mut u8, cq: &RingBuf, timeout: u32) -> bool {
        for _ in 0..timeout {
            if Self::reap(cq_head, phase, cq, 0) { return true; }
            core::hint::spin_loop();
        }
        false // timeout — controller may be hung
    }

    #[allow(clippy::too_many_arguments)]
    fn admin_cmd(&mut self, opcode: u8, nsid: u32, prp1: u64, prp2: u64,
                 cdw10: u32, cdw11: u32, cdw12: u32) -> bool {
        let cmd = NvmeCmd {
            cdw0: (opcode as u32) | ((self.next_cid as u32) << 18),
            nsid, rsvd2: 0, mptr: 0, prp1, prp2,
            cdw10, cdw11, cdw12, cdw13: 0, cdw14: 0, cdw15: 0,
        };
        self.next_cid = self.next_cid.wrapping_add(1);
        Self::submit(&mut self.sq_tail, &cmd, &self.admin_sq);
        self.ring_db(0, true, self.sq_tail);
        if !Self::reap_with_timeout(&mut self.cq_head, &mut self.phase, &self.admin_cq, ADMIN_TIMEOUT_LOOPS) {
            crate::serial_write("[NVMe] admin command timed out\n");
            return false;
        }
        true
    }

    fn io_cmd(&mut self, opcode: u8, nsid: u32, prp1: u64,
              lba: u64, count: u32) -> bool {
        let cmd = NvmeCmd {
            cdw0: (opcode as u32) | ((self.next_cid as u32) << 18),
            nsid, rsvd2: 0, mptr: 0, prp1, prp2: 0,
            cdw10: lba as u32, cdw11: (lba >> 32) as u32, cdw12: count - 1,
            cdw13: 0, cdw14: 0, cdw15: 0,
        };
        self.next_cid = self.next_cid.wrapping_add(1);
        Self::submit(&mut self.sq_tail, &cmd, &self.io_sq);
        self.ring_db(1, true, self.sq_tail);
        if !Self::reap_with_timeout(&mut self.cq_head, &mut self.phase, &self.io_cq, IO_TIMEOUT_LOOPS) {
            crate::serial_write("[NVMe] I/O command timed out\n");
            return false;
        }
        true
    }

    /// I/O command with PRP2 support for multi-page transfers.
    /// `prp2` is either a second physical page address or a PRP list pointer.
    fn io_cmd_prp2(&mut self, opcode: u8, nsid: u32, prp1: u64, prp2: u64,
                   lba: u64, count: u32) -> bool {
        let cmd = NvmeCmd {
            cdw0: (opcode as u32) | ((self.next_cid as u32) << 18),
            nsid, rsvd2: 0, mptr: 0, prp1, prp2,
            cdw10: lba as u32, cdw11: (lba >> 32) as u32, cdw12: count - 1,
            cdw13: 0, cdw14: 0, cdw15: 0,
        };
        self.next_cid = self.next_cid.wrapping_add(1);
        Self::submit(&mut self.sq_tail, &cmd, &self.io_sq);
        self.ring_db(1, true, self.sq_tail);
        if !Self::reap_with_timeout(&mut self.cq_head, &mut self.phase, &self.io_cq, IO_TIMEOUT_LOOPS) {
            crate::serial_write("[NVMe] I/O command (PRP2) timed out\n");
            return false;
        }
        true
    }

    /// Return the native sector size in bytes.
    pub fn sector_size(&self) -> u16 {
        self.sector_size
    }

    /// Perform a full controller reset (disable → wait → re-init).
    /// Used for error recovery. Returns true on success.
    pub fn reset(&mut self) -> bool {
        let cap = self.regs.cap.read();
        let to_val = ((cap >> CAP_TO_SHIFT) & CAP_TO_MASK) as u32;
        let timeout_ms = to_val * 500;
        // 1. Disable controller
        self.regs.cc.write(0);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        if !Self::wait_rdy(self.regs, false, timeout_ms) {
            crate::serial_write("[NVMe] reset: controller did not become ready\n");
            return false;
        }
        // 2. Re-create admin queues (old DMA buffers are dropped here)
        self.admin_sq = RingBuf::new(16, 64);
        self.admin_cq = RingBuf::new(16, 16);
        self.regs.aqa.write(((15u32) << 16) | 15);
        self.regs.asq.write(self.admin_sq.phys());
        self.regs.acq.write(self.admin_cq.phys());
        // 3. Re-enable controller
        self.regs.cc.write(CC_EN | CC_IOCQES_16 | CC_IOSQES_64 | CC_MPS_4K);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        if !Self::wait_rdy(self.regs, true, timeout_ms) {
            crate::serial_write("[NVMe] reset: controller did not become ready after re-enable\n");
            return false;
        }
        // 4. Reset queue state
        self.cq_head = 0;
        self.sq_tail = 0;
        self.phase = 1;
        // 5. Recreate I/O queues
        self.io_cq = RingBuf::new(16, 16);
        self.io_sq = RingBuf::new(16, 64);
        let ok1 = self.admin_cmd(ADMIN_CREATE_IO_CQ, 0, self.io_cq.phys(), 0,
            ((15u32) << 16) | 1, (QUEUE_PC | QUEUE_EN) as u32, 0);
        let ok2 = self.admin_cmd(ADMIN_CREATE_IO_SQ, 0, self.io_sq.phys(), 0,
            ((15u32) << 16) | 1, (QUEUE_EN | QUEUE_PC) as u32 | (1 << 16), 0);
        if ok1 && ok2 {
            crate::serial_write("[NVMe] reset: controller recovered\n");
            true
        } else {
            crate::serial_write("[NVMe] reset: failed to recreate I/O queues\n");
            false
        }
    }

    fn identify_nsid(&mut self) -> u32 {
        let buf = match DmaBuf::new(4096) { Some(b) => b, None => return 0 };
        if !self.admin_cmd(ADMIN_IDENTIFY, 0, buf.phys(), 0, 1, 0, 0) {
            return 0;
        }
        unsafe { *(buf.as_ptr().add(0x504) as *const u32) }
    }

    fn identify_ns(&mut self, nsid: u32) -> u64 {
        let buf = match DmaBuf::new(4096) { Some(b) => b, None => return 0 };
        if !self.admin_cmd(ADMIN_IDENTIFY, nsid, buf.phys(), 0, 0, 0, 0) {
            return 0;
        }
        unsafe { *(buf.as_ptr() as *const u64) }
    }

    /// Read the namespace's logical block size from Identify Namespace data.
    /// LBAF0 metadata size is at byte 26, LBA format index in LBMS.
    fn identify_sector_size(&mut self, nsid: u32) -> u16 {
        let buf = match DmaBuf::new(4096) { Some(b) => b, None => return 512 };
        if !self.admin_cmd(ADMIN_IDENTIFY, nsid, buf.phys(), 0, 0, 0, 0) {
            return 512;
        }
        // NSDA byte 26 = FLBAS (Formatted LBA Size)
        // bits 3:0 = index into LBAF table
        let flbas = unsafe { *(buf.as_ptr().add(26) as *const u8) };
        let lbaf_idx = (flbas & 0x0F) as usize;
        // LBAF0 starts at byte 128, each entry is 4 bytes
        // bytes 1:0 = metadata size (bytes 128+4*idx, 128+4*idx+1)
        // byte 2 = lbads (log2 of block size)
        let lbads = unsafe { *(buf.as_ptr().add(128 + 4 * lbaf_idx + 2) as *const u8) };
        if lbads >= 9 && lbads <= 16 {
            1u16 << lbads
        } else {
            512
        }
    }

    /// Query SMART/Health Information Log Page (NVMe admin cmd).
    /// Returns None if the admin command fails.
    pub fn smart_health(&mut self) -> Option<NvmeHealth> {
        let buf = DmaBuf::new(4096)?;
        // IDENTIFY with CNS=2 (Log Page) for SMART/Health
        // cdw10[7:0]=2 (CNS), cdw11=0 (no specific namespace)
        if !self.admin_cmd(ADMIN_IDENTIFY, self.nsid, buf.phys(), 0, 2, 0, 0) {
            return None;
        }
        unsafe {
            let ptr = buf.as_ptr();
            let critical_warning = ptr.add(SMART_CRITICAL_WARNING_OFFSET).read_volatile();
            let temp_raw = ptr.add(SMART_TEMPERATURE_OFFSET).read_volatile() as u16
                         | (ptr.add(SMART_TEMPERATURE_OFFSET + 1).read_volatile() as u16) << 8;
            let available_spare = ptr.add(SMART_AVAIL_SPARE_OFFSET).read_volatile();
            let percentage_used = ptr.add(SMART_PERCENT_USED_OFFSET).read_volatile();
            // Byte 47:16 are the composite temperature (Kelvin), but we already got
            // the simpler temp. For total data read/written (bytes 32:47 in the log):
            let data_read = (ptr.add(48).read_volatile() as u64)
                | (ptr.add(49).read_volatile() as u64) << 8
                | (ptr.add(50).read_volatile() as u64) << 16
                | (ptr.add(51).read_volatile() as u64) << 24
                | (ptr.add(52).read_volatile() as u64) << 32
                | (ptr.add(53).read_volatile() as u64) << 40
                | (ptr.add(54).read_volatile() as u64) << 48
                | (ptr.add(55).read_volatile() as u64) << 56;
            let data_written = (ptr.add(56).read_volatile() as u64)
                | (ptr.add(57).read_volatile() as u64) << 8
                | (ptr.add(58).read_volatile() as u64) << 16
                | (ptr.add(59).read_volatile() as u64) << 24
                | (ptr.add(60).read_volatile() as u64) << 32
                | (ptr.add(61).read_volatile() as u64) << 40
                | (ptr.add(62).read_volatile() as u64) << 48
                | (ptr.add(63).read_volatile() as u64) << 56;
            let power_cycles = (ptr.add(64).read_volatile() as u64)
                | (ptr.add(65).read_volatile() as u64) << 8
                | (ptr.add(66).read_volatile() as u64) << 16
                | (ptr.add(67).read_volatile() as u64) << 24
                | (ptr.add(68).read_volatile() as u64) << 32
                | (ptr.add(69).read_volatile() as u64) << 40
                | (ptr.add(70).read_volatile() as u64) << 48
                | (ptr.add(71).read_volatile() as u64) << 56;
            let power_on_hours = (ptr.add(72).read_volatile() as u64)
                | (ptr.add(73).read_volatile() as u64) << 8
                | (ptr.add(74).read_volatile() as u64) << 16
                | (ptr.add(75).read_volatile() as u64) << 24
                | (ptr.add(76).read_volatile() as u64) << 32
                | (ptr.add(77).read_volatile() as u64) << 40
                | (ptr.add(78).read_volatile() as u64) << 48
                | (ptr.add(79).read_volatile() as u64) << 56;
            let unsafe_shutdowns = (ptr.add(80).read_volatile() as u64)
                | (ptr.add(81).read_volatile() as u64) << 8
                | (ptr.add(82).read_volatile() as u64) << 16
                | (ptr.add(83).read_volatile() as u64) << 24
                | (ptr.add(84).read_volatile() as u64) << 32
                | (ptr.add(85).read_volatile() as u64) << 40
                | (ptr.add(86).read_volatile() as u64) << 48
                | (ptr.add(87).read_volatile() as u64) << 56;
            let media_errors = (ptr.add(88).read_volatile() as u64)
                | (ptr.add(89).read_volatile() as u64) << 8
                | (ptr.add(90).read_volatile() as u64) << 16
                | (ptr.add(91).read_volatile() as u64) << 24
                | (ptr.add(92).read_volatile() as u64) << 32
                | (ptr.add(93).read_volatile() as u64) << 40
                | (ptr.add(94).read_volatile() as u64) << 48
                | (ptr.add(95).read_volatile() as u64) << 56;
            let error_log_entries = (ptr.add(96).read_volatile() as u64)
                | (ptr.add(97).read_volatile() as u64) << 8
                | (ptr.add(98).read_volatile() as u64) << 16
                | (ptr.add(99).read_volatile() as u64) << 24
                | (ptr.add(100).read_volatile() as u64) << 32
                | (ptr.add(101).read_volatile() as u64) << 40
                | (ptr.add(102).read_volatile() as u64) << 48
                | (ptr.add(103).read_volatile() as u64) << 56;
            Some(NvmeHealth {
                temperature_celsius: temp_raw.saturating_sub(273),
                available_spare_pct: available_spare,
                percentage_used,
                critical_warning,
                data_read_units: data_read,
                data_written_units: data_written,
                power_cycles,
                power_on_hours,
                unsafe_shutdowns,
                media_errors,
                error_log_entries,
            })
        }
    }

    /// Set up MSI-X or legacy INTx interrupts for this controller.
    /// Returns the allocated vector, or 0 if operating in polled mode.
    fn setup_interrupts(&mut self, bus: u8, slot: u8, func: u8) -> u8 {
        // Try MSI-X first (cap ID 0x11)
        if let Some(msix_cap) = crate::pci::find_capability(bus, slot, func, PCI_CAP_MSIX) {
            let msg_ctrl = crate::pci::read_config_u16(bus, slot, func, msix_cap + 2);
            if let Some(base_vec) = crate::apic::msi::alloc() {
                // Enable MSI-X
                crate::pci::write_config_u16(bus, slot, func, msix_cap + 2, msg_ctrl | (1 << 15));
                crate::serial_write(&alloc::format!("[NVMe] MSI-X enabled, vector {}\n", base_vec));
                return base_vec;
            }
        }
        // Fall back to MSI (cap ID 0x05)
        if let Some(vector) = crate::pci::pci_enable_msi(bus, slot, func) {
            crate::serial_write(&alloc::format!("[NVMe] MSI enabled, vector {}\n", vector));
            return vector;
        }
        // Fall back to legacy INTx
        let irq = (crate::pci::read_config_u32(bus, slot, func, 0x3C) & 0xFF) as u8;
        if irq != 0 {
            if let Some(vector) = crate::pci::pci_route_legacy_irq(bus, slot, func, irq) {
                crate::serial_write(&alloc::format!("[NVMe] legacy INTx routed, vector {}\n", vector));
                return vector;
            }
        }
        0 // no interrupt available
    }

    /// Issue a Flush command (admin opcode 0x0C) to flush volatile write caches.
    pub fn flush(&mut self) -> bool {
        self.admin_cmd(ADMIN_FLUSH, self.nsid, 0, 0, 0, 0, 0)
    }

    /// Issue NVMe Flush as an I/O command (NVMe 1.4+ feature).
    pub fn io_flush(&mut self) -> bool {
        self.io_cmd(IO_FLUSH, self.nsid, 0, 0, 0)
    }
}

unsafe impl Send for NvmeDisk {}
unsafe impl Sync for NvmeDisk {}

struct NvmeDisk {
    ctrl: &'static mut NvmeController,
}

impl BlockDevice for NvmeDisk {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        let ssz = self.ctrl.sector_size() as usize;
        // Allocate exactly sector_size DMA buffer — no 4KB waste for 512-byte sectors
        let dma = PooledDma::alloc(ssz).ok_or(BlockDeviceError::ReadError)?;
        if !self.ctrl.io_cmd(IO_READ, self.ctrl.nsid, dma.phys(), sector, 1) {
            return Err(BlockDeviceError::ReadError);
        }
        let len = core::cmp::min(buf.len(), ssz);
        unsafe { core::ptr::copy_nonoverlapping(dma.as_ptr(), buf.as_mut_ptr(), len); }
        Ok(())
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        let ssz = self.ctrl.sector_size() as usize;
        let mut dma = PooledDma::alloc(ssz).ok_or(BlockDeviceError::WriteError)?;
        let len = core::cmp::min(buf.len(), ssz);
        unsafe { core::ptr::copy_nonoverlapping(buf.as_ptr(), dma.as_mut_ptr(), len); }
        if !self.ctrl.io_cmd(IO_WRITE, self.ctrl.nsid, dma.phys(), sector, 1) {
            return Err(BlockDeviceError::WriteError);
        }
        Ok(())
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        Ok(self.ctrl.sector_count)
    }

    fn sync(&mut self) {
        self.ctrl.flush();
    }
}
