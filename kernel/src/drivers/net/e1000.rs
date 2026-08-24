use volatile::Volatile;
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::phy::{Device, DeviceCapabilities, RxToken, TxToken, ChecksumCapabilities};
use smoltcp::time::Instant;
use crate::hal::dma::DmaBuf;

/// EEPROM signature for detection.
const EEPROM_SIG: u16 = 0xBEEB;
/// Maximum iterations to wait for link up (≈500ms at 100µs per iteration).
const LINK_UP_TIMEOUT: u32 = 5_000;
/// Maximum iterations to wait for TX descriptor clean (≈50ms).
const TX_TIMEOUT: u32 = 500_000;

pub const REG_CTRL: u32 = 0x0000;
pub const REG_STATUS: u32 = 0x0008;
pub const REG_EEPROM: u32 = 0x0014;
pub const REG_ICR: u32 = 0x00C0;
pub const REG_IMS: u32 = 0x00D0;
pub const REG_RCTL: u32 = 0x00100;
pub const REG_TCTL: u32 = 0x00400;
pub const REG_FCRTL: u32 = 0x02160;
pub const REG_FCRTH: u32 = 0x02168;
pub const REG_RCTL_BSIZE: u32 = 0x00300; // RCTL bits 17:16 = buffer size
pub const REG_ITR: u32 = 0x000C8; // Interrupt Throttling Rate
pub const REG_RAL: u32 = 0x05400;
pub const REG_RAH: u32 = 0x05404;
pub const REG_MTA_START: u32 = 0x05200; // Multicast Table Array

// E1000 interrupt cause bits
const ICR_RXT0: u32 = 1 << 7;  // RX Timer Interrupt
const ICR_LSC: u32 = 1 << 2;   // Link Status Change
const ICR_TXDW: u32 = 1 << 0;  // TX Descriptor Written Back
const ICR_RXDMT0: u32 = 1 << 4; // RX Descriptor Minimum Threshold Hit

// E1000 statistics registers
pub const REG_STAT_TXGOODPKT: u32 = 0x04004;
pub const REG_STAT_TXBYTES: u32 = 0x04008;
pub const REG_STAT_RXGOODPKT: u32 = 0x04010;
pub const REG_STAT_RXBYTES: u32 = 0x04018;
pub const REG_STAT_BADCRC: u32 = 0x0401C;
pub const REG_STAT_MISSED: u32 = 0x04030;

