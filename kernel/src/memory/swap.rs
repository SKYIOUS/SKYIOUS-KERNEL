use spin::Mutex;
use hashbrown::HashMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;

pub const SWAP_SIGNATURE: u32 = 0x534B5953;
const SECTOR_SIZE: usize = 512;

#[derive(Clone, Copy, Debug)]
pub struct SwapSlot {
    pub used: bool,
}

impl SwapSlot {
    pub fn new() -> Self {
        SwapSlot { used: false }
    }
    pub fn mark_used(&mut self) {
        self.used = true;
    }
}

pub struct SwapDevice {
    pub device_path: alloc::string::String,
    pub dev_node: Arc<dyn crate::vfs::VfsNode>,
    pub slot_count: usize,
    pub slots: Vec<SwapSlot>,
    pub page_count: AtomicU64,
}

pub static SWAP_DEVICES: Mutex<Vec<SwapDevice>> = Mutex::new(Vec::new());

// virt_page_addr → (device_idx, slot_idx)
// virt_page_addr is the page-aligned virtual address
lazy_static! {
    pub static ref SWAP_PAGE_MAP: Mutex<HashMap<u64, (usize, usize)>> = Mutex::new(HashMap::new());
}

fn swap_read(device: &SwapDevice, _slot: usize, buffer: &mut [u8; 4096]) -> Result<(), ()> {
    // ponytail: sequential VFS read — real swap needs seekable block I/O
    let data = device.dev_node.read(4096).map_err(|_| ())?;
    let len = data.len().min(4096);
    buffer[..len].copy_from_slice(&data[..len]);
    if len < 4096 {
        buffer[len..].fill(0);
    }
    Ok(())
}

fn swap_write(device: &SwapDevice, _slot: usize, buffer: &[u8; 4096]) -> Result<(), ()> {
    // ponytail: sequential VFS write — real swap needs seekable block I/O
    device.dev_node.write(buffer).map_err(|_| ())
}

pub fn allocate_swap_slot(dev_idx: usize) -> Option<usize> {
    let mut devices = SWAP_DEVICES.lock();
    let device = devices.get_mut(dev_idx)?;
    for (i, slot) in device.slots.iter_mut().enumerate() {
        if !slot.used {
            slot.used = true;
            device.page_count.fetch_add(1, Ordering::Relaxed);
            return Some(i);
        }
    }
    None
}

pub fn free_swap_slot(dev_idx: usize, slot_idx: usize) {
    let mut devices = SWAP_DEVICES.lock();
    if let Some(device) = devices.get_mut(dev_idx) {
        if let Some(slot) = device.slots.get_mut(slot_idx) {
            slot.used = false;
            device.page_count.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

pub fn swap_out_page(virt_page_addr: u64, phys_addr: u64) -> bool {
    let mut devs = SWAP_DEVICES.lock();
    if devs.is_empty() {
        return false;
    }
    for dev_idx in 0..devs.len() {
        let slot_count = devs[dev_idx].slot_count;
        for s in 0..slot_count {
            if devs[dev_idx].slots.get(s).map_or(true, |sl| sl.used) {
                continue;
            }
            devs[dev_idx].slots[s].used = true;
            devs[dev_idx].page_count.fetch_add(1, Ordering::Relaxed);
            drop(devs);

            let mut buffer = [0u8; 4096];
            let phys_offset = crate::memory::physical_memory_offset();
            let src = (phys_offset + phys_addr) as *const u8;
            // SAFETY: phys_addr is a valid mapped physical address from the frame allocator
            unsafe {
                core::ptr::copy_nonoverlapping(src, buffer.as_mut_ptr(), 4096);
            }

            let devs2 = SWAP_DEVICES.lock();
            if let Some(d) = devs2.get(dev_idx) {
                if swap_write(d, s, &buffer).is_err() {
                    drop(devs2);
                    let mut devs3 = SWAP_DEVICES.lock();
                    if let Some(d3) = devs3.get_mut(dev_idx) {
                        if let Some(sl) = d3.slots.get_mut(s) {
                            sl.used = false;
                            d3.page_count.fetch_sub(1, Ordering::Relaxed);
                        }
                    }
                    return false;
                }
                drop(devs2);
            }
            SWAP_PAGE_MAP.lock().insert(virt_page_addr, (dev_idx, s));
            return true;
        }
    }
    false
}

pub fn swap_in_page(virt_page_addr: u64) -> Option<u64> {
    let slot = SWAP_PAGE_MAP.lock().remove(&virt_page_addr)?;
    let (dev_idx, slot_idx) = slot;

    let devices = SWAP_DEVICES.lock();
    let device = devices.get(dev_idx)?;
    let mut buffer = [0u8; 4096];
    swap_read(device, slot_idx, &mut buffer).ok()?;
    drop(devices);

    let mut buddy = crate::memory::buddy::BUDDY_ALLOCATOR.lock();
    let frame = buddy.allocate_frame()?;
    drop(buddy);

    let phys_addr = frame.start_address().as_u64();
    let phys_offset = crate::memory::physical_memory_offset();
    let dst = (phys_offset + phys_addr) as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(buffer.as_ptr(), dst, 4096);
    }

    free_swap_slot(dev_idx, slot_idx);

    crate::memory::frame_info::increment(frame.start_address());
    Some(phys_addr)
}

pub fn try_evict_one_page() -> bool {
    let proc = match crate::task::process::CURRENT_PROCESS.lock().as_ref() {
        Some(p) => p.clone(),
        None => return false,
    };

    let vmas = proc.vmas.lock();
    for vma in vmas.iter() {
        if vma.start >= 0xFFFF_8000_0000_0000 {
            continue;
        }

        let page_addr = vma.start;
        let virt = x86_64::VirtAddr::new(page_addr);
        let page: x86_64::structures::paging::Page<x86_64::structures::paging::Size4KiB> = x86_64::structures::paging::Page::containing_address(virt);

        // SAFETY: mapper is valid through physical memory offset
        let mapper = unsafe { proc.address_space.mapper() };
        if mapper.is_none() {
            continue;
        }
        let mut mapper = unsafe { mapper.unwrap_unchecked() };

        use x86_64::structures::paging::Translate;
        match mapper.translate(virt) {
            x86_64::structures::paging::mapper::TranslateResult::Mapped { frame, flags, .. } => {
                if !flags.contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
                    continue;
                }
                if crate::memory::frame_info::count(frame.start_address()) != 1 {
                    continue;
                }

                let phys_addr = frame.start_address().as_u64();
                if swap_out_page(page_addr, phys_addr) {
                    drop(vmas);

                    use x86_64::structures::paging::Mapper;
                    if let Ok((oframe, t)) = mapper.unmap(page) {
                        t.flush();
                        crate::memory::frame_info::decrement(oframe.start_address());
                    }

                    use x86_64::instructions::tlb;
                    tlb::flush(virt);
                    #[cfg(feature = "smp")]
                    crate::smp::broadcast_tlb_flush(page_addr);

                    return true;
                }
            }
            _ => continue,
        }
    }
    false
}
