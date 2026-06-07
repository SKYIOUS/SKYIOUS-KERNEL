# Memory Protection Mechanisms

SkyOS uses hardware and software mechanisms to protect memory.

## Hardware Paging

The x86_64 paging architecture provides page-level access control:

```rust
pub enum PageFlags {
    READABLE = 1 << 0,
    WRITABLE = 1 << 1,
    USER_ACCESSIBLE = 1 << 2,
    WRITE_THROUGH = 1 << 3,
    CACHE_DISABLE = 1 << 4,
    ACCESSED = 1 << 5,
    DIRTY = 1 << 6,
    HUGE_PAGE = 1 << 7,
    GLOBAL = 1 << 8,
    NO_EXECUTE = 1 << 63,
}
```

## NX Bit (No-Execute)

The NX bit (bit 63 in page table entries) marks memory pages as non-executable. The kernel ensures:
- Data pages (stack, heap, BSS) are never executable
- Code pages are writable only during loading, then set to read-only + executable
- Guard pages have no permissions (prevent overflow)

## SMEP (Supervisor Mode Execution Prevention)

SMEP prevents the kernel from executing code in userspace pages. If the kernel tries to execute a page marked as user-accessible, a page fault is generated. This prevents ROP attacks that redirect kernel code to userspace shellcode.

## SMAP (Supervisor Mode Access Prevention)

SMAP prevents the kernel from reading or writing userspace memory directly. The kernel must explicitly disable SMAP (via the `AC` flag in RFLAGS) to access userspace data. This prevents accidental dereferencing of userspace pointers.

## Kernel Page Table Isolation (KPTI)

KPTI separates kernel and userspace page tables. When running in userspace, only the minimal set of kernel mappings is present (interrupt handlers, syscall entry). This prevents Meltdown-style side-channel attacks.

## Guard Pages

Guard pages are unmapped memory regions placed at:
- Bottom of the stack (detect stack overflow)
- Between heap regions (detect heap overflow)
- Around kernel stacks (detect kernel stack overflow)

## Memory Sanitizers

Debug builds include:
- **KASAN**: Tracks use-after-free and out-of-bounds access
- **Stack probing**: Detects stack overflow at runtime
- **Memory poisoning**: Marks freed memory with a pattern to detect use-after-free
