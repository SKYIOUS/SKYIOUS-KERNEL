# Memory Safety Approach in Kernel Space

Ensuring memory safety in kernel space requires strategies beyond what userspace Rust provides.

## The Challenge

Kernel code must manipulate hardware registers, construct page tables, and manage physical memory—all of which requires unsafe Rust. The challenge is to confine unsafe code to minimal, auditable regions.

## Safety Strategy

### 1. Encapsulation

Unsafe operations are wrapped in safe abstractions. For example, page table manipulation is exposed through a `PageTable` type that guarantees:

```rust
impl PageTable {
    /// Safety: The caller must ensure `phys_addr` is a valid mapped frame
    pub unsafe fn map_page(&mut self, virt: VirtAddr, phys: PhysAddr, flags: PageFlags) -> Result<()>;
    
    // Safe wrapper
    pub fn allocate_and_map(&mut self, virt: VirtAddr, size: usize) -> Result<()> {
        let frame = frame_allocator::allocate(size)?;
        unsafe { self.map_page(virt, frame.addr(), PageFlags::READABLE | PageFlags::WRITABLE) }
    }
}
```

### 2. Invariant Documentation

Every unsafe function includes a `Safety:` section documenting the caller's responsibilities. These invariants are checked during code review.

### 3. Runtime Checking

Debug builds include runtime assertions for safety invariants. For example, pointer validity is checked before dereference:

```rust
fn read_u32(ptr: *const u32) -> u32 {
    debug_assert!(!ptr.is_null() && (ptr as usize) % 4 == 0);
    unsafe { *ptr }
}
```

### 4. KASAN Integration

The Kernel Address Sanitizer detects use-after-free and out-of-bounds accesses in debug builds by maintaining shadow memory that tracks allocation status.

### 5. Type System Enforcement

The type system prevents common errors:
- `PhysFrame` vs `VirtAddr`: Different types prevent address space confusion
- `MappedPages` vs `UnmappedPages`: State transitions are enforced at compile time
- `IoPort` vs `MmioRegion`: Different access methods are type-checked
