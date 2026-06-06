//! VirtIO Block Device Driver
//!
//! Provides optimized block storage access in virtualized environments.

use crate::drivers::block::{BlockDevice, BlockDeviceError};
use crate::drivers::net::virtio::{VirtIOQueue, REG_DEVICE_STATUS, REG_QUEUE_SEL, REG_QUEUE_SIZE, REG_QUEUE_PFN, REG_QUEUE_NOTIFY, STATUS_ACK, STATUS_DRIVER, STATUS_DRIVER_OK};
use x86_64::VirtAddr;
use x86_64::instructions::port::Port;
use alloc::boxed::Box;

pub const VIRTIO_BLOCK_DEVICE_ID: u16 = 0x1001;

#[repr(C, packed)]
struct VirtioBlockHeader {
    pub type_: u32,
    pub ioprio: u32,
    pub sector: u64,
}

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;

pub struct VirtIOBlock {
    pub base_addr: u16,
    pub queue: VirtIOQueue,
    pub capacity: u64,
}

impl VirtIOBlock {
    pub fn new(base_addr: u16) -> Self {
        let mut status_port = Port::<u8>::new(base_addr + REG_DEVICE_STATUS as u16);
        
        unsafe { status_port.write(0); }
        unsafe { status_port.write(STATUS_ACK | STATUS_DRIVER); }
        
        let mut queue_sel_port = Port::<u16>::new(base_addr + REG_QUEUE_SEL as u16);
        let mut queue_size_port = Port::<u16>::new(base_addr + REG_QUEUE_SIZE as u16);
        let mut queue_pfn_port = Port::<u32>::new(base_addr + REG_QUEUE_PFN as u16);

        unsafe { queue_sel_port.write(0); }
        let size = unsafe { queue_size_port.read() };
        let queue = VirtIOQueue::new(size);
        unsafe { queue_pfn_port.write(queue.pfn()); }

        // Read capacity (sectors) - VirtIO spec: 64-bit capacity at 0x14
        let mut cap_port_low = Port::<u32>::new(base_addr + 0x14);
        let mut cap_port_high = Port::<u32>::new(base_addr + 0x18);
        let capacity = unsafe { (cap_port_low.read() as u64) | ((cap_port_high.read() as u64) << 32) };

        crate::println!("VirtIO-Block: Capacity: {} sectors ({} MB)", capacity, (capacity * 512) / (1024 * 1024));

        unsafe { status_port.write(STATUS_ACK | STATUS_DRIVER | STATUS_DRIVER_OK); }

        VirtIOBlock {
            base_addr,
            queue,
            capacity,
        }
    }
}

impl BlockDevice for VirtIOBlock {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        if buf.len() != 512 { return Err(BlockDeviceError::ReadError); }

