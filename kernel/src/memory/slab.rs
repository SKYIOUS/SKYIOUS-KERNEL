use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::ptr::NonNull;
use linked_list_allocator::LockedHeap;

/// The block sizes to use.
/// Must be powers of 2 because they are also used as alignment (except for very small ones).
/// Extended to include smaller sizes for better memory efficiency.
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Slab cache statistics for monitoring allocation patterns.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlabStats {
    pub total_allocs: u64,
    pub total_deallocs: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub active_bytes: u64,
}

/// A node in the linked list of blocks.
struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: LockedHeap,
    poison_on_free: bool, // Configurable memory poisoning
    stats: SlabStats,
}

impl FixedSizeBlockAllocator {
    /// Creates an empty FixedSizeBlockAllocator with poisoning enabled by default.
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LockedHeap::empty(),
            poison_on_free: true,
            stats: SlabStats {
                total_allocs: 0,
                total_deallocs: 0,
                cache_hits: 0,
                cache_misses: 0,
                active_bytes: 0,
            },
        }
    }

    /// Creates an empty FixedSizeBlockAllocator with configurable poisoning.
    #[allow(dead_code)]
    pub const fn with_poisoning(poison: bool) -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LockedHeap::empty(),
            poison_on_free: poison,
            stats: SlabStats {
                total_allocs: 0,
                total_deallocs: 0,
                cache_hits: 0,
                cache_misses: 0,
                active_bytes: 0,
            },
        }
    }

    /// Initializes the allocator with the given heap bounds.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.lock().init(heap_start as *mut u8, heap_size);
    }

    /// Allocates using the fallback allocator.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.lock().allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => ptr::null_mut(),
        }
    }

    /// Get current slab allocator statistics.
    pub fn stats(&self) -> SlabStats {
        self.stats
    }

    /// Get the number of free blocks in each size class.
    pub fn free_counts(&self) -> [usize; BLOCK_SIZES.len()] {
        let mut counts = [0usize; BLOCK_SIZES.len()];
        for (i, head) in self.list_heads.iter().enumerate() {
            let mut count = 0;
            let mut current = head;
            while let Some(node) = current {
                count += 1;
                current = &node.next;
            }
            counts[i] = count;
        }
        counts
    }
}

/// Choose an appropriate block size for the given layout.
/// Returns an index into the `BLOCK_SIZES` array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

/// A wrapper around crate::sync::IrqSafeMutex to permit trait implementations.
pub struct Locked<A> {
    inner: crate::sync::IrqSafeMutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: crate::sync::IrqSafeMutex::new(inner),
        }
    }

    pub fn lock(&self) -> crate::sync::IrqSafeMutexGuard<'_, A> {
        self.inner.lock()
    }
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        allocator.stats.total_allocs += 1;
        match list_index(&layout) {
            Some(index) => {
                match allocator.list_heads[index].take() {
                    Some(node) => {
                        allocator.list_heads[index] = node.next.take();
                        allocator.stats.cache_hits += 1;
                        allocator.stats.active_bytes += BLOCK_SIZES[index] as u64;
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // No free block in list, allocate a new block from fallback
                        let block_size = BLOCK_SIZES[index];
                        let block_layout = Layout::from_size_align(block_size, block_size)
                            .expect("Invalid block size/alignment in slab allocator");
                        let ptr = allocator.fallback_alloc(block_layout);
                        if !ptr.is_null() {
                            allocator.stats.cache_misses += 1;
                            allocator.stats.active_bytes += block_size as u64;
                        }
                        ptr
                    }
                }
            }
            None => {
                let ptr = allocator.fallback_alloc(layout);
                if !ptr.is_null() {
                    allocator.stats.cache_misses += 1;
                    allocator.stats.active_bytes += layout.size() as u64;
                }
                ptr
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        allocator.stats.total_deallocs += 1;
        match list_index(&layout) {
            Some(index) => {
                if allocator.poison_on_free {
                    let poison = core::slice::from_raw_parts_mut(ptr, BLOCK_SIZES[index]);
                    for b in poison.iter_mut() { *b = 0xDE; }
                }
                let new_node = ListNode {
                    next: allocator.list_heads[index].take(),
                };
                let new_node_ptr = ptr as *mut ListNode;
                new_node_ptr.write(new_node);
                allocator.list_heads[index] = Some(&mut *new_node_ptr);
                allocator.stats.active_bytes = allocator.stats.active_bytes.saturating_sub(BLOCK_SIZES[index] as u64);
            }
            None => {
                if allocator.poison_on_free {
                    let cap = layout.size().min(1024 * 1024);
                    let poison = core::slice::from_raw_parts_mut(ptr, cap);
                    for b in poison.iter_mut() { *b = 0xDE; }
                }
                allocator.stats.active_bytes = allocator.stats.active_bytes.saturating_sub(layout.size() as u64);
                allocator.fallback_allocator.lock().deallocate(
                    NonNull::new(ptr).expect("Deallocating null pointer in slab fallback"),
                    layout
                );
            }
        }
    }
}

