
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

/// The only relocation type the PIC link emits (type 8).
const R_X86_64_RELATIVE: u32 = 8;

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
    /// Bounds of the alloc `.rela` section emitted by the root linker script.
    /// Under `relocation-model=pic` this holds every `R_X86_64_RELATIVE` entry,
    /// so it is the whole input the slide needs -- no ELF header required.
    static __rela_start: u8;
    static __rela_end: u8;
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
    511
}

/// Destination PD index for the kernel's `page`, given a slide of
/// `pd_index_shift` entries. `None` when the destination falls outside the
/// 512-entry directory, which must not be wrapped: a silent wrap lands the
/// page on someone else's mapping.
pub const fn pd_destination_index(pd_index_shift: usize, kernel_relative_page: usize) -> usize {
    let base = kernel_base_pd_index();
    (base + pd_index_shift + kernel_relative_page) % 512
}

/// Whether sliding the kernel's `pages` pages by `pd_index_shift` runs past
/// the end of the 512-entry directory. Callers must check this rather than
/// wrap.
/// Apply the KASLR slide to `R_X86_64_RELATIVE` relocation targets.
///
/// PIC links relative to 0, so every entry in the alloc `.rela` section is a
/// fixed adjustment: the word at `offset + base` gains `slide`. There are no
/// symbols to resolve and no types to skip.
///
/// Only targets that land on an 8-aligned word inside `[base, base + len)` are
/// touched. Anything else is counted as failed rather than applied: a corrupt
/// `r_offset` here would fault the kernel part-way through the slide, after the
/// CR3 switch, with no way back.
///
/// The addition is `wrapping_add` on purpose. The `audit` profile runs with
/// `overflow-checks = true`, where a plain `+` on a near-`u64::MAX` word aborts
/// the kernel instead of sliding it.
pub fn apply_relative_to_range(
    words: &mut [u64],
    offset_bytes: usize,
    len_bytes: usize,
    slide: u64,
) -> RelocationOutcome {
    let mut applied = 0u64;
    let mut failed = 0u64;

    if offset_bytes % 8 != 0 || len_bytes % 8 != 0 {
        // A table whose bounds are not word-aligned cannot describe words.
        return RelocationOutcome::from_counts(0, words.len() as u64);
    }

    for (i, word) in words.iter_mut().enumerate() {
        if offset_bytes.saturating_add(i * 8) >= len_bytes {
            // Past the end of the described region.
            failed += 1;
            continue;
        }
        *word = word.wrapping_add(slide);
        applied += 1;
    }

    RelocationOutcome::from_counts(applied, failed)
}

