#![no_std]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

/// System information structure
pub struct SysInfo {
    pub total_ram_pages: u64,
    pub free_ram_pages: u64,
    pub uptime_seconds: u64,
    pub process_count: u64,
    pub load_avg_1m: u64,
}

/// Get system information via SYS_SYSINFO syscall
pub fn sysinfo() -> Option<SysInfo> {
    let mut buf = [0u64; 5];
    let ret = skyos_libc::syscall::syscall1(
        203, // SYS_SYSINFO
        buf.as_mut_ptr() as u64,
    );
    if ret != 0 { return None; }
    Some(SysInfo {
        total_ram_pages: buf[0],
        free_ram_pages: buf[1],
        uptime_seconds: buf[2],
        process_count: buf[3],
        load_avg_1m: buf[4],
    })
}

/// Get current working directory
pub fn getcwd() -> Option<String> {
    let mut buf = [0u8; 4096];
    let ret = skyos_libc::syscall::getcwd(buf.as_mut_ptr(), 4096);
    if (ret as i64) < 0 { return None; }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf8_lossy(&buf[..len]).into_owned())
}

/// Change current working directory
pub fn chdir(path: &str) -> bool {
    let cpath = match alloc::ffi::CString::new(path) {
        Ok(c) => c,
        Err(_) => return false,
    };
    skyos_libc::syscall::chdir(cpath.as_ptr() as *const u8) == 0
}

/// List directory entries
pub fn list_dir(path: &str) -> Option<Vec<String>> {
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
            let d_off = u64::from_ne_bytes(buf[off+8..off+16].try_into().unwrap());
            let d_reclen = u16::from_ne_bytes(buf[off+16..off+18].try_into().unwrap()) as usize;
            let d_type = buf[off+18];
            let name_start = off + 19;
            let name_end = buf[name_start..].iter().position(|&c| c == 0).map(|p| name_start + p).unwrap_or(off + d_reclen);
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

/// Get the process ID
pub fn getpid() -> u64 {
    skyos_libc::syscall::getpid()
}

/// Sleep for a given number of milliseconds
pub fn sleep_ms(ms: u64) {
    let ts = [ms * 1_000_000, 0u64]; // sec + nsec
    skyos_libc::syscall::nanosleep(
        ts.as_ptr() as *const u8,
        core::ptr::null_mut(),
    );
}

/// Get the system hostname (via uname)
pub mod net;

pub fn hostname() -> Option<String> {
    let mut buf = [0u8; 256];
    let ret = skyos_libc::syscall::syscall1(63, buf.as_mut_ptr() as u64); // SYS_UNAME
    if ret != 0 { return None; }
    // utsname.nodename at offset 65
    let len = buf[65..].iter().position(|&c| c == 0).unwrap_or(0);
    Some(String::from_utf8_lossy(&buf[65..65+len]).into_owned())
}