        // 1. Prepare Header
        let header = Box::new(VirtioBlockHeader {
            type_: VIRTIO_BLK_T_IN,
            ioprio: 0,
            sector,
        });
        let header_ptr = Box::into_raw(header);
        let header_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(header_ptr)).unwrap();

        // 2. Prepare Status byte
        let status = Box::new(0u8);
        let status_ptr = Box::into_raw(status);
        let status_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(status_ptr)).unwrap();

        let data_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(buf.as_ptr())).unwrap();

        // 3. Fill descriptors (Header -> Data -> Status)
        let head = (self.queue.index % self.queue.size) as usize;
        
        // Descriptor 0: Header (Read-only for device)
        self.queue.descriptors[head].addr = header_phys.as_u64();
        self.queue.descriptors[head].len = core::mem::size_of::<VirtioBlockHeader>() as u32;
        self.queue.descriptors[head].flags = 1; // F_NEXT
        self.queue.descriptors[head].next = ((head + 1) % self.queue.size as usize) as u16;

        // Descriptor 1: Data (Write-only for device)
        let data_idx = (head + 1) % self.queue.size as usize;
        self.queue.descriptors[data_idx].addr = data_phys.as_u64();
        self.queue.descriptors[data_idx].len = 512;
        self.queue.descriptors[data_idx].flags = 1 | 2; // F_NEXT | F_WRITE
        self.queue.descriptors[data_idx].next = ((head + 2) % self.queue.size as usize) as u16;

        // Descriptor 2: Status (Write-only for device)
        let status_idx = (head + 2) % self.queue.size as usize;
        self.queue.descriptors[status_idx].addr = status_phys.as_u64();
        self.queue.descriptors[status_idx].len = 1;
        self.queue.descriptors[status_idx].flags = 2; // F_WRITE
        self.queue.descriptors[status_idx].next = 0;

        // 4. Update Available Ring
        self.queue.available.ring[self.queue.available.idx as usize % self.queue.size as usize] = head as u16;
        self.queue.available.idx += 1;

        // 5. Notify
        let mut notify_port = Port::<u16>::new(self.base_addr + REG_QUEUE_NOTIFY as u16);
        unsafe { notify_port.write(0); }

        self.queue.index = (self.queue.index + 3) % self.queue.size;

        // 6. Poll for completion
        while self.queue.last_used == self.queue.used.idx {
            core::hint::spin_loop();
        }
        self.queue.last_used += 1;

        let status = unsafe { Box::from_raw(status_ptr) };
        let _header = unsafe { Box::from_raw(header_ptr) };

        if *status == 0 { Ok(()) } else { Err(BlockDeviceError::ReadError) }
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        if buf.len() != 512 { return Err(BlockDeviceError::WriteError); }

        // 1. Prepare Header
        let header = Box::new(VirtioBlockHeader {
            type_: VIRTIO_BLK_T_OUT,
            ioprio: 0,
            sector,
        });
        let header_ptr = Box::into_raw(header);
        let header_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(header_ptr)).unwrap();

        // 2. Prepare Status byte
        let status = Box::new(0u8);
        let status_ptr = Box::into_raw(status);
        let status_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(status_ptr)).unwrap();

        let data_phys = crate::memory::virt_to_phys(VirtAddr::from_ptr(buf.as_ptr())).unwrap();

        // 3. Fill descriptors
        let head = (self.queue.index % self.queue.size) as usize;
        
        self.queue.descriptors[head].addr = header_phys.as_u64();
        self.queue.descriptors[head].len = core::mem::size_of::<VirtioBlockHeader>() as u32;
        self.queue.descriptors[head].flags = 1;
        self.queue.descriptors[head].next = ((head + 1) % self.queue.size as usize) as u16;

        let data_idx = (head + 1) % self.queue.size as usize;
        self.queue.descriptors[data_idx].addr = data_phys.as_u64();
        self.queue.descriptors[data_idx].len = 512;
        self.queue.descriptors[data_idx].flags = 1; // F_NEXT (Read-only for device during write)
        self.queue.descriptors[data_idx].next = ((head + 2) % self.queue.size as usize) as u16;

        let status_idx = (head + 2) % self.queue.size as usize;
        self.queue.descriptors[status_idx].addr = status_phys.as_u64();
        self.queue.descriptors[status_idx].len = 1;
        self.queue.descriptors[status_idx].flags = 2; // F_WRITE
        self.queue.descriptors[status_idx].next = 0;

        self.queue.available.ring[self.queue.available.idx as usize % self.queue.size as usize] = head as u16;
        self.queue.available.idx += 1;

        let mut notify_port = Port::<u16>::new(self.base_addr + REG_QUEUE_NOTIFY as u16);
        unsafe { notify_port.write(0); }

        self.queue.index = (self.queue.index + 3) % self.queue.size;

        while self.queue.last_used == self.queue.used.idx {
            core::hint::spin_loop();
        }
        self.queue.last_used += 1;

        let status = unsafe { Box::from_raw(status_ptr) };
        let _header = unsafe { Box::from_raw(header_ptr) };

        if *status == 0 { Ok(()) } else { Err(BlockDeviceError::WriteError) }
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        Ok(self.capacity)
    }
}