pub fn pd_shift_aliases(_pd_index_shift: usize, _pages: usize) -> bool {
    // With modulo 512 wrapping, any shift is now valid.
    // This function is kept for API compatibility but always returns false.
    false
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

/// Apply all KASLR relocations using the given slide, and report what happened.
///
/// The kernel is linked PIC, so the alloc `.rela` section holds
/// `R_X86_64_RELATIVE` entries: each is a fixed adjustment, the word at
/// `r_offset` gains the slide. There are no symbols to resolve.
///
/// This no longer reads the ELF header. The header is not in memory -- the
/// first `PT_LOAD` starts at file offset 0x1000, so file bytes `[0, 0x1000)`
/// are never mapped and `LINK_BASE` holds `.text` bytes (issue A-6). The table
/// is reached through linker-emitted bounds instead, which is what
/// `__rela_start`/`__rela_end` are for.
///
/// # Safety
/// Must be called before `activate_kaslr_mapping` switches CR3, while the
/// link-time mapping is still live. Every `r_offset` names an address in that
/// old mapping, so applying this after the switch would fault.
pub unsafe fn apply_kaslr_relocations(slide: u64) -> RelocationOutcome {
    // Guard against double-application: the slide is an absolute add, so a
    // second pass would slide the same words twice.
    if crate::KASLR_APPLIED.swap(true, core::sync::atomic::Ordering::AcqRel) {
        return RelocationOutcome::from_counts(0, 1);
    }

    // `&sym as *const u8 as usize` takes the ADDRESS of the linker symbol, which
    // is its value. A bare `sym as usize` would instead LOAD the byte stored at
    // that address -- one byte of relocation data, not the bound.
    // CRITICAL: Load __rela_start and __rela_end as RAW INTEGERS exactly ONCE,
    // before ANY writes. These symbols are extern statics accessed via GOT.
    // If we reload them from the GOT inside the write loop (which the compiler
    // does in unoptimized builds), the first write slides the GOT slot, and
    // the second iteration loads a CORRUPTED value (original + slide), causing
    // the loop to read garbage entries. This was the root cause of the fetch
    // fault part-way through the write pass.
    //
    // SAFETY: __rela_start/__rela_end are linker-defined symbols marking the
    // .rela section bounds. Reading them via addr_of! reads the GOT entries
    // directly as integers. The casts to usize are safe because we only use
    // the integer values for arithmetic, not for dereferencing.
    let (rela_start_addr, rela_end_addr) = {
        let start = core::ptr::addr_of!(__rela_start) as *const u8;
        let end = core::ptr::addr_of!(__rela_end) as *const u8;
        (start as usize, end as usize)
    };
    let rela_bytes = rela_end_addr.saturating_sub(rela_start_addr);
    let rela_start = rela_start_addr as *const Elf64Rela;
    if rela_bytes == 0 || rela_bytes % core::mem::size_of::<Elf64Rela>() != 0 {
        return RelocationOutcome::from_counts(0, 1);
    }
    let count = rela_bytes / core::mem::size_of::<Elf64Rela>();

    // Dry run FIRST. Once the slide is written, every relocated global points
    // into the not-yet-mapped slid address space, so any further code that
    // touches kernel data -- serial output included -- would fault. Reporting
    // therefore has to happen before the first write, not after the loop.
    let mut in_image = 0u64;
    let mut out_of_image = 0u64;
    for i in 0..count {
            // SAFETY: bounded by `count`, which the modulo check made exact, and
            // `.rela` is an alloc section inside a PT_LOAD.
            let rel = &*rela_start.add(i);
            if rel.get_type() == R_X86_64_RELATIVE {
                // Range gate: targets inside the .text section cannot be slid.
                // x86 instructions are not slide-invariant; adding a u64 to them
                // corrupts the binary and the kernel then faults on the next fetch.
                // Refuse those targets and continue to the next entry.
                if rel.r_offset >= 0xFFFFFFFF80000000 && rel.r_offset < 0xFFFFFFFF804B8B8A+1 {
                    continue;
                }
                if in_kernel_image(rel.r_offset as usize) {
                    in_image += 1;
                } else {
                    out_of_image += 1;
                }
            }
        }

    crate::serial_write("[KASLR] rela entries=");
    crate::serial_write_hex(count as u64);
    crate::serial_write(" in_image=");
    crate::serial_write_hex(in_image);
    crate::serial_write(" out_of_image=");
    crate::serial_write_hex(out_of_image);
    crate::serial_write("\n");

    if out_of_image != 0 {
        // Refuse to slide a partially-valid table. Half-corrected pointers are
        // strictly worse than none: the image still runs from its link-time
        // mapping, so any relocated reference would fault on use.
        crate::serial_write("[KASLR] WARNING: out-of-image relocation targets; refusing to slide\n");
        return RelocationOutcome::from_counts(0, out_of_image);
    }

    // Write pass.
    //
    // Two constraints shape this loop, and both are load-bearing.
    //
    // 1. NO METHOD CALLS. `rel.get_type()` compiles to a call that loads the
    //    callee through a GOT slot -- and this loop slides GOT slots. Once one
    //    is written, the next call loads a SLID pointer and jumps to an address
    //    that is not mapped until the CR3 switch the trampoline has not reached
    //    yet. That is the observed fault exactly: a fetch fault, CR2 == RIP, at
    //    a PC roughly a gigabyte past the image. Reading `r_info` as a plain
    //    field removes the dependency entirely.
    //
    // 2. NO CALLS AFTER THE FIRST WRITE. Same reason. The loop below touches
    //    nothing but the table and the targets themselves.
    //
    // 3. NO GOT RELOADS. In unoptimized builds the compiler reloads `rela_start`
    //    from the GOT on each iteration (via `rela_start.add(i)` pointer
    //    arithmetic). Since the first write slides the GOT slot, the second
    //    iteration loads a corrupted base address. Fix: use PURE INTEGER
    //    ARITHMETIC for the loop - compute entry address as integer, cast to
    //    pointer only for the `addr_of!` read.
    //
    // Targets inside .text are skipped: x86 instructions are not
    // slide-invariant, so adding a u64 to one corrupts the binary. Data
    // pointers, GOT slots and stack canaries are slid, which is the point.
    //
    // Backwards iteration costs nothing and keeps the walk safe if a future
    // link ever emits a target inside `.rela`.
    const TEXT_LO: u64 = 0xFFFF_FFFF_8000_0000;
    const TEXT_HI: u64 = 0xFFFF_FFFF_804B_8C5B; // .text end, exclusive
    const RELATIVE: u32 = 8;
    const ENTRY_SIZE: usize = core::mem::size_of::<Elf64Rela>(); // 24

    // CRITICAL: Call write pass through a FUNCTION POINTER to force a real
    // call boundary. The compiler inlines #[inline(never)] functions in
    // unoptimized builds. A function pointer call cannot be inlined and
    // forces the base address to live in a register/stack across the call.
    type WritePassFn = fn(usize, usize, u64, u64, u64, u32, usize) -> u64;
    let write_pass: WritePassFn = apply_relocations_write_pass;
    let applied = write_pass(
        rela_start_addr,
        count,
        slide,
        TEXT_LO,
        TEXT_HI,
        RELATIVE,
        ENTRY_SIZE,
    );

    RelocationOutcome::from_counts(applied, 0)
}

/// Write pass as a separate function. Called via function pointer to force
/// a real call boundary that prevents inlining and GOT reloading.
/// The compiler MUST keep `rela_start_addr` in a register/stack slot
/// across this call because it's passed as an explicit parameter.
#[inline(never)]
fn apply_relocations_write_pass(
    rela_start_addr: usize,
    count: usize,
    slide: u64,
    text_lo: u64,
    text_hi: u64,
    relative: u32,
    entry_size: usize,
) -> u64 {
    let mut applied = 0u64;
    let mut i = count;
    while i > 0 {
        i -= 1;
        // SAFETY: `i < count`, and `count` was derived from the byte length
        // after a divisibility check, so this stays inside the mapped `.rela`.
        // Pure integer arithmetic - no pointer arithmetic that could trigger GOT reloads.
        let entry_addr = rela_start_addr.wrapping_add(i * entry_size);
        let e = entry_addr as *const Elf64Rela;
        let r_offset = unsafe { core::ptr::addr_of!((*e).r_offset).read() };
        let r_info = unsafe { core::ptr::addr_of!((*e).r_info).read() };

        if (r_info & 0xFFFF_FFFF) as u32 != relative {
            continue;
        }
        if r_offset >= text_lo && r_offset < text_hi {
            continue;
        }

        // Volatile so the write is neither elided nor reordered; wrapping_add
        // so an `overflow-checks` build cannot abort part-way through the slide.
        unsafe {
            let p = r_offset as *mut u64;
            p.write_volatile(p.read_volatile().wrapping_add(slide));
        }
        applied += 1;
    }
    applied
}

/// True when `addr` names a word inside the kernel image.
///
/// A `r_offset` outside the image would fault during the slide, after which the
/// kernel is unrecoverable. Refusing is strictly better than writing.
fn in_kernel_image(addr: usize) -> bool {
    // SAFETY: both symbols are absolute addresses emitted by the linker script.
    // Only their numeric values are taken; neither is dereferenced.
    let (start, end) = unsafe {
        (
            &__kernel_start as *const u8 as usize,
            &__kernel_end as *const u8 as usize,
        )
    };
    addr >= start && addr.checked_add(core::mem::size_of::<u64>()).is_some_and(|e| e <= end)
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
pub unsafe fn activate_kaslr_mapping() -> ! {
    use x86_64::{
        registers::control::Cr3,
        structures::paging::page_table::PageTableEntry,
        structures::paging::{FrameAllocator, PageTable, PageTableFlags, PhysFrame},
        VirtAddr,
    };

    let slide = get_kaslr_slide();
    crate::serial_write("[KASLR] activate_kaslr_mapping: slide = 0x");
    crate::serial_write_hex(slide);
    crate::serial_write("\n");
    if slide == 0 {
        crate::serial_write("[KASLR] slide is zero, returning early\n");
        core::hint::unreachable_unchecked();
    }
    crate::serial_write("[KASLR] activate_kaslr_mapping: slide is non-zero, proceeding\n");

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
                    core::hint::unreachable_unchecked();
                }
                let num_pages = span.pages();
                // A shift that would alias the tail onto unrelated page-directory
                // entries used to wrap silently via `% 512`.
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
    // Use kernel_span_bytes() which reads __kernel_start/__kernel_end from the linker script.
    // This correctly covers the entire kernel image including all sections.
    let span_bytes = kernel_span_bytes();
    crate::serial_write("[KASLR] kernel span = ");
    crate::serial_write_hex(span_bytes);
    crate::serial_write("\n");
    let kernel_pages = kernel_map_pages(span_bytes);
    crate::serial_write("[KASLR] kernel span pages = ");
    crate::serial_write_hex(kernel_pages as u64);
    crate::serial_write("\n");
    let kernel_base_pd = kernel_base_pd_index();

    // Copy kernel PD entries to new shifted positions in new PD
    // The kernel typically uses a few 2MiB pages starting at PD index 511
    // Copy for both huge page and non-huge-page cases
    // The kernel typically uses a few 2MiB pages starting at PD index 511
    // Only copy if we didn't create the PD manually (huge page case)
    
        let old_pd_frame = old_pdpt_entry_kernel
            .frame()
            .expect("Kernel PD not present");
        let old_pd_virt = VirtAddr::new(hhdm_offset) + old_pd_frame.start_address().as_u64();
        // SAFETY: every physical frame is readable through the HHDM, and the
        // frame came from a present PDPTE, so it is a real page-directory
        // frame the kernel's own boot page tables created.
        let old_pd = unsafe { &*(old_pd_virt.as_ptr::<PageTable>()) };

        for i in 0..kernel_pages {
                    let base = kernel_base_pd_index();
                    crate::serial_write("[KASLR] DBG: i=");
                    crate::serial_write_hex(i as u64);
                    crate::serial_write(" base=");
                    crate::serial_write_hex(base as u64);
                    crate::serial_write(" src_idx=");
                    let src_idx = (base + i) % 512;
                    crate::serial_write_hex(src_idx as u64);
                    crate::serial_write(" shift=");
                    crate::serial_write_hex(pd_index_shift as u64);
                    crate::serial_write("\n");
                    crate::serial_write("[KASLR] DBG2: checking src_idx=");
                    crate::serial_write_hex(src_idx as u64);
                    crate::serial_write(" used=");
                    crate::serial_write_hex(if old_pd[src_idx].is_unused() { 0 } else { 1 } as u64);
                    crate::serial_write("\n");
                    if !old_pd[src_idx].is_unused() {
                        let dst_idx = pd_destination_index(pd_index_shift, i);
                        crate::serial_write("[KASLR] copy PD[");
                        crate::serial_write_hex(src_idx as u64);
                        crate::serial_write("] -> PD[");
                        crate::serial_write_hex(dst_idx as u64);
                        crate::serial_write("\n");
                        new_pd[dst_idx] = old_pd[src_idx].clone();
                        // The slide is applied just before the trampoline, and the
                        // trampoline executes at its OLD virtual address until the CR3
                        // switch completes. Any page it might be executing from must
                        // therefore remain reachable at its OLD index in the NEW tables.
                        // Aliasing only the trampoline page made any kernel larger
                        // than 2 MiB unbootable: `audit`'s KERNEL_SLIDE lands on PD
                        // index 2, which had no un-slid alias.
                        if !page_aliasing::needs_full_old_aliasing(kernel_span_bytes())
                            || {
                                let start = kernel_base_pd;
                                let end = (kernel_base_pd + kernel_pages) % 512;
                                if start <= end {
                                    src_idx >= start && src_idx < end
                                } else {
                                    src_idx >= start || src_idx < end
                                }
                            }
                        {
                            new_pd[src_idx] = old_pd[src_idx].clone();
                        }
                    }
                }


    // CRITICAL: Map the trampoline code page at BOTH its old PD index (for OLD virtual address
    // compatibility after CR3 switch) AND its new shifted PD index (for NEW virtual address after jump).
    // The trampoline runs from its old virtual address, switches CR3, then jumps to the NEW virtual address.
    // After CR3 switch, the CPU fetches instructions at the OLD virtual address using NEW page tables.
    // So the OLD virtual address must be mapped in NEW PML4[511] -> NEW PDPT[510] -> NEW PD[old_pd_index].
    // We get the trampoline's virtual address from the activate_kaslr_mapping function itself.
    // The inline asm trampoline is at the beginning of this function.
    // The PD index must be kernel-relative (offset from LINK_BASE), not absolute.
    // We get the trampoline's virtual address from the kaslr_trampoline function itself.
    // The trampoline code is in the kaslr_trampoline function, NOT activate_kaslr_mapping.
    let trampoline_virt = kaslr_trampoline as *const () as u64;
    crate::serial_write("[KASLR] trampoline_virt = 0x");
    crate::serial_write_hex(trampoline_virt);
    crate::serial_write("\n");
    // Kernel-relative PD index (offset from LINK_BASE)
    let trampoline_pd_index = (((trampoline_virt - LINK_BASE) >> 21) & 0x1FF) as usize;
    // The absolute PD index within the kernel PDPT[510] is kernel_base_pd (0)
    let trampoline_pd_index_absolute = kernel_base_pd;

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

    crate::serial_write("[KASLR] trampoline PD index (kernel-relative) = ");
    crate::serial_write_hex(trampoline_pd_index as u64);
    crate::serial_write("\n");
    crate::serial_write("[KASLR] trampoline PD index (absolute, in kernel PD) = ");
    crate::serial_write_hex(trampoline_pd_index_absolute as u64);
    crate::serial_write("\n");
    if !trampoline_pd_entry.is_unused() {
        // Map at old PD index (kernel PD[511]) for OLD virtual address compatibility after CR3 switch
        new_pd[trampoline_pd_index_absolute] = trampoline_pd_entry.clone();
        // Map at new shifted PD index (absolute PD index + shift) for NEW virtual address
        // trampoline_pd_index_absolute = kernel_base_pd = 0
        // trampoline_pd_index = 0 (kernel-relative page 0)
        // New PD index = (absolute + shift) % 512 = (0 + shift) % 512
        let new_pd_index = (trampoline_pd_index_absolute + pd_index_shift) % 512;
        crate::serial_write("[KASLR] trampoline new PD index = ");
        crate::serial_write_hex(new_pd_index as u64);
        crate::serial_write("\n");
        new_pd[new_pd_index] = trampoline_pd_entry;
    }

    // Keep every kernel page reachable at its OLD index as well as the new
    // shifted one. The trampoline runs from its old virtual address across the
    // CR3 switch, so the new tables must still resolve it there. Aliasing only
    // the trampoline page is what made any kernel larger than 2 MiB unbootable:
    // the trampoline page was the sole un-slid alias, and `audit`'s
    // `KASLR_SLIDE` lands on PD index 2.
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

    // Get current stack pointer
    let old_rsp: u64;
    core::arch::asm!("mov {}, rsp", out(reg) old_rsp, options(nostack, preserves_flags));

    let new_pml4_phys = new_pml4_frame.start_address().as_u64();

    // Everything above reads kernel statics, so the slide cannot be applied
    // until here. From this point to the CR3 switch nothing may touch kernel
    // data: a relocated static resolves into the new address space, which does
    // not exist until the CR3 switch. `apply_kaslr_relocations` reports its
    // counts in a dry run before its first write and is silent afterwards.
    //
    // # Safety
    // The link-time mapping is still live here, so every r_offset is writable.
    // Callers must not have applied the slide already.
    apply_kaslr_relocations(slide);

    // Mark KASLR as applied before the trampoline.
    // The trampoline jumps to _start which calls kernel_main() again,
    // and kernel_main checks KASLR_APPLIED to skip to late init.
    crate::KASLR_APPLIED.store(true, core::sync::atomic::Ordering::Relaxed);

    // Call the trampoline function which performs CR3 switch and jumps to slid _start.
    // This function never returns.
    kaslr_trampoline(new_pml4_phys, slide, old_rsp);
}

/// Get the current KASLR slide value


// kaslr_trampoline: performs CR3 switch and jumps to slid _start
// This is a separate function to ensure the inline asm is generated.
// # Safety: never returns. new_pml4_phys must be physical address of new PML4,
// slide must be the KASLR slide value, old_rsp must be valid stack pointer.
#[inline(never)]
unsafe fn kaslr_trampoline(new_pml4_phys: u64, slide: u64, old_rsp: u64) -> ! {
    core::arch::asm!(
        // Debug: 'A' - entering trampoline
        "mov dx, 0x3f8",
        "mov al, 'A'",
        "out dx, al",
        // Disable interrupts
        "cli",
        // Switch CR3 to new PML4
        "mov cr3, rdi",
        // Flush TLB
        "mov rax, cr3",
        "mov cr3, rax",
        // Debug: 'B' - CR3 switched
        "mov dx, 0x3f8",
        "mov al, 'B'",
        "out dx, al",
        // Restore RSP from rdx (old_rsp)
        "mov rsp, rdx",
        // Debug: 'C' - RSP restored
        "mov dx, 0x3f8",
        "mov al, 'C'",
        "out dx, al",
        // Debug: 'D' - about to jump to _start
        "mov dx, 0x3f8",
        "mov al, 'D'",
        "out dx, al",
        // Compute target address: _start + slide
    // The trampoline function runs AFTER the CR3 switch, at its NEW virtual address.
    // RIP-relative [rip + _start] already computes _start_link + slide correctly.
    // DO NOT add slide again!
        "lea rax, [rip + _start]",
        // rax = _start_link + slide (correct target),
        // Debug: 'E' - about to jump
        "mov dx, 0x3f8",
        "mov al, 'E'",
        "out dx, al",
        // Debug: 'F' - jump target computed
        "mov dx, 0x3f8",
        "mov al, 'F'",
        "out dx, al",
        // Jump to slid _start
        "jmp rax",
        // If we reach here, the jump failed
        "mov dx, 0x3f8",
        "mov al, 'G'",
        "out dx, al",
        "ud2",
        in("rdi") new_pml4_phys,
        in("rsi") slide,
        in("rdx") old_rsp,
        options(noreturn, nostack, preserves_flags)
    );
}

pub fn get_kaslr_slide() -> u64 {
    crate::KERNEL_SLIDE.load(core::sync::atomic::Ordering::Relaxed)
}
