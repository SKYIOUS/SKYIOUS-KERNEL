use x86_64::instructions::port::Port;
use alloc::sync::Arc;
use crate::sync::IrqSafeMutex as Mutex;
use crate::drivers::block::{BlockDevice, BlockDeviceError, register_block_device};

const DATA: u16 = 0x1F0;
const SECTOR_COUNT: u16 = 0x1F2;
const LBA_LO: u16 = 0x1F3;
const LBA_MID: u16 = 0x1F4;
const LBA_HI: u16 = 0x1F5;
const DRIVE_SEL: u16 = 0x1F6;
const COMMAND: u16 = 0x1F7;
const STATUS: u16 = 0x1F7;

const CMD_IDENTIFY: u8 = 0xEC;
const CMD_READ_SECTORS: u8 = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const CMD_READ_SECTORS_EXT: u8 = 0x24;
const CMD_WRITE_SECTORS_EXT: u8 = 0x34;

const STATUS_BSY: u8 = 0x80;
const STATUS_DRQ: u8 = 0x08;
const STATUS_ERR: u8 = 0x01;

pub struct PataDevice {
    sector_count: u64,
}

impl PataDevice {
    const fn new() -> Self {
        PataDevice { sector_count: 0 }
    }

    fn wait_bsy(&self) -> Result<(), BlockDeviceError> {
        let mut status = Port::<u8>::new(STATUS);
        for _ in 0..1_000_000 {
            unsafe {
                if status.read() & STATUS_BSY == 0 {
                    return Ok(());
                }
            }
            core::hint::spin_loop();
        }
        Err(BlockDeviceError::DeviceError)
    }

    fn wait_drq(&self) -> Result<(), BlockDeviceError> {
        let mut status = Port::<u8>::new(STATUS);
        for _ in 0..1_000_000 {
            unsafe {
                let s = status.read();
                if s & STATUS_BSY == 0 {
                    if s & STATUS_DRQ != 0 {
                        return Ok(());
                    }
                    if s & STATUS_ERR != 0 {
                        return Err(BlockDeviceError::ReadError);
                    }
                    return Err(BlockDeviceError::DeviceError);
                }
            }
            core::hint::spin_loop();
        }
        Err(BlockDeviceError::DeviceError)
    }

    fn poll_ready(&self) -> Result<(), BlockDeviceError> {
        let mut status = Port::<u8>::new(STATUS);
        for _ in 0..1_000_000 {
            unsafe {
                let s = status.read();
                if s & STATUS_BSY == 0 {
                    if s & STATUS_ERR != 0 {
                        return Err(BlockDeviceError::ReadError);
                    }
                    return Ok(());
                }
            }
            core::hint::spin_loop();
        }
        Err(BlockDeviceError::DeviceError)
    }
}

