#![no_std]
#![no_main]
extern crate alloc;
use coreutils::{eprint, cstr, print};
use alloc::string::String;
#[global_allocator]
static A: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }
fn getenv(name: &str, envp: *const *const u8) -> Option<String> {
    if envp.is_null() { return None; }
    unsafe {
        let mut i = 0;
        while !(*envp.add(i)).is_null() {
            let entry = core::ffi::CStr::from_ptr(*envp.add(i) as *const i8).to_str().unwrap_or("");
            if let Some(pos) = entry.find('=') {
                if &entry[..pos] == name {
                    return Some(entry[pos+1..].into());
                }
            }
            i += 1;
        }
    }
    None
}
fn search_path(cmd: &str, envp: *const *const u8) -> Option<String> {
    let path = getenv("PATH", envp).unwrap_or_else(|| "/bin".into());
    for dir in path.split(':') {
        let full = alloc::format!("{}/{}\0", dir, cmd);
        let c = cstr(&full);
        let fd = skyos_libc::syscall::open(c.as_ptr() as *const u8, 0);
        if fd < 0xFFFF_FFFF_FFFF_FF00 { skyos_libc::syscall::close(fd); return Some(alloc::format!("{}/{}", dir, cmd)); }
    }
    None
}
#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    if _argc < 2 { eprint("which: missing command\n"); return 1; }
    let envp: *const *const u8 = unsafe { _argv.add((_argc as usize) + 1) };
    let mut found = false;
    for i in 1.._argc {
        let cmd = unsafe { core::ffi::CStr::from_ptr(*_argv.add(i as usize) as *const i8).to_str().unwrap_or("") };
        if let Some(path) = search_path(cmd, envp) {
            print(&path); print("\n"); found = true;
        }
    }
    if !found { return 1; }
    0
}
