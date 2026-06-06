#![no_std]
#![no_main]

extern crate alloc;



#[global_allocator]
static ALLOCATOR: skyos_libc::heap::Heap = skyos_libc::heap::Heap::new();

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { skyos_libc::syscall::exit(1); }

const BLKGETSIZE64: u64 = 0x80081272;
const BLKRD_SEC: u64 = 0x40001260;

#[repr(C, packed)]
struct BlkIoctlOp {
    sector: u64,
    count: u64,
    buf: u64,
}

fn read_sectors(fd: u64, sector: u64, count: u64, buf: &mut [u8]) -> bool {
    let op = BlkIoctlOp { sector, count, buf: buf.as_ptr() as u64 };
    skyos_libc::syscall::ioctl(fd, BLKRD_SEC, &op as *const _ as *mut u8) == 0
}

fn probe_fs(buf: &[u8; 2048]) -> &'static str {
    if buf[1080] == 0x53 && buf[1081] == 0xEF {
        return "ext2";
    }
    if buf[510] == 0x55 && buf[511] == 0xAA {
        if buf[82] == 0x28 || buf[82] == 0x29 {
            return "fat32";
        }
        if buf[54] == 0x46 && buf[55] == 0x41 && buf[56] == 0x54 && buf[57] == 0x33 && buf[58] == 0x32 {
            return "fat32";
        }
    }
    if buf[0] == 0xEB && buf[2] == 0x90 {
        return "fat";
    }
    if buf[510] == 0x55 && buf[511] == 0xAA {
        if buf[450] == 0x45 && buf[451] == 0x46 && buf[452] == 0x49 && buf[453] == 0x20 {
            return "gpt";
        }
        return "mbr";
    }
    "unknown"
}

const DEVICES: &[&str] = &[
    "/dev/sda", "/dev/sdb", "/dev/sdc", "/dev/sdd",
    "/dev/sda1", "/dev/sda2", "/dev/sda3", "/dev/sda4",
    "/dev/sdb1", "/dev/sdb2", "/dev/sdb3", "/dev/sdb4",
];

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    for dev in DEVICES {
        let cpath = alloc::ffi::CString::new(*dev).unwrap();
        let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
        if (fd as i64) < 0 { continue; }

        let mut size: u64 = 0;
        let ioctl_ret = skyos_libc::syscall::ioctl(fd, BLKGETSIZE64, &mut size as *mut u64 as *mut u8);
        if ioctl_ret != 0 { size = 0; }

        let mut buf = [0u8; 2048];
        let can_read = read_sectors(fd, 0, 4, &mut buf);
        skyos_libc::syscall::close(fd);

        if !can_read { continue; }
        let fstype = probe_fs(&buf);

        let size_mb = size / (1024 * 1024);
        let msg = alloc::format!("{}: {} ({} MB, {} sectors)\n", dev, fstype, size_mb, size / 512);
        skyos_libc::syscall::write(1, msg.as_bytes());
    }
    0
}
