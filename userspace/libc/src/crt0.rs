use core::arch::asm;

extern "C" {
    fn main(argc: u64, argv: *const *const u8) -> i32;
}

#[no_mangle]
#[link_section = ".text.startup"]
pub extern "C" fn _start() -> ! {
    let argc: u64;
    let argv: *const *const u8;
    unsafe {
        let rsp: *const u64;
        asm!("mov {}, rsp", out(reg) rsp);
        argc = *rsp;
        argv = rsp.add(1) as *const *const u8;
    }
    let code = unsafe { main(argc, argv) };
    crate::syscall::exit(code);
}
