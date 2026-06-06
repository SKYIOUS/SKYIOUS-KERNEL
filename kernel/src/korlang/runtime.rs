//! Korlang Resident Runtime for Vahi Kernel
//!
//! This module provides the `extern "C"` symbols expected by Korlang-compiled binaries.
//! It bridges the Korlang runtime ABI to Vahi kernel services.

use alloc::alloc::{alloc, dealloc, Layout};
use core::slice;
use core::str;

#[no_mangle]
pub extern "C" fn korlang_alloc(size: usize, align: usize) -> *mut u8 {
    if size == 0 { return core::ptr::null_mut(); }
    let layout = Layout::from_size_align(size, align.max(1)).unwrap();
    unsafe { alloc(layout) }
}

#[no_mangle]
pub extern "C" fn korlang_free(ptr: *mut u8, size: usize, align: usize) {
    if ptr.is_null() || size == 0 { return; }
    let layout = Layout::from_size_align(size, align.max(1)).unwrap();
    unsafe { dealloc(ptr, layout) }
}

#[no_mangle]
pub extern "C" fn korlang_gc_alloc(size: usize, align: usize) -> *mut u8 {
    // For now, GC alloc is just normal heap alloc
    korlang_alloc(size, align)
}

#[no_mangle]
pub extern "C" fn _kor_stdout_write(ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 { return; }
    let s = unsafe { slice::from_raw_parts(ptr, len) };
    if let Ok(st) = str::from_utf8(s) {
        crate::print!("{}", st);
    }
}

#[no_mangle]
pub extern "C" fn _kor_stderr_write(ptr: *const u8, len: usize) {
    _kor_stdout_write(ptr, len); // Map to same console for now
}

#[no_mangle]
pub extern "C" fn _kor_panic(ptr: *const u8, len: usize) -> ! {
    let msg = if !ptr.is_null() && len > 0 {
        let s = unsafe { slice::from_raw_parts(ptr, len) };
        str::from_utf8(s).unwrap_or("Unknown Korlang panic")
    } else {
        "Korlang panic"
    };
    panic!("KORLANG PANIC: {}", msg);
}

#[no_mangle]
pub extern "C" fn _kor_file_open(path_ptr: *const u8, path_len: usize) -> i64 {
    if path_ptr.is_null() || path_len == 0 { return -1; }
    let path_slice = unsafe { slice::from_raw_parts(path_ptr, path_len) };
    let path = str::from_utf8(path_slice).unwrap_or("");
    
    // Use VFS to open
    if let Some(_node) = crate::vfs::VFS.lock().resolve_path(path) {
        // In a real implementation, we'd add to the current process's FD table
        // For now, we'll return a dummy handle or use a global table if available
        // (Wait, we have the syscall FD table!)
        // However, this is kernel-resident, so we should probably use the same logic 
        // as the 'open' syscall.
        
        // For simplicity in Phase H1, let's just use a placeholder
        crate::println!("[KORLANG] Opening file: {}", path);
        return 100; // Placeholder handle
    }
    -1
}

#[no_mangle]
pub extern "C" fn _kor_file_close(_handle: i64) {
    // Stub
}

#[no_mangle]
pub extern "C" fn _kor_file_read(_handle: i64, _buf: *mut u8, _len: usize) -> i64 {
    0 // Stub
}

#[no_mangle]
pub extern "C" fn _kor_file_write(handle: i64, ptr: *const u8, len: usize) -> i64 {
    if handle == 1 { // stdout
        _kor_stdout_write(ptr, len);
        return len as i64;
    }
    -1
}
