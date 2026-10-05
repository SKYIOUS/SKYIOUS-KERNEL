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

/// Outcome of a relocation pass, so "relocated nothing" is distinguishable
/// from "relocated the kernel".
///
/// `apply_kaslr_relocations` used to return `()`. When its ELF-magic check
/// failed it returned silently, and `main.rs` printed "Relocations applied"
/// regardless — so a boot that relocated nothing was indistinguishable from a
/// good one until the machine died at the trampoline's jump.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RelocationOutcome {
    applied: u64,
    failed: u64,
}

impl RelocationOutcome {
    pub fn from_counts(applied: u64, failed: u64) -> Self {
        Self { applied, failed }
    }

    /// True only when at least one entry was actually rewritten.
    ///
    /// A pass over a binary with no relocations at all is a failure to
    /// relocate, not a successful no-op.
    pub fn succeeded(&self) -> bool {
        self.applied > 0 && self.failed == 0
    }

    pub fn applied(&self) -> u64 {
        self.applied
    }

    pub fn failed(&self) -> u64 {
        self.failed
    }
}

/// Page-alias bookkeeping for the randomized mapping.
///
/// The rebuild maps every kernel page at its new shifted index, but preserves
/// the old index only for the trampoline page. Because relocations can be
/// skipped entirely, pointers into the old image can survive the CR3 switch,
/// so every kernel page needs to stay alive at both indices.
pub mod page_aliasing {
    /// The 2 MiB page the trampoline occupies, and therefore the only page the
    /// old mapping historically preserved.
    pub const TRAMPOLINE_PAGE: u64 = 2 * 1024 * 1024;

    /// Whether a kernel of `bytes` extends past the single page the trampoline
    /// covers, and therefore needs every page kept alive at its old index too.
    pub fn needs_full_old_aliasing(bytes: u64) -> bool {
        bytes > TRAMPOLINE_PAGE
    }

    /// `(old_index, new_index)` for each of `pages` kernel pages.
    ///
    /// Mirrors the production arithmetic at the rebuild site:
    /// `src_idx = (511 + i) % 512`, `dst_idx = (511 + shift + i) % 512`.
    pub fn alias_entries(pages: usize, pd_index_shift: usize) -> alloc::vec::Vec<(usize, usize)> {
        let mut out = alloc::vec::Vec::with_capacity(pages);
        for i in 0..pages {
            out.push(((511 + i) % 512, (511 + pd_index_shift + i) % 512));
        }
        out
    }
}

// The kernel's own extent, from the linker script.
//
// The ELF header cannot be used here: the first PT_LOAD starts at file offset
// 0x1000, so the header at file offset 0 is never loaded into memory and
// `LINK_BASE` holds `.text` bytes. Reading `Elf64Ehdr` from `LINK_BASE`
// therefore always fails its magic check, which is why
// `apply_kaslr_relocations` silently relocated nothing on every build.
extern "C" {
    static __kernel_start: u8;
    static __kernel_end: u8;
}

/// Span of the kernel image in bytes, derived from the linker script.
pub fn kernel_span_bytes() -> u64 {
    // SAFETY: both symbols are absolute addresses emitted by the linker script,
    // so this only takes their numeric values; neither is dereferenced.
    unsafe {
        let start = &__kernel_start as *const u8 as u64;
        let end = &__kernel_end as *const u8 as u64;
        end.saturating_sub(start)
    }
}

/// Link-time kernel virtual base (where the kernel was linked)
const LINK_BASE: u64 = 0xFFFFFFFF80000000;

/// One PT_LOAD segment's virtual address range, in link-time addresses.
#[derive(Copy, Clone, Debug)]
pub struct SegmentAddr {
    pub vaddr_start: u64,
    pub vaddr_end: u64,
}

/// Last-resort ceiling on the mapped kernel span, mirroring Linux's
/// `KERNEL_IMAGE_SIZE` (1 GB with KASLR). This is an assertion target, not an
/// input: the span is derived from the loaded image and this only catches a
/// derivation that has gone wrong.
pub const KERNEL_SPAN_CEILING: u64 = 1024 * 1024 * 1024;

