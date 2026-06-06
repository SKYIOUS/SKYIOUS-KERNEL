#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

fn read_dir(path: &str) -> Option<Vec<String>> {
    let cpath = alloc::ffi::CString::new(path).ok()?;
    let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
    if (fd as i64) < 0 { return None; }

    let mut entries = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = skyos_libc::syscall::getdents64(fd, buf.as_mut_ptr(), 4096);
        if (n as i64) <= 0 { break; }
        let mut off = 0;
        while off < n as usize {
            let d_ino = u64::from_ne_bytes(buf[off..off+8].try_into().unwrap());
            let d_reclen = u16::from_ne_bytes(buf[off+16..off+18].try_into().unwrap()) as usize;
            let name_start = off + 19;
            let name_end = buf[name_start..].iter().position(|&c| c == 0).unwrap_or(off + d_reclen);
            if d_ino != 0 {
                let name = String::from_utf8_lossy(&buf[name_start..name_end]).into_owned();
                if name != "." && name != ".." {
                    entries.push(name);
                }
            }
            off += d_reclen;
        }
    }
    skyos_libc::syscall::close(fd);
    Some(entries)
}

fn file_size(path: &str) -> u64 {
    let cpath = alloc::ffi::CString::new(path).ok().unwrap_or_default();
    if cpath.as_bytes().is_empty() { return 0; }
    let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
    if (fd as i64) < 0 { return 0; }
    let mut st = [0u8; 56];
    let ret = skyos_libc::syscall::fstat(fd, st.as_mut_ptr());
    skyos_libc::syscall::close(fd);
    if (ret as i64) < 0 { return 0; }
    let size = i64::from_ne_bytes(st[40..48].try_into().unwrap());
    if size < 0 { 0 } else { size as u64 }
}

fn walk_dir(path: &str) -> (u64, u64) {
    let mut total_size = file_size(path);
    let mut total_blocks = 0u64;
    if let Some(entries) = read_dir(path) {
        for entry in &entries {
            let full = if path == "/" {
                alloc::format!("/{}", entry)
            } else {
                alloc::format!("{}/{}", path, entry)
            };
            let cpath = alloc::ffi::CString::new(full.as_str()).unwrap();
            let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
            if (fd as i64) < 0 { continue; }
            let mut st = [0u8; 56];
            let ret = skyos_libc::syscall::fstat(fd, st.as_mut_ptr());
            skyos_libc::syscall::close(fd);
            if (ret as i64) < 0 { continue; }
            let mode = u32::from_ne_bytes(st[16..20].try_into().unwrap());
            if mode & 0o040000 != 0 {
                let (size, blocks) = walk_dir(&full);
                total_size += size;
                total_blocks += blocks;
                let msg = alloc::format!("{}\t{}{}\n", (size + 1023) / 1024, full, if entry.ends_with('/') { "" } else { "/" });
                skyos_libc::syscall::write(1, msg.as_bytes());
            } else {
                let sz = i64::from_ne_bytes(st[40..48].try_into().unwrap());
                let sz_u = if sz < 0 { 0 } else { sz as u64 };
                total_size += sz_u;
                total_blocks += (sz_u + 511) / 512;
            }
        }
    }
    (total_size, total_blocks)
}

#[no_mangle]
pub extern "C" fn main(argc: u64, argv: *const *const u8) -> i32 {
    let path = if argc > 1 {
        unsafe {
            let ptr = *argv.add(1);
            if ptr.is_null() { "." }
            else {
                match core::ffi::CStr::from_ptr(ptr as *const i8).to_str() {
                    Ok(s) => s,
                    Err(_) => ".",
                }
            }
        }
    } else { "." };
    let (size, _blocks) = walk_dir(path);
    let msg = alloc::format!("{}\t{}\n", (size + 1023) / 1024, path);
    skyos_libc::syscall::write(1, msg.as_bytes());
    0
}
