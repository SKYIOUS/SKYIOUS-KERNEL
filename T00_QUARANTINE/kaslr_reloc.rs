//! KASLR Relocation Processing
//!
//! Applies R_X86_64_64, R_X86_64_32S, and R_X86_64_PLT32 relocations to apply the KASLR slide.
//! Uses the --emit-relocs linker flag to generate relocation sections.

use core::arch::global_asm;

global_asm!(
    r#"
.section .text.kaslr_trampoline, "ax", @progbits
.global kaslr_activate_mapping
.global kaslr_trampoline_entry

.set LINK_BASE, 0xFFFFFFFF80000000

// kaslr_activate_mapping:
// Input:
//   rdi = new_pml4_phys (physical address of new PML4)
//   rsi = slide (KERNEL_SLIDE value)
//   rdx = old_rsp (original stack pointer)
// Returns: never returns (jumps to relocated kernel)
kaslr_activate_mapping:
    // Save original RSP and compute new RIP
    mov [rsp - 8], rdx          // Save old RSP
    lea rax, [rip + kaslr_trampoline_continue]
    add rax, rsi                // rax = trampoline_continue + slide
    mov [rsp - 16], rax         // Save new RIP

    // Disable interrupts
    cli

    // Switch CR3 to new PML4
    mov cr3, rdi

    // Flush TLB
    mov rax, cr3
    mov cr3, rax

    // Restore RSP (stack is in HHDM, so it's identity-mapped and unaffected)
    mov rsp, [rsp - 8]

    // Jump to new RIP (relocated trampoline_continue)
    jmp qword ptr [rsp - 16]

// kaslr_trampoline_continue:
// This continues execution at the relocated address
kaslr_trampoline_continue:
    // Re-enable interrupts
    sti

    // Jump to the actual kernel entry point (relocated)
    // The slide is in RSI, entry point is LINK_BASE + _start offset
    lea rax, [rip + kaslr_kernel_entry]
    add rax, rsi                // rax = _start + slide
    jmp rax

kaslr_kernel_entry:
    // This symbol will be resolved to _start + slide at runtime
    // We'll patch this at runtime via the relocation mechanism
    .quad 0xFFFFFFFF80000000    // Placeholder for LINK_BASE

.size kaslr_activate_mapping, . - kaslr_activate_mapping
"#
);

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
    if slide == 0 {
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
    let shstrtab = &*shdr_base.add(shstrndx);
    let shstrtab_data = core::slice::from_raw_parts(
        (LINK_BASE + shdr.sh_offset) as *const u8,
        shstrtab.sh_size as usize,
    );

    // Iterate through section headers to find RELA sections
    for i in 0..ehdr.e_shnum as usize {
        let shdr = &*shdr_base.add(i);
        if shdr.sh_type != 4 { // SHT_RELA = 4
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
            let rela_end = (rela_start as usize + (rela_count as usize) * core::mem::size_of::<Elf64Rela>()) as *const Elf64Rela;

            apply_relocations_section(rela_start, rela_end, slide);
        }
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
    if slide == 0 {
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
    let shstrtab = &*shdr_base.add(shstrndx);
    let shstrtab_data = core::slice::from_raw_parts(
        (LINK_BASE + shdr.sh_offset) as *const u8,
        shstrtab.sh_size as usize,
    );

    // Iterate through section headers to find RELA sections
    for i in 0..ehdr.e_shnum as usize {
        let shdr = &*shdr_base.add(i);
        if shdr.sh_type != 4 { // SHT_RELA = 4
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
            let rela_end = (rela_start as usize + (rela_count as usize) * core::mem::size_of::<Elf64Rela>()) as *const Elf64Rela;

            apply_relocations_section(rela_start, rela_end, slide);
        }
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
    if slide == 0 {
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
    let shstrtab = &*shdr_base.add(shstrndx);
    let shstrtab_data = core::slice::from_raw_parts(
        (LINK_BASE + shdr.sh_offset) as *const u8,
        shstrtab.sh_size as usize,
    );

    // Iterate through section headers to find RELA sections
    for i in 0..ehdr.e_shnum as usize {
        let shdr = &*shdr_base.add(i);
        if shdr.sh_type != 4 { // SHT_RELA = 4
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
            let rela_end = (rela_start as usize + (rela_count as usize) * core::mem::size_of::<Elf64Rela>()) as *const Elf64Rela;

            apply_relocations_section(rela_start, rela_end, slide);
        }
    }
}

/// Get the current KASLR slide value
pub fn get_kaslr_slide() -> u64 {
    crate::KERNEL_SLIDE.load(core::sync::atomic::Ordering::Relaxed)
}