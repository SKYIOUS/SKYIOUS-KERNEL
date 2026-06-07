use xmas_elf::ElfFile;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use spin::Mutex;
use crate::memory::paging::AddressSpace;
use x86_64::structures::paging::PageTableFlags;

use core::sync::atomic::{AtomicU64, Ordering};

pub static CURRENT_PROCESS: Mutex<Option<Arc<Process>>> = Mutex::new(None);

lazy_static::lazy_static! {
    pub static ref PROCESS_TABLE: Mutex<alloc::collections::BTreeMap<u64, Arc<Process>>> = Mutex::new(alloc::collections::BTreeMap::new());
}

impl Process {
    pub fn next_id() -> u64 {
        static NEXT_PROCESS_ID: AtomicU64 = AtomicU64::new(100); // Start user PIDs at 100
        NEXT_PROCESS_ID.fetch_add(1, Ordering::Relaxed)
    }
}

/// Represents a region of virtual memory.
#[derive(Debug, Clone)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub flags: PageTableFlags,
        pub _name: &'static str,
}

use smoltcp::iface::SocketHandle;

#[derive(Clone, Copy, PartialEq)]
pub enum SocketType { Tcp, Udp }

#[derive(Clone)]
pub enum FileDescriptor {
    File { node: Arc<dyn VfsNode>, offset: usize },
    Socket(SocketHandle, SocketType),
}

pub struct Process {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub address_space: AddressSpace,
    pub vmas: Mutex<Vec<Vma>>,
    pub entry_point: u64,
    pub fd_table: Mutex<Vec<Option<FileDescriptor>>>,
    pub exit_code: Mutex<Option<i32>>,
    pub children: Mutex<Vec<u64>>,
    pub brk: Mutex<u64>,
    pub cwd: Mutex<String>,
    pub signals: Mutex<crate::syscalls::signal::SignalState>,
    pub signal_handlers: Mutex<[u64; 32]>,
    pub uid: Mutex<u32>,
    pub gid: Mutex<u32>,
    pub euid: Mutex<u32>,
    pub egid: Mutex<u32>,
}

use crate::vfs::VfsNode;

impl Process {
    pub fn new(id: u64, parent_id: Option<u64>, address_space: AddressSpace) -> Self {
        Process {
            id,
            parent_id,
            address_space,
            vmas: Mutex::new(Vec::new()),
            entry_point: 0,
            fd_table: Mutex::new(Vec::new()),
            exit_code: Mutex::new(None),
            children: Mutex::new(Vec::new()),
            brk: Mutex::new(0),
            cwd: Mutex::new(String::from("/")),
            signals: Mutex::new(crate::syscalls::signal::SignalState::new()),
            signal_handlers: Mutex::new([0; 32]),
            uid: Mutex::new(0),
            gid: Mutex::new(0),
            euid: Mutex::new(0),
            egid: Mutex::new(0),
        }
    }

    pub fn add_vma(&self, new_vma: Vma) {
        let mut vmas = self.vmas.lock();
        vmas.push(new_vma);
        vmas.sort_by(|a, b| a.start.cmp(&b.start));
        self.merge_vmas_inner(&mut vmas);
    }

    /// Merge overlapping and adjacent VMAs with compatible flags.
    fn merge_vmas_inner(&self, vmas: &mut Vec<Vma>) {
        let mut i = 0;
        while i + 1 < vmas.len() {
            let can_merge = vmas[i].flags == vmas[i + 1].flags;
            let overlaps_or_adjacent = vmas[i].end >= vmas[i + 1].start;
            if can_merge && overlaps_or_adjacent {
                vmas[i].end = vmas[i].end.max(vmas[i + 1].end);
                vmas.remove(i + 1);
            } else {
                i += 1;
            }
        }
    }

