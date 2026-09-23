//! KASLR Relocation Processing
//!
//! Applies R_X86_64_64, R_X86_64_32S, and R_X86_64_PLT32 relocations to apply the KASLR slide.
//! Uses the --emit-relocs linker flag to generate relocation sections.

extern crate alloc;
use core::arch::global_asm;

global_asm!(
    r#"
.section .text.kaslr_trampoline, "ax", @progbits
.global kaslr_activate_mapping
.global kaslr_trampoline_continue

.set LINK_BASE, 0xFFFFFFFF80000000

// kaslr_activate_mapping:
// Input:
//   rdi = new_pml4_phys (physical address of new PML4)
//   rsi = slide (KERNEL_SLIDE value)
//   rdx = old_rsp (original stack pointer)
// Returns: never returns (jumps to relocated kernel)
kaslr_activate_mapping:
    // Debug: write 'A' to serial port 0x3F8
    mov dx, 0x3F8
    mov al, 'A'
    out dx, al

    // Disable interrupts
    cli

    // Switch CR3 to new PML4 (rdi has new_pml4_phys)
    mov cr3, rdi

    // Flush TLB
    mov rax, cr3
    mov cr3, rax

    // Debug: write 'B' to serial
    mov dx, 0x3F8
    mov al, 'B'
    out dx, al

    // Restore RSP from rdx (old_rsp) - DO NOT add slide.
    // The stack is in a different PML4 region (linear map) and stays at old virtual address.
    // The new PML4 preserves all mappings (copied from old), so old stack address is still valid.
    mov rsp, rdx

    // Debug: write 'C' to serial
    mov dx, 0x3F8
    mov al, 'C'
    out dx, al

    // Jump to trampoline_continue at relocated address
    // rsi still has slide
    lea rax, [rip + kaslr_trampoline_continue]
    add rax, rsi                // rax = trampoline_continue + slide (OLD rip + offset + slide = NEW address)
    jmp rax

// kaslr_trampoline_continue:
// This continues execution at the relocated address
kaslr_trampoline_continue:
    // Debug: write 'D' to serial
    mov dx, 0x3F8
    mov al, 'D'
    out dx, al

    // Re-enable interrupts
    sti

    // Jump to post_kaslr_continue (relocated)
    // RIP is now at NEW virtual address, so lea gives NEW address directly.
    // DO NOT add slide again.
    lea rax, [rip + post_kaslr_continue]
    jmp rax

.size kaslr_activate_mapping, . - kaslr_activate_mapping
"#
);

extern "C" {
    fn kaslr_activate_mapping(new_pml4_phys: u64, slide: u64, old_rsp: u64) -> !;
    fn post_kaslr_continue() -> !;
}

/// ELF64 relocation entry (matching ELF specification)
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Elf64Rela {
    pub r_offset: u64, // Address to relocate
    pub r_info: u64,   // Relocation type and symbol index
    pub r_addend: i64, // Addend
}

impl Elf64Rela {
    #[inline]
    pub fn get_type(&self) -> u32 {
        (self.r_info & 0xFFFFFFFF) as u32
    }
}

/// ELF64 section header
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

/// ELF64 header
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

/// Relocation types we handle
const R_X86_64_64: u32 = 1;
const R_X86_64_32S: u32 = 11;
const R_X86_64_PLT32: u32 = 4;
const R_X86_64_PC32: u32 = 2;
const R_X86_64_GOTPCREL: u32 = 9;
const SHT_RELA: u32 = 4;

/// Link-time kernel virtual base (where the kernel was linked)
const LINK_BASE: u64 = 0xFFFFFFFF80000000;

/// Apply relocations for the given section range.
///
/// # Safety
/// Caller must ensure `rela_start`..`rela_end` is a valid, readable range of Elf64Rela.
/// The relocation targets must be writable memory.
unsafe fn apply_relocations_section(
    rela_start: *const Elf64Rela,
    rela_end: *const Elf64Rela,
    slide: u64,
) {
    let mut rela = rela_start;
    while rela < rela_end {
        let rel = &*rela;
        match rel.get_type() {
            R_X86_64_64 => {
                // 64-bit absolute address: add slide
                let target = rel.r_offset as *mut u64;
                *target = target.read_volatile().wrapping_add(slide);
            }
            R_X86_64_32S => {
                // 32-bit signed: add slide (may overflow if slide > 2GB)
                // We only handle this if the slide fits in 32-bit signed
                if slide <= i32::MAX as u64 {
                    let target = rel.r_offset as *mut i32;
                    let val = target.read_volatile() as i64;
                    let new_val = val.saturating_add(slide as i64);
                    target.write_volatile(new_val as i32);
                }
            }
            R_X86_64_PLT32 | R_X86_64_PC32 | R_X86_64_GOTPCREL => {
                // PC-relative relocations don't need adjustment for KASLR
                // because they're relative to the instruction pointer
            }
            _ => {
                // Ignore other relocation types
            }
        }
        rela = rela.add(1);
    }
}

