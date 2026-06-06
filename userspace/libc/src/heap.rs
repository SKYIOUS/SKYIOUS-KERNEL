use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

const HEAP_START: u64 = 0x6000_0000_0000;
const MIN_BLOCK: usize = 32;

struct BlockHeader {
    magic: u64,
    size: usize,
    free: bool,
    next: *mut BlockHeader,
    prev: *mut BlockHeader,
}

const MAGIC_FREE: u64 = 0x46726565426C6F5F; // "FreeBlo_"
const MAGIC_USED: u64 = 0x55736564426C6F5F; // "UsedBlo_"

pub struct Heap {
    first: AtomicUsize,
    initialized: core::sync::atomic::AtomicBool,
}

impl Heap {
    pub const fn new() -> Self {
        Heap {
            first: AtomicUsize::new(0),
            initialized: core::sync::atomic::AtomicBool::new(false),
        }
    }

    fn init(&self) {
        let start = crate::syscall::sbrk(0);
        if start == 0 { return; }
        let heap_start = if start < HEAP_START { HEAP_START } else { start };
        let _actual = crate::syscall::brk(heap_start + 64 * 1024 * 1024);
        let block = heap_start as *mut BlockHeader;
        unsafe {
            (*block).magic = MAGIC_FREE;
            (*block).size = (64 * 1024 * 1024) - core::mem::size_of::<BlockHeader>();
            (*block).free = true;
            (*block).next = ptr::null_mut();
            (*block).prev = ptr::null_mut();
        }
        self.first.store(block as usize, Ordering::Release);
        self.initialized.store(true, Ordering::Release);
    }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if !self.initialized.load(Ordering::Acquire) {
            self.init();
        }
        let size = layout.size().max(MIN_BLOCK);
        let first = self.first.load(Ordering::Acquire) as *mut BlockHeader;
        if first.is_null() { return ptr::null_mut(); }

        let mut curr = first;
        while !curr.is_null() {
            if (*curr).magic != MAGIC_FREE && (*curr).magic != MAGIC_USED { break; }
            if (*curr).free && (*curr).size >= size {
                let remaining = (*curr).size - size;
                if remaining >= core::mem::size_of::<BlockHeader>() + MIN_BLOCK {
                    let next_block = (curr as *mut u8).add(core::mem::size_of::<BlockHeader>() + size) as *mut BlockHeader;
                    (*next_block).magic = MAGIC_FREE;
                    (*next_block).size = remaining - core::mem::size_of::<BlockHeader>();
                    (*next_block).free = true;
                    (*next_block).next = (*curr).next;
                    (*next_block).prev = curr;
                    if !(*next_block).next.is_null() {
                        (*(*next_block).next).prev = next_block;
                    }
                    (*curr).next = next_block;
                    (*curr).size = size;
                }
                (*curr).free = false;
                (*curr).magic = MAGIC_USED;
                return (curr as *mut u8).add(core::mem::size_of::<BlockHeader>());
            }
            curr = (*curr).next;
        }
        ptr::null_mut()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() { return; }
        let block = (ptr as *mut BlockHeader).sub(1);
        if (*block).magic != MAGIC_USED { return; }
        (*block).free = true;
        (*block).magic = MAGIC_FREE;
        if !(*block).next.is_null() && (*(*block).next).free {
            let next = (*block).next;
            (*block).size += core::mem::size_of::<BlockHeader>() + (*next).size;
            (*block).next = (*next).next;
            if !(*block).next.is_null() {
                (*(*block).next).prev = block;
            }
        }
        if !(*block).prev.is_null() && (*(*block).prev).free {
            let prev = (*block).prev;
            (*prev).size += core::mem::size_of::<BlockHeader>() + (*block).size;
            (*prev).next = (*block).next;
            if !(*prev).next.is_null() {
                (*(*prev).next).prev = prev;
            }
        }
    }
}

// C-compatible malloc/free/realloc/calloc
#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
    let layout = core::alloc::Layout::from_size_align(size, 1).unwrap_or_else(|_| {
        core::alloc::Layout::from_size_align(0, 1).unwrap()
    });
    unsafe { HEAP_INSTANCE.alloc(layout) }
}

#[no_mangle]
pub extern "C" fn free(ptr: *mut u8) {
    if ptr.is_null() { return; }
    let layout = core::alloc::Layout::from_size_align(0, 1).unwrap();
    unsafe { HEAP_INSTANCE.dealloc(ptr, layout); }
}

#[no_mangle]
pub extern "C" fn calloc(nmemb: usize, size: usize) -> *mut u8 {
    let total = nmemb * size;
    let layout = core::alloc::Layout::from_size_align(total, 1).unwrap_or_else(|_| {
        core::alloc::Layout::from_size_align(0, 1).unwrap()
    });
    let ptr = unsafe { HEAP_INSTANCE.alloc(layout) };
    if !ptr.is_null() {
        unsafe { core::ptr::write_bytes(ptr, 0, total); }
    }
    ptr
}

#[no_mangle]
pub extern "C" fn realloc(ptr: *mut u8, new_size: usize) -> *mut u8 {
    if ptr.is_null() { return malloc(new_size); }
    if new_size == 0 { free(ptr); return core::ptr::null_mut(); }

    let block = unsafe { &*((ptr as *mut BlockHeader).sub(1)) };
    let old_size = block.size;
    if old_size >= new_size { return ptr; }

    let new_ptr = malloc(new_size);
    if !new_ptr.is_null() {
        unsafe { core::ptr::copy_nonoverlapping(ptr, new_ptr, core::cmp::min(old_size, new_size)); }
        free(ptr);
    }
    new_ptr
}

/// Global Heap instance used by C malloc/free and Rust GlobalAlloc.
#[no_mangle]
pub static HEAP_INSTANCE: Heap = Heap::new();
