#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

#[no_mangle]
pub extern "C" fn main(argc: u64, argv: *const *const u8) -> i32 {
    let mut freq: u32 = 1000;
    let mut dur: u32 = 200;
    let mut i = 1;
    while (i as u64) < argc {
        let arg = unsafe {
            let ptr = *argv.add(i as usize);
            if ptr.is_null() { break; }
            core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("")
        };
        match arg {
            "-f" => {
                i += 1;
                if (i as u64) < argc {
                    let val = unsafe {
                        let ptr = *argv.add(i as usize);
                        core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("1000")
                    };
                    freq = val.parse::<u32>().unwrap_or(1000);
                }
            }
            "-l" => {
                i += 1;
                if (i as u64) < argc {
                    let val = unsafe {
                        let ptr = *argv.add(i as usize);
                        core::ffi::CStr::from_ptr(ptr as *const i8).to_str().unwrap_or("200")
                    };
                    dur = val.parse::<u32>().unwrap_or(200);
                }
            }
            "-h" | "--help" => {
                let msg = "Usage: beep [-f freq_hz] [-l duration_ms]\n";
                skyos_libc::syscall::write(1, msg.as_bytes());
                return 0;
            }
            _ => {}
        }
        i += 1;
    }
    skyos_libc::syscall::beep(freq, dur);
    0
}
