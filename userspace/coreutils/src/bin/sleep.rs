#![no_std]
#![no_main]

extern crate alloc;


#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    let secs = if _argc < 2 { 1 } else {
        let s = unsafe {
            let ptr = *_argv.add(1);
            if ptr.is_null() { "1" } else {
                core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("1")
            }
        };
        s.parse::<u64>().unwrap_or(1)
    };
    let ts = [0u64; 2];
    let req = [secs * 1_000_000_000, 0u64];
    let _ = skyos_libc::syscall::syscall2(skyos_libc::SYS_NANOSLEEP, req.as_ptr() as u64, ts.as_ptr() as u64);
    0
}
