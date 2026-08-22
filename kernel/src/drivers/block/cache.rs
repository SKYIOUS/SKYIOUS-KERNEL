use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::IrqSafeMutex as Mutex;
use super::{BlockDevice, BlockDeviceError};

const CACHE_SIZE: usize = 256;
const SECTOR_SIZE: usize = 512;

struct CacheLine {
    sector: u64,
    data: [u8; SECTOR_SIZE],
    dirty: bool,
    valid: bool,
}

pub struct BlockCache {
    inner: Arc<Mutex<dyn BlockDevice>>,
    lines: Mutex<Vec<CacheLine>>,
    access_clock: AtomicU64,
}

impl BlockCache {
    pub fn new(inner: Arc<Mutex<dyn BlockDevice>>) -> Self {
        let mut lines = Vec::with_capacity(CACHE_SIZE);
        for _ in 0..CACHE_SIZE {
            lines.push(CacheLine {
                sector: 0,
                data: [0u8; SECTOR_SIZE],
                dirty: false,
                valid: false,
            });
        }
        BlockCache {
            inner,
            lines: Mutex::new(lines),
            access_clock: AtomicU64::new(0),
        }
    }

    fn evict_one(&self) -> Option<usize> {
        let mut lines = self.lines.lock();
        let clock = self.access_clock.fetch_add(1, Ordering::Relaxed);
        let i = (clock as usize) % CACHE_SIZE;
        if !lines[i].valid {
            return Some(i);
        }
        if lines[i].dirty {
            let sector = lines[i].sector;
            let data = lines[i].data;
            let mut dev = self.inner.lock();
            let _ = dev.write_sector(sector, &data);
            lines[i].dirty = false;
        }
        lines[i].valid = false;
        Some(i)
    }

    fn fetch_sector(&self, sector: u64, slot: usize) -> Result<(), BlockDeviceError> {
        let mut lines = self.lines.lock();
        let mut dev = self.inner.lock();
        dev.read_sector(sector, &mut lines[slot].data)?;
        lines[slot].sector = sector;
        lines[slot].dirty = false;
        lines[slot].valid = true;
        Ok(())
    }

    pub fn read_sector_cached(&self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        if buf.len() < SECTOR_SIZE { return Err(BlockDeviceError::DeviceError); }
        // Accept buffers spanning multiple sectors (e.g. FS block reads of
        // 1024/4096 bytes). Each 512-byte chunk is served via the cache.
        let n = buf.len() / SECTOR_SIZE;
        for i in 0..n {
            let chunk = &mut buf[i * SECTOR_SIZE..(i + 1) * SECTOR_SIZE];
            self.read_chunk(sector + i as u64, chunk)?;
        }
        Ok(())
    }

    /// Read exactly one sector-sized chunk through the cache.
    fn read_chunk(&self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        let lines = self.lines.lock();
        // Check cache under lock — no TOCTOU race with eviction
        if let Some(i) = lines.iter().position(|l| l.valid && l.sector == sector) {
            buf.copy_from_slice(&lines[i].data);
            return Ok(());
        }
        drop(lines);
        let slot = self.evict_one().ok_or(BlockDeviceError::DeviceError)?;
        self.fetch_sector(sector, slot)?;
        let lines = self.lines.lock();
        buf.copy_from_slice(&lines[slot].data);
        Ok(())
    }

    pub fn write_sector_cached(&self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        if buf.len() < SECTOR_SIZE { return Err(BlockDeviceError::InvalidSector); }
        let n = buf.len() / SECTOR_SIZE;
        for i in 0..n {
            let chunk = &buf[i * SECTOR_SIZE..(i + 1) * SECTOR_SIZE];
            self.write_chunk(sector + i as u64, chunk)?;
        }
        Ok(())
    }

    /// Write exactly one sector-sized chunk through the cache.
    fn write_chunk(&self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        let mut lines = self.lines.lock();
        // Check cache under lock — no TOCTOU race with eviction
        if let Some(i) = lines.iter().position(|l| l.valid && l.sector == sector) {
            lines[i].data.copy_from_slice(buf);
            lines[i].dirty = true;
            return Ok(());
        }
        drop(lines);
        let slot = self.evict_one().ok_or(BlockDeviceError::DeviceError)?;
        let mut lines = self.lines.lock();
        lines[slot].data.copy_from_slice(buf);
        lines[slot].sector = sector;
        lines[slot].dirty = true;
        lines[slot].valid = true;
        Ok(())
    }

    pub fn sync(&self) {
        let mut lines = self.lines.lock();
        let mut dev = self.inner.lock();
        for line in lines.iter_mut() {
            if line.dirty {
                let _ = dev.write_sector(line.sector, &line.data);
                line.dirty = false;
            }
        }
    }
}

impl BlockDevice for BlockCache {
    fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError> {
        self.read_sector_cached(sector, buf)
    }

    fn write_sector(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError> {
        self.write_sector_cached(sector, buf)
    }

    fn sector_count(&self) -> Result<u64, BlockDeviceError> {
        self.inner.lock().sector_count()
    }

    fn sync(&mut self) {
        let cache: &BlockCache = self;
        cache.sync();
    }
}