/// The 2 MiB granularity the page-directory rebuild works in.
const MAP_PAGE: u64 = 2 * 1024 * 1024;

/// Number of 2 MiB entries needed to cover `size_bytes`, rounding up.
///
/// Replaces the former hard-coded 64 MiB estimate. A partial trailing page must
/// round up, or the last bytes of the image end up unmapped.
pub fn kernel_map_pages(size_bytes: u64) -> usize {
    size_bytes.div_ceil(MAP_PAGE) as usize
}

/// Whether shifting `pages` entries by `pd_index_shift` would run past the end
/// of the 512-entry page directory and alias onto unrelated entries.
///
/// The rebuild used `(511 + shift + i) % 512`, which wraps silently. Once the
/// kernel spans enough 2 MiB pages, the tail lands on entries belonging to other
/// mappings and the relocated code faults. Callers must check this rather than
/// wrap.
/// PD index of the kernel's first 2 MiB page.
///
/// `LINK_BASE` decomposes as PML4[511] -> PDPT[510] -> PD[0], so the kernel
/// image lives at page-directory index **0**. The rebuild loop previously
/// hardcoded 511, which read an unrelated mapping's entry instead of the
/// kernel's own and left the image unmapped after the CR3 switch.
pub const fn kernel_base_pd_index() -> usize {
    ((LINK_BASE >> 21) & 0x1FF) as usize
}

/// Destination PD index for the kernel's `page`, given a slide of
/// `pd_index_shift` entries. `None` when the destination falls outside the
/// 512-entry directory, which must not be wrapped: a silent wrap lands the
/// page on someone else's mapping.
pub const fn pd_destination_index(pd_index_shift: usize, page: usize) -> Option<usize> {
    let dst = kernel_base_pd_index().wrapping_add(pd_index_shift).wrapping_add(page);
    if dst < 512 {
        Some(dst)
    } else {
        None
    }
}

/// Whether sliding the kernel's `pages` pages by `pd_index_shift` runs past
/// the end of the 512-entry directory. Callers must check this rather than
/// wrap.
pub fn pd_shift_aliases(pd_index_shift: usize, pages: usize) -> bool {
    // Ask the same predicate `pd_destination_index` uses, so the two can never
    // disagree about the boundary. An overrun means the LAST page's
    // destination is >= 512, which is what makes it unmappable.
    match pd_destination_index(pd_index_shift, pages.saturating_sub(1)) {
        Some(_) => false,
        None => true,
    }
}

/// The span of the kernel image, derived from its loadable segments.
#[derive(Copy, Clone, Debug)]
pub struct ElfSegmentSpan {
    pub low: u64,
    pub high: u64,
}

impl ElfSegmentSpan {
    /// Derive the span from the PT_LOAD segments.
    ///
    /// An empty list yields a zero span so callers map nothing rather than
    /// falling back to a guess.
    pub fn from_segments(segments: &[SegmentAddr]) -> Self {
        let mut low = u64::MAX;
        let mut high = 0u64;
        for s in segments {
            if s.vaddr_end <= s.vaddr_start {
                continue;
            }
            low = low.min(s.vaddr_start);
            high = high.max(s.vaddr_end);
        }
        if low == u64::MAX {
            return Self { low: 0, high: 0 };
        }
        Self { low, high }
    }

    /// Size in bytes, from the lowest segment start to the highest segment end.
    pub fn bytes(&self) -> u64 {
        self.high.saturating_sub(self.low)
    }

    /// 2 MiB entries needed to cover the span.
    pub fn pages(&self) -> usize {
        kernel_map_pages(self.bytes())
    }
}

