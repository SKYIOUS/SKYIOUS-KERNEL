#ifndef PAGING_H
#define PAGING_H

#include <stdint.h>

// ── x86-64 page table entry bits ──
#define PTE_PRESENT   0x001UL
#define PTE_RW        0x002UL
#define PTE_USER      0x004UL
#define PTE_WRITETH   0x008UL
#define PTE_NOCACHE   0x010UL
#define PTE_ACCESSED  0x020UL
#define PTE_DIRTY     0x040UL
#define PTE_HUGE      0x080UL          // 2 MiB or 1 GiB page
#define PTE_GLOBAL    0x100UL
#define PTE_NX        0x8000000000000000UL

// ── Page table entry (8 bytes) ──
typedef uint64_t pte_t;

// ── Page table (512 entries) ──
typedef struct { pte_t entry[512]; } page_table_t;

// ── Public API ──
void     paging_init(void);               // identity-map 1 GiB + install PF handler
void     paging_enable(void);             // set CR3, enable PG (if not already)
void     page_fault_handler(void);        // C entry point (called from asm stub)

pte_t   *get_pte(uint64_t virt, int create);
void     map_page(uint64_t virt, uint64_t phys, uint64_t flags);
uint64_t alloc_frame(void);

// ── Externals ──
extern page_table_t *pml4;                // allocated at boot

#endif
