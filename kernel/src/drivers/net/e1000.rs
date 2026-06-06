use volatile::Volatile;
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::phy::{Device, DeviceCapabilities, RxToken, TxToken, ChecksumCapabilities};
use smoltcp::time::Instant;

pub const REG_CTRL: u32 = 0x0000;
pub const REG_STATUS: u32 = 0x0008;
pub const REG_EEPROM: u32 = 0x0014;
pub const REG_ICR: u32 = 0x00C0;
pub const REG_IMS: u32 = 0x00D0;
pub const REG_RCTL: u32 = 0x0100;
pub const REG_TCTL: u32 = 0x0400;

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
    rx_cur: usize,
    tx_cur: usize,
    mac_addr: [u8; 6],
    irq: u8,
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
            rx_cur: 0,
            tx_cur: 0,
            mac_addr: [0; 6],
            irq: 0,
        }
    }
    
    pub fn set_irq(&mut self, irq: u8) {
        self.irq = irq;
    }
    
    fn write_reg_raw(base: usize, offset: u32, value: u32) {
        let ptr = (base + offset as usize) as *mut Volatile<u32>;
        unsafe { (*ptr).write(value) }
    }
    
    fn read_reg_raw(base: usize, offset: u32) -> u32 {
        let ptr = (base + offset as usize) as *const Volatile<u32>;
        unsafe { (*ptr).read() }
    }

    fn write_reg(&self, offset: u32, value: u32) {
        Self::write_reg_raw(self.base_addr, offset, value);
    }
    
    fn read_reg(&self, offset: u32) -> u32 {
        Self::read_reg_raw(self.base_addr, offset)
    }
    
    pub fn init(&mut self) {
        crate::println!("E1000: Initializing...");
        
        // Reset
        // self.write_reg(REG_CTRL, 0x04000000); // RST bit 26
        
        // Read MAC Address from EEPROM
        self.read_mac();
        
        // Setup RX/TX Rings
        self.rx_descs = self.init_rx();
        self.tx_descs = self.init_tx();
        
        // Link Up
        let ctrl = self.read_reg(REG_CTRL);
        self.write_reg(REG_CTRL, ctrl | 0x40); // SLU bit 6
        
        // Enable Interrupts
        // ICR (Interrupt Cause Read) - clear all
        self.read_reg(REG_ICR);
        // IMS (Interrupt Mask Set) - enable RXT0 (Receiver Timer Interrupt)
        self.write_reg(REG_IMS, 1 << 7); // LSC
        let ims = self.read_reg(REG_IMS);
        self.write_reg(REG_IMS, ims | (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 6) | (1 << 7));
        crate::println!("E1000: Interrupts enabled (IMS: 0x{:x})", self.read_reg(REG_IMS));
    }
    
    fn read_mac(&mut self) {
        let mut mac: [u8; 6] = [0; 6];
        let _tmp = self.read_reg(REG_EEPROM); // Read from EEPROM or use RAL/RAH
        // For E1000, 0x5400 is RAL0, 0x5404 is RAH0
        if self.read_reg(0x5400) != 0 {
            let ral = self.read_reg(0x5400);
            let rah = self.read_reg(0x5404);
            mac[0] = ral as u8;
            mac[1] = (ral >> 8) as u8;
            mac[2] = (ral >> 16) as u8;
            mac[3] = (ral >> 24) as u8;
            mac[4] = rah as u8;
            mac[5] = (rah >> 8) as u8;
            self.mac_addr = mac;
            crate::println!("E1000 MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", 
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        }
    }
    
    fn init_rx(&mut self) -> &'static mut [RxDesc] {
        use alloc::boxed::Box;
        use x86_64::VirtAddr;
        
        let desc_count = 32;
        let size = (core::mem::size_of::<RxDesc>() * desc_count) as u32; // 16 * 32 = 512 bytes
        
        // Allocate Descriptors
        // In a real allocator we need 16-byte alignment. Box is usually pointer aligned (8 bytes). 
        // We'll rely on luck or use a larger buffer and align manually in future.
        let descs = Box::leak(Box::new([RxDesc::default(); 32])); 
        let desc_ptr = descs.as_ptr();
        let desc_virt = VirtAddr::from_ptr(desc_ptr);
        let desc_phys = crate::memory::virt_to_phys(desc_virt).expect("RX Ring Phys failed");
        
        // Allocate Buffers for each descriptor
        for desc in descs.iter_mut() {
            let buf: &mut [u8] = Box::leak(vec![0u8; 2048].into_boxed_slice());
            let buf_virt = VirtAddr::from_ptr(buf.as_ptr());
            let buf_phys = crate::memory::virt_to_phys(buf_virt).expect("RX Buf Phys failed");
            
            desc.addr = buf_phys.as_u64();
            desc.status = 0;
        }
        
        // Write to RDBAL/RDBAH
        self.write_reg(0x2800, desc_phys.as_u64() as u32);
        self.write_reg(0x2804, (desc_phys.as_u64() >> 32) as u32);
        
        self.write_reg(0x2808, size); // RDLEN
        self.write_reg(0x2810, 0);    // RDH
        self.write_reg(0x2818, desc_count as u32 - 1); // RDT
        
        // Enable RX
        // EN | SBC | BAM | RDM_0 | SECRC
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
        
        self.write_reg(0x3800, desc_phys.as_u64() as u32);
        self.write_reg(0x3804, (desc_phys.as_u64() >> 32) as u32);
        
        self.write_reg(0x3808, size); // TDLEN
        self.write_reg(0x3810, 0);    // TDH
        self.write_reg(0x3818, 0);    // TDT
        
        // Enable TX
        // EN | PSP | CT=15 | COLD=64
        self.write_reg(REG_TCTL, (1 << 1) | (1 << 3) | (0x0F << 4) | (0x40 << 12)); 
        
        descs
    }

    pub fn send_packet(&mut self, data: &[u8]) {
        let cur = self.tx_cur;
        let len = self.tx_descs.len();
        let base = self.base_addr;
        let desc = &mut self.tx_descs[cur];
        
        // Copy data to buffer
        let buf_ptr = desc.addr as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, data.len());
        }
        
        desc.length = data.len() as u16;
        desc.cmd = (1 << 0) | (1 << 1) | (1 << 3); // EOP | IFCS | RS
        desc.status = 0;
        
        self.tx_cur = (cur + 1) % len;
        Self::write_reg_raw(base, 0x3818, self.tx_cur as u32); // TDT
        
        // Wait for send
        while (desc.status & 1) == 0 {}
    }

    pub fn receive_packet(&mut self) -> Option<Vec<u8>> {
        let cur = self.rx_cur;
        let desc = &mut self.rx_descs[cur];
        
        if (desc.status & 1) != 0 {
            let len = desc.length as usize;
            let mut buf = vec![0u8; len];
            let buf_ptr = desc.addr as *const u8;
            unsafe {
                core::ptr::copy_nonoverlapping(buf_ptr, buf.as_mut_ptr(), len);
            }
            
            desc.status = 0;
            self.rx_cur = (cur + 1) % self.rx_descs.len();
            self.write_reg(0x2818, cur as u32); // RDT
            
            return Some(buf);
        }
        
        None
    }
}

pub struct E1000Device {
    pub inner: E1000,
}

impl Device for E1000Device {
    type RxToken<'a> = E1000RxToken where Self: 'a;
    type TxToken<'a> = E1000TxToken<'a> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if let Some(packet) = self.inner.receive_packet() {
            Some((E1000RxToken { buffer: packet }, E1000TxToken { device: self }))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(E1000TxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.checksum = ChecksumCapabilities::ignored();
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