impl BlockDevice for PataDevice {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        self.wait_bsy()?;
        unsafe {
            if sector > 0x0FFFFFFF {
                let mut sc = Port::<u8>::new(SECTOR_COUNT);
                sc.write(0);
                let mut lba_lo = Port::<u8>::new(LBA_LO);
                lba_lo.write(((sector >> 24) & 0xFF) as u8);
                let mut lba_mid = Port::<u8>::new(LBA_MID);
                lba_mid.write(((sector >> 32) & 0xFF) as u8);
                let mut lba_hi = Port::<u8>::new(LBA_HI);
                lba_hi.write(((sector >> 40) & 0xFF) as u8);
                sc.write(1);
                lba_lo.write((sector & 0xFF) as u8);
                lba_mid.write(((sector >> 8) & 0xFF) as u8);
                lba_hi.write(((sector >> 16) & 0xFF) as u8);
                Port::<u8>::new(DRIVE_SEL).write(0xE0);
                Port::<u8>::new(COMMAND).write(CMD_READ_SECTORS_EXT);
            } else {
                Port::<u8>::new(DRIVE_SEL).write(0xE0 | ((sector >> 24) as u8 & 0x0F));
                Port::<u8>::new(SECTOR_COUNT).write(1);
                Port::<u8>::new(LBA_LO).write((sector & 0xFF) as u8);
                Port::<u8>::new(LBA_MID).write(((sector >> 8) & 0xFF) as u8);
                Port::<u8>::new(LBA_HI).write(((sector >> 16) & 0xFF) as u8);
                Port::<u8>::new(COMMAND).write(CMD_READ_SECTORS);
            }
            self.poll_ready()?;
            let mut data = Port::<u16>::new(DATA);
            for word in buf.chunks_exact_mut(2) {
                let val = data.read();
                word[0] = (val & 0xFF) as u8;
                word[1] = ((val >> 8) & 0xFF) as u8;
            }
        }
        Ok(())
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        self.wait_bsy()?;
        unsafe {
            if sector > 0x0FFFFFFF {
                let mut sc = Port::<u8>::new(SECTOR_COUNT);
                sc.write(0);
                let mut lba_lo = Port::<u8>::new(LBA_LO);
                lba_lo.write(((sector >> 24) & 0xFF) as u8);
                let mut lba_mid = Port::<u8>::new(LBA_MID);
                lba_mid.write(((sector >> 32) & 0xFF) as u8);
                let mut lba_hi = Port::<u8>::new(LBA_HI);
                lba_hi.write(((sector >> 40) & 0xFF) as u8);
                sc.write(1);
                lba_lo.write((sector & 0xFF) as u8);
                lba_mid.write(((sector >> 8) & 0xFF) as u8);
                lba_hi.write(((sector >> 16) & 0xFF) as u8);
                Port::<u8>::new(DRIVE_SEL).write(0xE0);
                Port::<u8>::new(COMMAND).write(CMD_WRITE_SECTORS_EXT);
            } else {
                Port::<u8>::new(DRIVE_SEL).write(0xE0 | ((sector >> 24) as u8 & 0x0F));
                Port::<u8>::new(SECTOR_COUNT).write(1);
                Port::<u8>::new(LBA_LO).write((sector & 0xFF) as u8);
                Port::<u8>::new(LBA_MID).write(((sector >> 8) & 0xFF) as u8);
                Port::<u8>::new(LBA_HI).write(((sector >> 16) & 0xFF) as u8);
                Port::<u8>::new(COMMAND).write(CMD_WRITE_SECTORS);
            }
            self.wait_drq()?;
            let mut data = Port::<u16>::new(DATA);
            for word in buf.chunks_exact(2) {
                let val = (word[1] as u16) << 8 | word[0] as u16;
                data.write(val);
            }
            self.poll_ready()?;
        }
        Ok(())
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        Ok(self.sector_count)
    }
}

fn probe_master() -> Option<PataDevice> {
    unsafe {
        let mut drive_sel = Port::<u8>::new(DRIVE_SEL);
        drive_sel.write(0xA0);
        for _ in 0..100_000 { core::hint::spin_loop(); }

        let mut status = Port::<u8>::new(STATUS);
        let mut timeout = 100_000i32;
        while timeout > 0 {
            if status.read() & STATUS_BSY == 0 { break; }
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout <= 0 { return None; }

        let mut cmd = Port::<u8>::new(COMMAND);
        cmd.write(CMD_IDENTIFY);

        if status.read() == 0 { return None; }

        let mut timeout = 1_000_000i32;
        while timeout > 0 {
            if status.read() & STATUS_BSY == 0 { break; }
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout <= 0 { return None; }
        if status.read() & STATUS_ERR != 0 { return None; }

        let mut timeout = 1_000_000i32;
        while timeout > 0 {
            if status.read() & STATUS_DRQ != 0 { break; }
            timeout -= 1;
            core::hint::spin_loop();
        }
        if timeout <= 0 { return None; }

        let mut identify = [0u16; 256];
        let mut data = Port::<u16>::new(DATA);
        for word in identify.iter_mut() {
            *word = data.read();
        }

        let sectors = identify[60] as u64 | (identify[61] as u64) << 16;
        if sectors == 0 { return None; }

        crate::serial_write(&alloc::format!("[PATA] Drive detected: {} sectors ({:.1} MB)\n",
            sectors, (sectors * 512) as f64 / 1_048_576.0));
        let mut dev = PataDevice::new();
        dev.sector_count = sectors;
        Some(dev)
    }
}

pub fn init() {
    crate::serial_write("[PATA] Probing primary IDE channel...\n");
    if let Some(dev) = probe_master() {
        let wrapped = Arc::new(Mutex::new(dev));
        register_block_device(wrapped);
        crate::serial_write("[PATA] Primary master registered.\n");
    } else {
        crate::serial_write("[PATA] No device on primary master.\n");
    }
}

pub fn test_read_sector0() -> Result<(), &'static str> {
    let devices = crate::drivers::block::BLOCK_DEVICES.lock();
    if devices.is_empty() {
        return Err("no block devices registered");
    }
    let dev = devices[0].clone();
    drop(devices);
    let mut buf = [0u8; 512];
    dev.lock().read_sector(0, &mut buf).map_err(|_| "read_sector(0) failed")?;
    if buf[510] == 0x55 && buf[511] == 0xAA {
        Ok(())
    } else {
        // non-partitioned disk is fine
        Ok(())
    }
}
