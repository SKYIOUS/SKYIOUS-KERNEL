// ── 4-level paging for x86-64: init, identity map 1 GiB, demand-paging PF handler ──
//
// Memory layout after paging_init():
//   0x00000000 – 0x3FFFFFFF    identity-mapped (1 GiB, 2 MiB pages)
//   0xFFFF8000_00000000 – …    phys-map window (1-to-1 linear for page-table walks)
//
// ponytail: single PML4, no ASID/KPTI; add per-process address spaces later.

#include "paging.h"
#include <stddef.h>
#include <string.h>

#define PHYS_OFFSET 0xFFFF800000000000ULL        // ponytail: fixed phys-map offset; match your kernel's

// ── physical frame allocator (bump) ──
// ponytail: bump allocator; replace with buddy if kernel grows beyond 64 MiB
static uint64_t bump_base = 0x1000000;          // start at 16 MiB
static uint64_t bump_end   = 0x4000000;          // end at 64 MiB  (enough for identity-map + page tables)

uint64_t alloc_frame(void) {
    uint64_t addr = __sync_fetch_and_add(&bump_base, 0x1000);
    if (addr >= bump_end) return 0;              // OOM
    memset((void *)(addr + PHYS_OFFSET), 0, 0x1000);  // zero via phys-map window
    return addr;
}

// ── page-table walking ──


static inline uint64_t read_cr2(void) {
    uint64_t v;
    __asm__ __volatile__("mov %%cr2, %0" : "=r"(v));
    return v;
}

static inline uint64_t read_cr3(void) {
    uint64_t v;
    __asm__ __volatile__("mov %%cr3, %0" : "=r"(v));
    return v;
}

static inline void invlpg(uint64_t virt) {
    __asm__ __volatile__("invlpg (%0)" : : "r"(virt) : "memory");
}

static page_table_t *table_at(uint64_t phys) {
    return (page_table_t *)(phys + PHYS_OFFSET);
}

// ── get_pte: walk 4-level page tables ──
// If `create` is set, allocate intermediate tables that are missing.
pte_t *get_pte(uint64_t virt, int create) {
    uint64_t offsets[4] = {
        (virt >> 39) & 0x1FF,   // PML4 index
        (virt >> 30) & 0x1FF,   // PDPT index
        (virt >> 21) & 0x1FF,   // PD index
        (virt >> 12) & 0x1FF,   // PT index
    };
    page_table_t *tables[5];
    tables[0] = pml4;                           // PML4 is always mapped

    for (int level = 0; level < 4; level++) {
        pte_t *e = &tables[level]->entry[offsets[level]];
        if (level == 3) return e;               // leaf level → return PTE

        // If this is a huge-page entry, return it directly
        if (*e & PTE_HUGE) return e;

        uint64_t next_phys = *e & 0x000FFFFFFFFFF000ULL;
        if (!next_phys) {
            if (!create) return NULL;
            next_phys = alloc_frame();
            if (!next_phys) return NULL;
            *e = next_phys | PTE_PRESENT | PTE_RW | PTE_USER;
        }
        tables[level + 1] = table_at(next_phys);
    }
    return NULL;                                // unreachable
}

// ── map a single 4 KiB page ──
void map_page(uint64_t virt, uint64_t phys, uint64_t flags) {
    pte_t *pte = get_pte(virt, 1);
    *pte = (phys & 0x000FFFFFFFFFF000ULL) | flags | PTE_PRESENT;
    invlpg(virt);
}

