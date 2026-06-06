#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;

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

fn probe_ext2(fd: u64) -> Option<(u64, u64)> {
    let mut buf = [0u8; 2048];
    if !read_sectors(fd, 2, 4, &mut buf) { return None; }
    if buf[0x418] != 0x53 || buf[0x419] != 0xEF { return None; }
    let total = u32::from_le_bytes(buf[0x404..0x408].try_into().unwrap()) as u64;
    let free = u32::from_le_bytes(buf[0x40C..0x410].try_into().unwrap()) as u64;
    let block_size = 1024u64 << buf[0x41C];
    Some((total * block_size / 512, free * block_size / 512))
}

const DEVICES: &[&str] = &[
    "/dev/sda", "/dev/sdb",
    "/dev/sda1", "/dev/sda2", "/dev/sda3", "/dev/sda4",
];

#[no_mangle]
pub extern "C" fn main(_argc: u64, _argv: *const *const u8) -> i32 {
    skyos_libc::syscall::write(1, b"Filesystem    1K-blocks    Used   Avail Use% Mounted\n");

    let mounts = [
        "/", "/dev", "/proc", "/tmp",
        "/mnt/ext2_0", "/mnt/fat_0",
        "/mnt/ext2_0_1", "/mnt/fat_0_1",
    ];

    for mount in &mounts {
        let cpath = alloc::ffi::CString::new(*mount).unwrap();
        let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
        if (fd as i64) < 0 { continue; }

        let mut st = [0u8; 56];
        let ret = skyos_libc::syscall::fstat(fd, st.as_mut_ptr());
        if (ret as i64) < 0 { skyos_libc::syscall::close(fd); continue; }

        let avail_blocks = 0u64;
        let total_blocks = 0u64;
        skyos_libc::syscall::close(fd);

        let msg = alloc::format!("{:<14} {}\n", *mount, if total_blocks > 0 {
            let used = total_blocks - avail_blocks;
            alloc::format!("{:>8} {:>8} {:>8} {:>3}%",
                total_blocks, used, avail_blocks,
                if total_blocks > 0 { used * 100 / total_blocks } else { 0 })
        } else {
            String::from("  -")
        });
        skyos_libc::syscall::write(1, msg.as_bytes());
    }

    for dev in DEVICES {
        let cpath = alloc::ffi::CString::new(*dev).unwrap();
        let fd = skyos_libc::syscall::open(cpath.as_ptr() as *const u8, 0);
        if (fd as i64) < 0 { continue; }

        let mut size: u64 = 0;
        skyos_libc::syscall::ioctl(fd, BLKGETSIZE64, &mut size as *mut u64 as *mut u8);

        if let Some((total_sec, free_sec)) = probe_ext2(fd) {
            let used_sec = total_sec - free_sec;
            let total_k = total_sec / 2;
            let used_k = used_sec / 2;
            let free_k = free_sec / 2;
            let pct = if total_sec > 0 { used_sec * 100 / total_sec } else { 0 };
            let msg = alloc::format!("{:<14} {:>8} {:>8} {:>8} {:>3}% {}\n",
                *dev, total_k, used_k, free_k, pct, "/mnt/ext2");
            skyos_libc::syscall::write(1, msg.as_bytes());
        } else {
            let size_mb = size / (1024 * 1024);
            let msg = alloc::format!("{:<14} {:>8} -\n", *dev, size_mb);
            skyos_libc::syscall::write(1, msg.as_bytes());
        }
        skyos_libc::syscall::close(fd);
    }
    0
}