/// E1000 driver statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct E1000Stats {
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_errors: u64,
    pub rx_missed: u64,
    pub link_up: bool,
    pub link_speed_mbps: u32,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RxDesc {
    pub addr: u64,
    pub length: u16,
    pub checksum: u16,
    pub status: u8,
    pub errors: u8,
    pub special: u16,
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxDesc {
    pub addr: u64,
    pub length: u16,
    pub cso: u8,
    pub cmd: u8,
    pub status: u8,
    pub css: u8,
    pub special: u16,
}

pub struct E1000 {
    base_addr: usize,
    rx_descs: &'static mut [RxDesc],
    tx_descs: &'static mut [TxDesc],
    rx_bufs: Vec<&'static mut [u8]>,
    tx_bufs: Vec<&'static mut [u8]>,
    rx_cur: usize,
    tx_cur: usize,
    mac_addr: [u8; 6],
    irq: u8,
    stats: E1000Stats,
}

impl E1000 {
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_addr
    }

    pub unsafe fn new(base_addr: usize) -> Self {
        E1000 { 
            base_addr,
            rx_descs: &mut [],
            tx_descs: &mut [],
            rx_bufs: Vec::new(),
            tx_bufs: Vec::new(),
            rx_cur: 0,
            tx_cur: 0,
            mac_addr: [0; 6],
            irq: 0,
            stats: E1000Stats::default(),
        }
    }
    
    pub fn set_irq(&mut self, irq: u8) {
        self.irq = irq;
    }
    
    fn write_reg_raw(base: usize, offset: u32, value: u32) {
        let ptr = (base + offset as usize) as *mut Volatile<u32>;
        unsafe {
            (*ptr).write(value);
            // Ensure MMIO write reaches the device before next operation.
            // Critical on bare-metal: PCI MMIO writes may be posted.
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        }
    }
    
    fn read_reg_raw(base: usize, offset: u32) -> u32 {
        let ptr = (base + offset as usize) as *const Volatile<u32>;
        unsafe { (*ptr).read() }
    }

    fn write_reg(&self, offset: u32, value: u32) {
        Self::write_reg_raw(self.base_addr, offset, value);
    }
    
    pub fn read_reg(&self, offset: u32) -> u32 {
        Self::read_reg_raw(self.base_addr, offset)
    }
    
    pub fn init(&mut self) {
        crate::println!("E1000: Initializing...");
        
        // 1. Detect EEPROM and read MAC
        self.read_mac();
        
        // 2. Reset the device: clear CTRL, re-disable TX/RX
        self.write_reg(REG_CTRL, 0x04000000); // POR reset
        for _ in 0..1000 { core::hint::spin_loop(); } // Wait for reset
        self.write_reg(REG_RCTL, 0);
        self.write_reg(REG_TCTL, 0);
        
        // 3. Wait for link up before enabling TX/RX
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | 0x40); // SLU bit 6
        let mut link_up = false;
        for _ in 0..LINK_UP_TIMEOUT {
            let status = self.read_reg(REG_STATUS);
            if status & (1 << 1) != 0 {
                link_up = true;
                break;
            }
            core::hint::spin_loop();
        }
        if link_up {
            let status = self.read_reg(REG_STATUS);
            let speed_bits = (status >> 18) & 0x3;
            let speed = match speed_bits { 0 => 10, 1 => 100, 2 => 1000, _ => 10 };
            crate::println!("E1000: Link UP @ {} Mbps", speed);
            self.stats.link_up = true;
            self.stats.link_speed_mbps = speed;
        } else {
            crate::println!("E1000: Link DOWN after timeout");
        }
        
        // 4. Allocate DMA-safe descriptor rings and buffers
        self.rx_descs = self.init_rx();
        self.tx_descs = self.init_tx();
        
        self.dump_rx_status();
        
        // Set up interrupt coalescing: batch interrupts every 100 µs
        self.set_interrupt_coalescing(100);
        
        // Don't enable interrupts here — wait until net::init() is ready
        // Call enable_interrupts() after net::init() instead
    }
    
    /// Enable E1000 interrupts. Call this AFTER net::init() sets up the interface.
    pub fn enable_interrupts(&mut self) {
        // Clear any pending interrupts
        self.read_reg(REG_ICR);
        // Enable only: RXT0 (bit 7) = RX Timer Interrupt, LSC (bit 2) = Link Status Change
        // Enable: RXT0 (bit 7) + RXDMT0 (bit 4) + LSC (bit 2) + TXDW (bit 0)
        self.write_reg(REG_IMS, ICR_RXT0 | ICR_RXDMT0 | ICR_LSC | ICR_TXDW);
        crate::println!("E1000: Interrupts enabled (IMS: 0x{:x})", self.read_reg(REG_IMS));
    }
    
    fn read_mac(&mut self) {
        let mut mac: [u8; 6] = [0; 6];
        let mut from_eeprom = false;
        
        // Try EEPROM first: read word 0x00, check for 0xBEEB signature
        let eeprom_check = self.read_eeprom_word(0x00);
        if eeprom_check == EEPROM_SIG {
            // EEPROM present — read MAC from words 0x01..0x03
            let w0 = self.read_eeprom_word(0x01);
            let w1 = self.read_eeprom_word(0x02);
            let w2 = self.read_eeprom_word(0x03);
            mac[0] = w0 as u8;
            mac[1] = (w0 >> 8) as u8;
            mac[2] = w1 as u8;
            mac[3] = (w1 >> 8) as u8;
            mac[4] = w2 as u8;
            mac[5] = (w2 >> 8) as u8;
            from_eeprom = true;
        }
        
        // Fallback: read from RAL/RAH registers (set by firmware/BIOS)
        if !from_eeprom && self.read_reg(REG_RAL) != 0 {
            let ral = self.read_reg(REG_RAL);
            let rah = self.read_reg(REG_RAH);
            mac[0] = ral as u8;
            mac[1] = (ral >> 8) as u8;
            mac[2] = (ral >> 16) as u8;
            mac[3] = (ral >> 24) as u8;
            mac[4] = rah as u8;
            mac[5] = (rah >> 8) as u8;
        }
        
        self.mac_addr = mac;
        crate::println!("E1000 MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}{}", 
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5],
            if from_eeprom { " (EEPROM)" } else { " (RAL/RAH)" });
    }
    
    /// Read a 16-bit word from the EEPROM via the EERD register.
    /// Returns 0xFFFF if no EEPROM is present.
    /// EERD layout (Intel E1000 datasheet): bit 0=START, bit 4=DONE,
    /// bits 15:8=ADDR, bits 31:16=DATA.
    fn read_eeprom_word(&self, word: u16) -> u16 {
        // Write START (bit 0) + address (shifted to bits 15:8)
        self.write_reg(REG_EEPROM, 1 | ((word as u32) << 8));
        // Wait for DONE bit (bit 4)
        for _ in 0..1000 {
            let val = self.read_reg(REG_EEPROM);
            if val & (1 << 4) != 0 {
                // DATA is in bits 31:16
                return ((val >> 16) & 0xFFFF) as u16;
            }
            core::hint::spin_loop();
        }
        0xFFFF // timeout — no EEPROM or failed
    }
    
    fn init_rx(&mut self) -> &'static mut [RxDesc] {
        use alloc::boxed::Box;
        use x86_64::VirtAddr;
        
        let desc_count = 32;
        let size = (core::mem::size_of::<RxDesc>() * desc_count) as u32;
        
        let descs = Box::leak(Box::new([RxDesc::default(); 32])); 
        let desc_ptr = descs.as_ptr();
        let desc_virt = VirtAddr::from_ptr(desc_ptr);
        let desc_phys = crate::memory::virt_to_phys(desc_virt).expect("RX Ring Phys failed");
        
        self.rx_bufs = Vec::with_capacity(desc_count);
        for desc in descs.iter_mut() {
            let buf: &'static mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());
            let buf_virt = VirtAddr::from_ptr(buf.as_ptr());
            let buf_phys = crate::memory::virt_to_phys(buf_virt).expect("RX Buf Phys failed");
            
            desc.addr = buf_phys.as_u64();
            desc.status = 0;
            self.rx_bufs.push(buf);
        }
        
        self.write_reg(0x2800, desc_phys.as_u64() as u32);
        self.write_reg(0x2804, (desc_phys.as_u64() >> 32) as u32);
        
        self.write_reg(0x2808, size); // RDLEN
        self.write_reg(0x2810, 0);    // RDH
        self.write_reg(0x2818, desc_count as u32 - 1); // RDT
        
        // Enable RX
        self.write_reg(REG_RCTL, (1 << 1) | (1 << 2) | (1 << 15) | (1 << 26)); 
        
        descs
    }
    
    fn init_tx(&mut self) -> &'static mut [TxDesc] {
        use alloc::boxed::Box;
        use x86_64::VirtAddr;
        
        let desc_count = 32;
        let size = (core::mem::size_of::<TxDesc>() * desc_count) as u32;

        let descs = Box::leak(Box::new([TxDesc::default(); 32]));
        let desc_ptr = descs.as_ptr();
        let desc_virt = VirtAddr::from_ptr(desc_ptr);
        let desc_phys = crate::memory::virt_to_phys(desc_virt).expect("TX Ring Phys failed");
        
        self.tx_bufs = Vec::with_capacity(desc_count);
        for desc in descs.iter_mut() {
            let buf: &'static mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());
            let buf_virt = VirtAddr::from_ptr(buf.as_ptr());
            let buf_phys = crate::memory::virt_to_phys(buf_virt).expect("TX Buf Phys failed");
            desc.addr = buf_phys.as_u64();
            desc.status = 0;
            self.tx_bufs.push(buf);
        }
        
        self.write_reg(0x3800, desc_phys.as_u64() as u32);
        self.write_reg(0x3804, (desc_phys.as_u64() >> 32) as u32);
        
        self.write_reg(0x3808, size); // TDLEN
        self.write_reg(0x3810, 0);    // TDH
        self.write_reg(0x3818, 0);    // TDT
        
        // Enable TX
        self.write_reg(REG_TCTL, (1 << 1) | (1 << 3) | (0x0F << 4) | (0x40 << 12)); 
        
        descs
    }

    pub fn send_packet(&mut self, data: &[u8]) {
        let cur = self.tx_cur;
        let len = self.tx_descs.len();
        
        if data.len() > 2048 {
            return;
        }
        
        let buf = &mut self.tx_bufs[cur];
        buf[..data.len()].copy_from_slice(data);
        
        unsafe {
            core::ptr::write_unaligned(core::ptr::addr_of_mut!(self.tx_descs[cur].length), data.len() as u16);
            core::ptr::write_unaligned(core::ptr::addr_of_mut!(self.tx_descs[cur].cmd), (1 << 0) | (1 << 1) | (1 << 3));
            core::ptr::write_unaligned(core::ptr::addr_of_mut!(self.tx_descs[cur].status), 0u8);
        }
        
        self.stats.tx_packets += 1;
        self.stats.tx_bytes += data.len() as u64;
        self.tx_cur = (cur + 1) % len;
        // Ring the doorbell — fence ensures write is visible to NIC
        Self::write_reg_raw(self.base_addr, 0x3818, self.tx_cur as u32);
        
        // Wait for TX descriptor to be completed with timeout
        for _ in 0..TX_TIMEOUT {
            let s;
            unsafe { s = core::ptr::read_unaligned(core::ptr::addr_of!(self.tx_descs[cur].status)); }
            if s & 1 != 0 {
                return;
            }
            core::hint::spin_loop();
        }
        // TX timeout — reset the NIC to recover
        crate::serial_write("[E1000] TX timeout, resetting\n");
        self.reset_nic();
    }
    
    /// Reset the NIC: disable TX/RX, full chip reset, re-init rings.
    fn reset_nic(&mut self) {
        self.write_reg(REG_RCTL, 0);   // Disable RX
        self.write_reg(REG_TCTL, 0);   // Disable TX
        self.write_reg(REG_CTRL, 0x04000000); // Full reset
        for _ in 0..1000 { core::hint::spin_loop(); }
        // Re-enable link
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | 0x40); // SLU
        // Re-write RX descriptor ring base address (reset by chip)
        if !self.rx_descs.is_empty() {
            let rx_phys = crate::memory::virt_to_phys(
                x86_64::VirtAddr::from_ptr(self.rx_descs.as_ptr())
            ).expect("RX ring phys");
            self.write_reg(0x2800, rx_phys.as_u64() as u32);
            self.write_reg(0x2804, (rx_phys.as_u64() >> 32) as u32);
            self.write_reg(0x2808, (self.rx_descs.len() * 16) as u32);
        }
        // Re-write TX descriptor ring base address
        if !self.tx_descs.is_empty() {
            let tx_phys = crate::memory::virt_to_phys(
                x86_64::VirtAddr::from_ptr(self.tx_descs.as_ptr())
            ).expect("TX ring phys");
            self.write_reg(0x3800, tx_phys.as_u64() as u32);
            self.write_reg(0x3804, (tx_phys.as_u64() >> 32) as u32);
            self.write_reg(0x3808, (self.tx_descs.len() * 16) as u32);
        }
        // Re-enable RX/TX with ring base addresses programmed
        self.write_reg(REG_RCTL, (1 << 1) | (1 << 2) | (1 << 15) | (1 << 26));
        self.write_reg(REG_TCTL, (1 << 1) | (1 << 3) | (0x0F << 4) | (0x40 << 12));
        self.rx_cur = 0;
        self.tx_cur = 0;
    }

    pub fn dump_rx_status(&self) {
        let rdh = self.read_reg(0x2810);
        let rdt = self.read_reg(0x2818);
        let icr = self.read_reg(REG_ICR);
        let sts = self.read_reg(REG_STATUS);
        let cur = self.rx_cur;
        let dd = if cur < self.rx_descs.len() {
            unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(self.rx_descs[cur].status)) & 1 }
        } else { 0 };
        crate::serial_write("[E1000] sts=");
        crate::serial_write(&alloc::format!("{:08x}", sts));
        crate::serial_write(" icr=");
        crate::serial_write(&alloc::format!("{:08x}", icr));
        crate::serial_write(" rdh=");
        crate::serial_write(&alloc::format!("{}", rdh));
        crate::serial_write(" rdt=");
        crate::serial_write(&alloc::format!("{}", rdt));
        crate::serial_write(" cur=");
        crate::serial_write(&alloc::format!("{}", cur));
        crate::serial_write(" dd=");
        crate::serial_write(&alloc::format!("{}", dd));
        crate::serial_write("\n");
    }
    
    pub fn receive_packet(&mut self) -> Option<Vec<u8>> {
        let cur = self.rx_cur;
        
        let status;
        let pkt_len;
        unsafe {
            status = core::ptr::read_unaligned(core::ptr::addr_of!(self.rx_descs[cur].status));
            pkt_len = core::ptr::read_unaligned(core::ptr::addr_of!(self.rx_descs[cur].length));
        }
        
        if status & 1 != 0 {
            let pkt_len = pkt_len.min(2048) as usize;
            let mut buf = vec![0u8; pkt_len];
            let src = &self.rx_bufs[cur][..pkt_len];
            buf.copy_from_slice(src);
            
            unsafe {
                core::ptr::write_unaligned(core::ptr::addr_of_mut!(self.rx_descs[cur].status), 0u8);
            }
            self.rx_cur = (cur + 1) % self.rx_descs.len();
            // Advance RDT to the new rx_cur — tells hardware the next descriptor it can use
            self.write_reg(0x2818, self.rx_cur as u32);
            
            self.stats.rx_packets += 1;
            self.stats.rx_bytes += pkt_len as u64;
            Some(buf)
        } else {
            None
        }
    }

    /// Read hardware statistics registers and update the stats field.
    pub fn update_stats(&mut self) {
        self.stats.rx_packets = self.read_reg(REG_STAT_RXGOODPKT) as u64;
        self.stats.tx_packets = self.read_reg(REG_STAT_TXGOODPKT) as u64;
        self.stats.rx_bytes = self.read_reg(REG_STAT_RXBYTES) as u64;
        self.stats.tx_bytes = self.read_reg(REG_STAT_TXBYTES) as u64;
        self.stats.rx_errors = self.read_reg(REG_STAT_BADCRC) as u64;
        self.stats.rx_missed = self.read_reg(REG_STAT_MISSED) as u64;
        // Link status from STATUS register (bit 1 = LU = Link Up)
        let status = self.read_reg(REG_STATUS);
        self.stats.link_up = (status & (1 << 1)) != 0;
        // Link speed: bits 19:18 of STATUS: 00=10, 01=100, 10=1000
        let speed_bits = (status >> 18) & 0x3;
        self.stats.link_speed_mbps = match speed_bits {
            0 => 10,
            1 => 100,
            2 => 1000,
            _ => 10,
        };
    }

    /// Return a snapshot of the driver statistics.
    pub fn get_stats(&mut self) -> E1000Stats {
        self.update_stats();
        self.stats
    }

    /// Configure interrupt coalescing via the ITR (Interrupt Throttling Rate) register.
    /// `usecs` = interval in microseconds between interrupt batches.
    /// Setting to 0 disables coalescing (interrupt on every packet).
    pub fn set_interrupt_coalescing(&mut self, usecs: u32) {
        // ITR register: value is in 256ns increments (at 1 GHz BCLK)
        // To coalesce N usecs: itr = usecs * 4 (approximate for 1 GHz)
        let itr_val = usecs * 4;
        self.write_reg(REG_ITR, itr_val);
        crate::serial_write(&alloc::format!("[E1000] Interrupt coalescing: {} us (ITR={})\n", usecs, itr_val));
    }

    /// Write link status to serial without heap allocation.
    fn write_link_status(link_up: bool, speed_mbps: u32) {
        let dir = if link_up { b"UP" } else { b"DOWN" };
        let mut buf = [0u8; 48];
        let mut pos = 0;
        for &b in b"[E1000] Link " { buf[pos] = b; pos += 1; }
        for &b in dir { buf[pos] = b; pos += 1; }
        for &b in b" @ " { buf[pos] = b; pos += 1; }
        if speed_mbps >= 1000 { buf[pos] = b'1'; pos += 1; for &b in b"000" { buf[pos] = b; pos += 1; }
        } else if speed_mbps >= 100 { buf[pos] = b'1'; pos += 1; for &b in b"00" { buf[pos] = b; pos += 1; }
        } else { buf[pos] = b'1'; pos += 1; for &b in b"0" { buf[pos] = b; pos += 1; }
        }
        for &b in b" Mbps\n" { buf[pos] = b; pos += 1; }
        crate::serial_write(core::str::from_utf8(&buf[..pos]).unwrap_or("[E1000] Link event\n"));
    }

    /// Handle an E1000 interrupt (called from the IRQ handler).
    /// Returns the ICR value so callers can check which events fired.
    pub fn handle_interrupt(&mut self) -> u32 {
        let icr = self.read_reg(REG_ICR);
        if icr == 0 {
            return 0;
        }
        // Clear handled interrupts by writing 1s back to ICR
        self.write_reg(REG_ICR, icr);

        if icr & ICR_LSC != 0 {
            // Link Status Change — re-read link state
            let status = self.read_reg(REG_STATUS);
            self.stats.link_up = (status & (1 << 1)) != 0;
            let speed_bits = (status >> 18) & 0x3;
            self.stats.link_speed_mbps = match speed_bits {
                0 => 10,
                1 => 100,
                2 => 1000,
                _ => 10,
            };
            Self::write_link_status(self.stats.link_up, self.stats.link_speed_mbps);
        }

        if icr & (ICR_RXT0 | ICR_RXDMT0) != 0 {
            // RX activity — update stats
            self.update_stats();
        }

        icr
    }

    /// Returns whether the link is currently up.
    pub fn is_link_up(&self) -> bool {
        self.stats.link_up
    }

    /// Poll the link status by reading the hardware STATUS register.
    /// Useful for periodic link state checks.
    pub fn poll_link_status(&mut self) {
        let status = self.read_reg(REG_STATUS);
        let was_up = self.stats.link_up;
        self.stats.link_up = (status & (1 << 1)) != 0;
        let speed_bits = (status >> 18) & 0x3;
        self.stats.link_speed_mbps = match speed_bits {
            0 => 10,
            1 => 100,
            2 => 1000,
            _ => 10,
        };
        if was_up != self.stats.link_up {
            Self::write_link_status(self.stats.link_up, self.stats.link_speed_mbps);
        }
    }

}

pub struct E1000Device {
    pub inner: E1000,
}

impl Device for E1000Device {
    type RxToken<'a> = E1000RxToken where Self: 'a;
    type TxToken<'a> = E1000TxToken<'a> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        self.inner.receive_packet().map(|packet| {
            (E1000RxToken { buffer: packet }, E1000TxToken { device: self })
        })
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(E1000TxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.checksum = ChecksumCapabilities::default();
        caps
    }
}

pub struct E1000RxToken {
    buffer: Vec<u8>,
}

impl RxToken for E1000RxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.buffer)
    }
}

pub struct E1000TxToken<'a> {
    device: &'a mut E1000Device,
}

impl<'a> TxToken for E1000TxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        self.device.inner.send_packet(&buffer);
        result
    }
}
