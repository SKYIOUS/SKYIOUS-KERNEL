use alloc::vec::Vec;
use alloc::string::{String, ToString};
use xmas_elf::ElfFile;
use xmas_elf::header;
use xmas_elf::program;
use x86_64::structures::paging::{Translate, Mapper, Page, Size4KiB, FrameAllocator, PageTableFlags};
use x86_64::VirtAddr;
use crate::memory::buddy::BuddyFrameAllocator;
use crate::memory::paging::AddressSpace;
use crate::vfs::VFS;
use crate::task::process::Vma;

const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_SYMTAB: u64 = 6;
const DT_RELA: u64 = 7;
const DT_RELASZ: u64 = 8;
const DT_RELAENT: u64 = 9;
const DT_STRSZ: u64 = 10;
const DT_SYMENT: u64 = 11;
const DT_JMPREL: u64 = 23;

const R_X86_64_GLOB_DAT: u64 = 6;
const R_X86_64_JUMP_SLOT: u64 = 7;
const R_X86_64_RELATIVE: u64 = 8;

#[repr(C)]
struct Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

unsafe impl Send for Sym {}
unsafe impl Sync for Sym {}

struct LibInfo {
    base: u64,
    _size: u64,
    symtab: *const Sym,
    sym_count: usize,
    _strtab: *const u8,
    _strtab_size: usize,
    _needed: Vec<String>,
}

unsafe impl Send for LibInfo {}
unsafe impl Sync for LibInfo {}

fn phys_to_virt(phys: u64) -> u64 {
    let offset = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
    offset + phys
}

fn map_lib_segments(
    elf_data: &[u8],
    address_space: &mut AddressSpace,
    vmas: &mut Vec<Vma>,
    load_base: u64,
) -> Result<u64, &'static str> {
    let elf = ElfFile::new(elf_data).map_err(|_| "Failed to parse ELF for dynamic loading")?;
    let mut max_end = load_base;

    for ph in elf.program_iter() {
        if let Ok(program::Type::Load) = ph.get_type() {
            let virt_start = load_base + ph.virtual_addr();
            let file_size = ph.file_size();
            let mem_size = ph.mem_size();
            let offset = ph.offset() as usize;
            let end = virt_start + mem_size;
            if end > max_end { max_end = end; }

            let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
            if ph.flags().is_write() { flags |= PageTableFlags::WRITABLE; }
            if !ph.flags().is_execute() { flags |= PageTableFlags::NO_EXECUTE; }

            vmas.push(Vma { start: virt_start, end, flags, _name: "shared_lib" });

            let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(virt_start));
            let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(end - 1));
            let mut frame_allocator = BuddyFrameAllocator;
            let mut mapper = unsafe { address_space.mapper().ok_or("Failed to get mapper for lib")? };

            for page in Page::range_inclusive(start_page, end_page) {
                let map_flags = flags | PageTableFlags::WRITABLE;
                let frame = match mapper.translate_page(page) {
                    Ok(f) => f,
                    Err(_) => {
                        let f = frame_allocator.allocate_frame().ok_or("OOM loading shared library")?;
                        unsafe {
                            mapper.map_to(page, f, map_flags, &mut frame_allocator)
                                .map_err(|_| "Failed to map lib page")?.flush();
                        }
                        crate::memory::frame_info::increment(f.start_address());
                        f
                    }
                };

                let page_start = page.start_address().as_u64();
                let copy_start = virt_start + if page_start > virt_start { page_start - virt_start } else { 0 };
                let copy_end = core::cmp::min(virt_start + file_size, page_start + 4096);
                if copy_start < copy_end {
                    let len = copy_end - copy_start;
                    let src_off = offset + (copy_start - virt_start) as usize;
                    unsafe {
                        let dst_ptr = VirtAddr::new(phys_to_virt(frame.start_address().as_u64())).as_mut_ptr::<u8>();
                        let page_offset = if page_start > virt_start { 0 } else { (virt_start - page_start) as usize };
                        core::ptr::copy_nonoverlapping(
                            elf_data[src_off..src_off + len as usize].as_ptr(),
                            dst_ptr.add(page_offset),
                            len as usize,
                        );
                    }
                }
                let _ = unsafe { mapper.update_flags(page, flags) };
            }
        }
    }
    Ok(max_end)
}

fn parse_dt_entries(dyn_vaddr: u64, mapper: &impl Translate) -> [(u64, u64); 32] {
    let phys = mapper.translate_addr(VirtAddr::new(dyn_vaddr))
        .expect("parse_dt_entries: address not mapped in target address space");
    let ptr = phys_to_virt(phys.as_u64()) as *const u64;
    let mut out = [(0u64, 0u64); 32];
    let mut idx = 0;
    for i in 0..1024 {
        let tag = unsafe { core::ptr::read_unaligned(ptr.add(i * 2)) };
        let val = unsafe { core::ptr::read_unaligned(ptr.add(i * 2 + 1)) };
        if tag == DT_NULL { break; }
        if idx < 32 {
            out[idx] = (tag, val);
            idx += 1;
        }
    }
    out
}