/// Apply all KASLR relocations using the given slide.
///
/// This parses the kernel's ELF relocation sections and applies the slide
/// to all absolute addresses that need to be adjusted for the randomized base.
///
/// # Safety
/// Must be called before any code that depends on the relocated addresses.
/// The kernel must be mapped at its link-time virtual address.
pub unsafe fn apply_kaslr_relocations(slide: u64) {
    // Use atomic guard to prevent double-application
    if crate::KASLR_APPLIED.swap(true, core::sync::atomic::Ordering::AcqRel) {
        return;
    }

    const LINK_BASE: u64 = 0xFFFFFFFF80000000;

    // Read ELF header from the kernel base
    let ehdr = &*(LINK_BASE as *const Elf64Ehdr);

    // Verify ELF magic
    if &ehdr.e_ident[0..4] != b"\x7fELF" {
        return;
    }

    // Get section headers
    let shdr_base = (LINK_BASE + ehdr.e_shoff) as *const Elf64Shdr;
    let shstrndx = ehdr.e_shstrndx as usize;
    let _shstrtab = &*shdr_base.add(shstrndx);
    let shstrtab_data = core::slice::from_raw_parts(
        (LINK_BASE + ehdr.e_shoff) as *const u8,
        ehdr.e_shentsize as usize * ehdr.e_shnum as usize,
    );

    // Iterate through section headers to find RELA sections
    for i in 0..ehdr.e_shnum as usize {
        let shdr = &*shdr_base.add(i);
        if shdr.sh_type != 4 {
            // SHT_RELA = 4
            continue;
        }

        // Get section name
        let name_offset = shdr.sh_name as usize;
        if name_offset >= shstrtab_data.len() {
            continue;
        }
        let name_end = shstrtab_data[name_offset..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| name_offset + pos)
            .unwrap_or(shstrtab_data.len());
        let name = core::str::from_utf8(&shstrtab_data[name_offset..name_end]).unwrap_or("");

        // Process .rela.text, .rela.rodata, .rela.data
        if name.starts_with(".rela.") {
            let rela_start = (LINK_BASE + shdr.sh_addr) as *const Elf64Rela;
            let rela_count = shdr.sh_size / shdr.sh_entsize;
            let rela_end = (rela_start as usize
                + (rela_count as usize) * core::mem::size_of::<Elf64Rela>())
                as *const Elf64Rela;

            apply_relocations_section(rela_start, rela_end, slide);
        }
    }
}