    /// Remove or trim VMAs that intersect [start, end).
    /// Returns the number of pages removed from the page table (caller must handle that).
    pub fn remove_vma_range(&self, start: u64, end: u64) {
        let mut vmas = self.vmas.lock();
        let mut i = 0;
        while i < vmas.len() {
            let v = &vmas[i];
            if v.end <= start || v.start >= end {
                i += 1;
                continue;
            }
            // v overlaps [start, end)
            if v.start < start && v.end > end {
                // Middle section removed — split into two
                let right = Vma { start: end, end: v.end, flags: v.flags, _name: v._name };
                vmas[i].end = start;
                vmas.insert(i + 1, right);
                return; // no further overlap possible with this VMA after split
            }
            if v.start >= start && v.end <= end {
                // Completely covered — remove
                vmas.remove(i);
                continue;
            }
            if v.start < start && v.end <= end {
                // Trim right
                vmas[i].end = start;
                i += 1;
            } else if v.start >= start && v.end > end {
                // Trim left
                vmas[i].start = end;
                i += 1;
            }
        }
    }

    /// Coalesce the entire VMA list (merges any adjacent/overlapping VMAs with matching flags).
    pub fn merge_all_vmas(&self) {
        let mut vmas = self.vmas.lock();
        if vmas.is_empty() { return; }
        vmas.sort_by(|a, b| a.start.cmp(&b.start));
        self.merge_vmas_inner(&mut vmas);
    }

    pub fn find_vma(&self, addr: u64) -> Option<Vma> {
        let vmas = self.vmas.lock();
        vmas.iter().find(|vma| addr >= vma.start && addr < vma.end).cloned()
    }

    pub fn load_elf(elf_data: &[u8], mut address_space: AddressSpace) -> Result<Self, &'static str> {
        let (entry, vmas) = Self::load_elf_static(elf_data, &mut address_space)?;
        
        let mut process = Process::new(Process::next_id(), None, address_space);
        process.entry_point = entry;
        
        // Add VMAs via add_vma to merge adjacent/overlapping segments
        for vma in vmas {
            process.add_vma(vma);
        }

        // Merge remaining after all segments added
        process.merge_all_vmas();