fn get_dt(entries: &[(u64, u64); 32], tag: u64) -> Option<u64> {
    entries.iter().find(|&&(t, _)| t == tag).map(|&(_, v)| v)
}

fn read_dt_str(offset: u64, strtab: u64, strsz: usize, mapper: &impl Translate) -> Result<String, &'static str> {
    let phys = mapper.translate_addr(VirtAddr::new(strtab))
        .expect("read_dt_str: strtab not mapped in target address space");
    let ptr = phys_to_virt(phys.as_u64()) as *const u8;
    let o = offset as usize;
    if o >= strsz { return Err("String offset out of bounds"); }
    let mut end = o;
    unsafe {
        while end < strsz && *ptr.add(end) != 0 { end += 1; }
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr.add(o), end - o) };
    core::str::from_utf8(slice).map(|s| s.to_string()).map_err(|_| "Invalid UTF-8 in DT_NEEDED")
}

fn load_library(name: &str, address_space: &mut AddressSpace, vmas: &mut Vec<Vma>) -> Result<LibInfo, &'static str> {
    let vfs = VFS.lock();
    let path = alloc::string::String::from("/lib/") + name;
    let node = vfs.resolve_path(&path).ok_or("Shared library not found")?;
    drop(vfs);
    let elf_data = node.read(usize::MAX).map_err(|_| "Failed to read shared library")?;

    let elf = ElfFile::new(&elf_data).map_err(|_| "Failed to parse shared library ELF")?;
    if elf.header.pt2.type_().as_type() != header::Type::SharedObject {
        return Err("Not a shared library (ET_DYN required)");
    }

    let load_base = 0x7f000000000;
    let max_end = map_lib_segments(&elf_data, address_space, vmas, load_base)?;
    let mapper = unsafe { address_space.mapper().ok_or("Failed to get mapper")? };

    let mut symtab = 0u64;
    let mut strtab = 0u64;
    let mut strsz = 0usize;
    let mut symt = 0usize;
    let mut needed = Vec::new();

    for ph in elf.program_iter() {
        if let Ok(program::Type::Dynamic) = ph.get_type() {
            let dyn_vaddr = load_base + ph.virtual_addr();
            let entries = parse_dt_entries(dyn_vaddr, &mapper);

            if let Some(v) = get_dt(&entries, DT_SYMTAB) { symtab = load_base + v; }
            if let Some(v) = get_dt(&entries, DT_STRTAB) { strtab = load_base + v; }
            if let Some(v) = get_dt(&entries, DT_STRSZ) { strsz = v as usize; }
            if let Some(v) = get_dt(&entries, DT_SYMENT) { symt = v as usize; }

            for &(tag, val) in &entries {
                if tag == DT_NEEDED && strtab != 0 {
                    needed.push(read_dt_str(val, strtab, strsz, &mapper)?);
                }
            }
            break;
        }
    }

    let sym_phys = mapper.translate_addr(VirtAddr::new(symtab));
    let str_phys = mapper.translate_addr(VirtAddr::new(strtab));
    let sym_ptr = if let Some(p) = sym_phys { phys_to_virt(p.as_u64()) as *const Sym } else { core::ptr::null() };
    let _str_ptr = if let Some(p) = str_phys { phys_to_virt(p.as_u64()) as *const u8 } else { core::ptr::null() };
    let sym_count = if symt > 0 { 4096 / symt } else { 0 };

    Ok(LibInfo {
        base: load_base,
        _size: max_end - load_base,
        symtab: sym_ptr,
        sym_count,
        _strtab: _str_ptr,
        _strtab_size: strsz,
        _needed: needed,
    })
}

fn resolve_sym(libs: &[LibInfo], sym_idx: usize) -> u64 {
    for lib in libs {
        if sym_idx < lib.sym_count {
            unsafe {
                let sym = &*lib.symtab.add(sym_idx);
                if sym.st_shndx != 0 && sym.st_value != 0 {
                    return lib.base + sym.st_value;
                }
            }
        }
    }
    0
}

