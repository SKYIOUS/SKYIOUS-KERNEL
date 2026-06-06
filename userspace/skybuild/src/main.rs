#![no_std]
#![no_main]

extern crate alloc;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use skyos_libc::syscall;

fn puts(s: &str) { syscall::write(1, s.as_bytes()); syscall::write(1, b"\n"); }
fn read_file(path: &str) -> String {
    let cpath = alloc::ffi::CString::new(path).ok();
    if cpath.is_none() { return String::new(); }
    let fd = syscall::open(cpath.unwrap().as_ptr() as *const u8, 0);
    if (fd as i64) < 0 { return String::new(); }
    let mut buf = [0u8; 4096];
    let n = syscall::read(fd, &mut buf);
    syscall::close(fd);
    if (n as i64) > 0 { String::from_utf8_lossy(&buf[..n as usize]).into_owned() } else { String::new() }
}

fn run_cmd(cmd: &str, args: &[&str]) -> i64 {
    let cpath = alloc::ffi::CString::new(cmd).ok();
    if cpath.is_none() { return -1; }
    let path_ptr = cpath.unwrap().into_raw() as *const u8;
    let mut argv = Vec::new();
    argv.push(path_ptr);
    for a in args {
        if let Ok(cs) = alloc::ffi::CString::new(*a) {
            argv.push(cs.into_raw() as *const u8);
        }
    }
    argv.push(core::ptr::null());
    let pid = syscall::fork();
    if pid == 0 {
        unsafe { syscall::execve(path_ptr, argv.as_ptr() as *const *const u8, core::ptr::null()); }
        0
    } else if (pid as i64) > 0 {
        let mut status: i32 = 0;
        syscall::wait4(pid as i64, &mut status, 0, core::ptr::null_mut());
        (status as u64 & 0xFF) as i64
    } else { -1 }
}

fn cmd_build(args: &[&str]) {
    if args.is_empty() { puts("Usage: skybuild <recipe>"); return; }
    let recipe = read_file(args[0]);
    if recipe.is_empty() { puts("Cannot read recipe"); return; }
    puts(&alloc::format!("Building from: {}", args[0]));
    puts("Running skypkg build...");
    let status = run_cmd("/bin/skypkg", &["build", args[0]]);
    if status == 0 {
        puts("Build succeeded.");
    } else {
        puts(&alloc::format!("Build failed (status {})", status));
    }
}

fn cmd_new(args: &[&str]) {
    let name = if args.is_empty() { "myapp" } else { args[0] };
    let recipe = alloc::format!(
        "name=\"{}\"\nversion=\"1.0.0\"\ndescription=\"A SkyOS application\"\narch=\"x86_64\"\nlicense=\"MIT\"\ndeps=\"\"\nmaintainer=\"developer\"\nsize=0\nsha256=\"\"\npayload:\n",
        name
    );
    let cpath = alloc::ffi::CString::new(name).ok();
    if cpath.is_none() { puts("Invalid name"); return; }
    let fd = syscall::open(cpath.unwrap().as_ptr() as *const u8, 0x42);
    if (fd as i64) < 0 { puts("Cannot create recipe"); return; }
    syscall::write(fd, recipe.as_bytes());
    syscall::close(fd);
    puts(&alloc::format!("Created recipe: {}", name));
}

fn cmd_init(_args: &[&str]) {
    puts("SkyOS Developer Toolchain (skybuild)");
    puts("");
    puts("Available:");
    puts("  skybuild new <name>    Create a new recipe scaffold");
    puts("  skybuild build <file>  Build a recipe into .skp");
    puts("  skybuild sysroot       Show SDK path");
    puts("  skybuild info          Show toolchain info");
}

fn cmd_sysroot(_args: &[&str]) {
    puts("/usr");
    puts("");
    puts("SDK structure:");
    puts("  /usr/include/   C/C++ headers");
    puts("  /usr/lib/       Static libraries");
    puts("  /usr/share/     Shared data");
    puts("  /bin/           Build tools");
}

fn cmd_info(_args: &[&str]) {
    puts("SkyOS Developer Toolchain");
    puts("Target: x86_64-skyos");
    puts("C Library: skyos-libc (Rust, no_std)");
    puts("Package format: .skp (skypkg)");
    puts("Build system: Cargo + custom target JSON");
}

#[no_mangle]
pub extern "C" fn main(_argc: u64, argv: *const *const u8) -> i32 {
    let mut args = Vec::new();
    if !argv.is_null() {
        for i in 1.. {
            let ptr = unsafe { argv.offset(i as isize) };
            if unsafe { *ptr }.is_null() { break; }
            let s = unsafe { core::ffi::CStr::from_ptr(*ptr as *const i8) }.to_str().unwrap_or("").to_string();
            args.push(s);
        }
    }
    let args_str: Vec<&str> = args.iter().map(|s: &String| s.as_str()).collect();
    let cmd = args_str.first().copied().unwrap_or("");
    match cmd {
        "build" => cmd_build(&args_str[1..]),
        "new" => cmd_new(&args_str[1..]),
        "sysroot" => cmd_sysroot(&args_str[1..]),
        "info" => cmd_info(&args_str[1..]),
        _ => cmd_init(&args_str[1..]),
    }
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