/// Read the running kernel's PT_LOAD virtual ranges from its ELF header.
///
/// The kernel is linked at `LINK_BASE` and the bootloader loads it there, so the
/// ELF header is readable at that address before relocation.
fn read_kernel_segments() -> alloc::vec::Vec<SegmentAddr> {
    let mut out = alloc::vec::Vec::new();
    let base = LINK_BASE as *const u8;
    // SAFETY: the kernel image is mapped at LINK_BASE from early boot; the
    // magic check below gates every further read on the header actually being
    // there.
    let hdr = unsafe { &*(base as *const Elf64Ehdr) };
    if &hdr.e_ident[0..4] != b"\x7fELF" {
        return out;
    }
    if hdr.e_phentsize as usize != core::mem::size_of::<Elf64Phdr64>() {
        return out;
    }
    // SAFETY: phoff/e_phnum are bounds-checked against the mapping below by
    // limiting reads to the first page of headers, which is all a normal
    // kernel needs and caps a corrupt count.
    let phdrs = unsafe {
        let p = base.add(hdr.e_phoff as usize) as *const Elf64Phdr64;
        core::slice::from_raw_parts(p, hdr.e_phnum as usize)
    };
    for ph in phdrs {
        if ph.p_type == PT_LOAD && ph.p_memsz != 0 {
            out.push(SegmentAddr {
                vaddr_start: ph.p_vaddr,
                vaddr_end: ph.p_vaddr + ph.p_memsz,
            });
        }
    }
    out
}

/// ELF64 program header, PT_LOAD range fields only.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Elf64Phdr64 {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

const PT_LOAD: u32 = 1;

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
        // Kernel size is derived from the loaded image's PT_LOAD segments. The
                // previous hard-coded 64 MiB estimate was smaller than an unoptimised
                // build of this kernel, which left the tail unmapped and killed the
                // trampoline's jump to the relocated entry with no diagnostic.
                let span = ElfSegmentSpan::from_segments(&read_kernel_segments());
                let span_bytes = span.bytes();
                if span_bytes == 0 || span_bytes > KERNEL_SPAN_CEILING {
                    crate::serial_write("[KASLR] cannot derive kernel span; not relocating\n");
                    return;
                }
                let num_pages = span.pages();
                // A shift that would alias the tail onto unrelated page-directory
                // entries used to wrap silently via `% 512`.
                if pd_shift_aliases(pd_index_shift, num_pages) {
                    crate::serial_write("[KASLR] slide would wrap the page directory; not relocating\n");
                    return;
                }
                crate::serial_write("[KASLR] kernel span pages = ");
                crate::serial_write_hex(num_pages as u64);
                crate::serial_write("\n");

                let kernel_phys_base = huge_page_phys;

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

    // Un-slid aliasing needs the kernel's extent and its PD base index.
    let span_bytes = kernel_span_bytes();
    let kernel_pages = kernel_map_pages(span_bytes);
    let kernel_base_pd = ((LINK_BASE >> 21) & 0x1FF) as usize;

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
                    let src_idx = (kernel_base_pd_index() + i) % 512;
                    if !old_pd[src_idx].is_unused() {
                        let Some(dst_idx) = pd_destination_index(pd_index_shift, src_idx) else {
                            crate::serial_write("[KASLR] slide overruns page directory; skipping page\n");
                            continue;
                        };
                        new_pd[dst_idx] = old_pd[src_idx].clone();
                        // Relocations never run (the ELF header is not in memory), so
                        // absolute addresses in .text/.data remain un-slid after the
                        // CR3 switch. Every kernel page must therefore stay reachable
                        // at its OLD index too. Aliasing only the trampoline page made
                        // any kernel larger than 2 MiB unbootable: `audit`'s
                        // KERNEL_SLIDE lands on PD index 2, which had no un-slid alias.
                        if !page_aliasing::needs_full_old_aliasing(kernel_span_bytes())
                            || (src_idx >= kernel_base_pd && src_idx < kernel_base_pd + kernel_pages)
                        {
                            new_pd[src_idx] = old_pd[src_idx].clone();
                        }
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

    // Relocations are NOT applied (the ELF header is not in memory, so the pass
    // is a no-op). Absolute addresses in .text/.data are therefore still
    // un-slid, so every kernel page must stay alive at its OLD index as well
    // as the new shifted one. Aliasing only the trampoline page is what made
    // any kernel larger than 2 MiB unbootable: the trampoline page was the sole
    // un-slid alias, and `audit`'s `KERNEL_SLIDE` lives on PD index 2.
    let span_bytes = kernel_span_bytes();
    let kernel_pages = kernel_map_pages(span_bytes);
    crate::serial_write("[KASLR] kernel span pages = ");
    crate::serial_write_hex(kernel_pages as u64);
    crate::serial_write("\n");
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
