use alloc::vec::Vec;
use alloc::vec;
use alloc::sync::Arc;
use spin::Mutex;
use x86_64::VirtAddr;
use smoltcp::phy::{Device, DeviceCapabilities, RxToken, TxToken, ChecksumCapabilities};
use smoltcp::time::Instant;

/// VirtIO PCI IDs
pub const VIRTIO_VENDOR_ID: u16 = 0x1AF4;
pub const VIRTIO_NET_DEVICE_ID: u16 = 0x1000;

/// VirtIO Net Register Offsets (Legacy)
pub const REG_DEVICE_FEATURES: u32 = 0x00;
pub const REG_GUEST_FEATURES: u32 = 0x04;
pub const REG_QUEUE_PFN: u32 = 0x08;
pub const REG_QUEUE_SIZE: u32 = 0x0C;
pub const REG_QUEUE_SEL: u32 = 0x10;
pub const REG_QUEUE_NOTIFY: u32 = 0x12;
pub const REG_DEVICE_STATUS: u32 = 0x14;
pub const REG_ISR_STATUS: u32 = 0x13;

/// VirtIO Status Bits
pub const STATUS_ACK: u8 = 1;
pub const STATUS_DRIVER: u8 = 2;
pub const STATUS_DRIVER_OK: u8 = 4;
pub const STATUS_FEATURES_OK: u8 = 8;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

pub const VRING_DESC_F_NEXT: u16 = 1;
pub const VRING_DESC_F_WRITE: u16 = 2;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct VirtqAvail {
    pub flags: u16,
    pub idx: u16,
    pub ring: [u16; 256],
}

impl Default for VirtqAvail {
    fn default() -> Self {
        VirtqAvail {
            flags: 0,
            idx: 0,
            ring: [0; 256],
        }
    }
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default)]
pub struct VirtqUsedElem {
    pub id: u32,
    pub len: u32,
}

#[repr(C, packed)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,
    pub ring: [VirtqUsedElem; 256],
}

pub struct VirtIOQueue {
    pub descriptors: &'static mut [VirtqDesc],
    pub available: &'static mut VirtqAvail,
    pub used: &'static mut VirtqUsed,
    pub size: u16,
    pub index: u16,
    pub last_used: u16,
}

impl VirtIOQueue {
    pub fn new(size: u16) -> Self {
        use alloc::boxed::Box;
        let descriptors = Box::leak(vec![VirtqDesc::default(); size as usize].into_boxed_slice());
        
        for i in 0..(size - 1) {
            descriptors[i as usize].next = i + 1;
            descriptors[i as usize].flags = VRING_DESC_F_NEXT;
        }
        descriptors[(size - 1) as usize].next = 0;
        descriptors[(size - 1) as usize].flags = 0;

        let available = Box::leak(Box::new(VirtqAvail::default()));
        let used = Box::leak(Box::new(unsafe { core::mem::zeroed::<VirtqUsed>() }));

        VirtIOQueue {
            descriptors,
            available,
            used,
            size,
            index: 0,
            last_used: 0,
        }
    }

    pub fn pfn(&self) -> u32 {
        let virt_addr = VirtAddr::from_ptr(self.descriptors.as_ptr());
        let phys_addr = crate::memory::virt_to_phys(virt_addr).expect("Virtq Phys failed");
        (phys_addr.as_u64() >> 12) as u32
    }
}

pub struct VirtIONet {
    pub base_addr: u16,
    pub rx_queue: VirtIOQueue,
    pub tx_queue: VirtIOQueue,
    pub mac_addr: [u8; 6],
}