/// Activate KASLR by switching to the randomized virtual address space.
///
/// This function:
/// 1. Creates a new PML4 with the kernel mapped at the randomized address
/// 2. Preserves the old mapping for safe transition
/// 3. Switches CR3 and jumps to the relocated address via assembly trampoline
///
/// # Safety
/// Must be called after relocations are applied, before any code depends on the
/// relocated addresses. Interrupts must be disabled during the CR3 switch.
#[cfg(not(target_arch = "aarch64"))]
pub unsafe fn activate_kaslr_mapping() {
    use x86_64::{
        registers::control::Cr3,
        structures::paging::page_table::PageTableEntry,
        structures::paging::{FrameAllocator, PageTable, PageTableFlags, PhysFrame},
        VirtAddr,
    };

    let slide = get_kaslr_slide();
    if slide == 0 {
        return;
    }

    const LINK_BASE: u64 = 0xFFFFFFFF80000000;
    const KERNEL_PML4_INDEX: usize = 511;
    const OLD_KERNEL_PML4_INDEX: usize = 510;

    // Get the HHDM offset
    let hhdm_offset = crate::limine::hhdm_offset();
    let _phys_mem_offset = VirtAddr::new(hhdm_offset);

    // Get the current PML4 (active page tables from Limine)
    let (current_pml4_frame, _cr3_flags) = Cr3::read();
    let current_pml4_virt =
        VirtAddr::new(hhdm_offset) + current_pml4_frame.start_address().as_u64();
    let current_pml4 = &mut *(current_pml4_virt.as_mut_ptr::<PageTable>());

    // Allocate a new PML4 frame
    let mut frame_allocator = crate::memory::buddy::BuddyFrameAllocator;
    let new_pml4_frame = frame_allocator
        .allocate_frame()
        .expect("Failed to allocate PML4 frame for KASLR");

    let new_pml4_virt = VirtAddr::new(hhdm_offset) + new_pml4_frame.start_address().as_u64();
    let new_pml4 = &mut *(new_pml4_virt.as_mut_ptr::<PageTable>());
    new_pml4.zero();

    // Copy ALL mappings from current PML4 to new PML4
    // This preserves HHDM, framebuffer, MMIO, identity map, etc.
    for i in 0..512 {
        new_pml4[i] = current_pml4[i].clone();
    }

    // The kernel is currently mapped at LINK_BASE (0xFFFFFFFF80000000)
    // which is PML4[511], PDPT[510], PD[511] (2MiB pages)
    // We want it at LINK_BASE + slide.
    // Since slide < 1GiB, PML4 index remains 511, PDPT index remains 510.
    // We need to create a new PD with entries shifted by (slide >> 21).

    // Allocate a new PDPT for the randomized kernel mapping
    let new_pdpt_frame = frame_allocator
        .allocate_frame()
        .expect("Failed to allocate PDPT frame for KASLR");
    let new_pdpt_virt = VirtAddr::new(hhdm_offset) + new_pdpt_frame.start_address().as_u64();
    let new_pdpt = &mut *(new_pdpt_virt.as_mut_ptr::<PageTable>());
    new_pdpt.zero();

    // Get the current kernel PDPT (from PML4 index 511)
    let old_pdpt_frame = current_pml4[511].frame().expect("Kernel PDPT not present");
    let old_pdpt_virt = VirtAddr::new(hhdm_offset) + old_pdpt_frame.start_address().as_u64();
    let old_pdpt = &*(old_pdpt_virt.as_ptr::<PageTable>());

    // Copy the old kernel PDPT to the new PDPT (for non-kernel PD entries)
    for i in 0..512 {
        new_pdpt[i] = old_pdpt[i].clone();
    }

    // Now create a NEW PD for the shifted kernel mapping
    let new_pd_frame = frame_allocator
        .allocate_frame()
        .expect("Failed to allocate PD frame for KASLR");
    let new_pd_virt = VirtAddr::new(hhdm_offset) + new_pd_frame.start_address().as_u64();
    let new_pd = &mut *(new_pd_virt.as_mut_ptr::<PageTable>());
    new_pd.zero();

    // The kernel starts at PD index 511 (since LINK_BASE >> 21 & 0x1FF = 511)
    // We need to map the same physical pages at new virtual addresses:
    // new_virt = LINK_BASE + slide + (old_virt - LINK_BASE) = old_virt + slide
    // This means PD index shift = (slide >> 21)

    let pd_index_shift = (slide >> 21) as usize; // 2 MiB pages per PD entry

    // Get the old PD (from PDPT index 510) - this has the 2MiB kernel mappings
    // Check if PDPT[510] is a huge page (1GiB) or a PD pointer
    const KERNEL_PDPT_INDEX: usize = 510; // LINK_BASE >> 30 & 0x1FF = 510
    let old_pdpt_entry_kernel = old_pdpt[KERNEL_PDPT_INDEX].clone();
    let mut created_new_pd = false;
    let mut huge_page_phys = 0u64; // Will be set if huge page case

    if old_pdpt_entry_kernel
        .flags()
        .contains(PageTableFlags::HUGE_PAGE)
    {
        // PDPT[510] is a 1GiB huge page - we need to create a new PD with the kernel mappings
        // The huge page covers 1GiB at LINK_BASE. Extract its physical address and create 2MiB pages.
        crate::serial_write(
            "[KASLR] PDPT[510] is 1GiB huge page, creating new PD with 2MiB pages\n",
        );
        huge_page_phys = old_pdpt_entry_kernel.addr().as_u64();
        // Kernel virtual base is LINK_BASE. Physical base = LINK_BASE - HHDM_OFFSET.
        // The huge page starts at LINK_BASE (aligned to 1GiB), so its physical address is huge_page_phys.
        // We need to map the kernel's physical pages at the new virtual addresses (LINK_BASE + slide).
        // Kernel size: estimate 64MiB (32 * 2MiB pages) - should cover text, rodata, data, bss
        let kernel_phys_base = huge_page_phys;
        let kernel_size_estimate = 64 * 1024 * 1024; // 64 MiB
        let num_pages = (kernel_size_estimate / (2 * 1024 * 1024)) as usize; // 32 pages

        for i in 0..num_pages {
            let page_phys = kernel_phys_base + (i as u64 * 2 * 1024 * 1024);
            let page_frame = PhysFrame::containing_address(x86_64::PhysAddr::new(page_phys));
            // Map at PD index (511 + pd_index_shift + i) in the new PD (which corresponds to LINK_BASE + slide + i * 2MiB)
            let pd_idx = (511 + pd_index_shift + i) % 512;
            new_pd[pd_idx].set_frame(
                page_frame,
                PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE,
            );
        }
        created_new_pd = true;
    }

    // Copy kernel PD entries to new shifted positions in new PD
    // The kernel typically uses a few 2MiB pages starting at PD index 511
    // Only copy if we didn't create the PD manually (huge page case)
    if !created_new_pd {
        let old_pd_frame = old_pdpt_entry_kernel
            .frame()
            .expect("Kernel PD not present");
        let old_pd_virt = VirtAddr::new(hhdm_offset) + old_pd_frame.start_address().as_u64();
        // SAFETY: every physical frame is readable through the HHDM, and the
        // frame came from a present PDPTE, so it is a real page-directory
        // frame the kernel's own boot page tables created.
        let old_pd = unsafe { &*(old_pd_virt.as_ptr::<PageTable>()) };

        for i in 0..512 {
            let src_idx = (511 + i) % 512;
            if !old_pd[src_idx].is_unused() {
                let dst_idx = (511 + pd_index_shift + i) % 512;
                new_pd[dst_idx] = old_pd[src_idx].clone();
            }
        }
    }

    // CRITICAL: Map the trampoline code page at BOTH its old PD index (for OLD virtual address
    // compatibility after CR3 switch) AND its new shifted PD index (for NEW virtual address after jump).
    // The trampoline runs from the OLD virtual address, switches CR3, then jumps to the NEW virtual address.
    // After CR3 switch, the CPU fetches instructions at the OLD virtual address using NEW page tables.
    // So the OLD virtual address must be mapped in NEW PML4[511] -> NEW PDPT[510] -> NEW PD[old_pd_index].
    // We get the trampoline's virtual address from the function pointer.
    let trampoline_virt = kaslr_activate_mapping as *const () as u64;
    let trampoline_pd_index = ((trampoline_virt >> 21) & 0x1FF) as usize;

    // Get the trampoline's page table entry (physical frame + flags)
    let trampoline_pd_entry = if created_new_pd {
        // Huge page case: we created the PD from scratch. Calculate which 2MiB page the trampoline is in.
        const LINK_BASE: u64 = 0xFFFFFFFF80000000;
        let page_index = ((trampoline_virt - LINK_BASE) / (2 * 1024 * 1024)) as usize;
        let page_phys = huge_page_phys + (page_index as u64 * 2 * 1024 * 1024);
        let page_frame = PhysFrame::containing_address(x86_64::PhysAddr::new(page_phys));
        // Create a synthetic PD entry with the same flags as our mapped pages
        let mut entry = PageTableEntry::new();
        entry.set_frame(
            page_frame,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE,
        );
        entry
    } else {
        // Non-huge-page case: read from the old PD
        let old_pd_frame = old_pdpt_entry_kernel
            .frame()
            .expect("Kernel PD not present");
        let old_pd_virt = VirtAddr::new(hhdm_offset) + old_pd_frame.start_address().as_u64();
        // SAFETY: same HHDM-read of a present PDPTE's frame as above; the
        // entry is only cloned out, never written through this reference.
        let old_pd = unsafe { &*(old_pd_virt.as_ptr::<PageTable>()) };
        old_pd[trampoline_pd_index].clone()
    };

    if !trampoline_pd_entry.is_unused() {
        // Map at old PD index for OLD virtual address compatibility after CR3 switch
        new_pd[trampoline_pd_index] = trampoline_pd_entry.clone();
        // Map at new shifted PD index for NEW virtual address
        let new_pd_index = (trampoline_pd_index + pd_index_shift) % 512;
        new_pd[new_pd_index] = trampoline_pd_entry;
    }

    // Point new PDPT[510] to the new PD
    let new_pd_phys = new_pd_frame.start_address().as_u64();
    let new_pd_frame_obj = PhysFrame::containing_address(x86_64::PhysAddr::new(new_pd_phys));
    new_pdpt[KERNEL_PDPT_INDEX].set_frame(
        new_pd_frame_obj,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
    );

    // Update the new PML4's kernel entry (index 511) to point to the new PDPT
    let new_pdpt_phys = new_pdpt_frame.start_address().as_u64();
    let new_pdpt_frame_obj = PhysFrame::containing_address(x86_64::PhysAddr::new(new_pdpt_phys));
    new_pml4[511].set_frame(
        new_pdpt_frame_obj,
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
    );

    // ALSO preserve the old kernel mapping at a different PML4 index (510)
    // This ensures the transition code remains accessible during CR3 switch
    new_pml4[510] = current_pml4[511].clone();

    // Get current stack pointer for the trampoline
    let old_rsp: u64;
    core::arch::asm!("mov {}, rsp", out(reg) old_rsp, options(nostack, preserves_flags));

    let new_pml4_phys = new_pml4_frame.start_address().as_u64();

    // Call the assembly trampoline - never returns
    kaslr_activate_mapping(new_pml4_phys, slide, old_rsp);
}

/// Get the current KASLR slide value
pub fn get_kaslr_slide() -> u64 {
    crate::KERNEL_SLIDE.load(core::sync::atomic::Ordering::Relaxed)
}