// ── identity-map the first 1 GiB ──
// Use 2 MiB pages for speed; first 2 MiB split into 4 KiB pages so the
// low 64 KiB (IVT/BDA) can have different attributes.
void paging_init(void) {
    // 1. Allocate PML4
    pml4 = table_at(alloc_frame());

    // 2. PML4[0] → PDPT at physical 0x20000
    uint64_t pdpt_phys = alloc_frame();
    pml4->entry[0] = pdpt_phys | PTE_PRESENT | PTE_RW | PTE_USER;

    // 3. PML4[256] = recursive mapping for PML4 itself (optional, not used here)
    // pml4->entry[256] = (uint64_t)pml4 - PHYS_OFFSET | PTE_PRESENT | PTE_RW;

    page_table_t *pdpt = table_at(pdpt_phys);

    // 4. Identity-map 1 GiB: 1 PDE covers 1 GiB with PS=1 (huge page).
    //    But the first PD must use 4 KiB entries for the lowest 2 MiB.
    // ponytail: skipping the first-PD split here; a real kernel needs it for
    //           legacy VGA/BIOS regions.  Everything gets 2 MiB pages.

    for (int pdpt_idx = 0; pdpt_idx < 4; pdpt_idx++) {
        // Each PDPT entry covers 1 GiB → point to a PD with huge-page entries
        uint64_t pd_phys = alloc_frame();
        pdpt->entry[pdpt_idx] = pd_phys | PTE_PRESENT | PTE_RW | PTE_USER;
        page_table_t *pd = table_at(pd_phys);

        for (int pd_idx = 0; pd_idx < 512; pd_idx++) {
            uint64_t base = ((uint64_t)pdpt_idx << 30) | ((uint64_t)pd_idx << 21);
            // ponytail: first 4 KiB page is identity-mapped as 2 MiB too.
            //           Split to 4 KiB if you need separate attributes for 0-64K.
            pd->entry[pd_idx] = base | PTE_PRESENT | PTE_RW | PTE_HUGE | PTE_GLOBAL | PTE_NX;
        }
    }

    // 5. Create phys-map window at PHYS_OFFSET
    // ponytail: simple recopy of the same PD entries at high PML4 slots.
    //           Production kernels rebuild with 1 GiB huge pages for speed.
    pml4->entry[511] = pml4->entry[0];           // map phys-map to same PDPT as low 1 GiB
    // ponytail: PML4 entries 256..510 unmapped — add when kernel needs more VA

    // 6. Install page-fault handler in IDT (stub below)
    //    (IDT setup omitted — depends on your kernel's interrupt infrastructure)
}

// ── enable paging: set CR3, set PG bit ──
// Call this if the bootloader hasn't already enabled long-mode paging.
// You must be in protected mode with PAE enabled before calling this.
void paging_enable(void) {
    uint64_t cr3_val = (uint64_t)pml4 - PHYS_OFFSET;
    __asm__ __volatile__(
        "mov %0, %%cr3\n"                    // load PML4
        "mov %%cr4, %%rax\n"
        "bts $5, %%rax\n"                    // set PAE
        "mov %%rax, %%cr4\n"
        "mov $0xC0000080, %%ecx\n"           // EFER MSR
        "rdmsr\n"
        "bts $8, %%eax\n"                    // set LME
        "wrmsr\n"
        "mov %%cr0, %%rax\n"
        "bts $31, %%rax\n"                   // set PG
        "mov %%rax, %%cr0\n"
        : : "r"(cr3_val) : "rax", "rcx", "cc", "memory");
    // After this, a far jump to a 64-bit code segment is needed.  Example:
    //   lgdt [gdt64_ptr]
    //   push $0x08; push $offset _start64; retfq
    // See startup.S for the full sequence.
}

// ── page-fault handler (called from asm stub) ──
// Reads CR2, allocates frame on non-present fault, maps it.
void page_fault_handler(void) {
    uint64_t fault_addr = read_cr2();

    // Read error code from stack (pushed by interrupt) — ponytail: assume
    // the assembly stub pushes the error code before calling this function.
    // For now we just check the P flag from CR2 heuristics.

    pte_t *existing = get_pte(fault_addr, 0);
    if (existing && (*existing & PTE_PRESENT)) {
        // ponytail: non-present check failed — this is a protection fault,
        //           segmentation fault, or OOM.  Kill the faulting process.
        //           Not implemented in this demo.
        for (;;) __asm__ __volatile__("hlt");
    }

    // Demand paging: allocate a new frame and map it RW + USER.
    uint64_t frame = alloc_frame();
    if (!frame) {
        for (;;) __asm__ __volatile__("hlt");   // OOM
    }

    // ponytail: default flags RW|USER|NX; override per VMA if you have one
    map_page(fault_addr & ~0xFFFULL, frame,
             PTE_RW | PTE_USER | PTE_NX | PTE_ACCESSED);

    invlpg(fault_addr & ~0xFFFULL);
}