        let vmas = process.vmas.lock();
        let mut initial_brk = 0;
        for vma in vmas.iter() {
            if vma.end > initial_brk {
                initial_brk = vma.end;
            }
        }
        drop(vmas);
        // Page align the initial break
        let initial_brk = (initial_brk + 4095) & !4095;
        *process.brk.lock() = initial_brk;
        Ok(process)
    }

    /// Loads an ELF into an existing AddressSpace without creating a Process yet.
    /// Returns (entry_point, vmas).
    pub fn load_elf_static(elf_data: &[u8], address_space: &mut AddressSpace) -> Result<(u64, Vec<Vma>), &'static str> {
        let elf = ElfFile::new(elf_data).map_err(|_| "Failed to parse ELF")?;
        
                        use x86_64::structures::paging::{Mapper, Page, Size4KiB, FrameAllocator, Translate};
                        use crate::memory::buddy::BuddyFrameAllocator;
                        let mut frame_allocator = BuddyFrameAllocator;
        let mut mapper = unsafe { address_space.mapper().ok_or("Failed to get mapper")? };

        let entry_point = elf.header.pt2.entry_point();
        let mut vmas = Vec::new();
        
        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
                let virt_start = ph.virtual_addr();
                let file_size = ph.file_size();
                let mem_size = ph.mem_size();
                let offset = ph.offset() as usize;

                let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
                if ph.flags().is_write() { flags |= PageTableFlags::WRITABLE; }
                if !ph.flags().is_execute() { flags |= PageTableFlags::NO_EXECUTE; }

                // Define VMA
                vmas.push(Vma {
                    start: virt_start,
                    end: virt_start + mem_size,
                    flags,
                    _name: "elf_phdr",
                });

                // Map and Copy
                let start_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(virt_start));
                let end_page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(virt_start + mem_size - 1));
                
                for page in Page::range_inclusive(start_page, end_page) {
                    let map_flags = flags | PageTableFlags::WRITABLE;
                    let mut was_mapped = true;
                    let frame = match mapper.translate_page(page) {
                        Ok(f) => {
                            // Page already mapped from a previous overlapping segment.
                            // Get current flags and add WRITABLE for the copy.
                            let addr = page.start_address();
                            let old_flags = match mapper.translate(addr) {
                                x86_64::structures::paging::mapper::TranslateResult::Mapped { flags, .. } => flags,
                                _ => map_flags,
                            };
                            unsafe {
                                let _ = mapper.update_flags(page, old_flags | PageTableFlags::WRITABLE);
                            }
                            f
                        }
                        Err(_) => {
                            was_mapped = false;
                            let f = frame_allocator.allocate_frame().ok_or("Out of memory during ELF load")?;
                            unsafe {
                                mapper.map_to(page, f, map_flags, &mut frame_allocator)
                                    .map_err(|_| "Failed to map ELF page")?.flush();
                            }
                            crate::memory::frame_info::increment(f.start_address());
                            f
                        }
                    };

                    let page_start = page.start_address().as_u64();
                    let offset_in_segment = if page_start > virt_start { page_start - virt_start } else { 0 };
                    let copy_start = virt_start + offset_in_segment;
                    let copy_end = core::cmp::min(virt_start + file_size, page_start + 4096);
                    
                    if copy_start < copy_end {
                        let len = copy_end - copy_start;
                        let src_off = offset + (copy_start - virt_start) as usize;
                        unsafe {
                            let dst_ptr = (x86_64::VirtAddr::new(*crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap()) + frame.start_address().as_u64()).as_mut_ptr::<u8>();
                            let page_offset = if page_start > virt_start { 0 } else { virt_start - page_start };
                            core::ptr::copy_nonoverlapping(
                                elf_data[src_off..src_off + len as usize].as_ptr(),
                                dst_ptr.add(page_offset as usize),
                                len as usize
                            );
                        }
                    }

                    // Set final flags only for freshly mapped pages.
                    // Overlapping pages keep RWX to satisfy all segments.
                    if !was_mapped {
                        unsafe {
                            mapper.update_flags(page, flags).map_err(|_| "Failed to update flags")?.flush();
                        }
                    }
                }
            }
        }
        
        // Apply R_X86_64_RELATIVE relocations from PT_DYNAMIC
        for ph in elf.program_iter() {
            if let Ok(xmas_elf::program::Type::Dynamic) = ph.get_type() {
                let dyn_off = ph.offset() as usize;
                let dyn_filesz = ph.file_size() as usize;
                let dyn_data = &elf_data[dyn_off..dyn_off + dyn_filesz];

                let mut rela_vaddr = 0u64;
                let mut rela_size = 0u64;
                let num_dyn = dyn_data.len() / 16;
                for i in 0..num_dyn {
                    unsafe {
                        let entry = dyn_data.as_ptr().add(i * 16) as *const u64;
                        let tag = *entry as i64;
                        let val = *entry.add(1);
                        if tag == 7 { rela_vaddr = val; }
                        else if tag == 8 { rela_size = val; }
                    }
                }

                if rela_vaddr != 0 && rela_size != 0 {
                    let mut rela_file_off = 0u64;
                    for ph2 in elf.program_iter() {
                        if let Ok(xmas_elf::program::Type::Load) = ph2.get_type() {
                            let seg_start = ph2.virtual_addr();
                            let seg_end = seg_start + ph2.file_size();
                            if rela_vaddr >= seg_start && rela_vaddr < seg_end {
                                rela_file_off = ph2.offset() + (rela_vaddr - seg_start);
                                break;
                            }
                        }
                    }

                    if rela_file_off != 0 || rela_vaddr == 0 {
                        let rela_end = (rela_file_off as usize + rela_size as usize).min(elf_data.len());
                        let rela_data = &elf_data[rela_file_off as usize..rela_end];
                        let num_rela = rela_data.len() / 24;
                        for i in 0..num_rela {
                            unsafe {
                                let entry = rela_data.as_ptr().add(i * 24) as *const u64;
                                let r_offset = *entry;
                                let r_info = *entry.add(1);
                                let r_addend = *entry.add(2) as i64;
                                let r_type = (r_info & 0xffffffff) as u32;

                                if r_type == 8 {
                                    let target_va = x86_64::VirtAddr::new(r_offset);
                                    use x86_64::structures::paging::mapper::TranslateResult;
                                    if let TranslateResult::Mapped { frame, offset, .. } = mapper.translate(target_va) {
                                        let phys_addr = frame.start_address() + offset;
                                        let kaddr = x86_64::VirtAddr::new(
                                            *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + phys_addr.as_u64()
                                        );
                                        *(kaddr.as_mut_ptr::<u64>()) = r_addend as u64;
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }

        Ok((entry_point, vmas))
    }

    pub fn register(process: Arc<Process>) {
        PROCESS_TABLE.lock().insert(process.id, process.clone());
    }

    /// PHASE D2: User stack setup in execve
    /// Maps 64KB stack at 0x7FFF_FFFF_E000 and populates argc/argv.
    pub fn setup_user_stack(&self, argv: &[alloc::string::String]) -> u64 {
                        use x86_64::structures::paging::{Mapper, Page, Size4KiB, PageTableFlags, FrameAllocator};
                        use crate::memory::buddy::BuddyFrameAllocator;
        let mut frame_allocator = BuddyFrameAllocator;
        let mut mapper = unsafe { self.address_space.mapper().expect("Failed to get mapper for stack setup") };

        let stack_top_addr = 0x7FFF_FFFF_E000u64;
        let stack_pages = 16; // 64 KB
        
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

        for i in 0..stack_pages {
             let page_addr = stack_top_addr - (i + 1) * 4096;
             let page = Page::<Size4KiB>::containing_address(x86_64::VirtAddr::new(page_addr));
             if let Some(frame) = frame_allocator.allocate_frame() {
                 unsafe {
                     mapper.map_to(page, frame, flags, &mut frame_allocator).unwrap().flush();
                 }
                 crate::memory::frame_info::increment(frame.start_address());
             }
        }

        // Add VMA for user stack
        self.add_vma(Vma {
            start: stack_top_addr - (stack_pages as u64) * 4096,
            end: stack_top_addr,
            flags,
            _name: "user_stack",
        });

        // Copy strings to the top of the stack
        let mut current_rsp = stack_top_addr;
        let mut arg_ptrs = Vec::new();

        for arg in argv.iter().rev() {
            let bytes = arg.as_bytes();
            current_rsp -= (bytes.len() + 1) as u64; // +1 for null terminator
            let virt = x86_64::VirtAddr::new(current_rsp);
            
            // Map virtual to physical for direct writing
            let phys = crate::memory::virt_to_phys(virt).expect("Failed to translate user stack address");
            let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
            let k_ptr = (offset + phys.as_u64()) as *mut u8;
            
            unsafe {
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), k_ptr, bytes.len());
                *k_ptr.add(bytes.len()) = 0;
            }
            arg_ptrs.push(current_rsp);
        }

        // Align RSP
        current_rsp &= !0xF;
        
        // Push argv pointers (null terminated)
        current_rsp -= 8; // NULL
        
        for ptr in arg_ptrs {
            current_rsp -= 8;
            let virt = x86_64::VirtAddr::new(current_rsp);
            let phys = crate::memory::virt_to_phys(virt).expect("Failed to translate user stack address for ptr");
            let k_ptr = (*crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + phys.as_u64()) as *mut u64;
            unsafe { *k_ptr = ptr; }
        }
        
        let _argv_start = current_rsp;

        // Push argc
        current_rsp -= 8;
        let virt = x86_64::VirtAddr::new(current_rsp);
        let phys = crate::memory::virt_to_phys(virt).expect("Failed to translate user stack address for argc");
        let k_ptr = (*crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap() + phys.as_u64()) as *mut u64;
        unsafe { *k_ptr = argv.len() as u64; }

        current_rsp
    }
}
