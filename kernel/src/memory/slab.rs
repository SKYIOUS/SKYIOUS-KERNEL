use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use linked_list_allocator::LockedHeap;

/// The block sizes to use.
/// Must be powers of 2 because they are also used as alignment (except for very small ones).
const BLOCK_SIZES: &[usize] = &[32, 64, 128, 256, 512, 1024, 2048];

/// A node in the linked list of blocks.
struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: LockedHeap,
}

impl FixedSizeBlockAllocator {
    /// Creates an empty FixedSizeBlockAllocator.
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: LockedHeap::empty(),
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
}

/// Choose an appropriate block size for the given layout.
/// Returns an index into the `BLOCK_SIZES` array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

/// A wrapper around spin::Mutex to permit trait implementations.
pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> spin::MutexGuard<'_, A> {
        self.inner.lock()
    }
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                match allocator.list_heads[index].take() {
                    Some(node) => {
                        allocator.list_heads[index] = node.next.take();
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // No free block in list, allocate a new block from fallback
                        let block_size = BLOCK_SIZES[index];
                        // try to allocate a block with alignment of block_size
                        let block_layout = Layout::from_size_align(block_size, block_size)
                            .expect("Invalid block size/alignment in slab allocator");
                        allocator.fallback_alloc(block_layout)
                    }
                }
            }
            None => allocator.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                // Poison freed block
                let poison = core::slice::from_raw_parts_mut(ptr, BLOCK_SIZES[index]);
                for b in poison.iter_mut() { *b = 0xDE; }
                let new_node = ListNode {
                    next: allocator.list_heads[index].take(),
                };
                let new_node_ptr = ptr as *mut ListNode;
                new_node_ptr.write(new_node);
                allocator.list_heads[index] = Some(&mut *new_node_ptr);
            }
            None => {
                // Poison freed large block (up to layout size)
                let poison = core::slice::from_raw_parts_mut(ptr, layout.size());
                for b in poison.iter_mut() { *b = 0xDE; }
                allocator.fallback_allocator.lock().deallocate(
                    NonNull::new(ptr).expect("Deallocating null pointer in slab fallback"), 
                    layout
                );
            }
        }
    }
}

use core::ptr::NonNull;