impl VirtIONet {
    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_addr
    }

    pub fn new(base_addr: u16) -> Self {
        use x86_64::instructions::port::Port;
        let mut status_port = Port::<u8>::new(base_addr + REG_DEVICE_STATUS as u16);
        
        unsafe { status_port.write(0); }
        unsafe { status_port.write(STATUS_ACK | STATUS_DRIVER); }
        
        let mut queue_sel_port = Port::<u16>::new(base_addr + REG_QUEUE_SEL as u16);
        let mut queue_size_port = Port::<u16>::new(base_addr + REG_QUEUE_SIZE as u16);
        let mut queue_pfn_port = Port::<u32>::new(base_addr + REG_QUEUE_PFN as u16);

        unsafe { queue_sel_port.write(0); }
        let rx_size = unsafe { queue_size_port.read() };
        let rx_queue = VirtIOQueue::new(rx_size);
        unsafe { queue_pfn_port.write(rx_queue.pfn()); }

        unsafe { queue_sel_port.write(1); }
        let tx_size = unsafe { queue_size_port.read() };
        let tx_queue = VirtIOQueue::new(tx_size);
        unsafe { queue_pfn_port.write(tx_queue.pfn()); }

        let mut mac = [0u8; 6];
        for i in 0..6 {
            let mut port = Port::<u8>::new(base_addr + 0x14 + i as u16);
            mac[i] = unsafe { port.read() };
        }

        crate::println!("VirtIO-Net MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);

        unsafe { status_port.write(STATUS_ACK | STATUS_DRIVER | STATUS_DRIVER_OK); }

        let mut nic = VirtIONet {
            base_addr,
            rx_queue,
            tx_queue,
            mac_addr: mac,
        };

        nic.populate_rx();
        nic
    }

    fn populate_rx(&mut self) {
        use alloc::boxed::Box;
        for i in 0..self.rx_queue.size {
            let buf: &'static mut [u8] = Box::leak(vec![0u8; 1526].into_boxed_slice());
            let phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(buf.as_ptr())).unwrap();
            
            self.rx_queue.descriptors[i as usize].addr = phys.as_u64();
            self.rx_queue.descriptors[i as usize].len = 1526;
            self.rx_queue.descriptors[i as usize].flags = VRING_DESC_F_WRITE;
            
            self.rx_queue.available.ring[i as usize] = i;
        }
        self.rx_queue.available.idx = self.rx_queue.size;
    }

    pub fn send_packet(&mut self, data: &[u8]) {
        use x86_64::instructions::port::Port;
        use alloc::boxed::Box;

        let mut packet = vec![0u8; 10 + data.len()];
        packet[10..].copy_from_slice(data);
        
        let leaked_pkt: &'static mut [u8] = Box::leak(packet.into_boxed_slice());
        let phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(leaked_pkt.as_ptr())).unwrap();

        let head = self.tx_queue.index % self.tx_queue.size;
        self.tx_queue.descriptors[head as usize].addr = phys.as_u64();
        self.tx_queue.descriptors[head as usize].len = leaked_pkt.len() as u32;
        self.tx_queue.descriptors[head as usize].flags = 0;

        self.tx_queue.available.ring[self.tx_queue.available.idx as usize % self.tx_queue.size as usize] = head;
        self.tx_queue.available.idx += 1;

        let mut notify_port = Port::<u16>::new(self.base_addr + REG_QUEUE_NOTIFY as u16);
        unsafe { notify_port.write(1); }

        self.tx_queue.index += 1;
    }

    pub fn receive_packet(&mut self) -> Option<Vec<u8>> {
        let last_used = self.rx_queue.last_used;
        let current_used = self.rx_queue.used.idx;

        if last_used != current_used {
            let elem = &self.rx_queue.used.ring[last_used as usize % self.rx_queue.size as usize];
            let desc_idx = elem.id as usize;
            let len = elem.len as usize;

            let desc = &self.rx_queue.descriptors[desc_idx];
            let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
            let virt_addr = offset + desc.addr;
            
            let result_len = if len > 10 { len - 10 } else { 0 };
            let mut data = vec![0u8; result_len];
            unsafe {
                core::ptr::copy_nonoverlapping((virt_addr + 10) as *const u8, data.as_mut_ptr(), result_len);
            }

            self.rx_queue.available.ring[self.rx_queue.available.idx as usize % self.rx_queue.size as usize] = desc_idx as u16;
            self.rx_queue.available.idx += 1;
            self.rx_queue.last_used = last_used.wrapping_add(1);

            return Some(data);
        }

        None
    }
}

pub struct VirtIONetDevice {
    pub inner: Arc<Mutex<VirtIONet>>,
}

impl Device for VirtIONetDevice {
    type RxToken<'a> = VirtIORxToken where Self: 'a;
    type TxToken<'a> = VirtIOTxToken<'a> where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let packet = self.inner.lock().receive_packet()?;
        Some((VirtIORxToken { buffer: packet }, VirtIOTxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(VirtIOTxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.checksum = ChecksumCapabilities::ignored();
        caps
    }
}

pub struct VirtIORxToken {
    buffer: Vec<u8>,
}

impl RxToken for VirtIORxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.buffer)
    }
}

pub struct VirtIOTxToken<'a> {
    device: &'a mut VirtIONetDevice,
}

impl<'a> TxToken for VirtIOTxToken<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        self.device.inner.lock().send_packet(&buffer);
        result
    }
}