fn apply_rela(
    libs: &[LibInfo],
    dyn_vaddr: u64,
    base: u64,
    mapper: &impl Translate,
) -> Result<(), &'static str> {
    let phys_off = *crate::memory::PHYSICAL_MEMORY_OFFSET.get().unwrap();
    let entries = parse_dt_entries(dyn_vaddr, mapper);

    let rela_addr = get_dt(&entries, DT_RELA);
    let rela_sz = get_dt(&entries, DT_RELASZ).unwrap_or(0) as usize;
    let rela_ent = get_dt(&entries, DT_RELAENT).unwrap_or(24) as usize;
    let jmp_addr = get_dt(&entries, DT_JMPREL);

    let count = if rela_ent > 0 { rela_sz / rela_ent } else { 0 };

    if let Some(rela_base) = rela_addr {
        let rela_base_abs = base + rela_base;
        for i in 0..count {
            let entry_vaddr = rela_base_abs + (i as u64) * 24;
            let entry_phys = mapper.translate_addr(VirtAddr::new(entry_vaddr))
                .expect("apply_rela: RELA entry not mapped");
            let entry_ptr = (phys_off + entry_phys.as_u64()) as *const u8;

            let r_offset = unsafe { core::ptr::read_unaligned::<u64>(entry_ptr as *const u64) };
            let r_info = unsafe { core::ptr::read_unaligned::<u64>(entry_ptr.add(8) as *const u64) };
            let r_addend = unsafe { core::ptr::read_unaligned::<i64>(entry_ptr.add(16) as *const i64) };
            let r_type = r_info & 0xffffffff;
            let sym_idx = (r_info >> 32) as usize;

            let target_vaddr = base + r_offset;
            let target_phys = mapper.translate_addr(VirtAddr::new(target_vaddr))
                .expect("apply_rela: RELA target not mapped");
            let target_ptr = (phys_off + target_phys.as_u64()) as *mut u64;

            match r_type {
                R_X86_64_RELATIVE => {
                    unsafe { core::ptr::write_unaligned(target_ptr, base + r_addend as u64); }
                }
                R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                    let sym_vaddr = resolve_sym(libs, sym_idx);
                    unsafe { core::ptr::write_unaligned(target_ptr, sym_vaddr + r_addend as u64); }
                }
                _ => {}
            }
        }
    }

    if let Some(jmp_base) = jmp_addr {
        let jmp_base_abs = base + jmp_base;
        for i in 0..128 {
            let entry_vaddr = jmp_base_abs + (i as u64) * 24;
            let entry_phys = mapper.translate_addr(VirtAddr::new(entry_vaddr))
                .expect("apply_rela: JMPREL entry not mapped");
            let entry_ptr = (phys_off + entry_phys.as_u64()) as *const u8;

            let r_offset = unsafe { core::ptr::read_unaligned::<u64>(entry_ptr as *const u64) };
            let r_info = unsafe { core::ptr::read_unaligned::<u64>(entry_ptr.add(8) as *const u64) };
            let r_addend = unsafe { core::ptr::read_unaligned::<i64>(entry_ptr.add(16) as *const i64) };
            let r_type = r_info & 0xffffffff;

            if r_type == 0 && r_offset == 0 { break; }
            let sym_idx = (r_info >> 32) as usize;

            let target_vaddr = base + r_offset;
            let target_phys = mapper.translate_addr(VirtAddr::new(target_vaddr))
                .expect("apply_rela: JMPREL target not mapped");
            let target_ptr = (phys_off + target_phys.as_u64()) as *mut u64;

            match r_type {
                R_X86_64_RELATIVE => {
                    unsafe { core::ptr::write_unaligned(target_ptr, base + r_addend as u64); }
                }
                R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                    let sym_vaddr = resolve_sym(libs, sym_idx);
                    unsafe { core::ptr::write_unaligned(target_ptr, sym_vaddr + r_addend as u64); }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub fn load_dynamic_binary(
    elf_data: &[u8],
    address_space: &mut AddressSpace,
    _entry_point: &mut u64,
    vmas: &mut Vec<Vma>,
) -> Result<(), &'static str> {
    let elf = ElfFile::new(elf_data).map_err(|_| "Failed to parse ELF for dynamic linking")?;
    let base: u64 = 0;

    let mut dyn_vaddr = 0u64;

    for ph in elf.program_iter() {
        if let Ok(program::Type::Dynamic) = ph.get_type() {
            dyn_vaddr = ph.virtual_addr();
            break;
        }
    }

    if dyn_vaddr == 0 {
        return Ok(());
    }

    let mapper = unsafe { address_space.mapper().ok_or("Failed to get mapper")? };

    let mut needed_libs = Vec::new();
    let entries = parse_dt_entries(base + dyn_vaddr, &mapper);
    let strtab_vaddr = get_dt(&entries, DT_STRTAB).map(|v| base + v).unwrap_or(0);
    let strsz = get_dt(&entries, DT_STRSZ).unwrap_or(0) as usize;

    for &(tag, val) in &entries {
        if tag == DT_NEEDED && strtab_vaddr != 0 {
            needed_libs.push(read_dt_str(val, strtab_vaddr, strsz, &mapper)?);
        }
    }

    if needed_libs.is_empty() {
        return Ok(());
    }

    let mut lib_infos = Vec::new();
    for name in &needed_libs {
        let info = load_library(name, address_space, vmas)?;
        lib_infos.push(info);
    }

    apply_rela(&lib_infos, base + dyn_vaddr, base, &mapper)?;

    Ok(())
}
