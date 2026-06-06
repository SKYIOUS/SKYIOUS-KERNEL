#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;

#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

const BLKGETSIZE64: u64 = 0x80081272;
const BLKRD_SEC: u64 = 0x40001260;
const BLKWR_SEC: u64 = 0x40001261;

#[repr(C, packed)]
struct BlkIoctlOp {
    sector: u64,
    count: u64,
    buf: u64,
}

fn get_device_size(fd: u64) -> u64 {
    let mut size: u64 = 0;
    let ret = skyos_libc::syscall::ioctl(fd, BLKGETSIZE64, &mut size as *mut u64 as *mut u8);
    if ret != 0 { return 0; }
    size
}

fn read_sectors(fd: u64, sector: u64, count: u64, buf: &mut [u8]) -> bool {
    let op = BlkIoctlOp { sector, count, buf: buf.as_ptr() as u64 };
    let ret = skyos_libc::syscall::ioctl(fd, BLKRD_SEC, &op as *const _ as *mut u8);
    ret == 0
}

fn write_sectors(fd: u64, sector: u64, count: u64, buf: &[u8]) -> bool {
    let op = BlkIoctlOp { sector, count, buf: buf.as_ptr() as u64 };
    let ret = skyos_libc::syscall::ioctl(fd, BLKWR_SEC, &op as *const _ as *mut u8);
    ret == 0
}

fn parse_arg(argv: *const *const u8, i: usize) -> Option<&'static str> {
    unsafe {
        let ptr = *argv.add(i);
        if ptr.is_null() { None }
        else { core::ffi::CStr::from_ptr(ptr as *const i8).to_str().ok() }
    }
}

#[no_mangle]
pub extern "C" fn main(argc: u64, argv: *const *const u8) -> i32 {
    let mut if_path: Option<&str> = None;
    let mut of_path: Option<&str> = None;
    let mut bs: u64 = 512;
    let mut count: u64 = core::u64::MAX;
    let mut skip: u64 = 0;
    let mut seek: u64 = 0;

    let mut i = 1;
    while (i as u64) < argc {
        let arg = match parse_arg(argv, i) { Some(s) => s, None => break };
        if arg.starts_with("if=") { if_path = Some(&arg[3..]); }
        else if arg.starts_with("of=") { of_path = Some(&arg[3..]); }
        else if arg.starts_with("bs=") { bs = arg[3..].parse().unwrap_or(512); }
        else if arg.starts_with("count=") { count = arg[6..].parse().unwrap_or(core::u64::MAX); }
        else if arg.starts_with("skip=") { skip = arg[5..].parse().unwrap_or(0); }
        else if arg.starts_with("seek=") { seek = arg[5..].parse().unwrap_or(0); }
        i += 1;
    }

    let if_path_c = alloc::ffi::CString::new(if_path.unwrap_or("")).unwrap_or_default();
    let of_path_c = alloc::ffi::CString::new(of_path.unwrap_or("")).unwrap_or_default();

    let if_fd = if !if_path_c.as_bytes().is_empty() {
        let fd = skyos_libc::syscall::open(if_path_c.as_ptr() as *const u8, 0);
        if (fd as i64) < 0 {
            let err = b"dd: failed to open input\n";
            skyos_libc::syscall::write(2, err);
            return 1;
        }
        fd
    } else { 0 };

    let of_fd = if !of_path_c.as_bytes().is_empty() {
        let fd = skyos_libc::syscall::open(of_path_c.as_ptr() as *const u8, 1);
        if (fd as i64) < 0 {
            let err = b"dd: failed to open output\n";
            skyos_libc::syscall::write(2, err);
            if if_fd != 0 { skyos_libc::syscall::close(if_fd); }
            return 1;
        }
        fd
    } else { 1 };

    let if_is_blk = if_path.map(|p| p.starts_with("/dev/")).unwrap_or(false);
    let of_is_blk = of_path.map(|p| p.starts_with("/dev/")).unwrap_or(false);

    let _if_size = if if_is_blk { get_device_size(if_fd) } else { 0 };
    let _of_size = if of_is_blk { get_device_size(of_fd) } else { 0 };

    let sector_count = if bs >= 512 { bs / 512 } else { 1 };
    let buf_size = (sector_count * 512) as usize;
    let mut buf = vec![0u8; buf_size];

    let mut in_sector = skip * sector_count;
    let mut out_sector = seek * sector_count;
    let mut blocks_copied: u64 = 0;

    for _ in 0..count {
        buf.fill(0);

        if if_is_blk {
            if !read_sectors(if_fd, in_sector, sector_count, &mut buf) { break; }
        } else {
            let n = skyos_libc::syscall::read(if_fd, &mut buf);
            if (n as i64) <= 0 { break; }
        }
        in_sector += sector_count;

        if of_is_blk {
            if !write_sectors(of_fd, out_sector, sector_count, &buf) { break; }
        } else {
            let written = skyos_libc::syscall::write(of_fd, &buf);
            if (written as i64) <= 0 { break; }
        }
        out_sector += sector_count;
        blocks_copied += 1;
    }

    if if_fd != 0 { skyos_libc::syscall::close(if_fd); }
    if of_fd != 1 { skyos_libc::syscall::close(of_fd); }

    let msg = alloc::format!("dd: copied {} blocks ({} bytes)\n", blocks_copied, blocks_copied * bs);
    skyos_libc::syscall::write(1, msg.as_bytes());
    0
}
