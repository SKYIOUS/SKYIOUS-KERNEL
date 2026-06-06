#![no_std]
#![no_main]
extern crate alloc;
use coreutils::print;
use alloc::string::ToString;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { return 0; }
    let fmt = unsafe { core::ffi::CStr::from_ptr(*_argv.add(1) as *const i8).to_str().unwrap_or("") };
    let mut arg_idx = 2u64;
    let mut last_was_percent = false;
    for c in fmt.chars() {
        if last_was_percent {
            match c {
                's' => {
                    if arg_idx < _argc {
                        let s = unsafe { core::ffi::CStr::from_ptr(*_argv.add(arg_idx as usize) as *const i8).to_str().unwrap_or("") };
                        print(s);
                        arg_idx += 1;
                    }
                }
                'd' | 'i' => {
                    if arg_idx < _argc {
                        let s = unsafe { core::ffi::CStr::from_ptr(*_argv.add(arg_idx as usize) as *const i8).to_str().unwrap_or("") };
                        print(s);
                        arg_idx += 1;
                    }
                }
                'c' => {
                    if arg_idx < _argc {
                        let s = unsafe { core::ffi::CStr::from_ptr(*_argv.add(arg_idx as usize) as *const i8).to_str().unwrap_or("") };
                        if !s.is_empty() { print(&s[..1]); }
                        arg_idx += 1;
                    }
                }
                'n' => { print("\n"); }
                '%' => { print("%"); }
                _ => { print(&c.to_string()); }
            }
            last_was_percent = false;
        } else if c == '%' {
            last_was_percent = true;
        } else if c == '\\' {
            // skip - simple escape handling
        } else {
            let s: alloc::string::String = c.into();
            print(&s);
        }
    }
    0
}
